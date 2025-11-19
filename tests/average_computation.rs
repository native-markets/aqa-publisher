use aqa_publisher::sources::{self, Source, fred::Fred, nyfed::NYFed, ofr::OFR};
use chrono::{Days, Local};
use rayon::prelude::*;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

#[test]
fn verify_computed_averages() {
    let end_date = Local::now().date_naive();
    // Test over 30 days
    let start_date = end_date.checked_sub_days(Days::new(30)).unwrap();

    // Collect dates every day
    let mut dates = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        dates.push(current);
        current = current.checked_add_days(Days::new(1)).unwrap();
    }

    println!(
        "Testing {} dates (last 30 days) for computed vs API average accuracy...",
        dates.len()
    );

    // Use Mutex for thread-safe result collection
    let errors = Mutex::new(Vec::new());
    let discrepancies = Mutex::new(Vec::new());
    let success_count = Mutex::new(0);

    // Define threshold for acceptable difference (30 units = 0.00003%)
    const MAX_DIFF: u64 = 30;

    // OFR has lag, so only validate dates up to 5 days ago
    let ofr_cutoff_date = end_date.checked_sub_days(Days::new(5)).unwrap();

    // Process dates in parallel with limited parallelism to avoid rate limiting
    dates.chunks(10).for_each(|chunk| {
        chunk.par_iter().for_each(|&date| {
            let mut date_errors = Vec::new();
            let mut date_discrepancies = Vec::new();
            let mut success = true;

            // Test NY Fed
            match test_nyfed(date, MAX_DIFF) {
                Ok(diff) => {
                    if diff > MAX_DIFF {
                        date_discrepancies.push(format!(
                            "NYFed difference {} exceeds threshold {}",
                            diff, MAX_DIFF
                        ));
                        success = false;
                    }
                }
                Err(e) => {
                    date_errors.push(format!("NYFed: {}", e));
                    success = false;
                }
            }

            // Test FRED
            match test_fred(date, MAX_DIFF) {
                Ok(diff) => {
                    if diff > MAX_DIFF {
                        date_discrepancies.push(format!(
                            "FRED difference {} exceeds threshold {}",
                            diff, MAX_DIFF
                        ));
                        success = false;
                    }
                }
                Err(e) => {
                    date_errors.push(format!("FRED: {}", e));
                    success = false;
                }
            }

            // Test OFR (only for dates more than 5 days ago)
            if date <= ofr_cutoff_date {
                match test_ofr(date, MAX_DIFF) {
                    Ok(diff) => {
                        if diff > MAX_DIFF {
                            date_discrepancies.push(format!(
                                "OFR difference {} exceeds threshold {}",
                                diff, MAX_DIFF
                            ));
                            success = false;
                        }
                    }
                    Err(e) => {
                        date_errors.push(format!("OFR: {}", e));
                        success = false;
                    }
                }
            }

            // Collect results
            if !date_errors.is_empty() {
                errors
                    .lock()
                    .unwrap()
                    .push(format!("Date {}: {}", date, date_errors.join("; ")));
            }
            if !date_discrepancies.is_empty() {
                discrepancies.lock().unwrap().push(format!(
                    "Date {}: {}",
                    date,
                    date_discrepancies.join("; ")
                ));
            }
            if success {
                *success_count.lock().unwrap() += 1;
            }
        });

        // Small delay between chunks to avoid rate limiting
        thread::sleep(Duration::from_millis(500));
    });

    // Extract results from Mutex
    let errors = errors.into_inner().unwrap();
    let discrepancies = discrepancies.into_inner().unwrap();
    let success_count = success_count.into_inner().unwrap();

    // Report results
    println!(
        "Verification complete: {}/{} dates passed all checks",
        success_count,
        dates.len()
    );

    if !errors.is_empty() {
        println!("\n{} errors found:", errors.len());
        for (i, error) in errors.iter().take(10).enumerate() {
            println!("  {}. {}", i + 1, error);
        }
        if errors.len() > 10 {
            println!("  ... and {} more errors", errors.len() - 10);
        }
    }

    if !discrepancies.is_empty() {
        println!("\n{} discrepancies found:", discrepancies.len());
        for (i, disc) in discrepancies.iter().take(10).enumerate() {
            println!("  {}. {}", i + 1, disc);
        }
        if discrepancies.len() > 10 {
            println!("  ... and {} more discrepancies", discrepancies.len() - 10);
        }
    }

    // Fail the test if there are errors or discrepancies
    assert!(
        errors.is_empty(),
        "Found {} errors (see output above)",
        errors.len()
    );

    assert!(
        discrepancies.is_empty(),
        "Found {} discrepancies exceeding threshold {} (see output above)",
        discrepancies.len(),
        MAX_DIFF
    );
}

fn test_nyfed(date: chrono::NaiveDate, _max_diff: u64) -> anyhow::Result<u64> {
    let source = NYFed::default();
    let (api_date, api_avg) = source.collect(date)?;
    let overnight_rates = NYFed::fetch_overnight_rates(date)?;
    let computed = sources::compute_compounded_average(api_date, &overnight_rates)?;
    Ok((api_avg as i64 - computed as i64).unsigned_abs())
}

fn test_fred(date: chrono::NaiveDate, _max_diff: u64) -> anyhow::Result<u64> {
    let source = Fred::default();
    let (api_date, api_avg) = source.collect(date)?;
    let overnight_rates = Fred::fetch_overnight_rates(date)?;
    let computed = sources::compute_compounded_average(api_date, &overnight_rates)?;
    Ok((api_avg as i64 - computed as i64).unsigned_abs())
}

fn test_ofr(date: chrono::NaiveDate, _max_diff: u64) -> anyhow::Result<u64> {
    // Compare OFR's computed value against NY Fed's API value (ground truth)
    let ofr_source = OFR::default();
    let (ofr_date, ofr_avg) = ofr_source.collect(date)?;

    // Get NY Fed's API-provided average for the same date
    let nyfed_source = NYFed::default();
    let (nyfed_date, nyfed_api_avg) = nyfed_source.collect(date)?;

    // Ensure we're comparing the same date
    if ofr_date != nyfed_date {
        anyhow::bail!("Date mismatch: OFR {} vs NYFed {}", ofr_date, nyfed_date);
    }

    Ok((ofr_avg as i64 - nyfed_api_avg as i64).unsigned_abs())
}
