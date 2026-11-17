// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The words the framework itself puts on screen**, in the reader's language
//! (`material_localizations.dart`).
//!
//! A framework says a certain amount out loud on an application's behalf. The label a
//! screen reader announces on a back arrow. The word on the cross that dismisses a
//! notification. The initials over a calendar's columns, and the name of the month above
//! them. None of it comes from the application, so none of it can be translated by the
//! application — and until this module every one of them was an English string constant.
//!
//! The reference's answer is `MaterialLocalizations`, reached from the tree. This is the
//! same idea with this framework's ambient-scope idiom (the one [`MediaQuery`] uses): a
//! table is installed for the thread, and [`of`](crate::localizations::of) hands out whatever is in force.
//!
//! # It always answers
//!
//! [`of`](crate::localizations::of) never fails. With nothing installed it answers [`English`], which is what every
//! one of those constants said anyway — so nothing breaks, and an application that says
//! nothing is exactly where it was.
//!
//! That has a cost worth naming: a test that forgets to install a table still passes, and
//! so would a shell that forgot to install one. The guard against that is not the default
//! but `a_shell_installs_the_application_s_words`, which drives the shell and checks the
//! words actually arrive.
//!
//! # It is not on the theme
//!
//! A theme is what an interface **looks like**. What it says is a different question with
//! a different owner — the reference keeps them apart too, and an application that ships
//! one theme in twelve languages would otherwise need twelve themes.
//!
//! # The languages it has
//!
//! [`English`], [`French`], [`German`], [`Spanish`], [`Japanese`], [`Thai`] and
//! [`Vietnamese`] (milestone 813) — [`LANGUAGES`]. [`for_locale`] picks one by language, so
//! `es-MX` gets Spanish and a language not on the list gets English, and the shell installs
//! it for an application that gives no table of its own: listing a language in the
//! application's supported locales is all it takes.
//!
//! The tables carry more than words: which day a week starts on, the order a date is typed
//! in, how a month and its year are written together, how a number's thousands are grouped,
//! and whether the clock has twelve hours or twenty-four.
//!
//! # Fonts
//!
//! A table is only words; drawing them takes a font with their letters. The bundled face
//! (DejaVu Sans, the `bundled-sans` feature) has every letter of the English, French,
//! German, Spanish and Vietnamese tables. It has **no Japanese and no Thai**:
//!
//! - **Desktop and Android** draw them with the system's own fonts, found the first time
//!   text in those scripts is shaped — nothing to ship.
//! - **The web** has no system font a page can reach, so an application shown in Japanese
//!   or Thai in a browser registers a face for the script (a Noto Sans JP or Noto Sans Thai
//!   subset, say) with `frus::fonts::add_font`, or the letters are boxes.
//!
//! No CJK or Thai face is bundled by default: either one is megabytes, and most
//! applications never show the script.
//!
//! [`MediaQuery`]: crate::MediaQuery

use std::cell::RefCell;
use std::rc::Rc;

/// Writes `number` with its thousands separated by `separator`, from the right.
///
/// Shared by the tables rather than written twice: what changes from one language to the
/// next is the character, not the arithmetic.
fn group_with(number: usize, separator: &str) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (seen, ch) in digits.chars().enumerate() {
        if seen > 0 && (digits.len() - seen).is_multiple_of(3) {
            out.push_str(separator);
        }
        out.push(ch);
    }
    out
}

/// **What the framework says**, for one language.
///
/// Every method has an English body, so implementing this means writing down only what
/// differs. The reference's table has around a hundred entries; this has the ones the
/// framework actually says today, and grows as it says more.
///
/// **A default is not agreement.** An entry left unanswered is an English string in a
/// French interface, and it compiles. So a table shipped here writes out every entry, even
/// the ones whose right answer in that language *is* the English one — "OK" is "OK" in a
/// good many languages — with the reason beside it; [`French`] does, and a test reads this
/// file to check that it still does.
pub trait Localizations {
    /// What a reader hears on a back arrow (`backButtonTooltip`).
    fn back_button_label(&self) -> &str {
        "Back"
    }

    /// And on the cross that dismisses something (`closeButtonTooltip`).
    fn close_button_label(&self) -> &str {
        "Close"
    }

    /// What a reader hears on the control that opens a side panel
    /// (`openAppDrawerTooltip`).
    ///
    /// **One word for both edges.** The reference says the same thing for a leading
    /// panel and a trailing one (`action_buttons.dart:331` against `:362`): a reader told
    /// which edge a panel comes in from is being told about the layout rather than about
    /// the action.
    fn open_drawer_label(&self) -> &str {
        "Open navigation menu"
    }

