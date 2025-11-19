use std::time::Duration;

use alloy::signers::local::PrivateKeySigner;
use anyhow::{Result, bail};
use chrono::prelude::Utc;
use reqwest::Client;
use serde_json::Value;

use super::signing::get_signed_vote;
use super::types::{ExchangeRequest, ExchangeResponse, SignatureReq};

/// Hyperliquid Mainnet `/exchange` URL
const MAINNET_API_EXCHANGE_URL: &str = "https://api.hyperliquid.xyz/exchange";

/// Hyperliquid Testnet `/exchange` URL
const TESTNET_API_EXCHANGE_URL: &str = "https://api.hyperliquid-testnet.xyz/exchange";

/// Minimal Hyperliquid request client
pub struct HyperliquidClient {
    http: Client,
    is_mainnet: bool,
    exchange_url: &'static str,
    wallet: PrivateKeySigner,
}

impl HyperliquidClient {
    pub fn new(wallet: PrivateKeySigner, is_mainnet: bool) -> Self {
        // Setup shared request client
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        // Initialize exchange URL based on selected network
        let exchange_url = if is_mainnet {
            MAINNET_API_EXCHANGE_URL
        } else {
            TESTNET_API_EXCHANGE_URL
        };

        Self {
            http,
            is_mainnet,
            exchange_url,
            wallet,
        }
    }

    /// Submit vote for a `rate` via `validatorL1Stream` HyperCore action
    pub async fn submit_vote(&self, rate: &str) -> Result<Value> {
        // Generate nonce, build action, sign action
        let nonce = Utc::now().timestamp_millis() as u64;
        let (action, signature) = get_signed_vote(&self.wallet, self.is_mainnet, nonce, rate)?;

        // Construct payload
        let request = ExchangeRequest {
            action,
            nonce,
            // Encode signature in expected format
            signature: SignatureReq {
                r: &format!("0x{:x}", signature.r()),
                s: &format!("0x{:x}", signature.s()),
                v: 27 + signature.v() as u64,
            },
        };

        // Send request
        let resp = self
            .http
            .post(self.exchange_url)
            .json(&request)
            .send()
            .await?;

        // Assert response success, 200-299
        if !resp.status().is_success() {
            bail!("HTTP error: {}", resp.status());
        }

        // Parse response
        // Standard action response deserializes based on `status` key in response payload
        match resp.json::<ExchangeResponse>().await? {
            ExchangeResponse::Ok { response } => Ok(response),
            ExchangeResponse::Err { response } => bail!("API Error: {}", response),
        }
    }
}
