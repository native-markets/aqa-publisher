use super::csv::{CSVRow, parse_csv_for_latest};
use super::de::{de_date, de_scaled_opt};
use crate::sources::{DEFAULT_LOOKBACK_WINDOW, Source, get_bytes, window};
use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use serde::Deserialize;

/// CSV row format returned from `fredgraph.csv` endpoint for 30-day average (SOFR30DAYAVG)
#[derive(Debug, Deserialize)]
struct FredCSVRow {
    #[serde(rename = "observation_date", deserialize_with = "de_date")]
    date: NaiveDate,

    #[serde(rename = "SOFR30DAYAVG", deserialize_with = "de_scaled_opt")]
    value: Option<u64>,
}

impl CSVRow for FredCSVRow {
    #[inline]
    fn date(&self) -> NaiveDate {
        self.date
    }
    #[inline]
    fn value(&self) -> u64 {
        self.value.unwrap()
    }
    #[inline]
    fn has_value(&self) -> bool {
        self.value.is_some()
    }
}

/// CSV row format for overnight SOFR rates from FRED (SOFR series, not SOFR30DAYAVG)
#[derive(Debug, Deserialize)]
struct FredOvernightRow {
    #[serde(rename = "observation_date", deserialize_with = "de_date")]
    date: NaiveDate,

    #[serde(rename = "SOFR", deserialize_with = "de_scaled_opt")]
    rate: Option<u64>,
}

impl CSVRow for FredOvernightRow {
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

/// Minimal FRED getter using public `fredgraph.csv` endpoint
#[derive(Default)]
pub struct Fred;

impl Fred {
    fn url(date: NaiveDate) -> String {
        let base_url = "https://fred.stlouisfed.org/graph/fredgraph.csv?id=SOFR30DAYAVG";
        let (start, end) = window(date, DEFAULT_LOOKBACK_WINDOW);
        format!("{base_url}&cosd={start}&coed={end}")
    }

    fn overnight_url(date: NaiveDate) -> String {
        let base_url = "https://fred.stlouisfed.org/graph/fredgraph.csv?id=SOFR";
        // Need 45 days lookback to ensure we have enough data for 30-day average computation
        let (start, end) = window(date, 45);
        format!("{base_url}&cosd={start}&coed={end}")
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
        for result in reader.deserialize::<FredOvernightRow>() {
            let row = result?;
            if row.has_value() {
                rates.insert(row.date(), row.value());
            }
        }

        Ok(rates)
    }
}

impl Source for Fred {
    fn name(&self) -> &'static str {
        "St. Louis FRED"
    }

    fn fetch(&self, date: NaiveDate) -> Result<Vec<u8>> {
        get_bytes(&Self::url(date))
    }

    fn parse(&self, body: &[u8]) -> Result<(NaiveDate, u64)> {
        parse_csv_for_latest::<FredCSVRow>(body)
            .map_err(|e| anyhow!("St. Louis FRED CSV parse error: {e}"))
    }
}