    /// **Where one destination sits among the rest** (`tabLabel`), for a reader who is
    /// hearing them one at a time and cannot see how many there are.
    ///
    /// `index` counts from one, as it reads: "Tab 1 of 3". It is the one entry here that
    /// takes arguments and so returns an owned string rather than a borrowed one — the
    /// numbers are the caller's, and a table cannot have written the sentence in advance.
    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("Tab {index} of {count}")
    }

    /// **What a reader hears on an account header** (`signedInLabel`): the name, the
    /// address and the control for switching arrive as one thing with one name, rather
    /// than as three unrelated nodes at the top of a panel.
    fn signed_in_label(&self) -> &str {
        "Signed in"
    }

    /// And on the control that reveals the other accounts (`showAccountsLabel`).
    fn show_accounts_label(&self) -> &str {
        "Show accounts"
    }

    /// And on the same control once they are showing (`hideAccountsLabel`).
    ///
    /// **Two entries rather than one that flips**, as the reference has it: a control
    /// named for what it *will do* is the only kind a reader can act on, and the two
    /// sentences are not each other's negation in every language.
    fn hide_accounts_label(&self) -> &str {
        "Hide accounts"
    }

    /// **What a reader hears on the cross that empties a field** (`clearButtonTooltip`).
    ///
    /// Not [`close_button_label`](Self::close_button_label): the same glyph means two
    /// different things depending on what it sits in, and *Close* on a control that empties
    /// a search box would send a reader looking for the thing it closed.
    fn clear_button_label(&self) -> &str {
        "Clear"
    }

    /// **What a reader hears on a control that opens more actions** — a split button's
    /// trailing half, a menu toggle (milestone 786).
    fn more_options_label(&self) -> &str {
        "More options"
    }

    /// The word on a confirming button (`okButtonLabel`).
    fn ok_button_label(&self) -> &str {
        "OK"
    }

    /// And on the one that backs out (`cancelButtonLabel`).
    fn cancel_button_label(&self) -> &str {
        "Cancel"
    }

    /// **The four words on the bar over a text selection** (`cutButtonLabel`,
    /// `copyButtonLabel`, `pasteButtonLabel`, `selectAllButtonLabel`, milestone 813). They
    /// were English constants on [`EditAction`](crate::EditAction) until then.
    fn cut_button_label(&self) -> &str {
        "Cut"
    }

    /// See [`cut_button_label`](Self::cut_button_label).
    fn copy_button_label(&self) -> &str {
        "Copy"
    }

    /// See [`cut_button_label`](Self::cut_button_label).
    fn paste_button_label(&self) -> &str {
        "Paste"
    }

    /// See [`cut_button_label`](Self::cut_button_label).
    fn select_all_button_label(&self) -> &str {
        "Select all"
    }

    /// **What a reader hears on an app bar's three dots**, the button that opens the
    /// actions that did not fit (`moreButtonTooltip`).
    fn more_button_label(&self) -> &str {
        "More"
    }

    /// **What a reader hears on a chip's cross** (`deleteButtonTooltip`).
    fn remove_button_label(&self) -> &str {
        "Remove"
    }

    /// **The button through to an application's licences**, on the box that says what the
    /// application is (`viewLicensesButtonLabel`).
    fn licenses_label(&self) -> &str {
        "Licences"
    }

    /// **How many licences cover a package**, under its name in the list of them
    /// (`licensesPackageDetailText`): a count, because the noun agrees with it.
    fn license_count_label(&self, count: usize) -> String {
        if count == 1 {
            "1 licence".to_string()
        } else {
            format!("{} licences", self.group_digits(count))
        }
    }

    /// **What stands in for content still on its way**: a file tree's folder whose
    /// children have not arrived yet (milestone 813).
    fn loading_label(&self) -> &str {
        "Loading…"
    }

    /// **The placeholder in a table's search field** (`searchFieldLabel`).
    fn search_field_label(&self) -> &str {
        "Search"
    }

    /// **What a table says when it has nothing to show.** A caller may say its own; this
    /// is what one that says nothing gets, and it used to be a constant.
    fn no_results_label(&self) -> &str {
        "No results"
    }

    /// **What a reader hears on the control that steps a number down**, and on the one
    /// that steps it up.
    ///
    /// The glyphs are a minus and a plus, which say nothing out loud.
    fn decrease_label(&self) -> &str {
        "Less"
    }

    /// See [`decrease_label`](Self::decrease_label).
    fn increase_label(&self) -> &str {
        "More"
    }

    /// **What a table announces when a row is ticked**, and the three sentences beside it
    /// (`selectedRowCountTitle`).
    ///
    /// Four whole sentences rather than one built from parts, for the reason
    /// [`hide_accounts_label`](Self::hide_accounts_label) is its own entry: a language
    /// that agrees its participle with the thing selected cannot be served by a formula
    /// that glues *row* to *selected*.
    fn row_selected_label(&self) -> &str {
        "Row selected"
    }

    /// See [`row_selected_label`](Self::row_selected_label).
    fn row_deselected_label(&self) -> &str {
        "Row deselected"
    }

    /// See [`row_selected_label`](Self::row_selected_label).
    fn all_rows_selected_label(&self) -> &str {
        "All rows selected"
    }

    /// See [`row_selected_label`](Self::row_selected_label).
    fn all_rows_deselected_label(&self) -> &str {
        "All rows deselected"
    }

    /// **The heading over a time picker's hours**, and the one over its minutes
    /// (`timePickerHourLabel`, `timePickerMinuteLabel`).
    fn hour_label(&self) -> &str {
        "Hour"
    }

    /// See [`hour_label`](Self::hour_label).
    fn minute_label(&self) -> &str {
        "Minute"
    }

    /// **The two halves of a twelve-hour clock** (`anteMeridiemAbbreviation`,
    /// `postMeridiemAbbreviation`).
    ///
    /// Left as the Latin abbreviations in a good many languages that never use a
    /// twelve-hour clock at all — which is not an oversight in a table that says so, and
    /// is why a test forbidding a French entry from matching the English one has to have
    /// exceptions.
    fn ante_meridiem_label(&self) -> &str {
        "AM"
    }

    /// See [`ante_meridiem_label`](Self::ante_meridiem_label).
    fn post_meridiem_label(&self) -> &str {
        "PM"
    }

    /// **The two ends of a range**: the heading over the first picker of a pair, and over
    /// the second.
    fn start_label(&self) -> &str {
        "Start"
    }

    /// See [`start_label`](Self::start_label).
    fn end_label(&self) -> &str {
        "End"
    }

    /// **The line under a paginated table**: which rows are showing, out of how many.
    ///
    /// `start` and `end` count from one and are inclusive, as the sentence reads. An empty
    /// table says so rather than saying "1–0 of 0".
    ///
    /// A whole sentence rather than a separator and a word, because the pieces do not go in
    /// the same order in every language, and because the numbers in it want grouping —
    /// which is [`group_digits`](Self::group_digits), and is not the same character
    /// everywhere.
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 of 0".to_string();
        }
        format!(
            "{}–{} of {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    /// **The label beside a table's page-size chooser.**
    fn rows_per_page_label(&self) -> &str {
        "Rows per page"
    }

    /// **What a table's bulk-action bar says about how much is selected.**
    ///
    /// Takes the count because languages disagree about what a count does to the words
    /// after it: English says "1 selected" and "3 selected", and French has to agree the
    /// participle with the number.
    fn selected_row_count_label(&self, count: usize) -> String {
        format!("{} selected", self.group_digits(count))
    }

    /// **A number with its thousands grouped**, as the reader's language groups them: a
    /// comma in English, a space in French, and a stop in a good many other places.
    ///
    /// Four thousand rows written `4000` is a number a reader has to count the digits of.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, ",")
    }

    /// **What a reader hears on the arrows either side of a calendar's month.**
    fn previous_month_label(&self) -> &str {
        "Previous month"
    }

    /// See [`previous_month_label`](Self::previous_month_label).
    fn next_month_label(&self) -> &str {
        "Next month"
    }

    /// The single letters over a calendar's columns (`narrowWeekdays`).
    ///
    /// **Always Sunday first**, whatever day the week starts on where the reader is —
    /// the reference is explicit about this (`date.dart:353`), and it is what lets
    /// [`first_day_of_week_index`](Self::first_day_of_week_index) be an index into this
    /// list rather than a second thing to keep in step with it.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["S", "M", "T", "W", "T", "F", "S"]
    }

    /// **Which day a week starts on**, as an index into
    /// [`narrow_weekdays`](Self::narrow_weekdays) — so `0` is Sunday and `1` is Monday
    /// (`material_localizations.dart`).
    ///
    /// Sunday in the United States, Monday across most of Europe, Saturday in much of the
    /// Middle East. A calendar that always started on Sunday was not merely untranslated:
    /// it put the days in the wrong columns.
    fn first_day_of_week_index(&self) -> usize {
        0
    }

    /// The months, January first.
    fn months(&self) -> [&str; 12] {
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ]
    }

    /// The heading of a date picker in its calendar (`datePickerHelpText`).
    fn date_picker_title(&self) -> &str {
        "Select date"
    }

    /// The heading of a date range picker.
    fn date_range_picker_title(&self) -> &str {
        "Select range"
    }

    /// The heading of a date picker in its text mode.
    fn date_input_title(&self) -> &str {
        "Enter date"
    }

    /// The label of a date picker's text field.
    fn date_input_label(&self) -> &str {
        "Date"
    }

    /// **The hint under a date field**: how a date is typed, in this language's letters and
    /// its order (`dateHelpText`) — `jj/mm/aaaa` in French, `tt.mm.jjjj` in German.
    ///
    /// Only words for a reader. The order a typed date is *read* in is
    /// [`date_input_format`](Self::date_input_format), because the letters cannot say it in
    /// every language: a `j` is the day in French and the year in German.
    fn date_input_pattern(&self) -> &str {
        "mm/dd/yyyy"
    }

    /// **The order a typed date is read and written in** (milestone 813): `dd`, `mm` and
    /// `yyyy`, always those letters, joined by the separator the language writes between
    /// them — `mm/dd/yyyy`, `dd.mm.yyyy`, `yyyy/mm/dd`. What
    /// [`parse_date`](crate::parse_date) and [`format_date`](crate::format_date) are given.
    fn date_input_format(&self) -> &str {
        "mm/dd/yyyy"
    }

    /// The button that switches a date picker to typing.
    fn input_mode_label(&self) -> &str {
        "Switch to input"
    }

    /// The button that switches a date picker back to its calendar.
    fn calendar_mode_label(&self) -> &str {
        "Switch to calendar"
    }

    /// Under a typed date that is not one.
    fn invalid_date_label(&self) -> &str {
        "Invalid format."
    }

    /// Under a typed date outside the dates that may be picked.
    fn date_out_of_range_label(&self) -> &str {
        "Out of range."
    }

    /// The button that keeps a full-screen picker's choice.
    fn save_button_label(&self) -> &str {
        "Save"
    }

    /// The button that opens a date picker's list of years.
    fn select_year_label(&self) -> &str {
        "Select year"
    }

    /// The days of the week, short, Sunday first.
    fn short_weekdays(&self) -> [&str; 7] {
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    }

    /// The months, short, January first.
    fn short_months(&self) -> [&str; 12] {
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
    }

    /// A date as a picker's headline says it: weekday, month and day — "Mon, Aug 17".
    /// `weekday` counts from Sunday, `month` from one.
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{}, {} {}",
            self.short_weekdays()[weekday % 7],
            self.short_months()[month_index(month)],
            day
        )
    }

    /// **A month and its year**, as over a calendar: "August 2026" (`formatMonthYear`).
    /// Not every language puts them side by side — "agosto de 2026", "2026年8月".
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} {year}", self.months()[month_index(month)])
    }

    /// **A day of a month, short**, as a range's ends say it: "Aug 17"
    /// (`formatShortMonthDay`).
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{} {day}", self.short_months()[month_index(month)])
    }

    /// **A whole date in words**: "August 17, 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{} {day}, {year}", self.months()[month_index(month)])
    }

    /// **Whether time is told on a twenty-four hour clock** where this language is
    /// spoken (`timeOfDayFormat`).
    ///
    /// The time pickers do not read it on their own — they are twenty-four hour unless
    /// asked otherwise, as they were — so an application that wants the reader's clock
    /// asks: `if !localizations::of().uses_24_hour_clock() { picker.hour12() }`.
    fn uses_24_hour_clock(&self) -> bool {
        false
    }
}

/// `month`, from one, as an index into a list of twelve — clamped, so a bad month cannot
/// index out of a table.
fn month_index(month: u32) -> usize {
    (month.clamp(1, 12) - 1) as usize
}

/// **The English table**: every method left as the trait wrote it.
///
/// It is what [`of`] answers when nothing has been installed, and the sensible thing for
/// another table to be written against — implement [`Localizations`] and override the
/// entries that differ.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct English;

impl Localizations for English {}

/// **The French table.**
///
/// Written rather than generated, and by someone who speaks it: a machine-translated
/// table is worse than none at all, because it silences the question for that language
/// and leaves a native reader with something subtly wrong and nobody looking at it.
///
/// Three things in here are not translation:
///
/// - **The week starts on Monday.** A calendar that always began on Sunday was not
///   untranslated; it put the days in the wrong columns.
/// - **The months are lower case**, which is the rule in French and looks like a mistake
///   to an English eye. "janvier 2026" is correct and "Janvier 2026" is not.
/// - **`AM` and `PM` stay as they are.** French tells the time on a twenty-four hour
///   clock and has no everyday words for the halves of a twelve-hour one; the reference's
///   own French table keeps the Latin abbreviations here too.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct French;

impl Localizations for French {
    fn back_button_label(&self) -> &str {
        "Retour"
    }

    fn close_button_label(&self) -> &str {
        "Fermer"
    }

    fn open_drawer_label(&self) -> &str {
        "Ouvrir le menu de navigation"
    }

    fn more_options_label(&self) -> &str {
        "Plus d'options"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("Onglet {index} sur {count}")
    }

    fn signed_in_label(&self) -> &str {
        "Connecté"
    }

    fn show_accounts_label(&self) -> &str {
        "Afficher les comptes"
    }

    fn hide_accounts_label(&self) -> &str {
        "Masquer les comptes"
    }

    fn clear_button_label(&self) -> &str {
        "Effacer"
    }

    /// **"OK" is "OK" in French**, and a table that translated it to look busy would be
    /// wrong about the language.
    fn ok_button_label(&self) -> &str {
        "OK"
    }

    fn cancel_button_label(&self) -> &str {
        "Annuler"
    }

    fn cut_button_label(&self) -> &str {
        "Couper"
    }

    fn copy_button_label(&self) -> &str {
        "Copier"
    }

    fn paste_button_label(&self) -> &str {
        "Coller"
    }

    fn select_all_button_label(&self) -> &str {
        "Tout sélectionner"
    }

    fn more_button_label(&self) -> &str {
        "Plus"
    }

    fn remove_button_label(&self) -> &str {
        "Supprimer"
    }

