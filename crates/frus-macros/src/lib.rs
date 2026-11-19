// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-macros` — the compile-time macros of frus.
//!
//! - [`include_font!`] embeds a font file the way `include_bytes!` does, and lets `frus build`
//!   swap in a copy cut down to the characters the application can show (milestone 823).
//!   Applications reach it as `frus::fonts::include_font!`.
//! - `messages!` reads an application's Fluent files at compile time and writes one function
//!   per message, its arguments typed (milestone 826). Applications reach it as
//!   `frus_l10n::messages!`, or `frus::l10n::messages!` with the `l10n` feature.
//! - `#[derive(Routes)]` turns an enum into an application's routes: their patterns, the parsing of
//!   a location into a variant and the location of a variant (milestone 826). Applications
//!   reach it as `frus::Routes`.

#![deny(missing_docs)]

mod messages;
mod routes;

use std::path::{Path, PathBuf};

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

/// The variable `frus build` sets to the directory holding the cut-down fonts.
const SUBSET_DIR: &str = "FRUS_FONT_SUBSET_DIR";

/// Embeds a font file, as `include_bytes!` would, and evaluates to its bytes, a
/// `&'static [u8; N]`.
///
/// ```ignore
/// frus::fonts::add_font(frus::fonts::include_font!("../assets/fonts/Inter-Regular.ttf").to_vec());
/// ```
///
/// The path is relative to the file that writes it, like every `include_*!`.
///
/// What it adds to `include_bytes!` is a release build that weighs less. `frus build` finds every
/// font embedded this way, cuts each one down to the characters the application can show (its
/// languages, the punctuation and symbols the widgets draw, the text written in its sources) and
/// points this macro at the cut copy. Any other build, `cargo build` and `frus run` included,
/// embeds the file as it is. A font that must stay whole is named in the application's manifest,
/// under `[package.metadata.frus.fonts] keep`.
#[proc_macro]
pub fn include_font(input: TokenStream) -> TokenStream {
    match expand(input) {
        Ok(tokens) => tokens,
        Err((message, span)) => compile_error(&message, span),
    }
}

/// Reads the Fluent files of a folder at compile time and writes one function per message.
///
/// Applications call it as `frus_l10n::messages!` (or `frus::l10n::messages!`), which hands
/// this macro the path of the runtime crate; the documentation is there.
#[doc(hidden)]
#[proc_macro]
pub fn messages(input: TokenStream) -> TokenStream {
    messages::expand(input.into(), |span| beside_the_caller(span.unwrap())).into()
}

/// Derives `Routes` for an enum whose variants are an application's routes.
///
/// ```ignore
/// #[derive(frus::Routes)]
/// enum Route {
///     #[route("/")]
///     Home,
///     #[route("/task/:id")]
///     Task { id: u64 },
///     // `q` is not in the pattern: a query parameter, `/search?q=rust`.
///     #[route("/search")]
///     Search { q: Option<String> },
/// }
/// ```
///
/// A `:parameter` is the field of that name, parsed with `FromStr` and written with `Display`;
/// any other field is a query parameter and is an `Option`. A pattern is checked when the
/// application compiles: it starts with `/`, each of its parameters is a field, and no two
/// patterns answer the same locations.
///
/// The generated code names the runtime crate `::frus`, or `::frus_widgets` in a package that
/// depends on `frus-widgets`; `#[routes(crate = "path")]` on the enum names another.
#[proc_macro_derive(Routes, attributes(route, routes))]
pub fn derive_routes(input: TokenStream) -> TokenStream {
    routes::expand(input.into()).into()
}

fn expand(input: TokenStream) -> Result<TokenStream, (String, Span)> {
    let (written, span) = single_literal(input)?;
    let relative = unquote(&written).ok_or_else(|| {
        (
            "include_font! takes a path, as a string literal: include_font!(\"../fonts/Inter.ttf\")"
                .to_string(),
            span,
        )
    })?;
    let original = beside_the_caller(span).join(&relative);
    let chosen = std::env::var_os(SUBSET_DIR)
        .and_then(|dir| {
            let canonical = std::fs::canonicalize(&original).ok()?;
            let cut = Path::new(&dir).join(subset_name(&canonical));
            cut.is_file().then_some(cut)
        })
        .unwrap_or(original);
    Ok(embed(&chosen.to_string_lossy()))
}

/// The one string literal the macro was given, through any invisible groups a `macro_rules!`
/// forwarding it wraps it in.
fn single_literal(input: TokenStream) -> Result<(String, Span), (String, Span)> {
    let mut tokens = input.into_iter().collect::<Vec<_>>();
    loop {
        match tokens.as_slice() {
            [TokenTree::Group(group)] if group.delimiter() == Delimiter::None => {
                tokens = group.stream().into_iter().collect();
            }
            [TokenTree::Literal(literal)] => return Ok((literal.to_string(), literal.span())),
            [first, ..] => {
                return Err((
                    "include_font! takes one string literal, the font file's path".into(),
                    first.span(),
                ))
            }
            [] => {
                return Err((
                    "include_font! needs the font file's path".into(),
                    Span::call_site(),
                ))
            }
        }
    }
}

