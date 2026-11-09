// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`DatePickerDialog`]: the full **modal date picker** — a calendar or a text field, a
//! list of years, a single date or a range — over [`DatePickerModel`], the state it keeps.
//!
//! Like every overlay here it is **controlled**. The application keeps a
//! `DatePickerModel` while the picker is open (an `Option` in its state: `Some` is open),
//! hands each [`DatePickerEvent`] the picker sends back to [`DatePickerModel::update`],
//! and acts on the [`DatePickerOutcome`] that comes back when the person confirms or
//! cancels:
//!
//! ```
//! use frus_widgets::{CalendarDate, DatePickerDialog, DatePickerEvent, DatePickerModel, DatePickerOutcome, Text};
//!
//! #[derive(Clone)]
//! enum Msg { Picker(DatePickerEvent) }
//!
//! let mut picker = Some(DatePickerModel::single(
//!     Some(CalendarDate::new(2026, 8, 17)),
//!     CalendarDate::new(2000, 1, 1),
//!     CalendarDate::new(2040, 12, 31),
//! ));
//! // In the view:
//! let _screen = DatePickerDialog::new(picker.as_ref(), Msg::Picker).body(Text::new("…"));
//! // In the update:
//! if let Some(model) = &mut picker {
//!     if let Some(DatePickerOutcome::Picked(date)) = model.update(DatePickerEvent::Confirm) {
//!         assert_eq!(date, CalendarDate::new(2026, 8, 17));
//!         picker = None;
//!     }
//! }
//! ```
//!
//! The calendar follows the reader's locale: the week starts where theirs does, the months
//! and weekdays are theirs, and a typed date is read in their order (day first in French).
//! The keyboard moves a cursor over the days (arrows, Page Up / Page Down for months, Home /
//! End for the week), Enter or Space picks the day under it.

use frus_core::{Color, Point, Rect, Scene, TextStyle};
use frus_layout::{Align, Dimension, Justify, Style};

use crate::flex::Flex;
use crate::interaction::Status;
use crate::shortcuts::{KeyStroke, ShortcutKey};
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::Widget;

// ---------------------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------------------

/// A day of the proleptic Gregorian calendar, with no time and no zone.
///
/// ```
/// use frus_widgets::CalendarDate;
/// let d = CalendarDate::new(2024, 2, 28);
/// assert_eq!(d.add_days(1), CalendarDate::new(2024, 2, 29)); // a leap year
/// assert_eq!(d.add_days(2), CalendarDate::new(2024, 3, 1));
/// assert_eq!(CalendarDate::new(2024, 1, 31).add_months(1), CalendarDate::new(2024, 2, 29));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalendarDate {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u32,
    /// The day of the month, from 1.
    pub day: u32,
}

impl CalendarDate {
    /// The date `year`-`month`-`day`, the month clamped to 1..=12 and the day to the month.
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        let month = month.clamp(1, 12);
        let day = day.clamp(1, days_in_month(year, month));
        Self { year, month, day }
    }

    /// Whether `year`-`month`-`day` is a real date.
    ///
    /// ```
    /// use frus_widgets::CalendarDate;
    /// assert!(CalendarDate::is_valid(2000, 2, 29));
    /// assert!(!CalendarDate::is_valid(1900, 2, 29));
    /// ```
    pub fn is_valid(year: i32, month: u32, day: u32) -> bool {
        (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)
    }

    /// Today, by the system clock (UTC).
    pub fn today() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self::from_days(secs.div_euclid(86_400))
    }

    /// Days since 1970-01-01.
    pub fn to_days(self) -> i64 {
        // Howard Hinnant's days_from_civil.
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        Self {
            year: (if m <= 2 { y + 1 } else { y }) as i32,
            month: m,
            day: d,
        }
    }

    /// The day of the week, 0 for Sunday to 6 for Saturday.
    ///
    /// ```
    /// use frus_widgets::CalendarDate;
    /// assert_eq!(CalendarDate::new(2026, 11, 8).weekday(), 0); // a Sunday
    /// ```
    pub fn weekday(self) -> usize {
        // 1970-01-01 was a Thursday.
        (self.to_days() + 4).rem_euclid(7) as usize
    }

    /// The date `n` days on (or back, for a negative `n`).
    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.to_days() + n)
    }

    /// The same day `n` months on, or the month's last day where it is shorter.
    pub fn add_months(self, n: i32) -> Self {
        let total = self.year * 12 + self.month as i32 - 1 + n;
        let (year, month) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        Self::new(year, month, self.day.min(days_in_month(year, month)))
    }

    /// Clamped between `first` and `last`.
    pub fn clamp_to(self, first: Self, last: Self) -> Self {
        self.max(first).min(last)
    }
}

/// Whether `year` is a leap year.
///
/// ```
/// use frus_widgets::is_leap_year;
/// assert!(is_leap_year(2024) && is_leap_year(2000));
/// assert!(!is_leap_year(1900) && !is_leap_year(2026));
/// ```
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The days in `month` (1..=12) of `year`.
///
/// ```
/// use frus_widgets::days_in_month;
/// assert_eq!(days_in_month(2024, 2), 29);
/// assert_eq!(days_in_month(2023, 2), 28);
/// ```
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

/// How many blank cells come before the 1st of `month` when the week starts on
/// `first_day_of_week` (0 = Sunday).
///
/// ```
/// use frus_widgets::month_lead_cells;
/// // 1 November 2026 is a Sunday: no blank cell from Sunday, six from Monday.
/// assert_eq!(month_lead_cells(2026, 11, 0), 0);
/// assert_eq!(month_lead_cells(2026, 11, 1), 6);
/// ```
pub fn month_lead_cells(year: i32, month: u32, first_day_of_week: usize) -> usize {
    let first = CalendarDate::new(year, month, 1).weekday();
    (first as isize - first_day_of_week as isize).rem_euclid(7) as usize
}

