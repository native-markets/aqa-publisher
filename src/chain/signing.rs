use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{Address, B256, keccak256},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::{SolStruct, eip712_domain},
};
use alloy_signer::{Signature, SignerSync};
use anyhow::Result;
use rmp_serde::to_vec_named;

use super::types::ValidatorL1StreamAction;

sol! {
    /// `Agent` message for `l1_payload`
    struct Agent {
        string source;
        bytes32 connectionId;
    }
}

/// EIP-712 domain for HyperCore message verification
/// Ref(Python): https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/8baad667965968a020a2bb90d0287df0922ca941/hyperliquid/utils/signing.py#L184
/// Ref(Rust): https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/blob/aac75585daf12d0a3761126cc7da7a5e035b5853/src/signature/agent.rs#L21
const CORE_DOMAIN: Eip712Domain = eip712_domain! {
    name: "Exchange",
    version: "1",
    chain_id: 1337u64,
    verifying_contract: Address::ZERO,
};

/// Minimal re-implementation of hashing action struct using MessagePack
///
/// Note: `rmp_serde` serializes Struct fields in the order defined in Rust,
/// unlike when using raw `Value` which is represented as a `BTreeMap` which
/// sorts alphabetically to enforce determinism; this impl. does not rely on JSON map.
///
/// Ref: https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/blob/aac75585daf12d0a3761126cc7da7a5e035b5853/src/exchange/exchange_client.rs#L87
fn action_hash(action: &ValidatorL1StreamAction, nonce: u64) -> Result<B256> {
    // Serialize action into MessagePack payload with field names
    let mut bytes = to_vec_named(&action)?;

    // Append big-endian timestamp (nonce) and empty vault byte (0)
    bytes.extend(nonce.to_be_bytes());
    bytes.push(0);

    Ok(keccak256(bytes))
}

/// Generate L1 payload hash to sign
fn l1_payload_hash(action_hash: B256, is_mainnet: bool) -> B256 {
    // Setup phantom agent source
    // Ref(Python): https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/8baad667965968a020a2bb90d0287df0922ca941/hyperliquid/utils/signing.py#L178
    // Ref(Rust): https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/blob/aac75585daf12d0a3761126cc7da7a5e035b5853/src/signature/create_signature.rs#L13
    let phantom_src = if is_mainnet { "a" } else { "b" };

    // Prepare EIP-712 L1 message payload to sign
    // Ref: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/8baad667965968a020a2bb90d0287df0922ca941/hyperliquid/utils/signing.py#L182
    let payload = Agent {
        source: phantom_src.to_string(),
        connectionId: action_hash,
    };

    // Encode EIP-712 signing hash for wallet to sign over
    payload.eip712_signing_hash(&CORE_DOMAIN)
}

/// Prepares payload (given `rate`, generates `validatorL1Stream` message payload), signs over payload hash
pub fn get_signed_vote(
    wallet: &PrivateKeySigner,
    is_mainnet: bool,
    nonce: u64,
    rate: &str,
) -> Result<(ValidatorL1StreamAction, Signature)> {
    // Prepare payload hash to sign
    let action = ValidatorL1StreamAction::new(rate);
    let action_hash = action_hash(&action, nonce)?;
    let typed_data_hash = l1_payload_hash(action_hash, is_mainnet);

    // Sign payload hash
    let signature = wallet.sign_hash_sync(&typed_data_hash)?;
    Ok((action, signature))
}