/// The text of a string literal as written in the source: `"…"` with its escapes, or a raw
/// `r"…"` / `r#"…"#`. None for anything else.
fn unquote(written: &str) -> Option<String> {
    if let Some(raw) = written.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let fence = "#".repeat(hashes);
        return raw
            .strip_prefix(&fence)?
            .strip_prefix('"')?
            .strip_suffix(&fence)?
            .strip_suffix('"')
            .map(str::to_string);
    }
    let inner = written.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            _ => return None,
        }
    }
    Some(out)
}

/// The directory of the file the macro is written in, which `include_bytes!` resolves against
/// too; the package's directory when the compiler cannot say.
fn beside_the_caller(span: Span) -> PathBuf {
    let here = std::env::current_dir().unwrap_or_default();
    match span.local_file() {
        Some(file) => here
            .join(file)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(here),
        None => std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or(here),
    }
}

/// `{ let _ = ::core::option_env!("FRUS_FONT_SUBSET_DIR"); ::core::include_bytes!("<path>") }`.
///
/// The `option_env!` is what makes the compiler record that the crate depends on the variable,
/// so a build that sets it, or stops setting it, compiles the crate again instead of reusing
/// the bytes of the other kind of build.
fn embed(path: &str) -> TokenStream {
    let span = Span::call_site();
    let path_of = |parts: &[&str]| -> Vec<TokenTree> {
        let mut out = Vec::new();
        for part in parts {
            out.push(Punct::new(':', Spacing::Joint).into());
            out.push(Punct::new(':', Spacing::Alone).into());
            out.push(Ident::new(part, span).into());
        }
        out
    };
    let call = |name: &str, argument: Literal| -> Vec<TokenTree> {
        let mut out = path_of(&["core", name]);
        out.push(Punct::new('!', Spacing::Alone).into());
        out.push(
            Group::new(
                Delimiter::Parenthesis,
                TokenStream::from(TokenTree::Literal(argument)),
            )
            .into(),
        );
        out
    };
    let mut body: Vec<TokenTree> = vec![
        Ident::new("let", span).into(),
        Ident::new("_", span).into(),
        Punct::new('=', Spacing::Alone).into(),
    ];
    body.extend(call("option_env", Literal::string(SUBSET_DIR)));
    body.push(Punct::new(';', Spacing::Alone).into());
    body.extend(call("include_bytes", Literal::string(path)));
    TokenStream::from(TokenTree::Group(Group::new(
        Delimiter::Brace,
        body.into_iter().collect(),
    )))
}

fn compile_error(message: &str, span: Span) -> TokenStream {
    let mut tokens = vec![
        TokenTree::from(Punct::new(':', Spacing::Joint)),
        Punct::new(':', Spacing::Alone).into(),
        Ident::new("core", span).into(),
        Punct::new(':', Spacing::Joint).into(),
        Punct::new(':', Spacing::Alone).into(),
        Ident::new("compile_error", span).into(),
        Punct::new('!', Spacing::Alone).into(),
    ];
    let mut literal = Literal::string(message);
    literal.set_span(span);
    let mut group = Group::new(
        Delimiter::Parenthesis,
        TokenStream::from(TokenTree::Literal(literal)),
    );
    group.set_span(span);
    tokens.push(group.into());
    tokens.into_iter().collect()
}

/// The file name the cut copy of the font at `canonical` has in the subset directory: a hash of
/// the whole path, so two fonts of one name in two packages do not meet, and the file's own name
/// after it, for whoever looks in the directory.
///
/// `frus-cli` computes the same name: the two must agree (both are tested on the same input).
fn subset_name(canonical: &Path) -> String {
    let path = canonical.to_string_lossy();
    // FNV-1a, 64 bits: stable across Rust versions and platforms, unlike `DefaultHasher`.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{hash:016x}-{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_is_read_with_its_escapes_and_raw_forms() {
        assert_eq!(unquote(r#""../a.ttf""#).as_deref(), Some("../a.ttf"));
        assert_eq!(
            unquote(r#""C:\\fonts\\a.ttf""#).as_deref(),
            Some(r"C:\fonts\a.ttf")
        );
        assert_eq!(unquote(r#"r"C:\fonts""#).as_deref(), Some(r"C:\fonts"));
        assert_eq!(unquote(r###"r#"a"b"#"###).as_deref(), Some(r#"a"b"#));
        assert_eq!(unquote("42"), None);
        assert_eq!(unquote(r#""\x41""#), None);
    }

    /// The same input and answer as `frus-cli`'s test of its copy: if one changes alone, a
    /// release build quietly embeds the whole font again.
    #[test]
    fn the_subset_name_is_the_one_the_cli_writes() {
        assert_eq!(
            subset_name(Path::new("/fonts/Inter-Regular.ttf")),
            "c2186180b0c7d968-Inter-Regular.ttf"
        );
    }
}