/// Reads a typed date in the order of `pattern` (`mm/dd/yyyy`, `dd/mm/yyyy`, or the
/// locale's own letters: `j` and `a` are read as day and year). `/`, `-` and `.` all split.
///
/// ```
/// use frus_widgets::{parse_date, CalendarDate};
/// assert_eq!(parse_date("8/17/2026", "mm/dd/yyyy"), Some(CalendarDate::new(2026, 8, 17)));
/// assert_eq!(parse_date("17.08.2026", "jj/mm/aaaa"), Some(CalendarDate::new(2026, 8, 17)));
/// assert_eq!(parse_date("2/30/2026", "mm/dd/yyyy"), None);
/// ```
pub fn parse_date(text: &str, pattern: &str) -> Option<CalendarDate> {
    let order: Vec<char> = pattern
        .split('/')
        .filter_map(|p| p.chars().next())
        .map(|c| match c.to_ascii_lowercase() {
            'j' | 't' => 'd',
            'a' | 'y' => 'y',
            other => other,
        })
        .collect();
    let parts: Vec<&str> = text.trim().split(['/', '-', '.']).map(str::trim).collect();
    if parts.len() != 3 || order.len() != 3 {
        return None;
    }
    let (mut y, mut m, mut d) = (None, None, None);
    for (part, which) in parts.iter().zip(order) {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n: i64 = part.parse().ok()?;
        match which {
            'y' => {
                if part.len() != 4 {
                    return None;
                }
                y = Some(n as i32)
            }
            'm' => m = Some(n as u32),
            'd' => d = Some(n as u32),
            _ => return None,
        }
    }
    let (y, m, d) = (y?, m?, d?);
    CalendarDate::is_valid(y, m, d).then(|| CalendarDate::new(y, m, d))
}

/// Writes a date in the order of `pattern`, zero-padded.
///
/// ```
/// use frus_widgets::{format_date, CalendarDate};
/// assert_eq!(format_date(CalendarDate::new(2026, 8, 7), "mm/dd/yyyy"), "08/07/2026");
/// assert_eq!(format_date(CalendarDate::new(2026, 8, 7), "jj/mm/aaaa"), "07/08/2026");
/// ```
pub fn format_date(date: CalendarDate, pattern: &str) -> String {
    pattern
        .split('/')
        .map(|p| match p.chars().next().map(|c| c.to_ascii_lowercase()) {
            Some('y' | 'a') => format!("{:04}", date.year),
            Some('m') => format!("{:02}", date.month),
            _ => format!("{:02}", date.day),
        })
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------

/// Whether the picker shows its calendar or a text field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DatePickerMode {
    /// The month calendar.
    #[default]
    Calendar,
    /// A text field (two for a range).
    Input,
}

/// Something the person did in a [`DatePickerDialog`]; hand it to
/// [`DatePickerModel::update`].
#[derive(Clone, Debug, PartialEq)]
pub enum DatePickerEvent {
    /// A day was tapped.
    Select(CalendarDate),
    /// The month shown moved by this many months.
    ShiftMonth(i32),
    /// The list of years was opened or closed.
    ToggleYears,
    /// A year was picked from the list.
    PickYear(i32),
    /// The calendar / text field switch.
    ToggleMode,
    /// The keyboard cursor moved by this many days.
    MoveCursor(i64),
    /// The keyboard cursor moved by this many months.
    MoveCursorMonths(i32),
    /// The keyboard cursor went to the start (`false`) or the end (`true`) of its week.
    CursorToWeekEdge(bool),
    /// The day under the keyboard cursor was picked.
    SelectCursor,
    /// The text field (the start's, for a range) now reads this.
    Input(String),
    /// A range's end field now reads this.
    InputEnd(String),
    /// OK, or Save.
    Confirm,
    /// Cancel, the scrim, or Escape.
    Cancel,
}

/// How a picker ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerOutcome {
    /// A single date was confirmed.
    Picked(CalendarDate),
    /// A range was confirmed: start, then end (the same day for a one-day range).
    PickedRange(CalendarDate, CalendarDate),
    /// The picker was dismissed.
    Cancelled,
}

/// What an open [`DatePickerDialog`] keeps between frames: the mode, the month shown, the
/// selection, what has been typed.
#[derive(Clone, Debug, PartialEq)]
pub struct DatePickerModel {
    /// Calendar or text field.
    pub mode: DatePickerMode,
    /// Picking a range rather than one date.
    pub range: bool,
    /// The earliest date that may be picked.
    pub first: CalendarDate,
    /// The latest date that may be picked.
    pub last: CalendarDate,
    /// Today, outlined in the calendar.
    pub today: CalendarDate,
    /// The month shown: its year and month.
    pub shown: (i32, u32),
    /// Is the list of years open?
    pub years: bool,
    /// The date picked so far (a range's start).
    pub selected: Option<CalendarDate>,
    /// A range's end.
    pub end: Option<CalendarDate>,
    /// The keyboard cursor.
    pub cursor: CalendarDate,
    /// Whether the cursor has been moved by the keyboard, and so is drawn.
    pub cursor_shown: bool,
    /// The text field (a range's start field).
    pub input: String,
    /// A range's end field.
    pub input_end: String,
    /// What is wrong with what was typed, once confirmed.
    pub error: Option<String>,
    /// A range's end field's error.
    pub error_end: Option<String>,
}

impl DatePickerModel {
    /// A picker for one date, opened on `initial` (else today), allowing `first..=last`.
    pub fn single(initial: Option<CalendarDate>, first: CalendarDate, last: CalendarDate) -> Self {
        let (first, last) = (first.min(last), first.max(last));
        let today = CalendarDate::today();
        let open = initial.unwrap_or(today).clamp_to(first, last);
        let pattern = crate::localizations::of().date_input_pattern().to_string();
        Self {
            mode: DatePickerMode::Calendar,
            range: false,
            first,
            last,
            today,
            shown: (open.year, open.month),
            years: false,
            selected: initial.map(|d| d.clamp_to(first, last)),
            end: None,
            cursor: open,
            cursor_shown: false,
            input: initial
                .map(|d| format_date(d, &pattern))
                .unwrap_or_default(),
            input_end: String::new(),
            error: None,
            error_end: None,
        }
    }

    /// A picker for a range, opened on `start..=end` if given.
    pub fn range(
        start: Option<CalendarDate>,
        end: Option<CalendarDate>,
        first: CalendarDate,
        last: CalendarDate,
    ) -> Self {
        let mut m = Self::single(start, first, last);
        m.range = true;
        m.end = end.map(|d| d.clamp_to(m.first, m.last));
        let pattern = crate::localizations::of().date_input_pattern().to_string();
        m.input_end = m.end.map(|d| format_date(d, &pattern)).unwrap_or_default();
        m
    }

