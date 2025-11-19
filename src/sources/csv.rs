use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use csv::{ReaderBuilder, Trim};
use serde::de::DeserializeOwned;

/// Trait helper so generic CSV parser can extract (date, value) from collected CSVs
pub trait CSVRow {
    fn date(&self) -> NaiveDate;
    fn value(&self) -> u64;
    fn has_value(&self) -> bool {
        true
    }
}

/// Generic latest row CSV parser; collect data --> sort by date --> pick latest
/// Malformed or missing rows will fast-fail via serde deserializers
pub fn parse_csv_for_latest<R>(body: &[u8]) -> Result<(NaiveDate, u64)>
where
    R: DeserializeOwned + CSVRow,
{
    // Strict CSV parse
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .trim(Trim::All)
        .from_reader(body);

    // Collect all rows, bad rows will force failure
    let mut rows: Vec<R> = reader.deserialize().collect::<Result<_, _>>()?;

    // Filter out rows without valid values
    rows.retain(|r| r.has_value());

    // Sort rows by date
    // @dev: note that NY Fed returns data in descending date
    rows.sort_unstable_by_key(|r| r.date());

    // Select most recent row
    let last = rows
        .last()
        .ok_or_else(|| anyhow!("no observation found in CSV"))?;
    Ok((last.date(), last.value()))
}
