use aqa_publisher::sources::{Source, fred::Fred, nyfed::NYFed, ofr::OFR};
use chrono::{Days, Local};
use rayon::prelude::*;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

#[test]
fn compare_sources_over_two_years() {
    let end_date = Local::now().date_naive();
    let start_date = end_date.checked_sub_days(Days::new(365)).unwrap();

    // Collect dates every 3 days
    let mut dates = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        dates.push(current);
        current = current.checked_add_days(Days::new(3)).unwrap();
    }

    // Define all sources to test
    let sources: Vec<(String, Box<dyn Source + Sync>)> = vec![
        ("FRED".to_string(), Box::new(Fred::default())),
        ("NYFed".to_string(), Box::new(NYFed::default())),
        ("OFR".to_string(), Box::new(OFR::default())),
    ];

    println!(
        "Testing {} dates over 1 year (every 3 days) with {} sources...",
        dates.len(),
        sources.len()
    );

    // Use Mutex for thread-safe result collection
    let errors = Mutex::new(Vec::new());
    let discrepancies = Mutex::new(Vec::new());
    let success_count = Mutex::new(0);

    // Process dates in parallel with limited parallelism to avoid rate limiting
    // Split into chunks to control request rate
    dates.chunks(20).for_each(|chunk| {
        chunk.par_iter().for_each(|&date| {
            // Collect results from all sources for this date
            let mut results = Vec::new();

            for (name, source) in &sources {
                match source.collect(date) {
                    Ok((effective_date, value)) => {
                        results.push((name.clone(), effective_date, value));
                    }
                    Err(e) => {
                        errors
                            .lock()
                            .unwrap()
                            .push(format!("Date {}: {} failed: {}", date, name, e));
                    }
                }
            }

            // If we have at least one successful result, check for discrepancies
            if !results.is_empty() {
                if results.len() == sources.len() {
                    *success_count.lock().unwrap() += 1;
                }

                // Find max discrepancy between any two sources
                let mut max_diff = 0u64;
                let mut diff_info = String::new();

                for i in 0..results.len() {
                    for j in (i + 1)..results.len() {
                        let (name1, date1, val1) = &results[i];
                        let (name2, date2, val2) = &results[j];
                        let diff = (*val1 as i64 - *val2 as i64).abs() as u64;

                        if diff > max_diff {
                            max_diff = diff;
                            diff_info = format!(
                                "{} on {} vs {} on {} (diff: {})",
                                name1, date1, name2, date2, diff
                            );
                        }
                    }
                }

                // Flag if max difference is more than 10 (0.00001% in actual rate)
                // This threshold allows for minor rounding differences
                if max_diff > 10 {
                    discrepancies
                        .lock()
                        .unwrap()
                        .push(format!("Date {}: large discrepancy - {}", date, diff_info));
                }
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
        "Comparison complete: {}/{} dates had all sources succeed",
        success_count,
        dates.len()
    );

    if !errors.is_empty() {
        println!("\n{} source errors found:", errors.len());
        for (i, error) in errors.iter().take(10).enumerate() {
            println!("  {}. {}", i + 1, error);
        }
        if errors.len() > 10 {
            println!("  ... and {} more errors", errors.len() - 10);
        }
    }

    if !discrepancies.is_empty() {
        println!("\n{} large discrepancies found:", discrepancies.len());
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
        "Found {} source errors (see output above)",
        errors.len()
    );

    assert!(
        discrepancies.is_empty(),
        "Found {} large discrepancies between sources (see output above)",
        discrepancies.len()
    );
}
