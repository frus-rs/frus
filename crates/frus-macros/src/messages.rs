// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `messages!`: an application's Fluent files read at compile time, one function per message.
//!
//! The rules the files are held to:
//!
//! - the **default locale** (`default = "en"`, `en` when not named) defines the messages and
//!   their arguments;
//! - every other locale has **every message** of the default locale, unless it is listed in
//!   `partial = [...]`: then a message it lacks is shown in the default locale (the runtime
//!   fallback of `Localizer`);
//! - no locale has a message the default locale lacks, partial or not;
//! - a translation uses **no argument** the default locale's message does not have.
//!
//! Each broken rule is a compile error naming the message, the locale and its file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fluent_syntax::ast;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{format_ident, quote};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Token};

/// What the macro was told.
struct Input {
    /// The path of the runtime crate, as `macro_rules!` handed it over (`$crate`).
    krate: TokenStream,
    dir: LitStr,
    default: Option<LitStr>,
    partial: Vec<LitStr>,
}

pub(crate) fn expand(input: TokenStream, caller_dir: impl Fn(Span) -> PathBuf) -> TokenStream {
    match try_expand(input, caller_dir) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

fn try_expand(
    input: TokenStream,
    caller_dir: impl Fn(Span) -> PathBuf,
) -> syn::Result<TokenStream> {
    let input = parse_input(input)?;
    let span = input.dir.span();
    let dir = caller_dir(span).join(input.dir.value());
    let files = read_folder(&dir).map_err(|message| syn::Error::new(span, message))?;
    let default = input
        .default
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_else(|| "en".to_string());
    let partial: Vec<String> = input.partial.iter().map(LitStr::value).collect();
    let catalog = analyse(&files, &default, &partial).map_err(|messages| {
        let mut errors = messages.into_iter().map(|m| syn::Error::new(span, m));
        let mut first = errors.next().expect("an error");
        for error in errors {
            first.combine(error);
        }
        first
    })?;
    Ok(generate(&input.krate, &files, &catalog))
}

fn parse_input(input: TokenStream) -> syn::Result<Input> {
    // `$crate ;` first, put there by the `macro_rules!` that forwards to this macro.
    let mut krate = TokenStream::new();
    let mut tokens = input.into_iter();
    for token in tokens.by_ref() {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == ';') {
            break;
        }
        krate.extend([token]);
    }
    let rest: TokenStream = tokens.collect();
    let parser = |stream: ParseStream| -> syn::Result<Input> {
        if stream.is_empty() {
            return Err(stream
                .error("messages! takes the folder of the `.ftl` files: messages!(\"../i18n\")"));
        }
        let dir: LitStr = stream.parse()?;
        let mut default = None;
        let mut partial = Vec::new();
        while !stream.is_empty() {
            stream.parse::<Token![,]>()?;
            if stream.is_empty() {
                break;
            }
            let key: Ident = stream.parse()?;
            stream.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "default" => default = Some(stream.parse()?),
                "partial" => {
                    let list;
                    syn::bracketed!(list in stream);
                    partial = Punctuated::<LitStr, Token![,]>::parse_terminated(&list)?
                        .into_iter()
                        .collect();
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "expected `default = \"en\"` or `partial = [\"de\", ...]`",
                    ))
                }
            }
        }
        Ok(Input {
            krate: krate.clone(),
            dir,
            default,
            partial,
        })
    };
    parser.parse2(rest)
}

/// One `.ftl` file: its locale (the file's name), where it is, and what it says.
pub(crate) struct File {
    pub(crate) locale: String,
    pub(crate) path: PathBuf,
    pub(crate) source: String,
}

fn read_folder(dir: &Path) -> Result<Vec<File>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("messages!: cannot read the folder `{}`: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ftl") {
            continue;
        }
        let Some(locale) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("messages!: cannot read `{}`: {e}", path.display()))?;
        files.push(File {
            locale: locale.to_string(),
            path,
            source,
        });
    }
    if files.is_empty() {
        return Err(format!(
            "messages!: the folder `{}` has no `.ftl` file; name each one after its locale: en.ftl, fr.ftl",
            dir.display()
        ));
    }
    files.sort_by(|a, b| a.locale.cmp(&b.locale));
    Ok(files)
}

