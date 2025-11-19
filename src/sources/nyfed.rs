use super::csv::{CSVRow, parse_csv_for_latest};
use super::de::{de_date, de_scaled, de_scaled_opt};
use crate::sources::{DEFAULT_LOOKBACK_WINDOW, Source, get_bytes, window};
use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use serde::Deserialize;

/// CSV row format returned from NY Fed Markets Data search CSV endpoint (SOFRAI - averages)
/// Only relevant subset of full set of fields are included, we are not strict matching
#[derive(Debug, Deserialize)]
struct NYFedCSVRow {
    #[serde(rename = "Effective Date", deserialize_with = "de_date")]
    date: NaiveDate,

    #[serde(rename = "30-Day Average SOFR", deserialize_with = "de_scaled")]
    value: u64,
}

impl CSVRow for NYFedCSVRow {
    #[inline]
    fn date(&self) -> NaiveDate {
        self.date
    }
    #[inline]
    fn value(&self) -> u64 {
        self.value
    }
}

/// CSV row format for overnight SOFR rates from NY Fed (SOFR endpoint - not SOFRAI)
#[derive(Debug, Deserialize)]
struct NYFedOvernightRow {
    #[serde(rename = "Effective Date", deserialize_with = "de_date")]
    date: NaiveDate,

    #[serde(rename = "Rate (%)", deserialize_with = "de_scaled_opt")]
    rate: Option<u64>,
}

impl CSVRow for NYFedOvernightRow {
    #[inline]
    fn date(&self) -> NaiveDate {
        self.date
    }
    #[inline]
    fn value(&self) -> u64 {
        self.rate.unwrap()
    }
    #[inline]
    fn has_value(&self) -> bool {
        self.rate.is_some()
    }
}

// Minimal NY Fed Markets Data getter using public sofrai via search CSV endpoint
#[derive(Default)]
pub struct NYFed;

impl NYFed {
    fn url(date: NaiveDate) -> String {
        let base_url = "https://markets.newyorkfed.org/api/rates/secured/sofrai/search.csv";
        let (start, end) = window(date, DEFAULT_LOOKBACK_WINDOW);
        format!("{base_url}?type=rate&startDate={start}&endDate={end}")
    }

    fn overnight_url(date: NaiveDate) -> String {
        let base_url = "https://markets.newyorkfed.org/api/rates/secured/sofr/search.csv";
        // Need 45 days lookback to ensure we have enough data for 30-day average computation
        let (start, end) = window(date, 45);
        format!("{base_url}?startDate={start}&endDate={end}")
    }

    /// Fetch overnight SOFR rates (not the pre-calculated averages)
    /// Returns a map of date -> scaled rate (1% = 1_000_000)
    ///
    /// This is used in addition to standard `Source::fetch` to doubly verify
    /// computed average rate with collected average rate
    pub fn fetch_overnight_rates(
        date: NaiveDate,
    ) -> Result<std::collections::BTreeMap<NaiveDate, u64>> {
        use csv::{ReaderBuilder, Trim};
        use std::collections::BTreeMap;

        let body = get_bytes(&Self::overnight_url(date))?;
        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .flexible(false)
            .trim(Trim::All)
            .from_reader(&body[..]);

        let mut rates = BTreeMap::new();
        for result in reader.deserialize::<NYFedOvernightRow>() {
            let row = result?;
            if row.has_value() {
                rates.insert(row.date(), row.value());
            }
        }

        Ok(rates)
    }
}

impl Source for NYFed {
    fn name(&self) -> &'static str {
        "NY Fed"
    }

    fn fetch(&self, date: NaiveDate) -> Result<Vec<u8>> {
        get_bytes(&Self::url(date))
    }

    /// @dev: we do not do a header check here given far more returned parameters in response
    fn parse(&self, body: &[u8]) -> Result<(NaiveDate, u64)> {
        parse_csv_for_latest::<NYFedCSVRow>(body)
            .map_err(|e| anyhow!("NY Fed CSV parse error: {e}"))
    }
}
