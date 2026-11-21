// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! What the shell decides about the system's **text services** (milestone 829): when the
//! spell checker is asked about the focused field, what its answers mean in the field's
//! characters, and what a correction or a processed selection does to the text.
//!
//! Kept apart from the Android half (`android_text_services`), which only carries
//! requests and answers across JNI, so that every decision here is tested off a device.

use frus_widgets::{SpellCheckResults, SpellCheckSpan, WidgetId};

/// One misspelled word as the platform reports it: start and length in **UTF-16 units** of
/// the text that was checked, and the suggestions.
pub(crate) type Utf16Word = (i32, i32, Vec<String>);

/// What [`SpellChecker::poll`] asks of the shell this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SpellPoll {
    /// The results shown so far belong to another field, or to none: take them away.
    pub clear: bool,
    /// Ask the checker about this text, as request number `.0`.
    pub send: Option<(i32, String)>,
}

/// The spell checker's conversation with the focused field: one request at a time, a new
/// one whenever the text differs from what was last checked, and none at all once the
/// platform said it has no checker.
#[derive(Debug, Default)]
pub(crate) struct SpellChecker {
    /// The field being checked.
    field: Option<WidgetId>,
    /// The request the checker is answering, and the text it was about.
    in_flight: Option<(i32, String)>,
    /// The text the shown results are about.
    checked: Option<String>,
    /// The next request's number.
    next: i32,
    /// The platform has no spell checker (or it is turned off): nothing more is asked.
    unavailable: bool,
}

impl SpellChecker {
    /// The focused field, when it asked to be checked, and its text: what to do now.
    pub(crate) fn poll(&mut self, focused: Option<(WidgetId, &str)>) -> SpellPoll {
        let mut poll = SpellPoll::default();
        let id = focused.map(|(id, _)| id);
        if id != self.field {
            poll.clear = self.field.is_some() || self.checked.is_some();
            self.field = id;
            self.in_flight = None;
            self.checked = None;
        }
        let Some((_, text)) = focused else {
            return poll;
        };
        if self.unavailable
            || self.in_flight.is_some()
            || self.checked.as_deref() == Some(text)
            || text.trim().is_empty() && self.checked.is_none()
        {
            return poll;
        }
        let sequence = self.next;
        self.next = self.next.wrapping_add(1);
        self.in_flight = Some((sequence, text.to_owned()));
        poll.send = Some((sequence, text.to_owned()));
        poll
    }

    /// The checker's answer to request `sequence`: `None` for "there is no checker". Returns
    /// the results to show, or `None` when the answer is stale or there is nothing to show.
    pub(crate) fn answer(
        &mut self,
        sequence: i32,
        words: Option<Vec<Utf16Word>>,
    ) -> Option<SpellCheckResults> {
        let (asked, text) = self.in_flight.take()?;
        if asked != sequence {
            // Not the request in flight: keep waiting for that one.
            self.in_flight = Some((asked, text));
            return None;
        }
        let Some(words) = words else {
            self.unavailable = true;
            log::info!("spell check: the platform offers no spell checker");
            return None;
        };
        let spans = spans_from_utf16(&text, &words);
        self.checked = Some(text.clone());
        Some(SpellCheckResults { text, spans })
    }
}

/// The platform's words, from UTF-16 units into the text's characters. A word that does not
/// fall on character boundaries inside the text is dropped rather than guessed at.
pub(crate) fn spans_from_utf16(text: &str, words: &[Utf16Word]) -> Vec<SpellCheckSpan> {
    // The character index at each UTF-16 offset that starts a character, and the end.
    let mut at = std::collections::HashMap::new();
    let mut unit = 0usize;
    for (index, c) in text.chars().enumerate() {
        at.insert(unit, index);
        unit += c.len_utf16();
    }
    at.insert(unit, text.chars().count());
    words
        .iter()
        .filter_map(|(start, length, suggestions)| {
            let start = usize::try_from(*start).ok()?;
            let end = start.checked_add(usize::try_from(*length).ok()?)?;
            let (start, end) = (*at.get(&start)?, *at.get(&end)?);
            (start < end).then(|| SpellCheckSpan {
                start,
                end,
                suggestions: suggestions.clone(),
            })
        })
        .collect()
}

