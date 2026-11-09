// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Line break opportunities (frus).
//!
//! The Unicode line breaking algorithm gives the letters of Thai, Lao, Khmer and Myanmar (the
//! `SA` class) no break opportunity between them, because those scripts put no space between
//! words: finding the words takes a dictionary. Without one, a paragraph in them breaks only
//! at spaces and punctuation, and a long sentence overflows its box or is cut mid-word.
//!
//! With the `dictionary-line-breaks` feature, the breaks between two `SA` letters come from
//! ICU4X's dictionary segmenter; every other break is the Unicode algorithm's, as before.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// The byte offsets at which a line may end, in ascending order, each one past the last byte
/// of the line it ends. The end of the text is always one.
pub(crate) fn linebreaks(text: &str) -> Vec<usize> {
    #[cfg_attr(not(feature = "dictionary-line-breaks"), allow(unused_mut))]
    let mut breaks: Vec<usize> = unicode_linebreak::linebreaks(text)
        .map(|(offset, _)| offset)
        .collect();
    #[cfg(feature = "dictionary-line-breaks")]
    if text.chars().any(is_complex_context) {
        dictionary_breaks(text, &mut breaks);
    }
    breaks
}

/// Whether `c` belongs to a script whose words are found with a dictionary: the `SA` line
/// breaking class (Thai, Lao, Khmer, Myanmar, Tai Tham, Tai Viet, New Tai Lue).
#[cfg_attr(not(feature = "dictionary-line-breaks"), allow(dead_code))]
pub(crate) fn is_complex_context(c: char) -> bool {
    matches!(c,
        '\u{0E00}'..='\u{0EFF}' // Thai, Lao
        | '\u{1000}'..='\u{109F}' // Myanmar
        | '\u{1780}'..='\u{17FF}' // Khmer
        | '\u{1950}'..='\u{19FF}' // Tai Le, New Tai Lue, Khmer Symbols
        | '\u{1A20}'..='\u{1AAF}' // Tai Tham
        | '\u{A9E0}'..='\u{A9FF}' // Myanmar Extended-B
        | '\u{AA60}'..='\u{AADF}' // Myanmar Extended-A, Tai Viet
    ) && !c.is_whitespace()
}

/// Adds the dictionary's breaks between two complex-context letters to `breaks`.
#[cfg(feature = "dictionary-line-breaks")]
fn dictionary_breaks(text: &str, breaks: &mut Vec<usize>) {
    use icu_segmenter::options::LineBreakOptions;
    use icu_segmenter::LineSegmenter;
    use std::sync::OnceLock;

    static SEGMENTER: OnceLock<icu_segmenter::LineSegmenterBorrowed<'static>> = OnceLock::new();
    let segmenter =
        SEGMENTER.get_or_init(|| LineSegmenter::new_dictionary(LineBreakOptions::default()));

    let mut added = false;
    for offset in segmenter.segment_str(text) {
        if offset == 0 || offset >= text.len() {
            continue;
        }
        let before = text[..offset].chars().next_back();
        let after = text[offset..].chars().next();
        if before.is_some_and(is_complex_context) && after.is_some_and(is_complex_context) {
            breaks.push(offset);
            added = true;
        }
    }
    if added {
        breaks.sort_unstable();
        breaks.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "I eat rice" in Thai, three words with no space between them: ฉัน กิน ข้าว.
    const THAI: &str = "ฉันกินข้าว";

    #[test]
    fn latin_breaks_are_the_unicode_algorithm() {
        assert_eq!(linebreaks("one two"), [4, 7]);
    }

    /// Without the dictionary, the Unicode algorithm alone finds no break inside a Thai
    /// sentence; with it, the breaks fall between the words.
    #[test]
    fn thai_breaks_between_words_with_the_dictionary() {
        let breaks = linebreaks(THAI);
        if cfg!(feature = "dictionary-line-breaks") {
            let first = "ฉัน".len();
            let second = first + "กิน".len();
            assert_eq!(breaks, [first, second, THAI.len()]);
        } else {
            assert_eq!(breaks, [THAI.len()]);
        }
    }
}