    /// The same word as the English table's, which spells it the British way.
    fn licenses_label(&self) -> &str {
        "Licences"
    }

    fn license_count_label(&self, count: usize) -> String {
        let plural = if count > 1 { "s" } else { "" };
        format!("{}\u{a0}licence{plural}", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "Chargement…"
    }

    fn search_field_label(&self) -> &str {
        "Rechercher"
    }

    fn no_results_label(&self) -> &str {
        "Aucun résultat"
    }

    fn decrease_label(&self) -> &str {
        "Moins"
    }

    fn increase_label(&self) -> &str {
        "Plus"
    }

    /// **Agreed with the thing selected**, which is why these are four sentences and not
    /// one formula: *ligne* is feminine, so the participle takes an `e`, and it takes an
    /// `s` as well once there are several of them.
    fn row_selected_label(&self) -> &str {
        "Ligne sélectionnée"
    }

    fn row_deselected_label(&self) -> &str {
        "Ligne désélectionnée"
    }

    fn all_rows_selected_label(&self) -> &str {
        "Toutes les lignes sélectionnées"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "Toutes les lignes désélectionnées"
    }

    fn hour_label(&self) -> &str {
        "Heure"
    }

    /// The same word as the English one, which is the right answer and not a gap.
    fn minute_label(&self) -> &str {
        "Minute"
    }

    /// **Kept as they are.** French tells the time on a twenty-four hour clock; these are
    /// written out rather than left to the trait so that nothing in this table is silence.
    fn ante_meridiem_label(&self) -> &str {
        "AM"
    }

    /// See [`French::ante_meridiem_label`].
    fn post_meridiem_label(&self) -> &str {
        "PM"
    }

    fn start_label(&self) -> &str {
        "Début"
    }

    fn end_label(&self) -> &str {
        "Fin"
    }

    /// « 11–20 sur 4 000 ». The word in the middle is *sur* and not *de*, and the numbers
    /// are grouped with a space.
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 sur 0".to_string();
        }
        format!(
            "{}–{} sur {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "Lignes par page"
    }

    /// **Agreed with the count**, which is why this entry takes one: *ligne* is feminine,
    /// so the participle is *sélectionnée*, and it takes an `s` from two upwards.
    fn selected_row_count_label(&self, count: usize) -> String {
        let plural = if count > 1 { "s" } else { "" };
        format!("{} sélectionnée{plural}", self.group_digits(count))
    }

    /// **A no-break space**, which is how French groups a number — and no-break because a
    /// line that broke between the 4 and the 000 would be two numbers.
    ///
    /// Typography asks for a *narrow* one (`U+202F`); this is the ordinary one
    /// (`U+00A0`), which every font has and which no shaper draws as a missing glyph.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, " ")
    }

    fn previous_month_label(&self) -> &str {
        "Mois précédent"
    }

    fn next_month_label(&self) -> &str {
        "Mois suivant"
    }

    /// Sunday first, as the trait requires, whatever day the week starts on here:
    /// dimanche, lundi, mardi, mercredi, jeudi, vendredi, samedi.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["D", "L", "M", "M", "J", "V", "S"]
    }

    /// **Monday.** Not a translation, and the thing most often left behind by one.
    fn first_day_of_week_index(&self) -> usize {
        1
    }

    fn months(&self) -> [&str; 12] {
        [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "Sélectionner une date"
    }

    fn date_range_picker_title(&self) -> &str {
        "Sélectionner une période"
    }

    fn date_input_title(&self) -> &str {
        "Saisir une date"
    }

    /// The same word in French.
    fn date_input_label(&self) -> &str {
        "Date"
    }

    fn date_input_pattern(&self) -> &str {
        "jj/mm/aaaa"
    }

    fn date_input_format(&self) -> &str {
        "dd/mm/yyyy"
    }

    fn input_mode_label(&self) -> &str {
        "Passer à la saisie"
    }

    fn calendar_mode_label(&self) -> &str {
        "Passer au calendrier"
    }

    fn invalid_date_label(&self) -> &str {
        "Format non valide."
    }

    fn date_out_of_range_label(&self) -> &str {
        "Hors de la plage autorisée."
    }

    fn save_button_label(&self) -> &str {
        "Enregistrer"
    }

    fn select_year_label(&self) -> &str {
        "Sélectionner l'année"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
            "nov.", "déc.",
        ]
    }

    /// "lun. 17 août": the day before the month.
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{} {} {}",
            self.short_weekdays()[weekday % 7],
            day,
            self.short_months()[month_index(month)]
        )
    }

    /// "janvier 2026", in lower case.
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} {year}", self.months()[month_index(month)])
    }

    /// "17 août".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{day} {}", self.short_months()[month_index(month)])
    }

    /// "17 août 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{day} {} {year}", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The German table** (milestone 813).
///
/// The parts that are not words: the week starts on **Monday**, a date is written
/// `17.08.2026` and typed in that order, a number is grouped with a **stop** (`4.000`),
/// and the clock has twenty-four hours. `AM` and `PM` are kept for the twelve-hour picker,
/// which German has no everyday words for.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct German;

impl Localizations for German {
    fn back_button_label(&self) -> &str {
        "Zurück"
    }

    fn close_button_label(&self) -> &str {
        "Schließen"
    }

    fn open_drawer_label(&self) -> &str {
        "Navigationsmenü öffnen"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("Tab {index} von {count}")
    }

    fn signed_in_label(&self) -> &str {
        "Angemeldet"
    }

    fn show_accounts_label(&self) -> &str {
        "Konten anzeigen"
    }

    fn hide_accounts_label(&self) -> &str {
        "Konten ausblenden"
    }

    fn clear_button_label(&self) -> &str {
        "Löschen"
    }

    fn more_options_label(&self) -> &str {
        "Weitere Optionen"
    }

    /// "OK" in German too.
    fn ok_button_label(&self) -> &str {
        "OK"
    }

    fn cancel_button_label(&self) -> &str {
        "Abbrechen"
    }

    fn cut_button_label(&self) -> &str {
        "Ausschneiden"
    }

    fn copy_button_label(&self) -> &str {
        "Kopieren"
    }

    fn paste_button_label(&self) -> &str {
        "Einsetzen"
    }

    fn select_all_button_label(&self) -> &str {
        "Alle auswählen"
    }

    fn more_button_label(&self) -> &str {
        "Mehr"
    }

    fn remove_button_label(&self) -> &str {
        "Löschen"
    }

    fn licenses_label(&self) -> &str {
        "Lizenzen"
    }

    fn license_count_label(&self, count: usize) -> String {
        let noun = if count == 1 { "Lizenz" } else { "Lizenzen" };
        format!("{}\u{a0}{noun}", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "Wird geladen…"
    }

    fn search_field_label(&self) -> &str {
        "Suchen"
    }

    fn no_results_label(&self) -> &str {
        "Keine Ergebnisse"
    }

    fn decrease_label(&self) -> &str {
        "Weniger"
    }

    fn increase_label(&self) -> &str {
        "Mehr"
    }

    fn row_selected_label(&self) -> &str {
        "Zeile ausgewählt"
    }

    fn row_deselected_label(&self) -> &str {
        "Zeilenauswahl aufgehoben"
    }

    fn all_rows_selected_label(&self) -> &str {
        "Alle Zeilen ausgewählt"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "Auswahl aller Zeilen aufgehoben"
    }

    fn hour_label(&self) -> &str {
        "Stunde"
    }

    /// The same word in German.
    fn minute_label(&self) -> &str {
        "Minute"
    }

    /// Kept: German tells the time on a twenty-four hour clock.
    fn ante_meridiem_label(&self) -> &str {
        "AM"
    }

    /// See [`German::ante_meridiem_label`].
    fn post_meridiem_label(&self) -> &str {
        "PM"
    }

    fn start_label(&self) -> &str {
        "Beginn"
    }

    fn end_label(&self) -> &str {
        "Ende"
    }

    /// "11–20 von 4.000".
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 von 0".to_string();
        }
        format!(
            "{}–{} von {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "Zeilen pro Seite"
    }

    fn selected_row_count_label(&self, count: usize) -> String {
        format!("{} ausgewählt", self.group_digits(count))
    }

    /// A stop: `4.000`.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, ".")
    }

    fn previous_month_label(&self) -> &str {
        "Vorheriger Monat"
    }

    fn next_month_label(&self) -> &str {
        "Nächster Monat"
    }

    /// Sonntag, Montag, Dienstag, Mittwoch, Donnerstag, Freitag, Samstag.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["S", "M", "D", "M", "D", "F", "S"]
    }

    /// Monday.
    fn first_day_of_week_index(&self) -> usize {
        1
    }

    fn months(&self) -> [&str; 12] {
        [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "Datum auswählen"
    }

    fn date_range_picker_title(&self) -> &str {
        "Zeitraum auswählen"
    }

    fn date_input_title(&self) -> &str {
        "Datum eingeben"
    }

    fn date_input_label(&self) -> &str {
        "Datum"
    }

    fn date_input_pattern(&self) -> &str {
        "tt.mm.jjjj"
    }

    fn date_input_format(&self) -> &str {
        "dd.mm.yyyy"
    }

    fn input_mode_label(&self) -> &str {
        "Zur Texteingabe wechseln"
    }

    fn calendar_mode_label(&self) -> &str {
        "Zum Kalender wechseln"
    }

    fn invalid_date_label(&self) -> &str {
        "Ungültiges Format."
    }

    fn date_out_of_range_label(&self) -> &str {
        "Außerhalb des Zeitraums."
    }

    fn save_button_label(&self) -> &str {
        "Speichern"
    }

    fn select_year_label(&self) -> &str {
        "Jahr auswählen"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.",
            "Dez.",
        ]
    }

    /// "Mo., 17. Aug.": the day, with its stop, before the month.
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{}, {day}. {}",
            self.short_weekdays()[weekday % 7],
            self.short_months()[month_index(month)]
        )
    }

    /// "August 2026".
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} {year}", self.months()[month_index(month)])
    }

    /// "17. Aug.".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{day}. {}", self.short_months()[month_index(month)])
    }

    /// "17. August 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{day}. {} {year}", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The Spanish table** (milestone 813), in Spain's wording.
