use anyhow::{Context, Result};
use chrono::{Local, Utc};
use log::{error, info};
use std::env;
use tokio::time::sleep;

use aqa_publisher::utils::{
    duration_until_next_execution, fetch_and_publish_aqa, fetch_aqa, fmt_duration,
};

// Fixed execution time: 10 PM UTC (22:00)
const EXECUTION_HOUR_UTC: u32 = 22;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();
    env_logger::init();

    // Check for publisher private key(s)
    if env::var("PUBLISHER_PRIVATE_KEY").is_err() {
        anyhow::bail!("PUBLISHER_PRIVATE_KEY environment variable must be set");
    }

    // Execute startup check (no-op)
    let (median_date, _, aqa_ref_rate) = fetch_aqa()
        .await
        .context("Failed to fetch data on startup")?;
    info!(
        "Executed startup data fetch (rate: {} on {})",
        aqa_ref_rate, median_date
    );

    // Calculate next scheduled execution
    let duration_until_next = duration_until_next_execution(EXECUTION_HOUR_UTC);
    info!(
        "Next scheduled execution: {} (in {})",
        Utc::now() + chrono::Duration::from_std(duration_until_next).unwrap(),
        fmt_duration(duration_until_next)
    );

    loop {
        // Sleep until next execution
        info!("Sleeping until next execution");
        let wait_duration = duration_until_next_execution(EXECUTION_HOUR_UTC);
        sleep(wait_duration).await;

        // Fetch and publish data
        info!("\n--- Scheduled run at {} ---", Utc::now());
        info!("Local time: {}", Local::now());
        if let Err(e) = fetch_and_publish_aqa().await {
            error!("Error during scheduled run: {}", e);
        }

        // Setup next scheduled execution
        let next_duration = duration_until_next_execution(EXECUTION_HOUR_UTC);
        info!("Next execution in: {}", fmt_duration(next_duration));
    }
}