    /// Opens in the text field rather than the calendar.
    pub fn input_mode(mut self) -> Self {
        self.mode = DatePickerMode::Input;
        self
    }

    /// Says what today is, for the outline (the system's clock unless told).
    pub fn with_today(mut self, today: CalendarDate) -> Self {
        self.today = today;
        self
    }

    /// Whether `date` may be picked.
    pub fn allows(&self, date: CalendarDate) -> bool {
        date >= self.first && date <= self.last
    }

    fn show(&mut self, date: CalendarDate) {
        self.shown = (date.year, date.month);
    }

    fn pick(&mut self, date: CalendarDate) {
        if !self.allows(date) {
            return;
        }
        self.cursor = date;
        let pattern = crate::localizations::of().date_input_pattern().to_string();
        if !self.range {
            self.selected = Some(date);
            self.input = format_date(date, &pattern);
            return;
        }
        match (self.selected, self.end) {
            (Some(start), None) if date >= start => {
                self.end = Some(date);
                self.input_end = format_date(date, &pattern);
            }
            _ => {
                self.selected = Some(date);
                self.end = None;
                self.input = format_date(date, &pattern);
                self.input_end.clear();
            }
        }
    }

    fn read(&self, text: &str) -> Result<CalendarDate, String> {
        let l10n = crate::localizations::of();
        match parse_date(text, l10n.date_input_pattern()) {
            None => Err(l10n.invalid_date_label().to_string()),
            Some(d) if !self.allows(d) => Err(l10n.date_out_of_range_label().to_string()),
            Some(d) => Ok(d),
        }
    }

    /// Applies one event; returns how the picker ended, if it did.
    ///
    /// ```
    /// use frus_widgets::{CalendarDate, DatePickerEvent, DatePickerModel, DatePickerOutcome};
    /// let mut m = DatePickerModel::range(None, None, CalendarDate::new(2026, 1, 1), CalendarDate::new(2026, 12, 31));
    /// m.update(DatePickerEvent::Select(CalendarDate::new(2026, 3, 4)));
    /// m.update(DatePickerEvent::Select(CalendarDate::new(2026, 3, 9)));
    /// assert_eq!(
    ///     m.update(DatePickerEvent::Confirm),
    ///     Some(DatePickerOutcome::PickedRange(CalendarDate::new(2026, 3, 4), CalendarDate::new(2026, 3, 9)))
    /// );
    /// ```
    pub fn update(&mut self, event: DatePickerEvent) -> Option<DatePickerOutcome> {
        match event {
            DatePickerEvent::Select(date) => self.pick(date),
            DatePickerEvent::ShiftMonth(n) => {
                let (y, m) = self.shown;
                let to = CalendarDate::new(y, m, 1).add_months(n);
                let lo = CalendarDate::new(self.first.year, self.first.month, 1);
                let hi = CalendarDate::new(self.last.year, self.last.month, 1);
                self.show(to.clamp_to(lo, hi));
            }
            DatePickerEvent::ToggleYears => self.years = !self.years,
            DatePickerEvent::PickYear(year) => {
                let year = year.clamp(self.first.year, self.last.year);
                let to = CalendarDate::new(year, self.shown.1, 1);
                let lo = CalendarDate::new(self.first.year, self.first.month, 1);
                let hi = CalendarDate::new(self.last.year, self.last.month, 1);
                self.show(to.clamp_to(lo, hi));
                self.cursor = CalendarDate::new(year, self.cursor.month, self.cursor.day)
                    .clamp_to(self.first, self.last);
                self.years = false;
            }
            DatePickerEvent::ToggleMode => {
                self.mode = match self.mode {
                    DatePickerMode::Calendar => DatePickerMode::Input,
                    DatePickerMode::Input => {
                        // What was typed, if it reads, is what the calendar shows.
                        if let Ok(d) = self.read(&self.input.clone()) {
                            self.selected = Some(d);
                            self.show(d);
                        }
                        if self.range {
                            if let Ok(d) = self.read(&self.input_end.clone()) {
                                self.end = Some(d);
                            }
                        }
                        DatePickerMode::Calendar
                    }
                };
                self.error = None;
                self.error_end = None;
                self.years = false;
            }
            DatePickerEvent::MoveCursor(n) => {
                self.cursor = self.cursor.add_days(n).clamp_to(self.first, self.last);
                self.cursor_shown = true;
                self.show(self.cursor);
            }
            DatePickerEvent::MoveCursorMonths(n) => {
                self.cursor = self.cursor.add_months(n).clamp_to(self.first, self.last);
                self.cursor_shown = true;
                self.show(self.cursor);
            }
            DatePickerEvent::CursorToWeekEdge(end) => {
                let first = crate::localizations::of().first_day_of_week_index() % 7;
                let into = (self.cursor.weekday() as i64 - first as i64).rem_euclid(7);
                let n = if end { 6 - into } else { -into };
                self.cursor = self.cursor.add_days(n).clamp_to(self.first, self.last);
                self.cursor_shown = true;
                self.show(self.cursor);
            }
            DatePickerEvent::SelectCursor => {
                let c = self.cursor;
                self.pick(c);
            }
            DatePickerEvent::Input(text) => {
                self.input = text;
                self.error = None;
            }
            DatePickerEvent::InputEnd(text) => {
                self.input_end = text;
                self.error_end = None;
            }
            DatePickerEvent::Confirm => return self.confirm(),
            DatePickerEvent::Cancel => return Some(DatePickerOutcome::Cancelled),
        }
        None
    }

