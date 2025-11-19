use anyhow::Result;
use chrono::Local;
use log::info;

use aqa_publisher::{get_aqa_ref_rate, utils::fmt_scaled_rate};

fn main() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();
    env_logger::init();

    // Collect and log AQA rate
    let date = Local::now().date_naive();
    let (median_date, _, aqa_ref_rate) = get_aqa_ref_rate(date)?;
    info!("AQA rate on {}: {}", median_date, aqa_ref_rate);
    info!(
        "Submission-formatted rate: {}",
        fmt_scaled_rate(aqa_ref_rate)
    );

    Ok(())
}
