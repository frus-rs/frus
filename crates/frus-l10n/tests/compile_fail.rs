// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The compile errors of `messages!`, word for word: a message missing from one locale, and a
//! translation using an argument the default locale does not give. Each `tests/ui/*.rs` is
//! compiled on its own and must fail with the `.stderr` beside it.

#[test]
fn broken_translations_do_not_compile() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