    fn confirm(&mut self) -> Option<DatePickerOutcome> {
        if self.mode == DatePickerMode::Input {
            let start = self.read(&self.input.clone());
            if !self.range {
                return match start {
                    Ok(d) => {
                        self.selected = Some(d);
                        Some(DatePickerOutcome::Picked(d))
                    }
                    Err(e) => {
                        self.error = Some(e);
                        None
                    }
                };
            }
            let end = self.read(&self.input_end.clone());
            let end = match (&start, end) {
                (Ok(s), Ok(e)) if e < *s => Err(crate::localizations::of()
                    .date_out_of_range_label()
                    .to_string()),
                (_, end) => end,
            };
            return match (start, end) {
                (Ok(s), Ok(e)) => Some(DatePickerOutcome::PickedRange(s, e)),
                (s, e) => {
                    self.error = s.err();
                    self.error_end = e.err();
                    None
                }
            };
        }
        match (self.range, self.selected, self.end) {
            (false, Some(d), _) => Some(DatePickerOutcome::Picked(d)),
            (true, Some(s), e) => Some(DatePickerOutcome::PickedRange(s, e.unwrap_or(s))),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// The look
// ---------------------------------------------------------------------------------------

/// Theme defaults for [`DatePickerDialog`]; each `None` falls through to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DatePickerDialogTheme {
    /// The dialog's surface. Unset, `surface_container_high`.
    pub background: Option<Color>,
    /// The headline's type. Unset, `headline_large` (`headline_small` for a range).
    pub headline_style: Option<TextStyle>,
    /// The small heading's type. Unset, `label_large`.
    pub help_style: Option<TextStyle>,
    /// A picked day's fill. Unset, `primary`.
    pub selected_day_background: Option<Color>,
    /// A picked day's figure. Unset, `on_primary`.
    pub selected_day_foreground: Option<Color>,
    /// Today's outline and figure. Unset, `primary`.
    pub today_color: Option<Color>,
    /// The band between a range's ends. Unset, `secondary_container`.
    pub range_background: Option<Color>,
}

/// A day cell's side.
const DAY: f32 = 40.0;
/// A day column's width: the dialog's 360 less 12 at each side, over seven.
const COLUMN: f32 = 48.0;
/// A day row's height.
const ROW: f32 = 42.0;
/// The dialog's width in calendar mode.
pub const DATE_PICKER_WIDTH: f32 = 360.0;

/// How a day cell is to be drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DayLook {
    day: u32,
    selected: bool,
    today: bool,
    disabled: bool,
    cursor: bool,
    /// The range band: (from the cell's centre leftward, rightward).
    band: (bool, bool),
}

struct DayCell<Msg> {
    look: DayLook,
    message: Option<Msg>,
}

impl<Msg: Clone> Widget<Msg> for DayCell<Msg> {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(COLUMN),
            height: Dimension::Length(ROW),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let l = self.look;
        if l.day == 0 {
            return;
        }
        let t = theme.widgets.date_picker_dialog;
        let o = status.opacity;
        let cx = bounds.x + bounds.width * 0.5;
        let cy = bounds.y + bounds.height * 0.5;
        let circle = Rect::new(cx - DAY * 0.5, cy - DAY * 0.5, DAY, DAY);
        let band_color = t
            .range_background
            .unwrap_or(theme.scheme.secondary_container)
            .fade(o);
        let (left, right) = l.band;
        if left {
            scene.fill_rect(
                Rect::new(bounds.x, circle.y, cx - bounds.x, DAY),
                band_color,
            );
        }
        if right {
            scene.fill_rect(
                Rect::new(cx, circle.y, bounds.x + bounds.width - cx, DAY),
                band_color,
            );
        }
        let primary = t.today_color.unwrap_or(theme.scheme.primary);
        let (fill, fg) = if l.selected {
            (
                Some(t.selected_day_background.unwrap_or(theme.scheme.primary)),
                t.selected_day_foreground.unwrap_or(theme.scheme.on_primary),
            )
        } else if l.disabled {
            (None, theme.scheme.on_surface.fade(0.38))
        } else if l.today {
            (None, primary)
        } else {
            (None, theme.scheme.on_surface)
        };
        if let Some(fill) = fill {
            scene.draw_rect(circle, fill.fade(o), DAY * 0.5, 0.0, Color::TRANSPARENT);
        } else if !l.disabled {
            let wash = theme.state_layer(Color::TRANSPARENT, theme.scheme.on_surface, &status);
            if wash.a > 0.0 {
                scene.draw_rect(circle, wash.fade(o), DAY * 0.5, 0.0, Color::TRANSPARENT);
            }
        }
        if l.today && !l.selected {
            scene.draw_rect(circle, Color::TRANSPARENT, DAY * 0.5, 1.0, primary.fade(o));
        }
        if l.cursor {
            let ring = Rect::new(circle.x - 2.0, circle.y - 2.0, DAY + 4.0, DAY + 4.0);
            scene.draw_rect(
                ring,
                Color::TRANSPARENT,
                DAY * 0.5 + 2.0,
                2.0,
                theme.scheme.secondary.fade(o),
            );
        }
        let label = l.day.to_string();
        let style = theme.text.body_large.resolved();
        let w = frus_text::measure_resolved(&label, &style).width;
        scene.text(
            Point::new(cx - w * 0.5, cy - style.line_height() * 0.5),
            label,
            &style,
            fg.fade(o),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        self.message.clone()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        if self.look.day == 0 {
            return None;
        }
        let mut s = frus_core::SemanticsProperties::new(frus_core::Role::Button)
            .label(self.look.day.to_string());
        if self.look.selected {
            s = s.toggled(true);
        }
        if self.look.disabled {
            s = s.disabled(true);
        }
        Some(s)
    }
}

/// A year in the list of years.
struct YearCell<Msg> {
    year: i32,
    selected: bool,
    current: bool,
    message: Option<Msg>,
}

impl<Msg: Clone> Widget<Msg> for YearCell<Msg> {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(104.0),
            height: Dimension::Length(52.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = theme.widgets.date_picker_dialog;
        let o = status.opacity;
        let pill = Rect::new(
            bounds.x + (bounds.width - 72.0) * 0.5,
            bounds.y + (bounds.height - 36.0) * 0.5,
            72.0,
            36.0,
        );
        let primary = t.today_color.unwrap_or(theme.scheme.primary);
        let fg = if self.selected {
            scene.draw_rect(
                pill,
                t.selected_day_background
                    .unwrap_or(theme.scheme.primary)
                    .fade(o),
                18.0,
                0.0,
                Color::TRANSPARENT,
            );
            t.selected_day_foreground.unwrap_or(theme.scheme.on_primary)
        } else {
            let wash = theme.state_layer(Color::TRANSPARENT, theme.scheme.on_surface, &status);
            if wash.a > 0.0 {
                scene.draw_rect(pill, wash.fade(o), 18.0, 0.0, Color::TRANSPARENT);
            }
            if self.current {
                scene.draw_rect(pill, Color::TRANSPARENT, 18.0, 1.0, primary.fade(o));
                primary
            } else if self.message.is_none() {
                theme.scheme.on_surface.fade(0.38)
            } else {
                theme.scheme.on_surface_variant
            }
        };
        let label = self.year.to_string();
        let style = theme.text.body_large.resolved();
        let w = frus_text::measure_resolved(&label, &style).width;
        scene.text(
            Point::new(
                pill.x + (pill.width - w) * 0.5,
                pill.y + (pill.height - style.line_height()) * 0.5,
            ),
            label,
            &style,
            fg.fade(o),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        self.message.clone()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut s = frus_core::SemanticsProperties::new(frus_core::Role::Button)
            .label(self.year.to_string());
        if self.selected {
            s = s.toggled(true);
        }
        Some(s)
    }
}

/// The days of the shown month as cells, in rows of seven starting where the reader's week
/// starts. Pure, so the grid is tested on its own.
fn month_cells(model: &DatePickerModel) -> Vec<DayLook> {
    let (year, month) = model.shown;
    let first = crate::localizations::of().first_day_of_week_index() % 7;
    let lead = month_lead_cells(year, month, first);
    let mut cells = vec![
        DayLook {
            day: 0,
            selected: false,
            today: false,
            disabled: true,
            cursor: false,
            band: (false, false),
        };
        lead
    ];
    let (start, end) = (model.selected, if model.range { model.end } else { None });
    for day in 1..=days_in_month(year, month) {
        let date = CalendarDate::new(year, month, day);
        let selected = Some(date) == start || Some(date) == end;
        let band = match (start, end) {
            (Some(s), Some(e)) if s < e => (date > s && date <= e, date >= s && date < e),
            _ => (false, false),
        };
        cells.push(DayLook {
            day,
            selected,
            today: date == model.today,
            disabled: !model.allows(date),
            cursor: model.cursor_shown && date == model.cursor,
            band,
        });
    }
    cells
}

/// The full modal date picker: a heading, the date picked in big type, a switch between a
/// calendar and a text field, and Cancel / OK.
///
/// ```
/// use frus_widgets::{CalendarDate, DatePickerDialog, DatePickerModel, Text};
/// let model = DatePickerModel::single(None, CalendarDate::new(2020, 1, 1), CalendarDate::new(2030, 12, 31));
/// let _picker: DatePickerDialog<()> = DatePickerDialog::new(Some(&model), |_| ());
/// ```
///
/// [`body`](Self::body) puts it in a modal [`Dialog`](crate::Dialog) over a screen; on its
/// own it is the panel, for a picker docked in a page. **Themed** through
/// [`DatePickerDialogTheme`] (`theme.widgets.date_picker_dialog`).
pub struct DatePickerDialog<Msg = crate::callback::Callback> {
    open: bool,
    children: Vec<Box<dyn Widget<Msg>>>,
    dismiss: Option<Msg>,
    width: f32,
}

impl<Msg: Clone + 'static> DatePickerDialog<Msg> {
    /// The picker for `model` (`None` builds a closed picker), sending what the person does
    /// through `on_event`.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        model: Option<&DatePickerModel>,
        on_event: impl Fn(DatePickerEvent) -> R + 'static,
    ) -> Self {
        let on: std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg> =
            std::rc::Rc::new(crate::callback::handler1(on_event));
        let Some(model) = model else {
            return Self {
                open: false,
                children: Vec::new(),
                dismiss: None,
                width: DATE_PICKER_WIDTH,
            };
        };
        let dismiss = Some(on(DatePickerEvent::Cancel));
        let panel = crate::themebuilder::ThemeBuilder::boxed({
            let model = model.clone();
            move |theme| Box::new(panel(&model, &on, theme))
        });
        Self {
            open: true,
            children: vec![Box::new(panel)],
            dismiss,
            width: DATE_PICKER_WIDTH,
        }
    }

