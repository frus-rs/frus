// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The system's **text services** for a field (milestone 829, [#214]): what the platform's
//! spell checker found in the focused field, and the "process text" actions other installed
//! applications offer on a selection (translate, search, define).
//!
//! Both answers come from the platform, so the shell writes them here and the field reads
//! them: [`set_spell_check_results`] after each check, [`set_process_text_actions`] once,
//! the first time a selection bar opens. Nothing here costs anything until one of them has
//! been written, and a field only reads them when it asked for the service
//! ([`TextField::spell_check`](crate::TextField::spell_check)) or when its bar is built.
//!
//! Every function that touches the thread-locals is `#[inline(never)]`, so that a hot-patched
//! application reaches the same ones as frus (see `tests/thread_locals.rs`).
//!
//! [#214]: https://github.com/frus-rs/frus/issues/214

use std::cell::RefCell;

/// One misspelled word, in **character** indices of the text that was checked, with what
/// the spell checker offers instead of it, best first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellCheckSpan {
    /// The first character of the word.
    pub start: usize,
    /// One past its last character.
    pub end: usize,
    /// The replacements on offer, best first. May be empty: a word can be wrong with
    /// nothing better to suggest.
    pub suggestions: Vec<String>,
}

/// What the spell checker said about one text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpellCheckResults {
    /// The text that was checked.
    pub text: String,
    /// Its misspelled words, in order.
    pub spans: Vec<SpellCheckSpan>,
}

impl SpellCheckResults {
    /// The misspelled words of `text`, which may have changed since the check: those the
    /// change did not touch are kept, moved by what was inserted or deleted before them,
    /// and the rest are dropped until the next check answers.
    ///
    /// Without this, every key typed would take every underline away until the checker
    /// answered again, and a field being typed in would flicker.
    pub fn spans_for(&self, text: &str) -> Vec<SpellCheckSpan> {
        if text == self.text {
            return self.spans.clone();
        }
        let old: Vec<char> = self.text.chars().collect();
        let new: Vec<char> = text.chars().collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let old_changed_end = old.len() - suffix;
        let new_changed_end = new.len() - suffix;
        // A word ends where the next character is not part of a word: the character after
        // a kept span must still be a boundary, or the word has grown and is another word.
        let boundary = |at: Option<&char>| at.is_none_or(|c| !c.is_alphanumeric());
        let mut kept = Vec::new();
        for span in &self.spans {
            if span.end <= prefix {
                if boundary(new.get(span.end)) {
                    kept.push(span.clone());
                }
            } else if span.start >= old_changed_end {
                let start = span.start - old_changed_end + new_changed_end;
                let end = span.end - old_changed_end + new_changed_end;
                let before = start.checked_sub(1).and_then(|i| new.get(i));
                if boundary(before) {
                    kept.push(SpellCheckSpan {
                        start,
                        end,
                        suggestions: span.suggestions.clone(),
                    });
                }
            }
        }
        kept
    }
}

thread_local! {
    /// The last answer of the spell checker, for the focused field.
    static RESULTS: RefCell<Option<SpellCheckResults>> = const { RefCell::new(None) };
    /// The labels of the installed "process text" actions, in the platform's order.
    static PROCESS_TEXT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Hands the field the spell checker's answer, or takes it away (`None`) when the focus
/// leaves. Called by the shell.
#[inline(never)]
pub fn set_spell_check_results(results: Option<SpellCheckResults>) {
    RESULTS.with(|cell| *cell.borrow_mut() = results);
}

/// The spell checker's last answer, if one is held.
#[inline(never)]
pub fn spell_check_results() -> Option<SpellCheckResults> {
    RESULTS.with(|cell| cell.borrow().clone())
}

/// The misspelled words of `text`, as the last answer has them now.
#[inline(never)]
pub fn misspelled_words(text: &str) -> Vec<SpellCheckSpan> {
    RESULTS.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|results| results.spans_for(text))
            .unwrap_or_default()
    })
}

