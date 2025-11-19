use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use log::{error, info};
use std::env;
use tokio::time::Duration;

use super::{chain::HyperliquidClient, get_aqa_ref_rate};

/// Convert scaled rate (where 1% = 1,000,000) to decimal string format (e.g., "0.045" for 4.5%)
/// Dev: (1) divide by 1MM to get percentage, (2) divide by 100 to get decimal, (3) return 8 decimals
///      4,500,000 -> 4.5% -> 0.045
pub fn fmt_scaled_rate(scaled_rate: u64) -> String {
    format!("{:.8}", scaled_rate as f64 / 100_000_000.0)
}

/// Adjusts a scaled rate from an ACT/360 basis to an ACT/365.25 basis
///
/// SOFR is published on an ACT/360 basis (simple interest over current
/// period, simplifying scaling). However, AQA rate publishing expects
/// an annualized rate based on ACT/365.25.
///
/// Formula: rate * (365.25 / 360)
/// Sans-decimal: rate * (1461 / 1440) = rate * (487 / 480)
/// Dev: floors as default behaviour (payor-friendly)
pub fn adjust_basis(scaled_rate: u64) -> u64 {
    (scaled_rate * 487u64) / 480u64
}

/// Format human-readable duration
/// Dev: returns a strictly-positive time <24h, rolls over if current time past target hour
pub fn fmt_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{}h {}m {}s", hours, minutes, seconds)
}

/// Calculate duration until next scheduled execution at `target_hour` UTC
pub fn duration_until_next_execution(target_hour: u32) -> Duration {
    let now = Utc::now();
    let mut next_run = now
        .date_naive()
        .and_hms_opt(target_hour, 0, 0)
        .unwrap()
        .and_utc();

    // If we've already passed today's target time, schedule for tomorrow
    if next_run <= now {
        next_run = next_run + chrono::Duration::days(1);
    }

    let duration = (next_run - now).to_std().unwrap();
    duration
}

/// Fetch AQA rate data without publishing
pub async fn fetch_aqa() -> Result<(NaiveDate, u64, u64)> {
    // Run blocking HTTP calls in a separate thread pool to avoid blocking the async runtime
    let (median_date, raw_sofr_avg, aqa_ref_rate) = tokio::task::spawn_blocking(|| {
        let date = Utc::now().date_naive();
        get_aqa_ref_rate(date)
    })
    .await
    .context("Failed to spawn blocking task")?
    .context("Failed to compute AQA reference rate")?;

    Ok((median_date, raw_sofr_avg, aqa_ref_rate))
}

/// Fetch and publish AQA rate via validator vote
pub async fn fetch_and_publish_aqa() -> Result<()> {
    // Get AQA reference rate
    let (median_date, _, aqa_ref_rate) = fetch_aqa().await?;
    info!("AQA rate on {}: {}", median_date, aqa_ref_rate);

    // Convert to decimal string format for API payload
    let rfr_rate = fmt_scaled_rate(aqa_ref_rate);
    info!("Submission-formatted rate: {}", rfr_rate);

    // Load signer from environment
    let private_key: String = env::var("PUBLISHER_PRIVATE_KEY")?;
    let signer: PrivateKeySigner = private_key.parse()?;
    info!("Loaded publishing signer: {}", signer.address());

    // Determine network (default to `mainnet`)
    let is_mainnet = match env::var("NETWORK").as_deref() {
        Ok("testnet") => {
            info!("Publishing to testnet");
            false
        }
        _ => {
            info!("Publishing to mainnet");
            true
        }
    };

    // Setup exchange client
    let hl_client = HyperliquidClient::new(signer, is_mainnet);

    // Submit vote with rate and track response
    let response = hl_client.submit_vote(&rfr_rate).await;
    match response {
        Ok(resp) => {
            info!("Validator vote success: {:?}", resp);
        }
        Err(e) => {
            error!("Failed to submit vote: {}", e);
            anyhow::bail!("Validator vote failed: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fmt_scaled_rate_tests {
        use super::*;

        #[test]
        fn decimal_scaling() {
            assert_eq!(fmt_scaled_rate(0), "0.00000000");
            assert_eq!(fmt_scaled_rate(1_000_000), "0.01000000");
            assert_eq!(fmt_scaled_rate(4_500_000), "0.04500000");
            assert_eq!(fmt_scaled_rate(100_000_000), "1.00000000");
            assert_eq!(fmt_scaled_rate(12_345_678), "0.12345678");
        }
    }

    mod adjust_basis_tests {
        use super::*;

        #[test]
        fn basis_scaling() {
            assert_eq!(adjust_basis(0), 0);
            assert_eq!(adjust_basis(100_000_000), 101_458_333);
            assert_eq!(adjust_basis(5_000_000), 5_072_916);
        }
    }
}