    /// The picker as a modal dialog over `body`.
    ///
    /// ```
    /// use frus_widgets::{CalendarDate, DatePickerDialog, DatePickerModel, Text};
    /// let model = DatePickerModel::single(None, CalendarDate::new(2020, 1, 1), CalendarDate::new(2030, 12, 31));
    /// let _screen = DatePickerDialog::<()>::new(Some(&model), |_| ()).body(Text::new("Page"));
    /// ```
    pub fn body(self, body: impl Widget<Msg> + 'static) -> crate::Dialog<Msg> {
        let mut dialog = crate::Dialog::new(self.open)
            .min_width(self.width)
            .max_width(self.width);
        if let Some(dismiss) = self.dismiss.clone() {
            dialog = dialog.on_dismiss(dismiss);
        }
        if let Some(panel) = self.children.into_iter().next() {
            dialog = dialog.child(Boxed(panel));
        }
        dialog.body(body)
    }
}

/// A boxed widget as a widget, with the child's box.
struct Boxed<Msg>(Box<dyn Widget<Msg>>);

impl<Msg: 'static> Widget<Msg> for Boxed<Msg> {
    fn style(&self) -> Style {
        self.0.style()
    }
    fn style_themed(&self, theme: &Theme) -> Style {
        self.0.style_themed(theme)
    }
    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        std::slice::from_ref(&self.0)
    }
    fn paint(&self, _: Rect, _: Status, _: &Theme, _: &mut Scene) {}
    fn on_click(&self) -> Option<Msg> {
        None
    }
}

