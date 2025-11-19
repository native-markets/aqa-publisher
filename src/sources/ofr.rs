use std::collections::BTreeMap;

use super::de::{de_date, de_decimal2};
use crate::sources::{Source, get_bytes, window};
use anyhow::{Result, anyhow, bail};
use chrono::{Days, NaiveDate};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use serde::Deserialize;

/// JSON tuple format returned from OFR FNYR dataset JSON endpoint
#[derive(Debug, Deserialize)]
struct OFRTupleRow(
    #[serde(deserialize_with = "de_date")] NaiveDate,
    #[serde(deserialize_with = "de_decimal2")] Decimal,
);

/// OFR: compute 30-day compounded SOFR average from overnight SOFR (FNYR-SOFR-A)
#[derive(Default)]
pub struct OFR;

impl OFR {
    // SOFR mnemonic to collect
    // Source: https://data.financialresearch.gov/v1/metadata/mnemonics?dataset=fnyr
    // Object: {"mnemonic": "FNYR-SOFR-A", "series_name": "Secured Overnight Financing Rate"}
    const SOFR_MNEMONIC: &'static str = "FNYR-SOFR-A";

    // Fetch ~45 days of data so we can safely carry prior business day rates
    fn url(date: NaiveDate) -> String {
        let base_url = "https://data.financialresearch.gov/v1/series/timeseries";
        let (start, end) = window(date, 45);
        format!(
            "{}?mnemonic={}&start_date={}&end_date={}",
            base_url,
            Self::SOFR_MNEMONIC,
            start,
            end
        )
    }

