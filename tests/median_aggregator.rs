use aqa_publisher::compute_validated_median;
use chrono::NaiveDate;

/// Helper to create a test date
fn test_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 10, 7).unwrap()
}

#[test]
fn test_all_three_sources_agree() {
    let query_date = test_date();
    let rate = 4_293_200u64; // 4.2932%

    let results = vec![
        ("FRED", query_date, rate),
        ("NYFed", query_date, rate),
        ("OFR", query_date, rate),
    ];

    let (median_date, median_value) = compute_validated_median(query_date, results).unwrap();
    assert_eq!(median_date, query_date);
    assert_eq!(median_value, rate);
}

#[test]
fn test_three_sources_with_median() {
    let query_date = test_date();
    // Values differ slightly but within 5 bps (50_000 scaled units = 0.05%)
    let fred_rate = 4_293_200u64; // 4.2932%
    let nyfed_rate = 4_303_200u64; // 4.3032% (10,000 units = 0.01% = 1 bp from FRED)
    let ofr_rate = 4_283_200u64; // 4.2832% (10,000 units = 0.01% = 1 bp from FRED)

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
        ("OFR", query_date, ofr_rate),
    ];

    let (median_date, median_value) = compute_validated_median(query_date, results).unwrap();
    assert_eq!(median_date, query_date);
    assert_eq!(median_value, fred_rate); // Middle value should be FRED
}

#[test]
fn test_two_sources_agree_within_tolerance() {
    let query_date = test_date();
    let fred_rate = 4_293_200u64;
    let nyfed_rate = 4_333_200u64; // Within 5 bps (40,000 units = 0.04% = 4 bps)

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
    ];

    let (median_date, median_value) = compute_validated_median(query_date, results).unwrap();
    assert_eq!(median_date, query_date);
    // With 2 sources, median is the average
    assert_eq!(median_value, (fred_rate + nyfed_rate) / 2);
}

#[test]
fn test_two_sources_differ_by_exactly_5bps() {
    let query_date = test_date();
    let fred_rate = 4_293_200u64;
    let nyfed_rate = 4_343_200u64; // Exactly 5 bps = 50_000 scaled units = 0.05%

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
    ];

    // Should succeed since we allow up to 5 bps (<=)
    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_two_sources_differ_by_more_than_5bps() {
    let query_date = test_date();
    let fred_rate = 4_293_200u64;
    let nyfed_rate = 4_343_201u64; // Just over 5 bps (50_001 units)

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("differ by more than 5 bps"));
}