/// The panel: heading, headline, body (calendar, years or fields), actions.
fn panel<Msg: Clone + 'static>(
    model: &DatePickerModel,
    on: &std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg>,
    theme: &Theme,
) -> Flex<Msg> {
    let t = theme.widgets.date_picker_dialog;
    let l10n = crate::localizations::of();
    let input = model.mode == DatePickerMode::Input;
    let help = if input {
        l10n.date_input_title()
    } else if model.range {
        l10n.date_range_picker_title()
    } else {
        l10n.date_picker_title()
    };
    let help_style = t.help_style.unwrap_or(theme.text.label_large);
    let mut headline_style = t.headline_style.unwrap_or(if model.range {
        theme.text.headline_small
    } else {
        theme.text.headline_large
    });
    if headline_style.color.is_none() {
        headline_style.color = Some(theme.scheme.on_surface_variant);
    }
    let say = |d: CalendarDate| l10n.medium_date(d.weekday(), d.month, d.day);
    // A range's ends without their weekdays, in the order the locale writes a date:
    // "Nov 3", "3 nov.".
    let month_first = l10n.date_input_pattern().starts_with('m');
    let short = |d: CalendarDate| {
        let m = l10n.short_months()[(d.month - 1) as usize];
        if month_first {
            format!("{m} {}", d.day)
        } else {
            format!("{} {m}", d.day)
        }
    };
    let headline = match (model.range, model.selected, model.end) {
        (false, Some(d), _) => say(d),
        (false, None, _) => help.to_string(),
        (true, s, e) => format!(
            "{} – {}",
            s.map(short)
                .unwrap_or_else(|| l10n.start_label().to_string()),
            e.map(short).unwrap_or_else(|| l10n.end_label().to_string())
        ),
    };
    let toggle = crate::IconButton::new(if input {
        crate::icons::Icons::CALENDAR_TODAY
    } else {
        crate::icons::Icons::EDIT
    })
    .label(if input {
        l10n.calendar_mode_label()
    } else {
        l10n.input_mode_label()
    })
    .on_press(on(DatePickerEvent::ToggleMode));
    let header = Flex::column()
        .padding_each(16.0, 12.0, 12.0, 24.0)
        .gap(if model.range { 8.0 } else { 36.0 })
        .child(Text::styled(help, help_style).color(theme.scheme.on_surface_variant))
        .child(
            Flex::row()
                .align(Align::Center)
                .child(
                    Flex::row()
                        .flex(1.0)
                        .child(Text::styled(headline, headline_style)),
                )
                .child(toggle),
        );
    let mut col = Flex::column()
        .width(DATE_PICKER_WIDTH)
        .child(header)
        .child(crate::Divider::new());
    col = if input {
        col.child(input_body(model, on, theme))
    } else if model.years {
        col.child(controls(model, on, theme))
            .child(year_grid(model, on))
    } else {
        col.child(controls(model, on, theme))
            .child(calendar(model, on, theme))
    };
    let ok = crate::Button::new(l10n.ok_button_label())
        .variant(crate::button::Variant::Text)
        .on_press(on(DatePickerEvent::Confirm));
    let cancel = crate::Button::new(l10n.cancel_button_label())
        .variant(crate::button::Variant::Text)
        .on_press(on(DatePickerEvent::Cancel));
    col.child(
        Flex::row()
            .justify(Justify::End)
            .gap(8.0)
            .padding_each(8.0, 12.0, 12.0, 12.0)
            .child(cancel)
            .child(ok),
    )
}

/// The month and year button and the two arrows.
fn controls<Msg: Clone + 'static>(
    model: &DatePickerModel,
    on: &std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg>,
    theme: &Theme,
) -> Flex<Msg> {
    let l10n = crate::localizations::of();
    let (y, m) = model.shown;
    let label = format!("{} {}", l10n.months()[(m - 1) as usize], y);
    let years = crate::Button::new(label)
        .variant(crate::button::Variant::Text)
        .icon(crate::Icon::new(if model.years {
            crate::icons::Icons::ARROW_DROP_UP
        } else {
            crate::icons::Icons::ARROW_DROP_DOWN
        }))
        .icon_alignment(crate::IconAlignment::End)
        .on_press(on(DatePickerEvent::ToggleYears));
    let shown = CalendarDate::new(y, m, 1);
    let can_back = shown > CalendarDate::new(model.first.year, model.first.month, 1);
    let can_fwd = shown < CalendarDate::new(model.last.year, model.last.month, 1);
    let mut back = crate::IconButton::new(crate::icons::Icons::CHEVRON_LEFT)
        .label(l10n.previous_month_label());
    if can_back {
        back = back.on_press(on(DatePickerEvent::ShiftMonth(-1)));
    }
    let mut fwd =
        crate::IconButton::new(crate::icons::Icons::CHEVRON_RIGHT).label(l10n.next_month_label());
    if can_fwd {
        fwd = fwd.on_press(on(DatePickerEvent::ShiftMonth(1)));
    }
    let _ = theme;
    let mut row = Flex::row()
        .align(Align::Center)
        .padding_each(4.0, 12.0, 0.0, 12.0)
        .child(years)
        .child(Flex::row().flex(1.0));
    if !model.years {
        row = row.child(back).child(fwd);
    }
    row
}

/// The weekday initials and the days, with the keyboard bound to the cursor.
fn calendar<Msg: Clone + 'static>(
    model: &DatePickerModel,
    on: &std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg>,
    theme: &Theme,
) -> Box<dyn Widget<Msg>> {
    let l10n = crate::localizations::of();
    let first = l10n.first_day_of_week_index() % 7;
    let initials = l10n.narrow_weekdays();
    let mut heads = Flex::row();
    for column in 0..7 {
        heads = heads.child(
            crate::Container::new()
                .width(COLUMN)
                .height(ROW)
                .alignment(frus_core::Alignment::CENTER)
                .child(
                    Text::styled(initials[(first + column) % 7], theme.text.body_large)
                        .color(theme.scheme.on_surface),
                ),
        );
    }
    let mut grid = Flex::column().child(heads);
    let cells = month_cells(model);
    let (year, month) = model.shown;
    for week in cells.chunks(7) {
        let mut row = Flex::row();
        for look in week {
            let date = CalendarDate::new(year, month, look.day.max(1));
            let message =
                (look.day > 0 && !look.disabled).then(|| on(DatePickerEvent::Select(date)));
            row = row.child(DayCell {
                look: *look,
                message,
            });
        }
        grid = grid.child(row);
    }
    // Six rows always, so the dialog keeps its height from month to month.
    for _ in cells.len().div_ceil(7)..6 {
        grid = grid.child(crate::Container::<Msg>::new().height(ROW));
    }
    let rtl = theme.direction.is_rtl();
    let (back, fwd) = if rtl { (1, -1) } else { (-1, 1) };
    let keys =
        crate::CallbackShortcuts::new(crate::Focus::new(grid.padding_each(0.0, 12.0, 0.0, 12.0)))
            .bind(
                KeyStroke::new(ShortcutKey::Left),
                on(DatePickerEvent::MoveCursor(back)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Right),
                on(DatePickerEvent::MoveCursor(fwd)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Up),
                on(DatePickerEvent::MoveCursor(-7)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Down),
                on(DatePickerEvent::MoveCursor(7)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::PageUp),
                on(DatePickerEvent::MoveCursorMonths(-1)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::PageDown),
                on(DatePickerEvent::MoveCursorMonths(1)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Home),
                on(DatePickerEvent::CursorToWeekEdge(false)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::End),
                on(DatePickerEvent::CursorToWeekEdge(true)),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Enter),
                on(DatePickerEvent::SelectCursor),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Space),
                on(DatePickerEvent::SelectCursor),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Escape),
                on(DatePickerEvent::Cancel),
            );
    Box::new(keys)
}

