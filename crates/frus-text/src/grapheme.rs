// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Where a reader sees one character: the **extended grapheme clusters** of a text.
//!
//! An editing index is a count of `char`s, but what a reader calls one character can be
//! several: a family emoji joined by zero-width joiners, a flag made of two regional
//! indicators, a hand with a skin tone, a letter with a combining accent, a Devanagari or Thai
//! syllable. The caret steps over such a cluster in one go, Backspace takes all of it, and a
//! press never lands inside it.

use unicode_segmentation::UnicodeSegmentation;

/// The **grapheme boundaries** of `text`, in characters: `0`, the start of every extended
/// grapheme cluster after the first, and the character count — ascending, with no repeat.
/// An empty text has the one boundary `0`.
pub fn grapheme_stops(text: &str) -> Vec<usize> {
    let mut stops = Vec::with_capacity(text.len() + 1);
    let mut chars = 0;
    for cluster in text.graphemes(true) {
        stops.push(chars);
        chars += cluster.chars().count();
    }
    stops.push(chars);
    stops
}

/// The grapheme boundaries of a text held as characters: [`grapheme_stops`] of their string.
pub fn grapheme_stops_of(chars: &[char]) -> Vec<usize> {
    grapheme_stops(&chars.iter().collect::<String>())
}

/// The boundary **before** character index `index` in `stops` (as [`grapheme_stops`] gives
/// them): the start of the cluster `index` is in or ends — `0` at the start.
pub fn grapheme_before(stops: &[usize], index: usize) -> usize {
    match stops.partition_point(|&stop| stop < index) {
        0 => 0,
        at => stops[at - 1],
    }
}

/// The boundary **after** character index `index` in `stops`: the end of the cluster `index`
/// starts or is inside — the last boundary at the end.
pub fn grapheme_after(stops: &[usize], index: usize) -> usize {
    let last = stops.last().copied().unwrap_or(0);
    let at = stops.partition_point(|&stop| stop <= index);
    stops.get(at).copied().unwrap_or(last)
}

/// `index` moved back to the boundary at or before it: a position inside a cluster is the
/// cluster's start.
pub fn grapheme_floor(stops: &[usize], index: usize) -> usize {
    match stops.partition_point(|&stop| stop <= index) {
        0 => 0,
        at => stops[at - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cluster, however many characters it is made of.
    #[test]
    fn a_cluster_is_one_stop_whatever_its_characters() {
        // A family: man, ZWJ, woman, ZWJ, girl — five characters.
        assert_eq!(
            grapheme_stops("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}b"),
            [0, 1, 6, 7]
        );
        // A flag: two regional indicators (T, G).
        assert_eq!(grapheme_stops("\u{1F1F9}\u{1F1EC}"), [0, 2]);
        // Two flags in a row are two clusters, not one of four or a stray pair in the middle.
        assert_eq!(
            grapheme_stops("\u{1F1F9}\u{1F1EC}\u{1F1EB}\u{1F1F7}"),
            [0, 2, 4]
        );
        // A thumb with a skin tone.
        assert_eq!(grapheme_stops("\u{1F44D}\u{1F3FD}!"), [0, 2, 3]);
        // A letter and a combining acute accent.
        assert_eq!(grapheme_stops("cafe\u{301}"), [0, 1, 2, 3, 5]);
        // Devanagari "ki" (KA + vowel sign I) and Thai "kam" (KO KAI + SARA AM).
        assert_eq!(grapheme_stops("\u{915}\u{93F}"), [0, 2]);
        assert_eq!(grapheme_stops("\u{E01}\u{E33}"), [0, 2]);
        // CR LF is one.
        assert_eq!(grapheme_stops("a\r\nb"), [0, 1, 3, 4]);
        assert_eq!(grapheme_stops(""), [0]);
    }

    #[test]
    fn before_after_and_floor_step_over_a_whole_cluster() {
        let stops = grapheme_stops("a\u{1F44D}\u{1F3FD}b"); // [0, 1, 3, 4]
        assert_eq!(grapheme_before(&stops, 3), 1);
        assert_eq!(
            grapheme_before(&stops, 2),
            1,
            "from inside: the cluster's start"
        );
        assert_eq!(grapheme_before(&stops, 0), 0);
        assert_eq!(grapheme_after(&stops, 1), 3);
        assert_eq!(
            grapheme_after(&stops, 2),
            3,
            "from inside: the cluster's end"
        );
        assert_eq!(grapheme_after(&stops, 4), 4);
        assert_eq!(grapheme_floor(&stops, 2), 1);
        assert_eq!(grapheme_floor(&stops, 3), 3);
    }
}
