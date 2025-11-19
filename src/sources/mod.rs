pub(crate) mod csv;
pub(crate) mod de;
pub mod fred;
pub mod nyfed;
pub mod ofr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Days, NaiveDate};
use log::warn;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::collections::BTreeMap;
use std::str::FromStr;

/// Default lookback for data collection window
pub const DEFAULT_LOOKBACK_WINDOW: u64 = 14;

/// Common trait implemented by each API data source
pub trait Source {
    /// Data source name
    fn name(&self) -> &'static str;

    /// Fetch raw response bytes for a small window
    /// Window is [date - 14 days, date] to account for holidays & weekends
    fn fetch(&self, date: NaiveDate) -> Result<Vec<u8>>;

    /// Parse fetched data into a single, scaled `u64` value (1e8 scale, 1% = 1_000_000)
    /// Returns (most recently available date, scaled `u64` yield value for said date)
    fn parse(&self, body: &[u8]) -> Result<(NaiveDate, u64)>;

    /// Unified fetch + parse
    fn collect(&self, date: NaiveDate) -> Result<(NaiveDate, u64)> {
        self.parse(&self.fetch(date)?)
    }
}

/// Small `GET` helper to fetch data from URL as bytes
/// Retries up to 3 times with exponential backoff (30s, 60s, 120s) on failures
pub(crate) fn get_bytes(url: &str) -> Result<Vec<u8>> {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_DELAY_SECS: u64 = 30;

    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        match reqwest::blocking::get(url)
            .with_context(|| format!("GET {url}"))
            .and_then(|resp| {
                resp.error_for_status()
                    .with_context(|| format!("status not OK for {url}"))
            })
            .and_then(|resp| resp.bytes().with_context(|| "reading body"))
        {
            Ok(bytes) => return Ok(bytes.to_vec()),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    let delay_secs = INITIAL_DELAY_SECS * (2u64.pow(attempt - 1));
                    warn!(
                        "Request to {} failed (attempt {}/{}), retrying in {}s...",
                        url, attempt, MAX_RETRIES, delay_secs
                    );
                    std::thread::sleep(std::time::Duration::from_secs(delay_secs));
                }
            }
        }
    }

    Err(last_error
        .unwrap()
        .context(format!("Failed after {} retries", MAX_RETRIES)))
}

/// Convert a percent value (e.g., 4.2932) to scaled `u64` (1% == 1_000_000)
/// Floors as default behaviour (payor-friendly): 4.2931999999 -> 4_293_199
pub fn percent_to_floored_u64(s: &str) -> Result<u64> {
    // Parse string, ensure valid value
    let raw = s.trim();
    if raw.is_empty() || raw == "." {
        bail!("missing percent value")
    }

    // Parse as decimal, ensure non-negative
    let dec = Decimal::from_str(raw)?;
    if dec.is_sign_negative() {
        bail!("negative percent not allowed: {raw}")
    }

    // Scaled = floor(percent * 1e6)
    let scaled = (dec * Decimal::from(1_000_000u64)).trunc();
    scaled
        .to_u64()
        .ok_or_else(|| anyhow!("overflow converting to u64"))
}

/// Inclusive date window [start, end] used for weekend/holiday fallbck
/// `days` is the look-back length (e.g., 14 for FRED/NYFed, 45 for computed OFR)
pub fn window(end_date: NaiveDate, days: u64) -> (NaiveDate, NaiveDate) {
    let start_date = end_date.checked_sub_days(Days::new(days)).unwrap();
    (start_date, end_date)
}

/// Parse a date string into `NaiveDate`
/// Accepts:
/// - ISO: `YYYY-MM-DD`
/// - US: `MM/DD/YYYY`
pub fn parse_ymd(s: &str) -> Result<NaiveDate> {
    let s = s.trim();
    // ISO
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d);
    }
    // US
    if let Ok(d) = NaiveDate::parse_from_str(s, "%m/%d/%Y") {
        return Ok(d);
    }
    bail!("invalid date format: '{s}'")
}

