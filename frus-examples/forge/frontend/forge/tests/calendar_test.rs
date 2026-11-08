//! Tests for `forge::models::calendar`.

use forge::models::calendar::*;

#[test]
fn weekdays_match_the_calendar() {
    // The design's month: September 2025 starts on a Monday.
    assert_eq!(weekday(2025, 9, 1), 1);
    assert_eq!(weekday(2024, 2, 29), 4);
    assert_eq!(weekday(2026, 11, 7), 6);
}

#[test]
fn months_have_their_length() {
    assert_eq!(days_in(2025, 9), 30);
    assert_eq!(days_in(2025, 12), 31);
    assert_eq!(days_in(2024, 2), 29);
    assert_eq!(days_in(2100, 2), 28);
}

#[test]
fn stepping_crosses_years() {
    assert_eq!(step(2025, 12, 1), (2026, 1));
    assert_eq!(step(2025, 1, -1), (2024, 12));
    assert_eq!(step(2025, 9, 0), (2025, 9));
}