/// `text` with characters `start..end` replaced by `with`, and the caret after `with`.
pub(crate) fn replace_chars(text: &str, start: usize, end: usize, with: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (start, end) = (start.min(chars.len()), end.min(chars.len()).max(start));
    let mut out: String = chars[..start].iter().collect();
    out.push_str(with);
    out.extend(&chars[end..]);
    (out, start + with.chars().count())
}

/// What deleting misspelled word `start..end` takes away: the word, and the space before it
/// when there is one (or else the one after it), so that no double space is left behind.
pub(crate) fn delete_word_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (start, end) = (start.min(chars.len()), end.min(chars.len()));
    if start > 0 && chars[start - 1] == ' ' {
        (start - 1, end)
    } else if chars.get(end) == Some(&' ') {
        (start, end + 1)
    } else {
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> WidgetId {
        WidgetId::from_u64(n)
    }

    /// One request at a time, and another only once the text differs from what was checked.
    #[test]
    fn the_checker_is_asked_once_per_text() {
        let mut checker = SpellChecker::default();
        let first = checker.poll(Some((id(1), "helo")));
        let (seq, text) = first.send.expect("asked");
        assert_eq!(text, "helo");
        // In flight: typing on asks nothing more yet.
        assert_eq!(checker.poll(Some((id(1), "helo w"))).send, None);
        let shown = checker
            .answer(seq, Some(vec![(0, 4, vec!["hello".into()])]))
            .expect("results");
        assert_eq!(shown.spans[0].end, 4);
        // The text moved on while the answer was coming: asked again.
        assert_eq!(
            checker.poll(Some((id(1), "helo w"))).send.map(|s| s.1),
            Some("helo w".into())
        );
    }

    /// The same text is not asked about twice.
    #[test]
    fn an_unchanged_text_is_not_asked_again() {
        let mut checker = SpellChecker::default();
        let (seq, _) = checker.poll(Some((id(1), "helo"))).send.unwrap();
        checker.answer(seq, Some(Vec::new()));
        assert_eq!(checker.poll(Some((id(1), "helo"))), SpellPoll::default());
    }

    /// Focus moving on takes the results with it, and a late answer for the old field is
    /// dropped.
    #[test]
    fn another_field_starts_afresh() {
        let mut checker = SpellChecker::default();
        let (old, _) = checker.poll(Some((id(1), "helo"))).send.unwrap();
        let moved = checker.poll(Some((id(2), "wrld")));
        assert!(moved.clear);
        let (new, _) = moved.send.expect("the new field is asked about");
        assert!(checker.answer(old, Some(Vec::new())).is_none());
        assert!(checker.answer(new, Some(Vec::new())).is_some());
        // And no field at all clears.
        assert!(checker.poll(None).clear);
    }

    /// A platform with no checker is asked once, and never again.
    #[test]
    fn no_checker_means_no_more_questions() {
        let mut checker = SpellChecker::default();
        let (seq, _) = checker.poll(Some((id(1), "helo"))).send.unwrap();
        assert!(checker.answer(seq, None).is_none());
        assert_eq!(checker.poll(Some((id(1), "other"))).send, None);
    }

    /// UTF-16 offsets become character indices, past characters that take two units.
    #[test]
    fn offsets_count_characters() {
        // "😀 helo": the emoji is two UTF-16 units and one character.
        let spans = spans_from_utf16("😀 helo", &[(3, 4, vec![])]);
        assert_eq!((spans[0].start, spans[0].end), (2, 6));
        // One that ends inside a character, or past the text, is dropped.
        assert!(spans_from_utf16("😀 helo", &[(1, 2, vec![])]).is_empty());
        assert!(spans_from_utf16("helo", &[(2, 9, vec![])]).is_empty());
        assert!(spans_from_utf16("helo", &[(-1, 2, vec![])]).is_empty());
    }

    /// A correction replaces the word and leaves the caret after it.
    #[test]
    fn a_correction_replaces_the_word() {
        assert_eq!(
            replace_chars("a helo b", 2, 6, "hello"),
            ("a hello b".into(), 7)
        );
        assert_eq!(replace_chars("é helo", 2, 6, "x"), ("é x".into(), 3));
    }

    /// Deleting a word takes one space with it, never two words' worth of text.
    #[test]
    fn deleting_a_word_takes_one_space() {
        assert_eq!(delete_word_range("a helo b", 2, 6), (1, 6));
        assert_eq!(delete_word_range("helo b", 0, 4), (0, 5));
        assert_eq!(delete_word_range("helo", 0, 4), (0, 4));
    }
}
