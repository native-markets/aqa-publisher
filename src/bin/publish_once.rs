use anyhow::Result;
use std::env;

use aqa_publisher::utils::fetch_and_publish_aqa;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();
    env_logger::init();

    // Check for publisher private key
    if env::var("PUBLISHER_PRIVATE_KEY").is_err() {
        anyhow::bail!("PUBLISHER_PRIVATE_KEY environment variable must be set");
    }

    // Fetch and publish data
    Ok(fetch_and_publish_aqa().await?)
}