///
/// The week starts on **Monday**, a date is typed day first, and the clock has twenty-four
/// hours. A number is grouped with a **stop**, and only from five digits: Spanish writes
/// `4000` but `40.000`. Wednesday's initial is an `X`, so that it is not Tuesday's `M`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Spanish;

impl Localizations for Spanish {
    fn back_button_label(&self) -> &str {
        "Atrás"
    }

    fn close_button_label(&self) -> &str {
        "Cerrar"
    }

    fn open_drawer_label(&self) -> &str {
        "Abrir el menú de navegación"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("Pestaña {index} de {count}")
    }

    fn signed_in_label(&self) -> &str {
        "Sesión iniciada"
    }

    fn show_accounts_label(&self) -> &str {
        "Mostrar cuentas"
    }

    fn hide_accounts_label(&self) -> &str {
        "Ocultar cuentas"
    }

    fn clear_button_label(&self) -> &str {
        "Borrar"
    }

    fn more_options_label(&self) -> &str {
        "Más opciones"
    }

    fn ok_button_label(&self) -> &str {
        "Aceptar"
    }

    fn cancel_button_label(&self) -> &str {
        "Cancelar"
    }

    fn cut_button_label(&self) -> &str {
        "Cortar"
    }

    fn copy_button_label(&self) -> &str {
        "Copiar"
    }

    fn paste_button_label(&self) -> &str {
        "Pegar"
    }

    fn select_all_button_label(&self) -> &str {
        "Seleccionar todo"
    }

    fn more_button_label(&self) -> &str {
        "Más"
    }

    fn remove_button_label(&self) -> &str {
        "Eliminar"
    }

    fn licenses_label(&self) -> &str {
        "Licencias"
    }

    fn license_count_label(&self, count: usize) -> String {
        let noun = if count == 1 { "licencia" } else { "licencias" };
        format!("{}\u{a0}{noun}", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "Cargando…"
    }

    fn search_field_label(&self) -> &str {
        "Buscar"
    }

    fn no_results_label(&self) -> &str {
        "No hay resultados"
    }

    fn decrease_label(&self) -> &str {
        "Menos"
    }

    fn increase_label(&self) -> &str {
        "Más"
    }

    /// *Fila* is feminine, and the participle agrees with it.
    fn row_selected_label(&self) -> &str {
        "Fila seleccionada"
    }

    fn row_deselected_label(&self) -> &str {
        "Fila deseleccionada"
    }

    fn all_rows_selected_label(&self) -> &str {
        "Todas las filas seleccionadas"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "Todas las filas deseleccionadas"
    }

    fn hour_label(&self) -> &str {
        "Hora"
    }

    fn minute_label(&self) -> &str {
        "Minuto"
    }

    fn ante_meridiem_label(&self) -> &str {
        "a. m."
    }

    fn post_meridiem_label(&self) -> &str {
        "p. m."
    }

    fn start_label(&self) -> &str {
        "Inicio"
    }

    fn end_label(&self) -> &str {
        "Fin"
    }

    /// "11–20 de 4000".
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 de 0".to_string();
        }
        format!(
            "{}–{} de {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "Filas por página"
    }

    /// Agreed with *fila*, feminine, and with the count.
    fn selected_row_count_label(&self, count: usize) -> String {
        let plural = if count == 1 { "" } else { "s" };
        format!("{} seleccionada{plural}", self.group_digits(count))
    }

    /// A stop, from five digits up: `4000`, `40.000`.
    fn group_digits(&self, number: usize) -> String {
        if number < 10_000 {
            number.to_string()
        } else {
            group_with(number, ".")
        }
    }

    fn previous_month_label(&self) -> &str {
        "Mes anterior"
    }

    fn next_month_label(&self) -> &str {
        "Mes siguiente"
    }

    /// Domingo, lunes, martes, miércoles (`X`), jueves, viernes, sábado.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["D", "L", "M", "X", "J", "V", "S"]
    }

    /// Monday.
    fn first_day_of_week_index(&self) -> usize {
        1
    }

    /// In lower case, as Spanish writes them.
    fn months(&self) -> [&str; 12] {
        [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "Seleccionar fecha"
    }

    fn date_range_picker_title(&self) -> &str {
        "Seleccionar periodo"
    }

    fn date_input_title(&self) -> &str {
        "Introduce una fecha"
    }

    fn date_input_label(&self) -> &str {
        "Fecha"
    }

    fn date_input_pattern(&self) -> &str {
        "dd/mm/aaaa"
    }

    fn date_input_format(&self) -> &str {
        "dd/mm/yyyy"
    }

    fn input_mode_label(&self) -> &str {
        "Cambiar a cuadro de texto"
    }

    fn calendar_mode_label(&self) -> &str {
        "Cambiar a calendario"
    }

    fn invalid_date_label(&self) -> &str {
        "Formato no válido."
    }

    fn date_out_of_range_label(&self) -> &str {
        "Fuera del periodo válido."
    }

    fn save_button_label(&self) -> &str {
        "Guardar"
    }

    fn select_year_label(&self) -> &str {
        "Seleccionar año"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
        ]
    }

    /// "lun, 17 ago".
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{}, {day} {}",
            self.short_weekdays()[weekday % 7],
            self.short_months()[month_index(month)]
        )
    }

    /// "agosto de 2026".
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} de {year}", self.months()[month_index(month)])
    }

    /// "17 ago".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{day} {}", self.short_months()[month_index(month)])
    }

    /// "17 de agosto de 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{day} de {} de {year}", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The Japanese table** (milestone 813).
///
/// The week starts on **Sunday**, as in Japan's calendars. A date is year first —
/// `2026/08/17` typed, `2026年8月17日` written — and a month is its number and 月, so the
/// month and the short month are the same word. The clock has twenty-four hours.
///
/// It needs a font with the kana and the kanji: see the module's notes on fonts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Japanese;

impl Localizations for Japanese {
    fn back_button_label(&self) -> &str {
        "戻る"
    }

    fn close_button_label(&self) -> &str {
        "閉じる"
    }

    fn open_drawer_label(&self) -> &str {
        "ナビゲーション メニューを開く"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("タブ: {index}/{count}")
    }

    fn signed_in_label(&self) -> &str {
        "ログイン中"
    }

    fn show_accounts_label(&self) -> &str {
        "アカウントを表示"
    }

    fn hide_accounts_label(&self) -> &str {
        "アカウントを非表示"
    }

    fn clear_button_label(&self) -> &str {
        "消去"
    }

    fn more_options_label(&self) -> &str {
        "その他のオプション"
    }

    /// "OK" in Japanese interfaces too.
    fn ok_button_label(&self) -> &str {
        "OK"
    }

    fn cancel_button_label(&self) -> &str {
        "キャンセル"
    }

    fn cut_button_label(&self) -> &str {
        "切り取り"
    }

    fn copy_button_label(&self) -> &str {
        "コピー"
    }

    fn paste_button_label(&self) -> &str {
        "貼り付け"
    }

    fn select_all_button_label(&self) -> &str {
        "すべてを選択"
    }

    fn more_button_label(&self) -> &str {
        "その他"
    }

    fn remove_button_label(&self) -> &str {
        "削除"
    }

    fn licenses_label(&self) -> &str {
        "ライセンス"
    }

    fn license_count_label(&self, count: usize) -> String {
        format!("ライセンス: {} 件", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "読み込み中…"
    }

    fn search_field_label(&self) -> &str {
        "検索"
    }

    fn no_results_label(&self) -> &str {
        "結果なし"
    }

    fn decrease_label(&self) -> &str {
        "減らす"
    }

    fn increase_label(&self) -> &str {
        "増やす"
    }

    fn row_selected_label(&self) -> &str {
        "行を選択しました"
    }

    fn row_deselected_label(&self) -> &str {
        "行の選択を解除しました"
    }

    fn all_rows_selected_label(&self) -> &str {
        "すべての行を選択しました"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "すべての行の選択を解除しました"
    }

    fn hour_label(&self) -> &str {
        "時"
    }

    fn minute_label(&self) -> &str {
        "分"
    }

    /// Kept as the Latin abbreviations, as Japanese time pickers show them.
    fn ante_meridiem_label(&self) -> &str {
        "AM"
    }

    /// See [`Japanese::ante_meridiem_label`].
    fn post_meridiem_label(&self) -> &str {
        "PM"
    }

    fn start_label(&self) -> &str {
        "開始"
    }

    fn end_label(&self) -> &str {
        "終了"
    }

    /// "11 - 20 行（合計 4,000 行）".
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 行（合計 0 行）".to_string();
        }
        format!(
            "{} - {} 行（合計 {} 行）",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "ページあたりの行数"
    }

    fn selected_row_count_label(&self, count: usize) -> String {
        format!("{} 件を選択中", self.group_digits(count))
    }