/// The list of years, three to a row, the shown one picked.
fn year_grid<Msg: Clone + 'static>(
    model: &DatePickerModel,
    on: &std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg>,
) -> Box<dyn Widget<Msg>> {
    let years: Vec<i32> = (model.first.year..=model.last.year).collect();
    let mut col = Flex::column().padding_each(0.0, 24.0, 0.0, 24.0);
    for row_years in years.chunks(3) {
        let mut row = Flex::row();
        for &year in row_years {
            row = row.child(YearCell {
                year,
                selected: year == model.shown.0,
                current: year == model.today.year,
                message: Some(on(DatePickerEvent::PickYear(year))),
            });
        }
        col = col.child(row);
    }
    // Opened on the shown year: it is in the first rows a reader sees.
    let row = years.iter().position(|y| *y == model.shown.0).unwrap_or(0) / 3;
    let _ = row;
    let list = crate::SingleChildScrollView::new()
        .height(7.0 * ROW)
        .child(col);
    Box::new(list)
}

/// The text field, or a range's two.
fn input_body<Msg: Clone + 'static>(
    model: &DatePickerModel,
    on: &std::rc::Rc<dyn Fn(DatePickerEvent) -> Msg>,
    theme: &Theme,
) -> Flex<Msg> {
    let l10n = crate::localizations::of();
    let _ = theme;
    let field =
        |value: &str, label: &str, error: &Option<String>, make: fn(String) -> DatePickerEvent| {
            let on = on.clone();
            let mut f = crate::TextField::new(value.to_string())
                .outlined()
                .label(label.to_string())
                .helper(l10n.date_input_pattern().to_string())
                .width(DATE_PICKER_WIDTH - 48.0)
                .on_input(move |text| on(make(text)));
            if let Some(e) = error {
                f = f.error(e.clone());
            }
            f
        };
    let mut col = Flex::column().gap(16.0).padding_each(16.0, 24.0, 8.0, 24.0);
    if model.range {
        col = col
            .child(field(
                &model.input,
                l10n.start_label(),
                &model.error,
                DatePickerEvent::Input,
            ))
            .child(field(
                &model.input_end,
                l10n.end_label(),
                &model.error_end,
                DatePickerEvent::InputEnd,
            ));
    } else {
        col = col.child(field(
            &model.input,
            l10n.date_input_label(),
            &model.error,
            DatePickerEvent::Input,
        ));
    }
    col
}