/// Compute the NY Fed 30-day compounded SOFR average from overnight rates
/// following the methodology from https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates#sofr_ai_calculation_methodology
///
/// This function is used in conjunction with overnight rates fetched from NYFed and Fred to
/// doubly verify computed compounded average matches collected compounded average.
///
/// Formula: ((∏(1 + SOFRi × ni/360)) - 1) × 360/dc
/// where:
/// - SOFRi = overnight SOFR rate for business day i (as decimal, e.g., 0.04293 for 4.293%)
/// - ni = number of calendar days for which SOFR_i applies (e.g., 3 for most Fridays)
/// - dc = number of calendar days in calculation period (30 for 30-day average)
///
/// The calculation:
/// 1. Takes overnight rates in scaled u64 format (1% = 1_000_000)
/// 2. Compounds once per business day, using ni to account for weekends/holidays
/// 3. Returns the result in scaled u64 format
pub fn compute_compounded_average(
    effective_date: NaiveDate,
    overnight_rates: &BTreeMap<NaiveDate, u64>,
) -> Result<u64> {
    if overnight_rates.is_empty() {
        bail!("no overnight rates provided")
    }

    // The effective_date is the publication date of the average.
    // The calculation period ends one business day earlier (the value date of the last SOFR).
    // For a 30-day average published on Oct 7:
    // - Calculation period: Sep 7 to Oct 6 (30 calendar days)
    let calculation_end_date = effective_date
        .checked_sub_days(Days::new(1))
        .ok_or_else(|| anyhow!("date underflow"))?;

    // Start date is 30 days before publication date
    let start_date = effective_date
        .checked_sub_days(Days::new(30))
        .ok_or_else(|| anyhow!("date underflow"))?;

    // Find the rate for the last business day before (or at) start_date
    let initial_rate = overnight_rates
        .range(..=start_date)
        .next_back()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow!("insufficient history before {start_date}"))?;

    // Build a list of (rate, ni) tuples where ni = number of calendar days this rate applies
    let mut business_days: Vec<(u64, u64)> = Vec::new();

    let mut current_rate = initial_rate;
    let mut current_rate_start = start_date; // Track when current rate started applying

    // Iterate through the 30-day calculation period
    let period_days = calculation_end_date
        .signed_duration_since(start_date)
        .num_days() as u64;
    for day_offset in 0..=period_days {
        let day = start_date
            .checked_add_days(Days::new(day_offset))
            .ok_or_else(|| anyhow!("date overflow"))?;

        // Check if this is a new business day (has a rate)
        if let Some(&new_rate) = overnight_rates.get(&day) {
            // Finalize the current rate's ni count (from current_rate_start to day-1)
            let duration = day.signed_duration_since(current_rate_start);
            let ni = duration.num_days() as u64;
            if ni > 0 {
                business_days.push((current_rate, ni));
            }

            current_rate = new_rate;
            current_rate_start = day;
        }
    }

    // Handle the final rate (from current_rate_start to calculation_end_date inclusive)
    let duration = calculation_end_date.signed_duration_since(current_rate_start);
    let ni = duration.num_days() as u64 + 1;
    business_days.push((current_rate, ni));

    // Compound using the ni-grouped approach
    let one_million = Decimal::from(1_000_000u64);
    let d360 = Decimal::from(360);
    let d100 = Decimal::from(100);

    let mut factor = Decimal::ONE;

    for (rate, ni) in business_days {
        // Convert scaled u64 rate to decimal percentage (1% = 0.01)
        let rate_decimal = Decimal::from(rate) / one_million / d100;

        // Compound: factor *= (1 + rate × ni/360)
        let ni_decimal = Decimal::from(ni);
        factor *= Decimal::ONE + rate_decimal * ni_decimal / d360;
    }

    // Annualize: ((factor - 1) × 360/30) and convert to percentage then to scaled u64
    let avg_pct = (factor - Decimal::ONE) * (d360 / Decimal::from(30));

    // Convert back to scaled u64: percentage × 1_000_000
    let scaled = (avg_pct * d100 * one_million).trunc();
    scaled
        .to_u64()
        .ok_or_else(|| anyhow!("overflow converting to u64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    mod percent_to_floored_tests {
        use super::*;

        #[test]
        fn basic_integers() {
            assert_eq!(percent_to_floored_u64("0").unwrap(), 0);
            assert_eq!(percent_to_floored_u64("1").unwrap(), 1_000_000);
            assert_eq!(percent_to_floored_u64("100").unwrap(), 100_000_000);
            assert_eq!(percent_to_floored_u64("123456").unwrap(), 123_456_000_000);
        }

        #[test]
        fn floor_decimals() {
            assert_eq!(percent_to_floored_u64("4.2932").unwrap(), 4_293_200);
            assert_eq!(
                percent_to_floored_u64("4.293199999999999683").unwrap(),
                4_293_199
            );
            assert_eq!(percent_to_floored_u64("1.000000").unwrap(), 1_000_000);
            assert_eq!(
                percent_to_floored_u64("1.0000000000001").unwrap(),
                1_000_000
            );
        }

        #[test]
        fn floor_after_six_decimals() {
            assert_eq!(percent_to_floored_u64("2.123456").unwrap(), 2_123_456);
            assert_eq!(percent_to_floored_u64("2.1234560").unwrap(), 2_123_456);
            assert_eq!(percent_to_floored_u64("2.123456789").unwrap(), 2_123_456);
            assert_eq!(percent_to_floored_u64("0.0000009").unwrap(), 0);
        }

        #[test]
        fn trim_whitespaces() {
            assert_eq!(percent_to_floored_u64("   4.5 ").unwrap(), 4_500_000);
            assert_eq!(percent_to_floored_u64("\t\n3.25\r").unwrap(), 3_250_000);
        }

        #[test]
        fn reject_invalid() {
            assert!(percent_to_floored_u64("").is_err());
            assert!(percent_to_floored_u64(".").is_err());
            assert!(percent_to_floored_u64("..1").is_err());
            assert!(percent_to_floored_u64("abc").is_err());
            assert!(percent_to_floored_u64("-0.01").is_err());
        }

        #[test]
        fn reject_exponent_notation() {
            assert!(percent_to_floored_u64("4.2e0").is_err());
            assert!(percent_to_floored_u64("1e2").is_err());
            assert!(percent_to_floored_u64("-1e2").is_err());
        }
    }

    mod window_tests {
        use super::*;

        #[test]
        fn valid_lookback_14days() {
            // 2025-10-05 -> 2025-09-21 (14 days)
            let actual_end = NaiveDate::from_ymd_opt(2025, 10, 5).unwrap();
            let actual_start = NaiveDate::from_ymd_opt(2025, 9, 21).unwrap();
            let (calc_start, calc_end) = window(actual_end, DEFAULT_LOOKBACK_WINDOW);

            assert_eq!(calc_end, actual_end);
            assert_eq!(calc_start, actual_start);
            assert_eq!(
                (calc_end - calc_start).num_days() as u64,
                DEFAULT_LOOKBACK_WINDOW
            );
        }

        #[test]
        fn valid_leap_lookback_14days() {
            // 2024 is leap year
            // 2024-03-01 -> 2024-02-16
            let actual_end = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
            let actual_start = NaiveDate::from_ymd_opt(2024, 2, 16).unwrap();
            let (calc_start, calc_end) = window(actual_end, DEFAULT_LOOKBACK_WINDOW);

            assert_eq!(calc_end, actual_end);
            assert_eq!(calc_start, actual_start);
            assert_eq!(
                (calc_end - calc_start).num_days() as u64,
                DEFAULT_LOOKBACK_WINDOW
            );
        }

        #[test]
        fn valid_leap_lookback_45days() {
            // 2024 is leap year
            // 2024-03-01 -> 2024-01-16
            let actual_end = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
            let actual_start = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
            let (calc_start, calc_end) = window(actual_end, 45);

            assert_eq!(calc_end, actual_end);
            assert_eq!(calc_start, actual_start);
            assert_eq!((calc_end - calc_start).num_days() as u64, 45);
        }
    }

    mod parse_ymd_tests {
        use super::*;

        #[test]
        fn well_formed_date() {
            assert_eq!(
                parse_ymd("2025-10-03").unwrap(),
                NaiveDate::from_ymd_opt(2025, 10, 3).unwrap()
            );

            // Leap year
            assert_eq!(
                parse_ymd("2024-02-29").unwrap(),
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
            );

            // Lenient (non zero-padded)
            assert_eq!(
                parse_ymd("2025-2-9").unwrap(),
                NaiveDate::from_ymd_opt(2025, 2, 9).unwrap()
            );
        }

        #[test]
        fn trim_whitspace() {
            assert_eq!(
                parse_ymd("  2025-01-05  ").unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 5).unwrap()
            );
        }

        #[test]
        fn reject_bad_formed_or_invalid_dates() {
            // Invalid format
            assert!(parse_ymd("03-10-2025").is_err());

            // Not a leap year
            assert!(parse_ymd("2025-02-29").is_err());

            // Invalid month
            assert!(parse_ymd("2025-13-01").is_err());

            // Invalid dates
            assert!(parse_ymd("2025-00-01").is_err());
            assert!(parse_ymd("2025-01-00").is_err());
            assert!(parse_ymd("not-a-date").is_err());
            assert!(parse_ymd("").is_err());
        }

        #[test]
        fn accepts_us_date() {
            assert_eq!(
                parse_ymd("10/03/2025").unwrap(),
                NaiveDate::from_ymd_opt(2025, 10, 3).unwrap()
            );

            // Non-zero padded (lenient parse)
            assert_eq!(
                parse_ymd("7/4/2025").unwrap(),
                NaiveDate::from_ymd_opt(2025, 7, 4).unwrap()
            );
        }

        #[test]
        fn trims_whitespace_in_us_date() {
            assert_eq!(
                parse_ymd("  10/03/2025 ").unwrap(),
                NaiveDate::from_ymd_opt(2025, 10, 3).unwrap()
            );
        }
    }
}