    /// A comma, as in English.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, ",")
    }

    fn previous_month_label(&self) -> &str {
        "前月"
    }

    fn next_month_label(&self) -> &str {
        "来月"
    }

    /// 日 (Sunday) to 土 (Saturday): one character is already the short name.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["日", "月", "火", "水", "木", "金", "土"]
    }

    /// Sunday, written out rather than left to the trait.
    fn first_day_of_week_index(&self) -> usize {
        0
    }

    fn months(&self) -> [&str; 12] {
        [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "日付の選択"
    }

    fn date_range_picker_title(&self) -> &str {
        "期間の選択"
    }

    fn date_input_title(&self) -> &str {
        "日付を入力"
    }

    fn date_input_label(&self) -> &str {
        "日付"
    }

    /// The Latin letters, as Japanese date fields write their hint.
    fn date_input_pattern(&self) -> &str {
        "yyyy/mm/dd"
    }

    fn date_input_format(&self) -> &str {
        "yyyy/mm/dd"
    }

    fn input_mode_label(&self) -> &str {
        "入力に切り替え"
    }

    fn calendar_mode_label(&self) -> &str {
        "カレンダーに切り替え"
    }

    fn invalid_date_label(&self) -> &str {
        "形式が無効です。"
    }

    fn date_out_of_range_label(&self) -> &str {
        "範囲外です。"
    }

    fn save_button_label(&self) -> &str {
        "保存"
    }

    fn select_year_label(&self) -> &str {
        "年を選択"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["日", "月", "火", "水", "木", "金", "土"]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ]
    }

    /// "8月17日(月)".
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{}{day}日({})",
            self.short_months()[month_index(month)],
            self.short_weekdays()[weekday % 7]
        )
    }

    /// "2026年8月": the year first.
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{year}年{}", self.months()[month_index(month)])
    }

    /// "8月17日".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{}{day}日", self.short_months()[month_index(month)])
    }

    /// "2026年8月17日".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{year}年{}{day}日", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The Thai table** (milestone 813).
///
/// The week starts on **Sunday**, a date is typed day first, and the clock has twenty-four
/// hours. Years are the **Gregorian** ones: Thai calendars often count from the Buddhist
/// era (2026 is 2569), but the platform conventions this table follows do not, so neither
/// does it — an application that wants the Buddhist year writes its own `month_year` and
/// `long_date`.
///
/// It needs a font with the Thai script: see the module's notes on fonts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Thai;

impl Localizations for Thai {
    fn back_button_label(&self) -> &str {
        "กลับ"
    }

    fn close_button_label(&self) -> &str {
        "ปิด"
    }

    fn open_drawer_label(&self) -> &str {
        "เปิดเมนูการนำทาง"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("แท็บที่ {index} จาก {count}")
    }

    fn signed_in_label(&self) -> &str {
        "ลงชื่อเข้าใช้"
    }

    fn show_accounts_label(&self) -> &str {
        "แสดงบัญชี"
    }

    fn hide_accounts_label(&self) -> &str {
        "ซ่อนบัญชี"
    }

    fn clear_button_label(&self) -> &str {
        "ล้าง"
    }

    fn more_options_label(&self) -> &str {
        "ตัวเลือกเพิ่มเติม"
    }

    fn ok_button_label(&self) -> &str {
        "ตกลง"
    }

    fn cancel_button_label(&self) -> &str {
        "ยกเลิก"
    }

    fn cut_button_label(&self) -> &str {
        "ตัด"
    }

    fn copy_button_label(&self) -> &str {
        "คัดลอก"
    }

    fn paste_button_label(&self) -> &str {
        "วาง"
    }

    fn select_all_button_label(&self) -> &str {
        "เลือกทั้งหมด"
    }

    fn more_button_label(&self) -> &str {
        "เพิ่มเติม"
    }

    fn remove_button_label(&self) -> &str {
        "ลบ"
    }

    fn licenses_label(&self) -> &str {
        "ใบอนุญาต"
    }

    fn license_count_label(&self, count: usize) -> String {
        format!("ใบอนุญาต {} ใบ", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "กำลังโหลด…"
    }

    fn search_field_label(&self) -> &str {
        "ค้นหา"
    }

    fn no_results_label(&self) -> &str {
        "ไม่มีผลลัพธ์"
    }

    fn decrease_label(&self) -> &str {
        "ลด"
    }

    fn increase_label(&self) -> &str {
        "เพิ่ม"
    }

    fn row_selected_label(&self) -> &str {
        "เลือกแถวแล้ว"
    }

    fn row_deselected_label(&self) -> &str {
        "ยกเลิกการเลือกแถวแล้ว"
    }

    fn all_rows_selected_label(&self) -> &str {
        "เลือกทุกแถวแล้ว"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "ยกเลิกการเลือกทุกแถวแล้ว"
    }

    fn hour_label(&self) -> &str {
        "ชั่วโมง"
    }

    fn minute_label(&self) -> &str {
        "นาที"
    }

    /// Kept as the Latin abbreviations, as Thai time pickers show them.
    fn ante_meridiem_label(&self) -> &str {
        "AM"
    }

    /// See [`Thai::ante_meridiem_label`].
    fn post_meridiem_label(&self) -> &str {
        "PM"
    }

    fn start_label(&self) -> &str {
        "เริ่มต้น"
    }

    fn end_label(&self) -> &str {
        "สิ้นสุด"
    }

    /// "11–20 จาก 4,000".
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 จาก 0".to_string();
        }
        format!(
            "{}–{} จาก {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "แถวต่อหน้า"
    }

    fn selected_row_count_label(&self, count: usize) -> String {
        format!("เลือกแล้ว {} รายการ", self.group_digits(count))
    }

    /// A comma, with the Western digits Thai interfaces use.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, ",")
    }

    fn previous_month_label(&self) -> &str {
        "เดือนที่แล้ว"
    }

    fn next_month_label(&self) -> &str {
        "เดือนหน้า"
    }

    /// อาทิตย์ (Sunday) to เสาร์ (Saturday); Thursday takes two letters, so that it is not
    /// Wednesday's.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["อา", "จ", "อ", "พ", "พฤ", "ศ", "ส"]
    }

    /// Sunday, written out rather than left to the trait.
    fn first_day_of_week_index(&self) -> usize {
        0
    }

    fn months(&self) -> [&str; 12] {
        [
            "มกราคม",
            "กุมภาพันธ์",
            "มีนาคม",
            "เมษายน",
            "พฤษภาคม",
            "มิถุนายน",
            "กรกฎาคม",
            "สิงหาคม",
            "กันยายน",
            "ตุลาคม",
            "พฤศจิกายน",
            "ธันวาคม",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "เลือกวันที่"
    }

    fn date_range_picker_title(&self) -> &str {
        "เลือกช่วง"
    }

    fn date_input_title(&self) -> &str {
        "ป้อนวันที่"
    }

    fn date_input_label(&self) -> &str {
        "วันที่"
    }

    /// วว (day), ดด (month), ปปปป (year): day first, the order the field reads.
    fn date_input_pattern(&self) -> &str {
        "วว/ดด/ปปปป"
    }

    fn date_input_format(&self) -> &str {
        "dd/mm/yyyy"
    }

    fn input_mode_label(&self) -> &str {
        "เปลี่ยนเป็นโหมดป้อนข้อความ"
    }

    fn calendar_mode_label(&self) -> &str {
        "เปลี่ยนเป็นปฏิทิน"
    }

    fn invalid_date_label(&self) -> &str {
        "รูปแบบไม่ถูกต้อง"
    }

    fn date_out_of_range_label(&self) -> &str {
        "ไม่อยู่ในช่วง"
    }

    fn save_button_label(&self) -> &str {
        "บันทึก"
    }

    fn select_year_label(&self) -> &str {
        "เลือกปี"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["อา.", "จ.", "อ.", "พ.", "พฤ.", "ศ.", "ส."]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "ม.ค.",
            "ก.พ.",
            "มี.ค.",
            "เม.ย.",
            "พ.ค.",
            "มิ.ย.",
            "ก.ค.",
            "ส.ค.",
            "ก.ย.",
            "ต.ค.",
            "พ.ย.",
            "ธ.ค.",
        ]
    }

    /// "จ. 17 ส.ค.".
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{} {day} {}",
            self.short_weekdays()[weekday % 7],
            self.short_months()[month_index(month)]
        )
    }

    /// "สิงหาคม 2026".
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} {year}", self.months()[month_index(month)])
    }

    /// "17 ส.ค.".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{day} {}", self.short_months()[month_index(month)])
    }

    /// "17 สิงหาคม 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{day} {} {year}", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The Vietnamese table** (milestone 813).
///
/// The week starts on **Monday**, a date is typed day first, a number is grouped with a
/// **stop**, and the clock has twenty-four hours. A month is *tháng* and its number, so
/// "August 2026" is "tháng 8 năm 2026". Every letter is in DejaVu Sans, the bundled face.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Vietnamese;

impl Localizations for Vietnamese {
    fn back_button_label(&self) -> &str {
        "Quay lại"
    }

    fn close_button_label(&self) -> &str {
        "Đóng"
    }

    fn open_drawer_label(&self) -> &str {
        "Mở menu di chuyển"
    }

    fn tab_label(&self, index: usize, count: usize) -> String {
        format!("Tab {index} trong tổng số {count}")
    }

    fn signed_in_label(&self) -> &str {
        "Đã đăng nhập"
    }

    fn show_accounts_label(&self) -> &str {
        "Hiển thị tài khoản"
    }

    fn hide_accounts_label(&self) -> &str {
        "Ẩn tài khoản"
    }

    fn clear_button_label(&self) -> &str {
        "Xoá"
    }

    fn more_options_label(&self) -> &str {
        "Tùy chọn khác"
    }

    /// "OK" in Vietnamese interfaces too.
    fn ok_button_label(&self) -> &str {
        "OK"
    }

    fn cancel_button_label(&self) -> &str {
        "Huỷ"
    }

    fn cut_button_label(&self) -> &str {
        "Cắt"
    }

    fn copy_button_label(&self) -> &str {
        "Sao chép"
    }

    fn paste_button_label(&self) -> &str {
        "Dán"
    }

    fn select_all_button_label(&self) -> &str {
        "Chọn tất cả"
    }

    fn more_button_label(&self) -> &str {
        "Thêm"
    }

    fn remove_button_label(&self) -> &str {
        "Xóa"
    }

    fn licenses_label(&self) -> &str {
        "Giấy phép"
    }