impl<Msg: Clone + 'static> Widget<Msg> for DatePickerDialog<Msg> {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(self.width),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        if !self.open {
            return;
        }
        let bg = theme
            .widgets
            .date_picker_dialog
            .background
            .unwrap_or(theme.scheme.surface_container_high);
        scene.draw_rect(
            bounds,
            bg.fade(status.opacity),
            28.0,
            0.0,
            Color::TRANSPARENT,
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> CalendarDate {
        CalendarDate::new(y, m, day)
    }

    #[test]
    fn days_round_trip_and_weekdays_are_right() {
        for days in [-1_000_000i64, -1, 0, 1, 365, 10_957, 20_000, 2_000_000] {
            assert_eq!(CalendarDate::from_days(days).to_days(), days);
        }
        assert_eq!(d(1970, 1, 1).to_days(), 0);
        assert_eq!(d(2000, 3, 1).weekday(), 3); // a Wednesday
        assert_eq!(d(2026, 11, 8).weekday(), 0); // a Sunday
        assert_eq!(d(1900, 1, 1).weekday(), 1); // a Monday
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2100));
        assert_eq!(d(2024, 2, 28).add_days(1), d(2024, 2, 29));
        assert_eq!(d(2023, 2, 28).add_days(1), d(2023, 3, 1));
        assert_eq!(d(2100, 2, 28).add_days(1), d(2100, 3, 1));
        // A month on from the 31st of January is the end of February.
        assert_eq!(d(2024, 1, 31).add_months(1), d(2024, 2, 29));
        assert_eq!(d(2023, 1, 31).add_months(1), d(2023, 2, 28));
        assert_eq!(d(2024, 3, 15).add_months(-3), d(2023, 12, 15));
        assert!(!CalendarDate::is_valid(2023, 2, 29));
    }

    #[test]
    fn the_week_starts_where_the_reader_s_does() {
        // 1 March 2026 is a Sunday.
        assert_eq!(month_lead_cells(2026, 3, 0), 0);
        assert_eq!(month_lead_cells(2026, 3, 1), 6);
        assert_eq!(month_lead_cells(2026, 3, 6), 1); // weeks from Saturday
                                                     // And the grid built for a Monday-first reader puts the 1st in the last column.
        struct Fr;
        impl crate::Localizations for Fr {
            fn first_day_of_week_index(&self) -> usize {
                1
            }
        }
        crate::localizations::scope(std::rc::Rc::new(Fr), || {
            let model =
                DatePickerModel::single(Some(d(2026, 3, 10)), d(2026, 1, 1), d(2026, 12, 31));
            let cells = month_cells(&model);
            assert_eq!(cells.iter().position(|c| c.day == 1), Some(6));
            // Home goes back to Monday the 9th, End on to Sunday the 15th.
            let mut m = model.clone();
            m.update(DatePickerEvent::CursorToWeekEdge(false));
            assert_eq!(m.cursor, d(2026, 3, 9));
            m.update(DatePickerEvent::CursorToWeekEdge(true));
            assert_eq!(m.cursor, d(2026, 3, 15));
        });
    }

    #[test]
    fn a_range_is_picked_start_then_end() {
        let mut m = DatePickerModel::range(None, None, d(2026, 1, 1), d(2026, 12, 31));
        m.update(DatePickerEvent::Select(d(2026, 5, 10)));
        assert_eq!((m.selected, m.end), (Some(d(2026, 5, 10)), None));
        // An earlier day starts again.
        m.update(DatePickerEvent::Select(d(2026, 5, 2)));
        assert_eq!((m.selected, m.end), (Some(d(2026, 5, 2)), None));
        m.update(DatePickerEvent::Select(d(2026, 5, 20)));
        assert_eq!(
            (m.selected, m.end),
            (Some(d(2026, 5, 2)), Some(d(2026, 5, 20)))
        );
        // A third tap starts a new range.
        m.update(DatePickerEvent::Select(d(2026, 6, 1)));
        assert_eq!((m.selected, m.end), (Some(d(2026, 6, 1)), None));
        m.update(DatePickerEvent::Select(d(2026, 6, 3)));
        // The band covers the days between, and half of each end.
        m.shown = (2026, 6);
        let cells = month_cells(&m);
        let day = |n: u32| *cells.iter().find(|c| c.day == n).unwrap();
        assert_eq!(day(1).band, (false, true));
        assert_eq!(day(2).band, (true, true));
        assert_eq!(day(3).band, (true, false));
        assert!(day(1).selected && day(3).selected && !day(2).selected);
        assert_eq!(
            m.update(DatePickerEvent::Confirm),
            Some(DatePickerOutcome::PickedRange(d(2026, 6, 1), d(2026, 6, 3)))
        );
    }

    #[test]
    fn days_outside_the_bounds_cannot_be_picked() {
        let mut m =
            DatePickerModel::single(None, d(2026, 3, 5), d(2026, 3, 20)).with_today(d(2026, 3, 1));
        m.update(DatePickerEvent::Select(d(2026, 3, 4)));
        assert_eq!(m.selected, None);
        let cells = month_cells(&m);
        assert!(cells.iter().find(|c| c.day == 4).unwrap().disabled);
        assert!(!cells.iter().find(|c| c.day == 5).unwrap().disabled);
        // The month cannot be moved past the bounds.
        m.update(DatePickerEvent::ShiftMonth(-1));
        assert_eq!(m.shown, (2026, 3));
        // Nor can the cursor.
        m.cursor = d(2026, 3, 19);
        m.update(DatePickerEvent::MoveCursor(7));
        assert_eq!(m.cursor, d(2026, 3, 20));
        assert_eq!(
            m.update(DatePickerEvent::Confirm),
            None,
            "nothing picked yet"
        );
    }

    #[test]
    fn the_keyboard_moves_the_cursor_and_picks() {
        let mut m = DatePickerModel::single(Some(d(2026, 1, 30)), d(2020, 1, 1), d(2030, 12, 31));
        m.update(DatePickerEvent::MoveCursor(3));
        assert_eq!(m.cursor, d(2026, 2, 2));
        assert_eq!(m.shown, (2026, 2), "the month follows the cursor");
        assert!(m.cursor_shown);
        m.update(DatePickerEvent::MoveCursorMonths(1));
        assert_eq!(m.cursor, d(2026, 3, 2));
        m.update(DatePickerEvent::SelectCursor);
        assert_eq!(
            m.update(DatePickerEvent::Confirm),
            Some(DatePickerOutcome::Picked(d(2026, 3, 2)))
        );
    }

    #[test]
    fn a_year_from_the_list_shows_that_year() {
        let mut m = DatePickerModel::single(Some(d(2026, 8, 17)), d(2000, 1, 1), d(2040, 12, 31));
        m.update(DatePickerEvent::ToggleYears);
        assert!(m.years);
        m.update(DatePickerEvent::PickYear(2031));
        assert_eq!((m.shown, m.years), ((2031, 8), false));
    }

    #[test]
    fn typed_dates_are_read_in_the_locale_s_order_and_checked() {
        let mut m = DatePickerModel::single(None, d(2000, 1, 1), d(2030, 12, 31)).input_mode();
        m.update(DatePickerEvent::Input("13/45/2026".into()));
        assert_eq!(m.update(DatePickerEvent::Confirm), None);
        assert_eq!(m.error.as_deref(), Some("Invalid format."));
        m.update(DatePickerEvent::Input("02/29/2031".into()));
        assert_eq!(m.update(DatePickerEvent::Confirm), None);
        m.update(DatePickerEvent::Input("12/24/2035".into()));
        m.update(DatePickerEvent::Confirm);
        assert_eq!(m.error.as_deref(), Some("Out of range."));
        m.update(DatePickerEvent::Input("02/29/2028".into()));
        assert_eq!(
            m.update(DatePickerEvent::Confirm),
            Some(DatePickerOutcome::Picked(d(2028, 2, 29)))
        );
        // Day first where the reader writes it so.
        assert_eq!(parse_date("29/02/2028", "jj/mm/aaaa"), Some(d(2028, 2, 29)));
        assert_eq!(
            parse_date("29/02/28", "dd/mm/yyyy"),
            None,
            "four-figure years"
        );
        // Switching back to the calendar shows what was typed.
        let mut m = DatePickerModel::single(None, d(2000, 1, 1), d(2030, 12, 31)).input_mode();
        m.update(DatePickerEvent::Input("07/04/2027".into()));
        m.update(DatePickerEvent::ToggleMode);
        assert_eq!(
            (m.mode, m.selected, m.shown),
            (DatePickerMode::Calendar, Some(d(2027, 7, 4)), (2027, 7))
        );
    }

    #[test]
    fn the_headline_follows_the_locale() {
        let l10n = crate::localizations::English;
        use crate::Localizations;
        let date = d(2026, 8, 17);
        assert_eq!(
            l10n.medium_date(date.weekday(), date.month, date.day),
            "Mon, Aug 17"
        );
        let fr = crate::localizations::French;
        assert_eq!(
            fr.medium_date(date.weekday(), date.month, date.day),
            "lun. 17 août"
        );
    }
}