    // Compute the NY Fed 30-day compounded SOFR average on calendar days [eff-29, eff]
    // Using ni-grouped approach: compound once per business day with ni calendar days
    fn compute_compounded(effective_date: NaiveDate, data: &[OFRTupleRow]) -> Result<Decimal> {
        // Assert some data exists and that at least 30d of data exists
        if data.is_empty() {
            bail!("OFR: no observations available")
        }

        // Create map of business-day rates
        // Ignore any future dates > requested `effective_date`
        let mut map: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();
        for d in data {
            if d.0 <= effective_date {
                map.insert(d.0, d.1);
            }
        }
        if map.is_empty() {
            bail!("OFR: no business-day observations <= {effective_date}")
        }

        // The effective_date is the publication date of the average.
        // The calculation period ends one business day earlier (the value date of the last SOFR).
        // For a 30-day average published on Oct 7:
        // - Calculation period: Sep 7 to Oct 6 (30 calendar days)
        let calculation_end_date = effective_date.checked_sub_days(Days::new(1)).unwrap();

        // Start date is 30 days before publication date
        let start_date = effective_date.checked_sub_days(Days::new(30)).unwrap();

        // Find yield rate of last business day before (or at) `start_date`
        let initial_rate = map
            .range(..=start_date)
            .next_back()
            .map(|(_, r)| *r)
            .ok_or_else(|| anyhow!("OFR: insufficient history before {start_date}"))?;

        // Build a list of (rate, ni) tuples where ni = number of calendar days this rate applies
        let mut business_days: Vec<(Decimal, u64)> = Vec::new();

        let mut current_rate = initial_rate;
        let mut current_rate_start = start_date; // Track when current rate started applying

        // Iterate through the 30-day calculation period
        let period_days = calculation_end_date
            .signed_duration_since(start_date)
            .num_days() as u64;
        for day_offset in 0..=period_days {
            let day = start_date.checked_add_days(Days::new(day_offset)).unwrap();

            // Check if this is a new business day (has a rate)
            if let Some(&new_rate) = map.get(&day) {
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
        let one = Decimal::ONE;
        let mut factor = Decimal::ONE;
        let d360 = Decimal::from_i32(360).unwrap();
        let d100 = Decimal::from_i32(100).unwrap();

        for (rate, ni) in business_days {
            // Compound: factor *= (1 + rate × ni/360)
            let ni_decimal = Decimal::from(ni);
            factor *= one + (rate / d100) * ni_decimal / d360;
        }

        // Annualize over 30 calendar days and convert to percentage
        // Result is in decimal form (e.g., 0.04293), multiply by 100 to get percentage (4.293)
        Ok((factor - one) * (d360 / Decimal::from_i32(30).unwrap()) * d100)
    }
}

impl Source for OFR {
    fn name(&self) -> &'static str {
        "OFR (computed)"
    }

    fn fetch(&self, date: NaiveDate) -> Result<Vec<u8>> {
        get_bytes(&Self::url(date))
    }

    fn parse(&self, body: &[u8]) -> Result<(NaiveDate, u64)> {
        // Parse returned data as array of tuples
        let rows: Vec<OFRTupleRow> = serde_json::from_slice(body)?;
        if rows.is_empty() {
            bail!("OFR JSON data: no observations found")
        }

        // Collect effective date (must be the last business day
        // to conform to NYFED, FRED collection behavior)
        let effective_date = rows
            .iter()
            .map(|r| r.0)
            .max()
            .ok_or_else(|| anyhow!("OFR JSON data: no dates"))?;

        // Compute compounded 30-day average ending on `effective_date`
        let avg_pct = Self::compute_compounded(effective_date, &rows)?;
        let scaled = super::percent_to_floored_u64(&avg_pct.to_string())?;
        Ok((effective_date, scaled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, NaiveDate};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn make_row(date_str: &str, rate: &str) -> OFRTupleRow {
        OFRTupleRow(
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap(),
            Decimal::from_str(rate).unwrap(),
        )
    }

    #[test]
    fn compute_compounded_basic() {
        // Simple test: flat 4.00% rate for 30 consecutive business days
        // With flat rate, compounded average should equal the rate
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap();
        let mut data = vec![];

        // Generate 45 days of flat 4.00% rates (all business days)
        for i in 0..45 {
            let date = effective_date.checked_sub_days(Days::new(44 - i)).unwrap();
            data.push(make_row(&date.to_string(), "4.00"));
        }

        let result = OFR::compute_compounded(effective_date, &data).unwrap();

        // With flat rate, compounded average ≈ flat rate
        // Allow small rounding difference due to compounding
        let expected = dec!(4.00);
        let diff = (result - expected).abs();
        assert!(diff < dec!(0.01), "Expected ~4.00, got {}", result);
    }

    #[test]
    fn compute_compounded_with_weekends() {
        // Test that rates carry forward over weekends (non-business days)
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap(); // Friday

        let mut data = vec![];

        // Add business days only (Mon-Fri pattern, skip weekends)
        // Starting from 45 days back
        let start = effective_date.checked_sub_days(Days::new(44)).unwrap();
        for i in 0..45 {
            let date = start.checked_add_days(Days::new(i)).unwrap();
            // Skip Saturdays and Sundays
            let weekday = date.weekday().num_days_from_monday();
            if weekday < 5 {
                // Monday=0, Friday=4
                data.push(make_row(&date.to_string(), "4.25"));
            }
        }

        let result = OFR::compute_compounded(effective_date, &data).unwrap();

        // Should successfully compute even with missing weekend data
        let expected = dec!(4.25);
        let diff = (result - expected).abs();
        assert!(diff < dec!(0.01), "Expected ~4.25, got {}", result);
    }

    #[test]
    fn compute_compounded_insufficient_history() {
        // Test error when there's not enough historical data
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap();

        // Only provide data for the effective date (not enough history)
        let data = vec![make_row("2025-10-03", "4.00")];

        let result = OFR::compute_compounded(effective_date, &data);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("insufficient history")
        );
    }

    #[test]
    fn compute_compounded_empty_data() {
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap();
        let data = vec![];

        let result = OFR::compute_compounded(effective_date, &data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no observations"));
    }

    #[test]
    fn compute_compounded_filters_future_dates() {
        // Test that dates after effective_date are filtered out
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap();

        let mut data = vec![];
        // Add sufficient historical data
        for i in 0..35 {
            let date = effective_date.checked_sub_days(Days::new(34 - i)).unwrap();
            data.push(make_row(&date.to_string(), "4.00"));
        }

        // Add future dates with very different rates
        data.push(make_row("2025-10-04", "10.00"));
        data.push(make_row("2025-10-05", "10.00"));

        let result = OFR::compute_compounded(effective_date, &data).unwrap();

        // Should be close to 4.00, not affected by future 10.00 rates
        let expected = dec!(4.00);
        let diff = (result - expected).abs();
        assert!(
            diff < dec!(0.01),
            "Future dates should be filtered out. Expected ~4.00, got {}",
            result
        );
    }

    #[test]
    fn compute_compounded_realistic_variation() {
        // Test with realistic rate variation
        let effective_date = NaiveDate::from_ymd_opt(2025, 10, 3).unwrap();

        let mut data = vec![];
        let rates = ["4.29", "4.30", "4.31", "4.32", "4.30", "4.29", "4.28"];

        for i in 0..35 {
            let date = effective_date.checked_sub_days(Days::new(34 - i)).unwrap();
            let rate = rates[(i as usize) % rates.len()];
            data.push(make_row(&date.to_string(), rate));
        }

        let result = OFR::compute_compounded(effective_date, &data).unwrap();

        // Should be in the range of input rates (4.28 - 4.32)
        assert!(
            result > dec!(4.27) && result < dec!(4.33),
            "Expected rate in range [4.27, 4.33], got {}",
            result
        );
    }

    #[test]
    fn parse_json_format() {
        // Test that we can parse the OFR JSON format correctly
        let json = r#"[
            ["2025-09-20", 4.29],
            ["2025-09-23", 4.30],
            ["2025-09-24", 4.31],
            ["2025-10-01", 4.32],
            ["2025-10-02", 4.30],
            ["2025-10-03", 4.29]
        ]"#;

        let rows: Vec<OFRTupleRow> = serde_json::from_str(json).unwrap();

        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].0, NaiveDate::from_ymd_opt(2025, 9, 20).unwrap());
        assert_eq!(rows[0].1, dec!(4.29));
        assert_eq!(rows[5].0, NaiveDate::from_ymd_opt(2025, 10, 3).unwrap());
        assert_eq!(rows[5].1, dec!(4.29));
    }