    fn license_count_label(&self, count: usize) -> String {
        format!("{} giấy phép", self.group_digits(count))
    }

    fn loading_label(&self) -> &str {
        "Đang tải…"
    }

    fn search_field_label(&self) -> &str {
        "Tìm kiếm"
    }

    fn no_results_label(&self) -> &str {
        "Không có kết quả"
    }

    fn decrease_label(&self) -> &str {
        "Giảm"
    }

    fn increase_label(&self) -> &str {
        "Tăng"
    }

    fn row_selected_label(&self) -> &str {
        "Đã chọn hàng"
    }

    fn row_deselected_label(&self) -> &str {
        "Đã bỏ chọn hàng"
    }

    fn all_rows_selected_label(&self) -> &str {
        "Đã chọn tất cả các hàng"
    }

    fn all_rows_deselected_label(&self) -> &str {
        "Đã bỏ chọn tất cả các hàng"
    }

    fn hour_label(&self) -> &str {
        "Giờ"
    }

    fn minute_label(&self) -> &str {
        "Phút"
    }

    /// *Sáng* and *chiều*, morning and afternoon.
    fn ante_meridiem_label(&self) -> &str {
        "SÁNG"
    }

    fn post_meridiem_label(&self) -> &str {
        "CHIỀU"
    }

    fn start_label(&self) -> &str {
        "Bắt đầu"
    }

    fn end_label(&self) -> &str {
        "Kết thúc"
    }

    /// "11–20 trong tổng số 4.000".
    fn page_range_label(&self, start: usize, end: usize, total: usize) -> String {
        if total == 0 {
            return "0 trong tổng số 0".to_string();
        }
        format!(
            "{}–{} trong tổng số {}",
            self.group_digits(start),
            self.group_digits(end),
            self.group_digits(total)
        )
    }

    fn rows_per_page_label(&self) -> &str {
        "Số hàng mỗi trang"
    }

    fn selected_row_count_label(&self, count: usize) -> String {
        format!("Đã chọn {} mục", self.group_digits(count))
    }

    /// A stop: `4.000`.
    fn group_digits(&self, number: usize) -> String {
        group_with(number, ".")
    }

    fn previous_month_label(&self) -> &str {
        "Tháng trước"
    }

    fn next_month_label(&self) -> &str {
        "Tháng sau"
    }

    /// Chủ nhật (Sunday), then thứ hai to thứ bảy: the second to the seventh day.
    fn narrow_weekdays(&self) -> [&str; 7] {
        ["CN", "T2", "T3", "T4", "T5", "T6", "T7"]
    }

    /// Monday.
    fn first_day_of_week_index(&self) -> usize {
        1
    }

    fn months(&self) -> [&str; 12] {
        [
            "tháng 1",
            "tháng 2",
            "tháng 3",
            "tháng 4",
            "tháng 5",
            "tháng 6",
            "tháng 7",
            "tháng 8",
            "tháng 9",
            "tháng 10",
            "tháng 11",
            "tháng 12",
        ]
    }

    fn date_picker_title(&self) -> &str {
        "Chọn ngày"
    }

    fn date_range_picker_title(&self) -> &str {
        "Chọn phạm vi"
    }

    fn date_input_title(&self) -> &str {
        "Nhập ngày"
    }

    fn date_input_label(&self) -> &str {
        "Ngày"
    }

    /// Day first, the order Vietnamese writes a date and the field reads it.
    fn date_input_pattern(&self) -> &str {
        "dd/mm/yyyy"
    }

    fn date_input_format(&self) -> &str {
        "dd/mm/yyyy"
    }

    fn input_mode_label(&self) -> &str {
        "Chuyển sang chế độ nhập"
    }

    fn calendar_mode_label(&self) -> &str {
        "Chuyển sang lịch"
    }

    fn invalid_date_label(&self) -> &str {
        "Định dạng không hợp lệ."
    }

    fn date_out_of_range_label(&self) -> &str {
        "Ngoài phạm vi."
    }

    fn save_button_label(&self) -> &str {
        "Lưu"
    }

    fn select_year_label(&self) -> &str {
        "Chọn năm"
    }

    fn short_weekdays(&self) -> [&str; 7] {
        ["CN", "Th 2", "Th 3", "Th 4", "Th 5", "Th 6", "Th 7"]
    }

    fn short_months(&self) -> [&str; 12] {
        [
            "thg 1", "thg 2", "thg 3", "thg 4", "thg 5", "thg 6", "thg 7", "thg 8", "thg 9",
            "thg 10", "thg 11", "thg 12",
        ]
    }

    /// "Th 2, 17 thg 8".
    fn medium_date(&self, weekday: usize, month: u32, day: u32) -> String {
        format!(
            "{}, {day} {}",
            self.short_weekdays()[weekday % 7],
            self.short_months()[month_index(month)]
        )
    }

    /// "tháng 8 năm 2026".
    fn month_year(&self, month: u32, year: i32) -> String {
        format!("{} năm {year}", self.months()[month_index(month)])
    }

    /// "17 thg 8".
    fn short_month_day(&self, month: u32, day: u32) -> String {
        format!("{day} {}", self.short_months()[month_index(month)])
    }

    /// "17 tháng 8, 2026".
    fn long_date(&self, year: i32, month: u32, day: u32) -> String {
        format!("{day} {}, {year}", self.months()[month_index(month)])
    }

    fn uses_24_hour_clock(&self) -> bool {
        true
    }
}

/// **The languages the framework has its own words in**, as language subtags: what
/// [`for_locale`] can answer with something other than English.
pub const LANGUAGES: [&str; 7] = ["en", "fr", "de", "es", "ja", "th", "vi"];

/// **The framework's own table for `locale`**, by its language: `es-MX` gets [`Spanish`],
/// `fr-CA` [`French`], and a language the framework has no words in gets [`English`].
///
/// This is what the shell installs when an application gives no table of its own, so an
/// application listing German among its supported locales gets German calendars, dialogs and
/// selection bars without writing anything. The tables are kept per thread once made, so
/// asking every frame costs a reference count.
#[inline(never)]
pub fn for_locale(locale: &crate::locale::Locale) -> Rc<dyn Localizations> {
    let index = LANGUAGES
        .iter()
        .position(|language| *language == locale.language_code())
        .unwrap_or(0);
    BUILT_IN.with(|built_in| {
        built_in[index]
            .get_or_init(|| match LANGUAGES[index] {
                "fr" => Rc::new(French) as Rc<dyn Localizations>,
                "de" => Rc::new(German),
                "es" => Rc::new(Spanish),
                "ja" => Rc::new(Japanese),
                "th" => Rc::new(Thai),
                "vi" => Rc::new(Vietnamese),
                _ => Rc::new(English),
            })
            .clone()
    })
}

thread_local! {
    /// The built-in tables, one per entry of [`LANGUAGES`], made the first time each is
    /// asked for.
    static BUILT_IN: [std::cell::OnceCell<Rc<dyn Localizations>>; LANGUAGES.len()] =
        const { [const { std::cell::OnceCell::new() }; LANGUAGES.len()] };
}

thread_local! {
    /// The table in force on this thread. `RefCell` rather than `Cell`, the value being
    /// a trait object behind an `Rc` and not `Copy`.
    static AMBIENT: RefCell<Option<Rc<dyn Localizations>>> = const { RefCell::new(None) };
}

/// **The table in force**, or [`English`] when nothing has been installed.
///
/// Cheap enough to call from a paint: it is a reference count, not a table.
#[inline(never)]
pub fn of() -> Rc<dyn Localizations> {
    AMBIENT.with(|ambient| {
        ambient
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(English) as Rc<dyn Localizations>)
            .clone()
    })
}

/// Installs `table` for this thread, from now on.
///
/// The shell does this every frame from [`Application::localizations`], so an application
/// that changes language while it is running is obeyed on the next frame.
///
/// [`Application::localizations`]: https://docs.rs/frus-shell
#[inline(never)]
pub fn install(table: Rc<dyn Localizations>) {
    AMBIENT.with(|ambient| *ambient.borrow_mut() = Some(table));
}

/// Runs `f` with `table` in force, and puts back whatever was there before — including
/// when `f` panics, so one bad frame cannot leave a stale language installed for every
/// frame after it.
pub fn scope<R>(table: Rc<dyn Localizations>, f: impl FnOnce() -> R) -> R {
    let _restore = Restore(swap(Some(table)));
    f()
}

/// Puts `next` in place, and gives back what was there. Not generic, and not inlined: see
/// `component.rs` on thread-locals and hot patches.
#[inline(never)]
fn swap(next: Option<Rc<dyn Localizations>>) -> Option<Rc<dyn Localizations>> {
    AMBIENT.with(|ambient| std::mem::replace(&mut *ambient.borrow_mut(), next))
}

/// Puts back the previous table when dropped, panic or not.
struct Restore(Option<Rc<dyn Localizations>>);