/// What one message of one locale uses.
#[derive(Default, Clone, Debug)]
struct Usage {
    /// Its arguments, in the order they first appear.
    args: Vec<String>,
    /// The arguments it treats as numbers: a plural selector, `NUMBER($n)`.
    numbers: BTreeSet<String>,
    /// The messages it refers to, whose arguments it passes on.
    references: Vec<String>,
    /// Its text, when it is plain text: shown in the generated function's documentation.
    plain: Option<String>,
    comment: Option<String>,
}

impl Usage {
    fn arg(&mut self, name: &str) {
        if !self.args.iter().any(|a| a == name) {
            self.args.push(name.to_string());
        }
    }
}

/// One message as the generated code sees it.
#[derive(Debug, PartialEq)]
pub(crate) struct Message {
    pub(crate) key: String,
    /// `(argument, is a number)`, in the default locale's order.
    pub(crate) args: Vec<(String, bool)>,
    pub(crate) doc: String,
}

/// The checked catalogue.
#[derive(Debug)]
pub(crate) struct Catalog {
    pub(crate) default: String,
    pub(crate) messages: Vec<Message>,
}

/// Reads every file and holds them to the rules in the module's documentation. Every broken
/// rule is reported, not just the first.
pub(crate) fn analyse(
    files: &[File],
    default: &str,
    partial: &[String],
) -> Result<Catalog, Vec<String>> {
    let mut errors = Vec::new();
    let file_name = |locale: &str| format!("{locale}.ftl");
    if !files.iter().any(|f| f.locale == default) {
        let found: Vec<&str> = files.iter().map(|f| f.locale.as_str()).collect();
        return Err(vec![format!(
            "messages!: the default locale `{default}` has no file `{default}.ftl` (found: {}); name it with `default = \"..\"`",
            found.join(", ")
        )]);
    }
    for name in partial {
        if name == default {
            errors.push(format!(
                "messages!: the default locale `{default}` cannot be partial"
            ));
        } else if !files.iter().any(|f| &f.locale == name) {
            errors.push(format!(
                "messages!: `partial` names the locale `{name}`, which has no file `{name}.ftl`"
            ));
        }
    }

    // Every locale's messages, arguments resolved through message references.
    let mut locales: BTreeMap<&str, BTreeMap<String, Usage>> = BTreeMap::new();
    for file in files {
        let resource = match fluent_syntax::parser::parse(file.source.as_str()) {
            Ok(resource) => resource,
            Err((resource, parse_errors)) => {
                for error in parse_errors {
                    let line = file.source[..error.pos.start.min(file.source.len())]
                        .matches('\n')
                        .count()
                        + 1;
                    errors.push(format!(
                        "messages!: {} line {line}: {error}",
                        file_name(&file.locale)
                    ));
                }
                resource
            }
        };
        let mut messages = BTreeMap::new();
        for entry in &resource.body {
            let ast::Entry::Message(message) = entry else {
                continue;
            };
            // A message with attributes only has no text of its own to format.
            let Some(value) = &message.value else {
                continue;
            };
            let mut usage = Usage {
                comment: message.comment.as_ref().map(|c| c.content.join("\n")),
                ..Usage::default()
            };
            walk_pattern(value, &mut usage);
            usage.plain = plain_text(value);
            messages.insert(message.id.name.to_string(), usage);
        }
        resolve_references(&mut messages);
        locales.insert(file.locale.as_str(), messages);
    }

    let base = &locales[default];
    for (&locale, messages) in &locales {
        if locale == default {
            continue;
        }
        if !partial.iter().any(|p| p == locale) {
            for key in base.keys() {
                if !messages.contains_key(key) {
                    errors.push(format!(
                        "messages!: the message `{key}` is missing from the locale `{locale}` ({}); \
                         every locale has every message of the default locale `{default}`, \
                         unless it is listed in `partial = [..]`",
                        file_name(locale)
                    ));
                }
            }
        }
        for (key, usage) in messages {
            let Some(original) = base.get(key) else {
                errors.push(format!(
                    "messages!: the message `{key}` of the locale `{locale}` ({}) is not in the \
                     default locale `{default}`; add it to {} or remove it",
                    file_name(locale),
                    file_name(default)
                ));
                continue;
            };
            for arg in &usage.args {
                if !original.args.contains(arg) {
                    let unused: Vec<&String> = original
                        .args
                        .iter()
                        .filter(|a| !usage.args.contains(a))
                        .collect();
                    let hint = match unused.as_slice() {
                        [one] => format!("; did you mean `${one}`?"),
                        _ => String::new(),
                    };
                    errors.push(format!(
                        "messages!: the message `{key}` in the locale `{locale}` ({}) uses the \
                         argument `${arg}`, which the default locale `{default}` does not give it{hint}",
                        file_name(locale)
                    ));
                }
            }
        }
    }

    // The functions' names must be distinct Rust names.
    let mut names: BTreeMap<String, &str> = BTreeMap::new();
    for key in base.keys() {
        let name = rust_name(key);
        if let Some(other) = names.insert(name.clone(), key) {
            errors.push(format!(
                "messages!: the messages `{other}` and `{key}` would both be the function `{name}`; rename one"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    let messages = base
        .iter()
        .map(|(key, usage)| {
            let numbers: BTreeSet<&String> = locales
                .values()
                .filter_map(|m| m.get(key))
                .flat_map(|u| u.numbers.iter())
                .collect();
            let args = usage
                .args
                .iter()
                .map(|a| (a.clone(), numbers.contains(a)))
                .collect();
            let mut doc = String::new();
            if let Some(comment) = &usage.comment {
                doc.push_str(comment);
                doc.push_str("\n\n");
            }
            match &usage.plain {
                Some(text) => doc.push_str(&format!("`{key}`: \"{text}\" in `{default}`.")),
                None => doc.push_str(&format!("The message `{key}`.")),
            }
            Message {
                key: key.clone(),
                args,
                doc,
            }
        })
        .collect();
    Ok(Catalog {
        default: default.to_string(),
        messages,
    })
}

fn walk_pattern(pattern: &ast::Pattern<&str>, usage: &mut Usage) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            walk_expression(expression, usage);
        }
    }
}

fn walk_expression(expression: &ast::Expression<&str>, usage: &mut Usage) {
    match expression {
        ast::Expression::Select { selector, variants } => {
            walk_inline(selector, usage);
            let plural = variants.iter().any(|v| match &v.key {
                ast::VariantKey::NumberLiteral { .. } => true,
                ast::VariantKey::Identifier { name } => {
                    matches!(*name, "zero" | "one" | "two" | "few" | "many")
                }
            });
            if let (true, ast::InlineExpression::VariableReference { id }) = (plural, selector) {
                usage.numbers.insert(id.name.to_string());
            }
            for variant in variants {
                walk_pattern(&variant.value, usage);
            }
        }
        ast::Expression::Inline(inline) => walk_inline(inline, usage),
    }
}

fn walk_inline(inline: &ast::InlineExpression<&str>, usage: &mut Usage) {
    match inline {
        ast::InlineExpression::VariableReference { id } => usage.arg(id.name),
        ast::InlineExpression::FunctionReference { id, arguments } => {
            if id.name == "NUMBER" {
                for argument in &arguments.positional {
                    if let ast::InlineExpression::VariableReference { id } = argument {
                        usage.numbers.insert(id.name.to_string());
                    }
                }
            }
            walk_arguments(arguments, usage);
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => walk_arguments(arguments, usage),
        ast::InlineExpression::MessageReference { id, .. } => {
            usage.references.push(id.name.to_string())
        }
        ast::InlineExpression::Placeable { expression } => walk_expression(expression, usage),
        _ => {}
    }
}

fn walk_arguments(arguments: &ast::CallArguments<&str>, usage: &mut Usage) {
    for argument in &arguments.positional {
        walk_inline(argument, usage);
    }
    for named in &arguments.named {
        walk_inline(&named.value, usage);
    }
}

/// A message that refers to another passes its arguments on: the referring message takes the
/// referred one's arguments too.
fn resolve_references(messages: &mut BTreeMap<String, Usage>) {
    loop {
        let mut changed = false;
        let snapshot = messages.clone();
        for usage in messages.values_mut() {
            for reference in usage.references.clone() {
                if let Some(other) = snapshot.get(&reference) {
                    for arg in &other.args {
                        if !usage.args.contains(arg) {
                            usage.args.push(arg.clone());
                            changed = true;
                        }
                    }
                    for number in &other.numbers {
                        changed |= usage.numbers.insert(number.clone());
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn plain_text(pattern: &ast::Pattern<&str>) -> Option<String> {
    let mut out = String::new();
    for element in &pattern.elements {
        match element {
            ast::PatternElement::TextElement { value } => out.push_str(value),
            ast::PatternElement::Placeable { .. } => return None,
        }
    }
    Some(out.trim().replace('\n', " "))
}

/// The Rust name of a Fluent identifier: `task-count` is `task_count`.
pub(crate) fn rust_name(key: &str) -> String {
    key.replace('-', "_").to_lowercase()
}

/// An identifier for `name`, raw when it is a keyword (`r#type`), with a trailing `_` for the
/// keywords that cannot be raw.
fn ident(name: &str) -> Ident {
    match name {
        "self" | "super" | "crate" | "Self" => format_ident!("{name}_"),
        _ if syn::parse_str::<Ident>(name).is_ok() => format_ident!("{name}"),
        _ => Ident::new_raw(name, Span::call_site()),
    }
}

fn generate(krate: &TokenStream, files: &[File], catalog: &Catalog) -> TokenStream {
    let default = &catalog.default;
    let locales: Vec<&str> = files.iter().map(|f| f.locale.as_str()).collect();
    let loads = files.iter().map(|file| {
        let locale = &file.locale;
        let path = file.path.to_string_lossy().into_owned();
        quote! { localizer.add(#locale, ::core::include_str!(#path)); }
    });
    let methods = catalog.messages.iter().map(|message| {
        let name = ident(&rust_name(&message.key));
        let key = &message.key;
        let doc = &message.doc;
        let params = message.args.iter().map(|(arg, number)| {
            let arg = ident(&rust_name(arg));
            if *number {
                quote! { #arg: impl #krate::Number }
            } else {
                quote! { #arg: &str }
            }
        });
        let values = message.args.iter().map(|(arg, number)| {
            let ident = ident(&rust_name(arg));
            if *number {
                quote! { (#arg, #krate::Arg::Num(#krate::Number::to_f64(#ident))) }
            } else {
                quote! { (#arg, #krate::Arg::Str(#ident)) }
            }
        });
        quote! {
            #[doc = #doc]
            pub fn #name(&self, #(#params),*) -> ::std::string::String {
                localizer().format_for(&self.locale, #key, &[#(#values),*])
            }
        }
    });
    quote! {
        /// Every locale the application has a file for.
        #[allow(dead_code)]
        pub const LOCALES: &[&str] = &[#(#locales),*];

        /// The locale whose file defines the messages, and the one a missing message falls
        /// back to.
        #[allow(dead_code)]
        pub const DEFAULT_LOCALE: &str = #default;

        /// Every message of every locale, loaded once. Its string lookups (`format`,
        /// `format_for`) keep working beside the typed functions of [`Messages`].
        #[allow(dead_code)]
        pub fn localizer() -> &'static #krate::Localizer {
            static LOCALIZER: ::std::sync::OnceLock<#krate::Localizer> =
                ::std::sync::OnceLock::new();
            LOCALIZER.get_or_init(|| {
                let mut localizer = #krate::Localizer::new(#default);
                #(#loads)*
                localizer
            })
        }

        /// The messages in `locale`: the closest locale there is a file for (`fr-CA` gets
        /// `fr`), the default one when there is none.
        #[allow(dead_code)]
        pub fn messages(locale: &str) -> Messages {
            Messages {
                locale: localizer().negotiate(locale),
            }
        }

        /// The application's messages in one locale: one function per message, its
        /// arguments typed. Made with [`messages`].
        #[derive(Clone, Debug)]
        pub struct Messages {
            locale: #krate::__private::LanguageIdentifier,
        }

        #[allow(dead_code)]
        impl Messages {
            #(#methods)*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<File> {
        list.iter()
            .map(|(locale, source)| File {
                locale: locale.to_string(),
                path: PathBuf::from(format!("{locale}.ftl")),
                source: source.to_string(),
            })
            .collect()
    }

    const EN: &str = "hello = Hello, { $name }!\n\
                      tasks = { $n ->\n    [one] { $n } task\n   *[other] { $n } tasks\n }\n\
                      title = My Tasks\n";

    #[test]
    fn a_complete_set_gives_one_typed_function_per_message() {
        let fr = "hello = Bonjour, { $name } !\n\
                  tasks = { $n ->\n    [one] { $n } tâche\n   *[other] { $n } tâches\n }\n\
                  title = Mes tâches\n";
        let catalog = analyse(&files(&[("en", EN), ("fr", fr)]), "en", &[]).unwrap();
        let keys: Vec<_> = catalog.messages.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["hello", "tasks", "title"]);
        assert_eq!(catalog.messages[0].args, [("name".to_string(), false)]);
        // A plural selector makes the argument a number.
        assert_eq!(catalog.messages[1].args, [("n".to_string(), true)]);
        assert!(catalog.messages[2].doc.contains("\"My Tasks\""));
    }

    #[test]
    fn a_missing_message_names_the_key_and_the_locale() {
        let fr = "hello = Bonjour, { $name } !\ntitle = Mes tâches\n";
        let errors = analyse(&files(&[("en", EN), ("fr", fr)]), "en", &[]).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`tasks`"), "{}", errors[0]);
        assert!(errors[0].contains("locale `fr` (fr.ftl)"), "{}", errors[0]);
    }

    #[test]
    fn a_partial_locale_may_lack_a_message_but_not_add_one() {
        let fr = "hello = Bonjour, { $name } !\n";
        assert!(analyse(&files(&[("en", EN), ("fr", fr)]), "en", &["fr".into()]).is_ok());
        let extra = "hello = Bonjour, { $name } !\nextra = En plus\n";
        let errors =
            analyse(&files(&[("en", EN), ("fr", extra)]), "en", &["fr".into()]).unwrap_err();
        assert!(errors[0].contains("`extra`") && errors[0].contains("not in the default locale"));
    }

    #[test]
    fn a_misnamed_argument_names_the_message_the_locale_and_the_argument() {
        let fr = "hello = Bonjour, { $nme } !\n\
                  tasks = { $n ->\n    [one] { $n } tâche\n   *[other] { $n } tâches\n }\n\
                  title = Mes tâches\n";
        let errors = analyse(&files(&[("en", EN), ("fr", fr)]), "en", &[]).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("`hello` in the locale `fr`"),
            "{}",
            errors[0]
        );
        assert!(errors[0].contains("`$nme`") && errors[0].contains("did you mean `$name`"));
    }

    #[test]
    fn a_syntax_error_names_the_file_and_the_line() {
        let source = "ok = Fine\nbroken = { }\nnext = Fine\n";
        let errors = analyse(&files(&[("en", source)]), "en", &[]).unwrap_err();
        assert!(
            errors[0].starts_with("messages!: en.ftl line 2:"),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn a_reference_passes_its_arguments_on_and_number_marks_a_number() {
        let en = "name = { $first } { $last }\n\
                  greeting = Hello, { name }!\n\
                  total = { NUMBER($sum, minimumFractionDigits: 2) }\n";
        let catalog = analyse(&files(&[("en", en)]), "en", &[]).unwrap();
        let greeting = catalog
            .messages
            .iter()
            .find(|m| m.key == "greeting")
            .unwrap();
        assert_eq!(
            greeting.args,
            [("first".to_string(), false), ("last".to_string(), false)]
        );
        let total = catalog.messages.iter().find(|m| m.key == "total").unwrap();
        assert_eq!(total.args, [("sum".to_string(), true)]);
    }

    #[test]
    fn names_that_meet_and_a_missing_default_are_errors() {
        let errors = analyse(&files(&[("en", "a-b = 1\na_b = 2\n")]), "en", &[]).unwrap_err();
        assert!(
            errors[0].contains("both be the function `a_b`"),
            "{}",
            errors[0]
        );
        let errors = analyse(&files(&[("fr", "a = 1\n")]), "en", &[]).unwrap_err();
        assert!(errors[0].contains("no file `en.ftl`"), "{}", errors[0]);
    }

    #[test]
    fn keywords_become_raw_identifiers() {
        assert_eq!(ident("type").to_string(), "r#type");
        assert_eq!(ident("self").to_string(), "self_");
        assert_eq!(ident("title").to_string(), "title");
    }
}