#[test]
fn test_fred_fails_two_sources_agree() {
    // Simulating scenario where FRED API fails but NYFed and OFR succeed
    let query_date = test_date();
    let nyfed_rate = 4_293_200u64;
    let ofr_rate = 4_303_200u64; // Within 5 bps (10,000 units = 1 bp)

    let results = vec![
        ("NYFed", query_date, nyfed_rate),
        ("OFR", query_date, ofr_rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_fred_fails_nyfed_ofr_differ_by_more_than_5bps() {
    // FRED API errors, and NYFed and OFR differ by > 5 bps (should reject)
    let query_date = test_date();
    let nyfed_rate = 4_293_200u64;
    let ofr_rate = 4_393_200u64; // 100,000 units = 0.1% = 10 bps difference

    let results = vec![
        ("NYFed", query_date, nyfed_rate),
        ("OFR", query_date, ofr_rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("differ by more than 5 bps"));
}

#[test]
fn test_only_one_source_succeeds() {
    let query_date = test_date();
    let fred_rate = 4_293_200u64;

    let results = vec![("FRED", query_date, fred_rate)];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Need at least 2 sources"));
}

#[test]
fn test_no_sources_succeed() {
    let query_date = test_date();
    let results: Vec<(&str, NaiveDate, u64)> = vec![];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Need at least 2 sources"));
}

#[test]
fn test_three_sources_one_outlier_within_tolerance() {
    // Test that even with one outlier, if two sources agree within tolerance, we succeed
    let query_date = test_date();
    let fred_rate = 4_293_200u64;
    let nyfed_rate = 4_333_200u64; // Within 5 bps of FRED (40,000 units = 4 bps)
    let ofr_rate = 4_393_200u64; // Outlier, >5 bps from both (50 bps from FRED, 6 bps from NYFed)

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
        ("OFR", query_date, ofr_rate),
    ];

    // Should succeed because FRED and NYFed agree within tolerance
    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
    let (_, median_value) = result.unwrap();
    // Median of [4_293_200, 4_333_200, 4_393_200] is the middle value
    assert_eq!(median_value, nyfed_rate);
}

#[test]
fn test_three_sources_all_disagree() {
    // All three sources differ by more than 5 bps from each other
    let query_date = test_date();
    let fred_rate = 4_000_000u64; // 4.00%
    let nyfed_rate = 4_100_000u64; // 4.10% (100,000 units = 0.1% = 10 bps from FRED)
    let ofr_rate = 4_200_000u64; // 4.20% (10 bps from NYFed, 20 bps from FRED)

    let results = vec![
        ("FRED", query_date, fred_rate),
        ("NYFed", query_date, nyfed_rate),
        ("OFR", query_date, ofr_rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("differ by more than 5 bps"));
}

#[test]
fn test_different_dates_uses_median_date() {
    // OFR might have a different effective date (typically 1-2 days behind)
    let query_date = NaiveDate::from_ymd_opt(2025, 10, 7).unwrap();
    let date1 = NaiveDate::from_ymd_opt(2025, 10, 7).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2025, 10, 6).unwrap();
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", date1, rate),
        ("NYFed", date1, rate),
        ("OFR", date2, rate), // OFR is a day behind
    ];

    let (median_date, _) = compute_validated_median(query_date, results).unwrap();
    // Median date should be from the middle value after sorting by rate
    // Since all rates are equal, it will use the first one's date after sorting
    assert!(median_date == date1 || median_date == date2);
}

#[test]
fn test_edge_case_very_small_rates() {
    let query_date = test_date();
    let rate1 = 100u64;
    let rate2 = 200u64;

    let results = vec![
        ("Source1", query_date, rate1),
        ("Source2", query_date, rate2),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_edge_case_large_rates() {
    let query_date = test_date();
    let rate1 = 10_000_000u64; // 10%
    let rate2 = 10_040_000u64; // Within 5 bps (40,000 units = 0.04% = 4 bps)

    let results = vec![
        ("Source1", query_date, rate1),
        ("Source2", query_date, rate2),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_staleness_check_passes_within_7_days() {
    // Data from 5 days ago should pass
    let query_date = test_date();
    let data_date = NaiveDate::from_ymd_opt(2025, 10, 2).unwrap(); // 5 days before
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", data_date, rate),
        ("NYFed", data_date, rate),
        ("OFR", data_date, rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_staleness_check_passes_exactly_7_days() {
    // Data from exactly 7 days ago should pass
    let query_date = test_date();
    let data_date = NaiveDate::from_ymd_opt(2025, 9, 30).unwrap(); // Exactly 7 days before
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", data_date, rate),
        ("NYFed", data_date, rate),
        ("OFR", data_date, rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_staleness_check_fails_over_7_days() {
    // Data from 8 days ago should fail
    let query_date = test_date();
    let data_date = NaiveDate::from_ymd_opt(2025, 9, 29).unwrap(); // 8 days before
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", data_date, rate),
        ("NYFed", data_date, rate),
        ("OFR", data_date, rate),
    ];

    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("too stale"));
    assert!(err_msg.contains("8 days behind"));
}

#[test]
fn test_staleness_check_uses_median_date() {
    // Test that we use the median date for staleness check
    // FRED and NYFed are recent, OFR is 10 days old
    // Median should be recent, so should pass
    let query_date = test_date();
    let recent_date = NaiveDate::from_ymd_opt(2025, 10, 6).unwrap(); // 1 day behind
    let old_date = NaiveDate::from_ymd_opt(2025, 9, 27).unwrap(); // 10 days behind
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", recent_date, rate),
        ("NYFed", recent_date, rate),
        ("OFR", old_date, rate),
    ];

    // Should pass because median date is recent_date (2 out of 3 sources)
    let result = compute_validated_median(query_date, results);
    assert!(result.is_ok());
}

#[test]
fn test_staleness_check_median_fails() {
    // Test that staleness check fails when median is too old
    // 2 sources are 10 days old, 1 is recent
    let query_date = test_date();
    let recent_date = NaiveDate::from_ymd_opt(2025, 10, 6).unwrap(); // 1 day behind
    let old_date = NaiveDate::from_ymd_opt(2025, 9, 27).unwrap(); // 10 days behind
    let rate = 4_293_200u64;

    let results = vec![
        ("FRED", old_date, rate),
        ("NYFed", old_date, rate),
        ("OFR", recent_date, rate),
    ];

    // Should fail because median date is old_date (2 out of 3 sources)
    let result = compute_validated_median(query_date, results);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("too stale"));
}