impl Drop for Restore {
    fn drop(&mut self) {
        swap(self.0.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table that starts its weeks on Monday and says so in French.
    struct Fr;

    impl Localizations for Fr {
        fn back_button_label(&self) -> &str {
            "Retour"
        }

        fn first_day_of_week_index(&self) -> usize {
            1
        }

        fn narrow_weekdays(&self) -> [&str; 7] {
            ["D", "L", "M", "M", "J", "V", "S"]
        }
    }

    /// Reads this file back, with its line endings normalised — a checkout on Windows
    /// hands it over with CRLF while the patterns below are written with `\n`.
    fn source() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/localizations.rs");
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {path}: {e}"))
            .replace("\r\n", "\n")
    }

    /// The names of the methods declared between `start` and the end of its block.
    fn methods_in(source: &str, start: &str) -> Vec<String> {
        let body = &source[source.find(start).expect("the block") + start.len()..];
        let end = body.find("\n}\n").expect("the end of the block");
        body[..end]
            .lines()
            .filter_map(|line| line.trim().strip_prefix("fn "))
            .map(|rest| rest[..rest.find(['(', '<']).unwrap_or(rest.len())].to_string())
            .collect()
    }

    /// **A French table answers every entry the trait declares**, checked against the
    /// trait itself rather than against a list kept up to date by hand.
    ///
    /// This is the bug the whole module is about, one level down: an entry left at its
    /// default is an **English string in a French interface**, and it compiles, and no
    /// test that only checks the entries someone remembered would ever say so. An entry
    /// whose right answer in French *is* the English one — "OK", "Minute", "AM" — is
    /// written out with a reason beside it, so silence never means agreement.
    ///
    /// Since milestone 813 it reads **every** table the framework ships, so an entry added
    /// to the trait fails here until each language has said it.
    #[test]
    fn every_table_answers_every_entry_the_trait_declares() {
        let source = source();
        let declared = methods_in(&source, "pub trait Localizations {");
        assert!(declared.len() >= 60, "found only {declared:?}");
        for table in [
            "French",
            "German",
            "Spanish",
            "Japanese",
            "Thai",
            "Vietnamese",
        ] {
            let answered = methods_in(&source, &format!("impl Localizations for {table} {{"));
            for entry in &declared {
                assert!(
                    answered.contains(entry),
                    "`{entry}` is not in the {table} table, so its readers get the English words"
                );
            }
        }
        // And every table is one `for_locale` can answer with.
        let shipped = source
            .lines()
            .filter(|line| line.starts_with("impl Localizations for "))
            .count();
        assert_eq!(
            shipped,
            LANGUAGES.len(),
            "a table the framework ships that is not in LANGUAGES"
        );
    }

    /// Every string entry of a table, by name, to compare one table with another.
    fn said(t: &dyn Localizations) -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = vec![
            ("back", t.back_button_label().into()),
            ("close", t.close_button_label().into()),
            ("drawer", t.open_drawer_label().into()),
            ("tab", t.tab_label(1, 3)),
            ("signed in", t.signed_in_label().into()),
            ("show accounts", t.show_accounts_label().into()),
            ("hide accounts", t.hide_accounts_label().into()),
            ("clear", t.clear_button_label().into()),
            ("more options", t.more_options_label().into()),
            ("ok", t.ok_button_label().into()),
            ("cancel", t.cancel_button_label().into()),
            ("cut", t.cut_button_label().into()),
            ("copy", t.copy_button_label().into()),
            ("paste", t.paste_button_label().into()),
            ("select all", t.select_all_button_label().into()),
            ("more", t.more_button_label().into()),
            ("remove", t.remove_button_label().into()),
            ("licences", t.licenses_label().into()),
            ("licence count", t.license_count_label(3)),
            ("loading", t.loading_label().into()),
            ("search", t.search_field_label().into()),
            ("no results", t.no_results_label().into()),
            ("decrease", t.decrease_label().into()),
            ("increase", t.increase_label().into()),
            ("row selected", t.row_selected_label().into()),
            ("row deselected", t.row_deselected_label().into()),
            ("all selected", t.all_rows_selected_label().into()),
            ("all deselected", t.all_rows_deselected_label().into()),
            ("hour", t.hour_label().into()),
            ("minute", t.minute_label().into()),
            ("am", t.ante_meridiem_label().into()),
            ("pm", t.post_meridiem_label().into()),
            ("start", t.start_label().into()),
            ("end", t.end_label().into()),
            ("page range", t.page_range_label(11, 20, 40_000)),
            ("rows per page", t.rows_per_page_label().into()),
            ("selected count", t.selected_row_count_label(3)),
            ("previous month", t.previous_month_label().into()),
            ("next month", t.next_month_label().into()),
            ("date picker", t.date_picker_title().into()),
            ("range picker", t.date_range_picker_title().into()),
            ("date input", t.date_input_title().into()),
            ("date label", t.date_input_label().into()),
            ("input mode", t.input_mode_label().into()),
            ("calendar mode", t.calendar_mode_label().into()),
            ("invalid", t.invalid_date_label().into()),
            ("out of range", t.date_out_of_range_label().into()),
            ("save", t.save_button_label().into()),
            ("select year", t.select_year_label().into()),
            ("medium date", t.medium_date(1, 8, 17)),
            ("month year", t.month_year(3, 2026)),
            ("short month day", t.short_month_day(8, 17)),
            ("long date", t.long_date(2026, 8, 17)),
        ];
        out.extend(t.months().iter().map(|m| ("month", m.to_string())));
        out.extend(
            t.short_weekdays()
                .iter()
                .map(|d| ("weekday", d.to_string())),
        );
        out
    }

    /// **Each table says its own words**: every entry differs from the English one, save
    /// the few each table names whose right answer *is* the English one — a machine pass
    /// that copied English through would pass the test above and fail this one.
    #[test]
    fn every_table_says_its_own_words() {
        let english = said(&English);
        let tables: [(&str, &dyn Localizations, &[&str]); 6] = [
            (
                "fr",
                &French,
                &["OK", "Minute", "AM", "PM", "Licences", "Date"],
            ),
            // "OK" is "OK" in German too, and so are "Minute", "April", "August", "September",
            // "November"; AM and PM stay for a clock German does not use.
            (
                "de",
                &German,
                &[
                    "OK",
                    "Minute",
                    "AM",
                    "PM",
                    "April",
                    "August",
                    "September",
                    "November",
                ],
            ),
            ("es", &Spanish, &[]),
            ("ja", &Japanese, &["OK", "AM", "PM"]),
            ("th", &Thai, &["AM", "PM"]),
            ("vi", &Vietnamese, &["OK"]),
        ];
        for (language, table, same_on_purpose) in tables {
            for ((what, theirs), (_, ours)) in said(table).into_iter().zip(&english) {
                if same_on_purpose.contains(&theirs.as_str()) {
                    continue;
                }
                assert_ne!(&theirs, ours, "{language}: {what} was not translated");
            }
        }
    }

    /// **Picked by language, with English for the rest**: `es-MX` is Spanish, `zh` is
    /// English, and asking twice hands back the same table rather than making another.
    #[test]
    fn a_table_is_picked_by_its_language_and_english_answers_the_rest() {
        use crate::locale::Locale;
        let pick = |tag: &str| for_locale(&Locale::parse(tag).unwrap());
        assert_eq!(pick("es-MX").cancel_button_label(), "Cancelar");
        assert_eq!(pick("es-419").cancel_button_label(), "Cancelar");
        assert_eq!(pick("de-AT").cancel_button_label(), "Abbrechen");
        assert_eq!(pick("fr-CA").cancel_button_label(), "Annuler");
        assert_eq!(pick("ja-JP").cancel_button_label(), "キャンセル");
        assert_eq!(pick("th").cancel_button_label(), "ยกเลิก");
        assert_eq!(pick("vi-VN").cancel_button_label(), "Huỷ");
        assert_eq!(pick("zh-Hant-TW").cancel_button_label(), "Cancel");
        assert_eq!(pick("en-GB").cancel_button_label(), "Cancel");
        assert!(
            Rc::ptr_eq(&pick("de"), &pick("de-CH")),
            "made once per thread"
        );
        for language in LANGUAGES {
            let table = pick(language);
            assert_eq!(
                language == "en",
                table.cancel_button_label() == "Cancel",
                "{language} has a table of its own"
            );
        }
    }

    /// **The parts that are not words**, per language: the first day of the week, the clock,
    /// the order a date is typed in and how it is written out, and how a number is grouped.
    #[test]
    fn each_language_gets_its_calendar_clock_and_numbers_right() {
        // The week: Monday in Europe and Vietnam, Sunday in Japan and Thailand.
        let first: Vec<usize> = [
            &English as &dyn Localizations,
            &French,
            &German,
            &Spanish,
            &Japanese,
            &Thai,
            &Vietnamese,
        ]
        .iter()
        .map(|t| t.first_day_of_week_index())
        .collect();
        assert_eq!(first, vec![0, 1, 1, 1, 0, 0, 1]);
        assert_eq!(Spanish.narrow_weekdays()[3], "X", "miércoles is not martes");

        // The clock.
        assert!(!English.uses_24_hour_clock());
        for t in [&German as &dyn Localizations, &Spanish, &Japanese, &Thai] {
            assert!(t.uses_24_hour_clock());
        }

        // A date typed and written in the language's order.
        let august = crate::CalendarDate::new(2026, 8, 7);
        let typed = |t: &dyn Localizations| crate::format_date(august, t.date_input_format());
        assert_eq!(typed(&English), "08/07/2026");
        assert_eq!(typed(&German), "07.08.2026");
        assert_eq!(typed(&Japanese), "2026/08/07");
        assert_eq!(typed(&Thai), "07/08/2026");
        assert_eq!(
            crate::parse_date("7.8.2026", German.date_input_format()),
            Some(august),
            "a German date, read day first"
        );
        assert_eq!(
            crate::parse_date("2026/8/7", Japanese.date_input_format()),
            Some(august)
        );

        // Written out.
        assert_eq!(German.long_date(2026, 8, 17), "17. August 2026");
        assert_eq!(Spanish.month_year(8, 2026), "agosto de 2026");
        assert_eq!(Japanese.month_year(8, 2026), "2026年8月");
        assert_eq!(Japanese.medium_date(1, 8, 17), "8月17日(月)");
        assert_eq!(
            Thai.month_year(8, 2026),
            "สิงหาคม 2026",
            "the Gregorian year"
        );
        assert_eq!(Vietnamese.month_year(8, 2026), "tháng 8 năm 2026");
        assert_eq!(English.short_month_day(8, 17), "Aug 17");
        assert_eq!(German.short_month_day(8, 17), "17. Aug.");

        // Numbers.
        assert_eq!(German.group_digits(4_000), "4.000");
        assert_eq!(
            Spanish.group_digits(4_000),
            "4000",
            "Spanish waits for five digits"
        );
        assert_eq!(Spanish.group_digits(40_000), "40.000");
        assert_eq!(Japanese.group_digits(1_234_567), "1,234,567");
        assert_eq!(
            Vietnamese.page_range_label(1, 10, 4_000),
            "1–10 trong tổng số 4.000"
        );
        assert_eq!(Spanish.selected_row_count_label(1), "1 seleccionada");
        assert_eq!(Spanish.selected_row_count_label(2), "2 seleccionadas");
        assert_eq!(German.license_count_label(1), "1\u{a0}Lizenz");
    }

