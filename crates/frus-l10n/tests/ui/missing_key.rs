// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>
// `add` is in en.ftl and not in fr.ftl: the build stops, naming the message and the locale.
mod words {
    frus_l10n::messages!("missing_key", default = "en");
}

fn main() {}
