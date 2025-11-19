use serde::{Deserialize, Serialize};

/// `validatorL1Stream` vote action
/// Ref: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint?q=validatorL1Stream#validator-vote-on-risk-free-rate-for-aligned-quote-asset
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorL1StreamAction {
    /// Forced rename because `type` is a reserved keyword in Rust
    #[serde(rename = "type")]
    pub type_string: String,
    pub risk_free_rate: String,
}

impl ValidatorL1StreamAction {
    pub fn new(rate: &str) -> Self {
        Self {
            type_string: "validatorL1Stream".to_string(),
            risk_free_rate: rate.to_string(),
        }
    }
}

/// `/exchange` request payload
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRequest<'a> {
    pub action: ValidatorL1StreamAction,
    pub nonce: u64,
    pub signature: SignatureReq<'a>,
}

/// Encoded wallet signature (r, s, v)
#[derive(Debug, Serialize)]
pub struct SignatureReq<'a> {
    pub r: &'a str,
    pub s: &'a str,
    pub v: u64,
}

/// `/exchange` request response
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ExchangeResponse {
    // Handles `ok`, `err` `status` fields dynamically
    Ok { response: serde_json::Value },
    Err { response: String },
}