/// The misspelled word of `text` the caret at `cursor` stands in or against, if any: the
/// word a tap there offers corrections for.
pub fn misspelled_word_at(text: &str, cursor: usize) -> Option<SpellCheckSpan> {
    misspelled_words(text)
        .into_iter()
        .find(|span| span.start <= cursor && cursor <= span.end)
}

/// Tells the fields which "process text" actions the platform offers — the labels, in the
/// order they are shown. Called by the shell; an action is then known by its index here.
#[inline(never)]
pub fn set_process_text_actions(labels: Vec<String>) {
    PROCESS_TEXT.with(|cell| *cell.borrow_mut() = labels);
}

/// The "process text" actions the platform offers, as the shell last said.
#[inline(never)]
pub fn process_text_actions() -> Vec<String> {
    PROCESS_TEXT.with(|cell| cell.borrow().clone())
}

/// The words of action `index`, or nothing for an index the platform never gave.
#[inline(never)]
pub(crate) fn process_text_label(index: u16) -> String {
    PROCESS_TEXT.with(|cell| {
        cell.borrow()
            .get(usize::from(index))
            .cloned()
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(text: &str, spans: &[(usize, usize)]) -> SpellCheckResults {
        SpellCheckResults {
            text: text.into(),
            spans: spans
                .iter()
                .map(|&(start, end)| SpellCheckSpan {
                    start,
                    end,
                    suggestions: vec!["x".into()],
                })
                .collect(),
        }
    }

    fn ranges(spans: Vec<SpellCheckSpan>) -> Vec<(usize, usize)> {
        spans.into_iter().map(|s| (s.start, s.end)).collect()
    }

    /// The text checked is the text shown: every span, as it was.
    #[test]
    fn the_same_text_keeps_every_span() {
        let checked = results("helo wrld", &[(0, 4), (5, 9)]);
        assert_eq!(ranges(checked.spans_for("helo wrld")), [(0, 4), (5, 9)]);
    }

    /// Typing after the words keeps them; typing **into** a word drops it, since it is
    /// another word now.
    #[test]
    fn an_edit_keeps_only_the_words_it_did_not_touch() {
        let checked = results("helo wrld", &[(0, 4), (5, 9)]);
        // A space typed at the end: both words are still the words that were checked.
        assert_eq!(ranges(checked.spans_for("helo wrld ")), [(0, 4), (5, 9)]);
        // A letter typed onto the end of the second word: that one is another word.
        assert_eq!(ranges(checked.spans_for("helo wrldx")), [(0, 4)]);
        // A letter typed into the first word: the second moves along by one.
        assert_eq!(ranges(checked.spans_for("hello wrld")), [(6, 10)]);
        // A word inserted between them: the first stays, the second moves.
        assert_eq!(
            ranges(checked.spans_for("helo big wrld")),
            [(0, 4), (9, 13)]
        );
        // Everything deleted: nothing is left to underline.
        assert!(checked.spans_for("").is_empty());
    }

    /// The word a tap offers corrections for: the caret inside it or against either end.
    #[test]
    fn the_word_at_the_caret() {
        set_spell_check_results(Some(results("a helo b", &[(2, 6)])));
        assert!(misspelled_word_at("a helo b", 1).is_none());
        assert_eq!(misspelled_word_at("a helo b", 2).map(|s| s.start), Some(2));
        assert_eq!(misspelled_word_at("a helo b", 4).map(|s| s.start), Some(2));
        assert_eq!(misspelled_word_at("a helo b", 6).map(|s| s.start), Some(2));
        assert!(misspelled_word_at("a helo b", 7).is_none());
        // Another text entirely: the answer was not about it.
        assert!(misspelled_word_at("zzzzzzzz", 4).is_none());
        set_spell_check_results(None);
        assert!(misspelled_word_at("a helo b", 4).is_none());
    }

    /// The actions are known by index, and an index the platform never gave has no words.
    #[test]
    fn process_text_actions_are_known_by_index() {
        set_process_text_actions(vec!["Translate".into(), "Search".into()]);
        assert_eq!(process_text_label(1), "Search");
        assert_eq!(process_text_label(7), "");
        set_process_text_actions(Vec::new());
    }
}