    #[test]
    fn parse_integration() {
        // Integration test: parse JSON and compute compounded average
        let json = r#"[
            ["2025-09-03", 4.28],
            ["2025-09-04", 4.28],
            ["2025-09-05", 4.29],
            ["2025-09-06", 4.30],
            ["2025-09-09", 4.31],
            ["2025-09-10", 4.32],
            ["2025-09-11", 4.30],
            ["2025-09-12", 4.29],
            ["2025-09-13", 4.28],
            ["2025-09-16", 4.29],
            ["2025-09-17", 4.30],
            ["2025-09-18", 4.31],
            ["2025-09-19", 4.30],
            ["2025-09-20", 4.29],
            ["2025-09-23", 4.30],
            ["2025-09-24", 4.31],
            ["2025-09-25", 4.32],
            ["2025-09-26", 4.30],
            ["2025-09-27", 4.29],
            ["2025-09-30", 4.30],
            ["2025-10-01", 4.31],
            ["2025-10-02", 4.30],
            ["2025-10-03", 4.29]
        ]"#;

        let ofr = OFR::default();
        let result = ofr.parse(json.as_bytes()).unwrap();

        // Should return latest date and a reasonable scaled value
        assert_eq!(result.0, NaiveDate::from_ymd_opt(2025, 10, 3).unwrap());

        // Rate should be around 4.30% (4_300_000 scaled)
        // Allow reasonable range given compounding
        assert!(
            result.1 > 4_280_000 && result.1 < 4_320_000,
            "Expected scaled value around 4,300,000, got {}",
            result.1
        );
    }
}
