//! Dates for the calendar: weekdays, month lengths, stepping between months.

/// The weekday of a date, 0 for Sunday.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    ((y + y / 4 - y / 100 + y / 400 + OFFSETS[month as usize - 1] + day as i32).rem_euclid(7))
        as u32
}

/// The days in a month.
pub fn days_in(year: i32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The month after (`+1`) or before (`-1`) `(year, month)`.
pub fn step(year: i32, month: u32, by: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + by;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32)
}
