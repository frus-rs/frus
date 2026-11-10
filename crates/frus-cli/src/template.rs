// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The application template, built into the program.
//!
//! The files live in `template/`, beside this crate's manifest, so that `cargo publish` ships
//! them inside `frus-cli` and `cargo install` builds them in: a `frus` installed from crates.io
//! needs no checkout and no network to create an application. A test compares this list with
//! the directory, so a file added to one and not the other fails the build's tests.

/// One file of the template: its path inside the template, with `/` separators and the
/// template syntax still in it, and its bytes.
pub struct TemplateFile {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! files {
    ($dir:literal: $($path:literal),* $(,)?) => {
        &[$(TemplateFile {
            path: $path,
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $dir, "/", $path)),
        }),*]
    };
}

/// Every file of the template, in byte order of their paths.
pub static FILES: &[TemplateFile] = files!["template":
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    ".gitignore",
    "Cargo.toml.liquid",
    "README.md",
    "cargo-generate.toml",
    "res/mipmap-hdpi/ic_launcher.png",
    "res/mipmap-mdpi/ic_launcher.png",
    "res/mipmap-xhdpi/ic_launcher.png",
    "res/mipmap-xxhdpi/ic_launcher.png",
    "res/mipmap-xxxhdpi/ic_launcher.png",
    "res/values/styles.xml",
    "src/bin/{{project-name}}.rs",
    "src/counter.rs",
    "src/lib.rs",
    "tests/counter_test.rs",
    "web/index.html",
];

/// `frus create --template package`: a library of widgets for frus applications (milestone 791).
pub static PACKAGE_FILES: &[TemplateFile] = files!["template-package":
    ".gitignore",
    "Cargo.toml.liquid",
    "README.md",
    "examples/gallery.rs",
    "src/lib.rs",
    "tests/tag_test.rs",
];

/// `frus create --template plugin`: something the platform does, called from Rust, with its
/// Android side in Java (milestone 791).
pub static PLUGIN_FILES: &[TemplateFile] = files!["template-plugin":
    ".gitignore",
    "Cargo.toml.liquid",
    "README.md",
    "android/java/{{org_path}}/{{crate_name}}/{{ClassName}}Plugin.java",
    "build.rs",
    "examples/demo.rs",
    "src/android.rs",
    "src/lib.rs",
    "tests/plugin_test.rs",
];
