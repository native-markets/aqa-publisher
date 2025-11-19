use chrono::NaiveDate;
use rust_decimal::{Decimal, prelude::FromPrimitive};
use serde::{Deserialize, de::Error as DeError};

use crate::sources::{parse_ymd, percent_to_floored_u64};

/// Strict date deserializer for `YYYY-MM-DD` or `MM/DD/YYYY` string
/// - Trims whitespace
/// - Errors on invalid dates
pub fn de_date<'de, D>(de: D) -> std::result::Result<NaiveDate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    parse_ymd(&s).map_err(DeError::custom)
}

/// Strict percent field deserializer for percent string
/// Returns a floored `u64` scaled to `1e8`
pub fn de_scaled<'de, D>(de: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    let t = s.trim();
    if t.is_empty() || t == "." {
        return Err(DeError::custom("missing percent value"));
    }
    percent_to_floored_u64(t).map_err(DeError::custom)
}

/// Optional percent field deserializer for percent string
/// Returns None for missing/empty values instead of erroring
pub fn de_scaled_opt<'de, D>(de: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    let t = s.trim();
    if t.is_empty() || t == "." {
        return Ok(None);
    }
    percent_to_floored_u64(t).map(Some).map_err(DeError::custom)
}

/// Relaxed decimal deserializer for percent values with 2 decimal precision
pub fn de_decimal2<'de, D>(de: D) -> std::result::Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let f = f64::deserialize(de)?;

    // Sanity checks
    if !f.is_finite() {
        return Err(DeError::custom("non-finite number"));
    }
    if f < 0.0 {
        return Err(DeError::custom("negative percent not allowed"));
    }

    // Convert to decimal and snap to two decimal places
    // @dev: OFR only ever returns two decimal places
    Ok(Decimal::from_f64(f)
        .ok_or_else(|| DeError::custom("cannot convert float to decimal"))?
        .round_dp(2))
}
