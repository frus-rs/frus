// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>
// fr.ftl's `hello` uses `$nom` where en.ftl gives `$name`: the build stops, naming both.
mod words {
    frus_l10n::messages!("misnamed_argument");
}

fn main() {}