    /// **And every one of them is a French sentence**, save the few whose right answer in
    /// French is the English one. A machine-translated table would pass the test above and
    /// fail this one on nothing at all; this is what catches an entry answered by copying.
    #[test]
    fn the_french_words_are_not_the_english_ones() {
        // "OK" is "OK" in a good many languages, "Minute" is spelt the same in both, and
        // French has no everyday words for the halves of a clock it does not use.
        let same_on_purpose = ["OK", "Minute", "AM", "PM"];
        let (en, fr) = (English, French);
        let entries: Vec<(&str, &str, &str)> = vec![
            ("back", en.back_button_label(), fr.back_button_label()),
            ("close", en.close_button_label(), fr.close_button_label()),
            ("drawer", en.open_drawer_label(), fr.open_drawer_label()),
            ("signed in", en.signed_in_label(), fr.signed_in_label()),
            (
                "show accounts",
                en.show_accounts_label(),
                fr.show_accounts_label(),
            ),
            (
                "hide accounts",
                en.hide_accounts_label(),
                fr.hide_accounts_label(),
            ),
            ("clear", en.clear_button_label(), fr.clear_button_label()),
            ("ok", en.ok_button_label(), fr.ok_button_label()),
            ("cancel", en.cancel_button_label(), fr.cancel_button_label()),
            ("search", en.search_field_label(), fr.search_field_label()),
            ("no results", en.no_results_label(), fr.no_results_label()),
            ("decrease", en.decrease_label(), fr.decrease_label()),
            ("increase", en.increase_label(), fr.increase_label()),
            (
                "row selected",
                en.row_selected_label(),
                fr.row_selected_label(),
            ),
            (
                "row deselected",
                en.row_deselected_label(),
                fr.row_deselected_label(),
            ),
            (
                "all selected",
                en.all_rows_selected_label(),
                fr.all_rows_selected_label(),
            ),
            (
                "all deselected",
                en.all_rows_deselected_label(),
                fr.all_rows_deselected_label(),
            ),
            ("hour", en.hour_label(), fr.hour_label()),
            ("minute", en.minute_label(), fr.minute_label()),
            ("am", en.ante_meridiem_label(), fr.ante_meridiem_label()),
            ("pm", en.post_meridiem_label(), fr.post_meridiem_label()),
            ("start", en.start_label(), fr.start_label()),
            ("end", en.end_label(), fr.end_label()),
            (
                "rows per page",
                en.rows_per_page_label(),
                fr.rows_per_page_label(),
            ),
            (
                "previous month",
                en.previous_month_label(),
                fr.previous_month_label(),
            ),
            ("next month", en.next_month_label(), fr.next_month_label()),
        ];
        for (what, english, french) in entries {
            if same_on_purpose.contains(&french) {
                continue;
            }
            assert_ne!(english, french, "{what} was not translated");
        }
        assert_ne!(en.tab_label(1, 3), fr.tab_label(1, 3));
        assert_ne!(
            en.page_range_label(11, 20, 4000),
            fr.page_range_label(11, 20, 4000)
        );
        assert_ne!(
            en.selected_row_count_label(3),
            fr.selected_row_count_label(3)
        );
        assert_ne!(en.group_digits(4000), fr.group_digits(4000));
        assert_ne!(en.months(), fr.months());
        assert_ne!(en.narrow_weekdays(), fr.narrow_weekdays());
    }

    /// **The parts that are not words.** A week that starts on Monday and a month written
    /// in lower case are the two things a translation of the strings alone would leave
    /// behind, and the first of them puts the days in the wrong columns.
    #[test]
    fn the_french_table_gets_the_parts_that_are_not_words_right() {
        assert_eq!(French.first_day_of_week_index(), 1, "lundi");
        assert_eq!(
            French.narrow_weekdays()[French.first_day_of_week_index()],
            "L",
            "the index is into a list that always starts on Sunday"
        );
        for month in French.months() {
            assert_eq!(
                month.chars().next().unwrap().to_lowercase().to_string(),
                month.chars().next().unwrap().to_string(),
                "{month} is capitalised, which is an English habit"
            );
        }
    }

    /// **The words reach the widgets that say them.**
    ///
    /// A table nobody reads is a table that translates nothing, and until this milestone
    /// five widgets said their own English out loud where no table could reach it. This
    /// walks the ones that were rewired and looks for French on the far side — through the
    /// scene and the accessibility tree, not through the table it already tested.
    ///
    /// Each is built **inside** the scope, which is the contract: a widget composes its
    /// children when it is constructed, so it speaks the language in force at that moment.
    /// In an application that is every frame, the shell installing the table before the
    /// view is built.
    #[test]
    fn the_words_reach_the_widgets_that_say_them() {
        use crate::theme::Theme;
        use crate::ui::build_ui;
        use frus_core::Size;

        scope(Rc::new(French), || {
            // A calendar: the month over it, and the arrows either side of it.
            let picker: crate::DatePicker<()> =
                crate::DatePicker::new(2026, 1, Some(15), |_: u32| (), |_: i32| ());
            let ui = build_ui(
                &picker,
                Size::new(320.0, 360.0),
                &crate::Runtime::default(),
                &Theme::default(),
            );
            let words: Vec<String> = ui
                .scene()
                .primitives()
                .iter()
                .filter_map(|p| match p {
                    frus_core::Primitive::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert!(
                words.iter().any(|w| w == "janvier 2026"),
                "the month, in lower case: {words:?}"
            );
            let labels: Vec<String> = ui
                .semantics()
                .iter()
                .filter_map(|(_, _, s)| s.label.clone())
                .collect();
            assert!(
                labels.iter().any(|l| l == "Mois précédent"),
                "the arrows: {labels:?}"
            );

            // A stepper: two buttons whose glyphs are a minus and a plus, and which say
            // nothing at all out loud without these.
            let stepper = crate::Stepper::new(3, |_: i32| ());
            let buttons: Vec<String> = crate::widget::Widget::<()>::children(&stepper)
                .iter()
                .filter_map(|child| child.semantics().and_then(|s| s.label))
                .collect();
            assert_eq!(buttons, vec!["Moins".to_string(), "Plus".to_string()]);

            // A table announcing what a tick will do.
            let table = crate::Table::<()>::new(2)
                .header(&["Nom", "Note"])
                .checkboxes(|_| (), ())
                .row(&["Ada", "5"]);
            let row = crate::widget::Widget::<()>::children(&table)[1].children()[0].announce();
            assert_eq!(row.as_deref(), Some("Ligne sélectionnée"));
        });
    }

    /// **A calendar's heading is written the language's way** (milestone 813): the year
    /// first in Japanese, *de* between them in Spanish — not a month's name glued to a year.
    #[test]
    fn a_calendar_heading_is_written_the_language_s_way() {
        use crate::theme::Theme;
        use crate::ui::build_ui;
        use frus_core::Size;

        let heading = |table: Rc<dyn Localizations>| {
            scope(table, || {
                let picker: crate::DatePicker<()> =
                    crate::DatePicker::new(2026, 1, Some(15), |_: u32| (), |_: i32| ());
                let ui = build_ui(
                    &picker,
                    Size::new(320.0, 360.0),
                    &crate::Runtime::default(),
                    &Theme::default(),
                );
                ui.scene()
                    .primitives()
                    .iter()
                    .filter_map(|p| match p {
                        frus_core::Primitive::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<String>>()
            })
        };
        let ja = heading(Rc::new(Japanese));
        assert!(ja.iter().any(|w| w == "2026年1月"), "{ja:?}");
        let es = heading(Rc::new(Spanish));
        assert!(es.iter().any(|w| w == "enero de 2026"), "{es:?}");
        let de = heading(Rc::new(German));
        assert!(de.iter().any(|w| w == "Januar 2026"), "{de:?}");
    }

    /// **Nothing installed still answers**, in English — which is what every string
    /// constant this replaces already said.
    #[test]
    fn the_words_are_english_until_someone_says_otherwise() {
        assert_eq!(of().back_button_label(), "Back");
        assert_eq!(of().close_button_label(), "Close");
        assert_eq!(of().first_day_of_week_index(), 0);
        assert_eq!(of().months()[0], "January");
    }

    /// A table overrides only what differs, and the rest stays as the trait wrote it.
    #[test]
    fn a_table_says_only_what_differs() {
        scope(Rc::new(Fr), || {
            assert_eq!(of().back_button_label(), "Retour");
            assert_eq!(of().first_day_of_week_index(), 1);
            assert_eq!(
                of().close_button_label(),
                "Close",
                "not said, so the trait's own answer stands"
            );
        });
    }

    /// And a scope puts back what it found, **including when what it runs panics** — one
    /// bad frame must not leave a language installed for every frame after it.
    #[test]
    fn a_scope_puts_back_what_it_found() {
        assert_eq!(of().back_button_label(), "Back");
        scope(Rc::new(Fr), || {
            assert_eq!(of().back_button_label(), "Retour");
        });
        assert_eq!(of().back_button_label(), "Back", "restored");

        let fell_over = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scope(Rc::new(Fr), || panic!("one bad frame"));
        }));
        assert!(fell_over.is_err());
        assert_eq!(
            of().back_button_label(),
            "Back",
            "restored through the unwind as well"
        );
    }
}
