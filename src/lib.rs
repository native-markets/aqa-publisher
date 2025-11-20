pub mod chain;
pub mod sources;
pub mod utils;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use log::{debug, error};
use sources::{Source, fred::Fred, nyfed::NYFed, ofr::OFR};

use crate::utils::adjust_basis;

/// Query all three SOFR data sources and return the median value.
///
/// This function queries FRED, NY Fed, and OFR sources for the 30-day SOFR average.
/// It prints the result from each source (or an error message if a source fails).
///
/// # Returns
/// Returns the median (date, value) tuple if validation passes. The value is in scaled
/// units where 1% = 1,000,000.
///
/// # Errors
/// - If fewer than 2 sources succeed data collection
/// - If every pair of available sources differs by more than 5 basis points
/// - If the median date from sources is more than 7 days behind the query date
///
/// # Example
/// ```no_run
/// use chrono::Local;
/// use aqa_publisher::get_median_sofr_avg;
///
/// let date = Local::now().date_naive();
/// let (median_date, median_value) = get_median_sofr_avg(date)?;
/// println!("Median: {} on {}", median_value, median_date);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn get_median_sofr_avg(date: NaiveDate) -> Result<(NaiveDate, u64)> {
    // Setup all three data sources
    let fred = Fred::default();
    let nyfed = NYFed::default();
    let ofr = OFR::default();

    // Track returned results from each data source
    let mut results: Vec<(&str, NaiveDate, u64)> = Vec::new();

    let mut collect_from = |source: &dyn Source| match source.collect(date) {
        Ok((source_date, source_value)) => {
            debug!(
                "{} 30-day SOFR avg on {}: {}",
                source.name(),
                source_date,
                source_value
            );
            results.push((source.name(), source_date, source_value));
        }
        Err(e) => {
            error!("{} failed: {}", source.name(), e);
        }
    };

    collect_from(&fred);
    collect_from(&nyfed);
    collect_from(&ofr);

    compute_validated_median(date, results)
}

/// Compute the validated median from a set of source results.
///
/// # Arguments
/// * `query_date` - The date that was queried (for staleness checking)
/// * `results` - Vector of (source_name, date, value) tuples
///
/// # Returns
/// Returns the median (date, value) tuple if validation passes.
///
/// # Errors
/// - If fewer than 2 sources are provided
/// - If every pair of values differs by more than 5 basis points
/// - If the median date from sources is more than 7 days behind the query date
pub fn compute_validated_median(
    query_date: NaiveDate,
    results: Vec<(&str, NaiveDate, u64)>,
) -> Result<(NaiveDate, u64)> {
    // Validate: need at least 2 sources
    if results.len() < 2 {
        bail!("Need at least 2 sources to succeed, got {}", results.len());
    }

    // Validate: at least one pair must differ by 5 bps or less
    // 5 basis points = 0.05% = 50_000 in scaled units (where 1% = 1_000_000)
    const MAX_DIFF_BPS: u64 = 50_000;

    let mut has_valid_pair = false;
    for i in 0..results.len() {
        for j in (i + 1)..results.len() {
            let diff = results[i].2.abs_diff(results[j].2);

            if diff <= MAX_DIFF_BPS {
                has_valid_pair = true;
                break;
            }
        }
        if has_valid_pair {
            break;
        }
    }

    if !has_valid_pair {
        bail!(
            "All pairs of sources differ by more than 5 bps. Values: {}",
            results
                .iter()
                .map(|(name, _, val)| format!("{}: {}", name, val))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Validate: bounds checking with wide safety margins
    // Reject rates outside plausible range to catch compromised data or parsing errors
    // Note: -5% to 15% is wide to handle extreme market scenarios
    const MIN_RATE: i64 = -5_000_000; // -5% in scaled units
    const MAX_RATE: u64 = 15_000_000; // 15% in scaled units

    for (name, _, val) in &results {
        // Check upper bound
        if *val > MAX_RATE {
            bail!(
                "Rate from {} ({}) exceeds maximum plausible value of 15%",
                name,
                val
            );
        }
        // Check lower bound (treating u64 values > i64::MAX as negative via two's complement)
        let val_signed = *val as i64;
        if val_signed < MIN_RATE {
            bail!(
                "Rate from {} ({}) below minimum plausible value of -5%",
                name,
                val
            );
        }
    }

    // Validate: check date staleness (median date shouldn't be > 7 days behind query date)
    // This protects against stale data from all sources (e.g., APIs not being updated)
    let mut dates: Vec<NaiveDate> = results.iter().map(|(_, d, _)| *d).collect();
    dates.sort();

    let median_date_idx = dates.len() / 2;
    let median_returned_date = if dates.len() % 2 == 0 {
        // For even number, use the OLDER (less recent) of the two middle dates
        // This is more conservative: ensures all sources meet the staleness requirement
        dates[median_date_idx - 1]
    } else {
        dates[median_date_idx]
    };

    let days_behind = query_date
        .signed_duration_since(median_returned_date)
        .num_days();
    const MAX_STALENESS_DAYS: i64 = 7;

    if days_behind > MAX_STALENESS_DAYS {
        bail!(
            "Data is too stale: median source date {} is {} days behind query date {} (max {} days allowed)",
            median_returned_date,
            days_behind,
            query_date,
            MAX_STALENESS_DAYS
        );
    }

    // Calculate median
    let mut sorted_results = results.clone();
    sorted_results.sort_by_key(|(_, _, v)| *v);

    let median_idx = sorted_results.len() / 2;
    let (median_date, median_value) = if sorted_results.len() % 2 == 0 {
        // Even number of sources: average the two middle values
        let (_, d1, v1) = sorted_results[median_idx - 1];
        let (_, _, v2) = sorted_results[median_idx];
        (d1, (v1 + v2) / 2)
    } else {
        // Odd number of sources: take the middle value
        let (_, d, v) = sorted_results[median_idx];
        (d, v)
    };

    Ok((median_date, median_value))
}

/// Scalar applied to the median SOFR average which best approximates a deployer's
/// offchain reserve income. This scalar is derived from market research and is an
/// estimate of the ratio of offchain reserve income to SOFR in competitive conditions.
///
/// Represented as a ratio (numerator, denominator) to avoid floating point arithmetic.
/// AQA rate = 85% of SOFR rate
const AQA_SCALAR_NUMERATOR: u64 = 85;
const AQA_SCALAR_DENOMINATOR: u64 = 100;

/// Get both the raw 30-day SOFR average and the AQA reference rate.
///
/// # Returns
/// Returns a tuple of (date, median_value, reference_rate) where:
/// - `date` is the median date from the sources
/// - `median_value` is the scaled 30-day SOFR average
/// - `reference_rate` is the scaled rate (basis_adjusted_raw_sofr_avg * 0.85)
///
/// All values are in scaled units where 1% = 1,000,000.
pub fn get_aqa_ref_rate(date: NaiveDate) -> Result<(NaiveDate, u64, u64)> {
    let (median_date, median_value) = get_median_sofr_avg(date)?;
    // Adjust SOFR basis
    let basis_adjusted_rate = adjust_basis(median_value);
    // Use integer arithmetic to avoid floating point rounding issues
    let reference_rate = (basis_adjusted_rate * AQA_SCALAR_NUMERATOR) / AQA_SCALAR_DENOMINATOR;
    Ok((median_date, median_value, reference_rate))
}
