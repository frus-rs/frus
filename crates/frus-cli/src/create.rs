// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus create`: a new application, written from the template.
//!
//! The work is split in three so that each part can be tested without the others: the names
//! are checked ([`Project::new`]), the files are rendered in memory ([`render`]), and only then
//! is anything written ([`create`]). A name that would not build stops the command before the
//! disk is touched.
//!
//! In a directory that has files already — `frus create .` in an application — only the
//! template's files that are **missing** are written: a deleted `web/` or icon comes back, and
//! nothing that is there is overwritten. An application's code is its own: the template's
//! sample code is not added beside it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platforms::{self, AppPlatform, Deeplink};
use crate::template::{TemplateFile, FILES};

/// The organisation an application gets when nobody names one, as the template's own default.
pub const DEFAULT_ORG: &str = "com.example";

/// What `frus create` was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Where the application goes: a new directory, or one whose missing files are filled in.
    pub dir: PathBuf,
    /// The package's name; the directory's own name when `None`.
    pub name: Option<String>,
    /// The reverse domain the Android package starts with.
    pub org: String,
    /// A checkout of frus to build against, instead of the crates on crates.io.
    pub frus_path: Option<PathBuf>,
    /// Whether to make the directory a git repository.
    pub git: bool,
    /// `--platforms`: what the application is made for. None: every platform for a new
    /// application; for one that exists, the ones it has. In one that exists, those named are
    /// added to its own.
    pub platforms: Option<Vec<AppPlatform>>,
    /// `--deeplink`: the links that open the application.
    pub deeplink: Option<Deeplink>,
    /// `--description`: a line saying what it is, for the manifest and the web page.
    pub description: Option<String>,
    /// `--template`: an application, a package of widgets, or a plugin.
    pub template: Template,
}

/// What an application is made of beyond its names (milestone 791): its platforms, the links
/// that open it, its description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub platforms: Vec<AppPlatform>,
    pub deeplink: Option<Deeplink>,
    pub description: Option<String>,
}

impl Default for Shape {
    /// What `frus create` makes when asked nothing: every platform, no links.
    fn default() -> Self {
        Shape {
            platforms: AppPlatform::ALL.to_vec(),
            deeplink: None,
            description: None,
        }
    }
}

/// The description the template's manifest and page are written with.
const TEMPLATE_DESCRIPTION: &str = "A frus application";

/// The three values the template is written with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The package's name, as given: `my_app` or `my-app`. It names the desktop program.
    pub project_name: String,
    /// The same name as Rust spells a crate: `my_app`.
    pub crate_name: String,
    /// The reverse domain of the Android package, `<org>.<crate_name>`.
    pub org: String,
}

impl Project {
    /// Checks the name and the organisation, and derives the crate's name.
    pub fn new(name: &str, org: &str) -> Result<Project, String> {
        check_name(name)?;
        let crate_name = name.replace('-', "_");
        check_org(org, &crate_name)?;
        Ok(Project {
            project_name: name.to_string(),
            crate_name,
            org: org.to_string(),
        })
    }

    /// The Android package: what a store knows the application by.
    pub fn android_package(&self) -> String {
        format!("{}.{}", self.org, self.crate_name)
    }

    fn substitute(&self, text: &str) -> String {
        text.replace("{{project-name}}", &self.project_name)
            .replace("{{crate_name}}", &self.crate_name)
            .replace("{{org}}", &self.org)
            .replace("{{org_path}}", &self.org.replace('.', "/"))
            .replace("{{ClassName}}", &self.class_name())
    }

    /// The crate's name as a Java class spells one: `my_plugin` → `MyPlugin`.
    pub fn class_name(&self) -> String {
        self.crate_name
            .split('_')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }
}

/// What `frus create` makes (milestone 791).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Template {
    /// An application, for the platforms it is made for.
    #[default]
    App,
    /// A library of widgets for applications, with an example and tests.
    Package,
    /// Something the platform does, called from Rust: an Android side in Java, the others in Rust.
    Plugin,
}

impl Template {
    pub fn parse(word: &str) -> Result<Template, String> {
        match word {
            "app" => Ok(Template::App),
            "package" => Ok(Template::Package),
            "plugin" => Ok(Template::Plugin),
            other => Err(format!(
                "`{other}` is not a template: --template takes app, package or plugin"
            )),
        }
    }

    fn files(self) -> &'static [TemplateFile] {
        match self {
            Template::App => FILES,
            Template::Package => crate::template::PACKAGE_FILES,
            Template::Plugin => crate::template::PLUGIN_FILES,
        }
    }
}

/// Rust's keywords, the strict and the reserved ones: the crate's name is written as a path in
/// the desktop binary (`my_app::run()`), where a keyword does not parse.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Names a package cannot take: the standard library's crates, the directories cargo puts
/// beside a binary, the frus crates and the template's other dependency, which a package of
/// the same name would shadow.
const TAKEN_NAMES: &[&str] = &[
    "alloc",
    "core",
    "proc_macro",
    "proc-macro",
    "std",
    "test",
    "build",
    "deps",
    "examples",
    "incremental",
    "frus",
    "frus-cli",
    "frus-core",
    "frus-gpu",
    "frus-image",
    "frus-l10n",
    "frus-layout",
    "frus-shell",
    "frus-test",
    "frus-text",
    "frus-widgets",
    "wasm-bindgen",
    "wasm_bindgen",
];

/// Names Windows keeps for devices, which no file may have.
const WINDOWS_DEVICES: &[&str] = &[
    "aux", "con", "nul", "prn", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Java's keywords and literals, which no part of an Android package may be.
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "void",
    "volatile",
    "while",
];

/// A name every platform accepts: a package, a crate, a program, a `.wasm` file and the last
/// part of an Android package. That is lowercase ASCII letters, digits, `_` and `-`, starting
/// with a letter.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the name is empty".into());
    }
    if name.len() > 64 {
        return Err(format!(
            "`{name}` is longer than 64 characters, crates.io's limit"
        ));
    }
    let lower = name.to_ascii_lowercase();
    if name != lower && is_name_shaped(&lower) {
        return Err(format!(
            "`{name}` has capital letters: a package's name is lowercase, `{lower}`"
        ));
    }
    if !is_name_shaped(name) {
        return Err(format!(
            "`{name}` is not a valid name: use lowercase letters, digits, `_` and `-`, \
             starting with a letter (`my_app`, `my-app`)"
        ));
    }
    let crate_name = name.replace('-', "_");
    if RUST_KEYWORDS.contains(&crate_name.as_str()) {
        return Err(format!(
            "`{name}` is a Rust keyword: the crate could not be named"
        ));
    }
    // Cargo reads `-` and `_` as the same character in a crate's name.
    if TAKEN_NAMES
        .iter()
        .any(|taken| taken.replace('-', "_") == crate_name)
    {
        return Err(format!(
            "`{name}` is taken: it is the name of a crate the application builds with"
        ));
    }
    if WINDOWS_DEVICES.contains(&name) {
        return Err(format!(
            "`{name}` is a device name on Windows, where no file may have it"
        ));
    }
    Ok(())
}

fn is_name_shaped(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// An organisation is one or more lowercase parts separated by dots, `com.example`; with the
/// crate's name after it, it is the Android package.
pub fn check_org(org: &str, crate_name: &str) -> Result<(), String> {
    for part in org.split('.') {
        let shaped = part.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !shaped {
            return Err(format!(
                "`{org}` is not a valid organisation: use lowercase parts separated by dots, \
                 each starting with a letter (`com.example`)"
            ));
        }
        if JAVA_KEYWORDS.contains(&part) {
            return Err(format!(
                "`{org}` contains `{part}`, a Java keyword, which Android refuses in a package"
            ));
        }
    }
    if JAVA_KEYWORDS.contains(&crate_name) {
        return Err(format!(
            "`{crate_name}` is a Java keyword, which Android refuses as the last part of the \
             package `{org}.{crate_name}`: choose another name"
        ));
    }
    Ok(())
}

/// Template files copied byte for byte, the same ones `cargo-generate.toml` excludes: the
/// icons are binary, and the workflows' `${{ … }}` is GitHub's syntax, not the template's.
pub fn is_verbatim(path: &str) -> bool {
    path.ends_with(".png") || (path.starts_with(".github/workflows/") && path.ends_with(".yml"))
}

/// The template's own description, which `cargo generate` reads and an application does not
/// need.
const TEMPLATE_CONFIG: &str = "cargo-generate.toml";

/// One file of the new application: its path inside it, `/`-separated, and its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// The application's files, in memory. With `frus_path`, a checkout of frus, the manifest
/// depends on that checkout's `frus` crate rather than on the published one.
pub fn render(project: &Project, frus_path: Option<&Path>) -> Result<Vec<RenderedFile>, String> {
    render_shaped(project, frus_path, &Shape::default())
}

/// The application's files for `shape`: only the files of its platforms, the manifest with only
/// their regions, the README with only their sections, the workflows with only their jobs.
pub fn render_shaped(
    project: &Project,
    frus_path: Option<&Path>,
    shape: &Shape,
) -> Result<Vec<RenderedFile>, String> {
    let wanted = |path: &str| {
        let has = |platform| shape.platforms.contains(&platform);
        if path.starts_with("res/") {
            has(AppPlatform::Android)
        } else if path.starts_with("web/") {
            has(AppPlatform::Web)
        } else if path.starts_with("src/bin/") {
            shape.platforms.iter().any(|p| p.is_desktop())
        } else {
            true
        }
    };
    let mut files = FILES
        .iter()
        .filter(|file| file.path != TEMPLATE_CONFIG && wanted(file.path))
        .map(|file| render_file(project, file, frus_path))
        .collect::<Result<Vec<_>, _>>()?;
    if *shape == Shape::default() {
        return Ok(files);
    }
    for file in &mut files {
        let Ok(text) = std::str::from_utf8(&file.bytes) else {
            continue;
        };
        let shaped = match file.path.as_str() {
            "Cargo.toml" => shape_manifest(text, shape),
            "README.md" => platforms::trim_readme(text, &shape.platforms),
            "web/index.html" => match &shape.description {
                Some(description) => text.replacen(
                    &format!("content=\"{TEMPLATE_DESCRIPTION}\""),
                    &format!("content=\"{}\"", html_escape(description)),
                    1,
                ),
                None => continue,
            },
            path if path.starts_with(".github/workflows/") => {
                platforms::trim_workflow(text, &shape.platforms)
            }
            _ => continue,
        };
        file.bytes = shaped.into_bytes();
    }
    Ok(files)
}

/// The template's manifest for `shape`: the regions of the platforms left out cut, the list of
/// platforms, the description and the links written.
fn shape_manifest(manifest: &str, shape: &Shape) -> String {
    let mut text = manifest.to_string();
    for platform in AppPlatform::ALL {
        if let Some(heading) = platforms::manifest_heading(platform) {
            if !shape.platforms.contains(&platform) {
                text = platforms::cut_region(&text, heading, "# --- ").0;
            }
        }
    }
    text = text.replacen(
        &format!("platforms = {}", platforms::toml_list(&AppPlatform::ALL)),
        &format!("platforms = {}", platforms::toml_list(&shape.platforms)),
        1,
    );
    if let Some(description) = &shape.description {
        text = text.replacen(
            &format!("description = \"{TEMPLATE_DESCRIPTION}\""),
            &format!("description = {}", toml_string(description)),
            1,
        );
    }
    if let Some(link) = &shape.deeplink {
        text = platforms::set_key(
            &text,
            "package.metadata.frus",
            "deeplink",
            &toml_string(&link.text()),
        )
        .unwrap_or(text);
        if shape.platforms.contains(&AppPlatform::Android) {
            if let Some(at) = text.find("# --- Platforms") {
                text.insert_str(
                    at,
                    &format!(
                        "{}
",
                        link.android_filter()
                    ),
                );
            }
        }
    }
    text
}

/// Text for an HTML attribute.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_file(
    project: &Project,
    file: &TemplateFile,
    frus_path: Option<&Path>,
) -> Result<RenderedFile, String> {
    let path = project.substitute(file.path.strip_suffix(".liquid").unwrap_or(file.path));
    let bytes = if file.path.ends_with(".png") {
        file.bytes.to_vec()
    } else {
        // A checkout on Windows has CRLF files; the application gets LF, wherever `frus` was
        // built.
        let text = std::str::from_utf8(file.bytes)
            .map_err(|_| format!("template file {} is not UTF-8", file.path))?
            .replace("\r\n", "\n");
        let mut text = if is_verbatim(file.path) {
            text
        } else {
            project.substitute(&text)
        };
        if !is_verbatim(file.path) && text.contains("{{") {
            return Err(format!(
                "template file {} uses template syntax `frus create` does not know",
                file.path
            ));
        }
        if path == "Cargo.toml" {
            if let Some(checkout) = frus_path {
                text = depend_on_checkout(&text, checkout)?;
            }
        }
        text.into_bytes()
    };
    Ok(RenderedFile { path, bytes })
}

/// Replaces the manifest's `frus = "x.y"` with a path to the checkout's `frus` crate. The
/// facade reaches every other frus crate by path, so this one line moves all of them.
fn depend_on_checkout(manifest: &str, checkout: &Path) -> Result<String, String> {
    let facade = checkout.join("crates").join("frus");
    let line = manifest
        .lines()
        .find(|line| line.starts_with("frus = \""))
        .ok_or("the template's manifest has no `frus = \"…\"` dependency to replace")?;
    let path = toml_string(&display_path(&facade));
    let replacement = format!(
        "# Built against a checkout of frus (`frus create --frus-path`). To build against the\n\
         # published crates instead: {line}\n\
         frus = {{ path = {path} }}"
    );
    Ok(manifest.replacen(line, &replacement, 1))
}

/// A path as a person would type it: Windows' `\\?\` prefix, which `canonicalize` adds, removed.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_string(),
        _ => text,
    }
}

/// A TOML basic string.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What happened to version control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Git {
    /// `--no-git`.
    Skipped,
    /// The directory is a new repository.
    Initialised,
    /// The directory is inside a repository already, which keeps it.
    InsideRepository,
    /// git could not be run, or failed: the application is there all the same.
    Failed(String),
}

/// What `frus create` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub project: Project,
    pub dir: PathBuf,
    /// The files written, `/`-separated, in the template's order.
    pub written: Vec<String>,
    /// How many of the template's files were there already and were left as they are.
    pub kept: usize,
    /// Whether the directory held an application already (a `Cargo.toml` with a `[package]`).
    pub existing: bool,
    pub git: Git,
    /// The platforms the application is made for, now.
    pub platforms: Vec<AppPlatform>,
    /// The links that open it, when `--deeplink` named them.
    pub deeplink: Option<Deeplink>,
    /// In an application that exists: what was added to its `Cargo.toml`, said for a person.
    pub manifest_changes: Vec<String>,
}

/// The template's sample application: the code a new application starts with. An application
/// that exists has its own, and these are not added beside it — a `tests/counter_test.rs`
/// next to code that has no counter would not even build.
const SAMPLE_CODE: &[&str] = &["src/lib.rs", "src/counter.rs", "tests/counter_test.rs"];

/// A package or a plugin (`--template`): a library crate, written into a new directory.
fn create_library(options: &Options) -> Result<Created, String> {
    if options.dir.join("Cargo.toml").exists() {
        return Err(format!(
            "{} has a Cargo.toml already: a {} is made in a new directory",
            options.dir.display(),
            if options.template == Template::Package {
                "package"
            } else {
                "plugin"
            }
        ));
    }
    if options.platforms.is_some() || options.deeplink.is_some() {
        return Err("--platforms and --deeplink are an application's (--template app)".into());
    }
    let name = match &options.name {
        Some(name) => name.clone(),
        None => name_of(&options.dir)?,
    };
    let project = Project::new(&name, &options.org)?;
    let frus_path = match &options.frus_path {
        Some(path) => Some(check_checkout(path)?),
        None => None,
    };
    let mut files = options
        .template
        .files()
        .iter()
        .map(|file| render_file(&project, file, frus_path.as_deref()))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(description) = &options.description {
        for file in files.iter_mut().filter(|f| f.path == "Cargo.toml") {
            let text = String::from_utf8_lossy(&file.bytes).into_owned();
            let text =
                platforms::set_key(&text, "package", "description", &toml_string(description))
                    .unwrap_or(text);
            file.bytes = text.into_bytes();
        }
    }
    let (present, files): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|file| options.dir.join(&file.path).exists());
    let made_dir = prepare_dir(&options.dir)?;
    if let Err(error) = write_files(&options.dir, &files) {
        if made_dir {
            let _ = fs::remove_dir_all(&options.dir);
        }
        return Err(error);
    }
    let git = if options.git {
        git_init(&options.dir)
    } else {
        Git::Skipped
    };
    Ok(Created {
        project,
        dir: options.dir.clone(),
        written: files.into_iter().map(|file| file.path).collect(),
        kept: present.len(),
        existing: false,
        git,
        platforms: Vec::new(),
        deeplink: None,
        manifest_changes: Vec::new(),
    })
}

/// Creates the application: checks the names, renders the files, then writes the ones that
/// are missing.
pub fn create(options: &Options) -> Result<Created, String> {
    if options.template != Template::App {
        return create_library(options);
    }
    let existing = existing_application(&options.dir)?;
    let project = match &existing {
        // The application says what it is called, and that is what its files are written
        // with: the directory's name may be anything.
        Some(app) => {
            if let Some(name) = options.name.as_ref().filter(|name| **name != app.package) {
                return Err(format!(
                    "{} holds the application {}: --name {name} does not match it",
                    options.dir.display(),
                    app.package
                ));
            }
            let crate_name = app.lib.clone();
            // The organisation the manifest's Android package already has, when it reads as
            // one; the one asked for otherwise.
            let suffix = format!(".{crate_name}");
            let org = app
                .android_package
                .strip_suffix(&suffix)
                .filter(|org| check_org(org, &crate_name).is_ok())
                .unwrap_or(&options.org)
                .to_string();
            check_org(&org, &crate_name)?;
            Project {
                project_name: app.package.clone(),
                crate_name,
                org,
            }
        }
        None => {
            let name = match &options.name {
                Some(name) => name.clone(),
                None => name_of(&options.dir)?,
            };
            Project::new(&name, &options.org)?
        }
    };

    let frus_path = match &options.frus_path {
        Some(path) => Some(check_checkout(path)?),
        None => None,
    };
    // The platforms: those asked for in a new application; in one that exists, its own and
    // those asked for. A manifest without a list is made for all of them.
    let listed = existing
        .as_ref()
        .and_then(|app| app.platforms.as_deref())
        .map(platforms::from_words);
    let had = existing
        .as_ref()
        .map(|_| listed.clone().unwrap_or_else(|| AppPlatform::ALL.to_vec()));
    let mut chosen: Vec<AppPlatform> = match (&had, &options.platforms) {
        (Some(had), Some(asked)) => had.iter().chain(asked).copied().collect(),
        (Some(had), None) => had.clone(),
        (None, Some(asked)) => asked.clone(),
        (None, None) => AppPlatform::ALL.to_vec(),
    };
    chosen.sort();
    chosen.dedup();
    let shape = Shape {
        platforms: chosen,
        deeplink: options.deeplink.clone(),
        description: options.description.clone(),
    };
    let rendered = render_shaped(&project, frus_path.as_deref(), &shape)?;
    // What the application's manifest gains: the regions of the platforms added, the list, the
    // links. Worked out before anything is written, like the rest.
    let manifest_update = match (&existing, &had) {
        (Some(app), Some(had)) => {
            let path = app.root.join("Cargo.toml");
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            let template = render(&project, None)?
                .into_iter()
                .find(|file| file.path == "Cargo.toml")
                .map(|file| String::from_utf8_lossy(&file.bytes).into_owned())
                .unwrap_or_default();
            let (updated, changes) =
                update_manifest(&text, &template, had, listed.is_some(), &shape);
            (updated != text).then_some((path, updated, changes))
        }
        _ => None,
    };
    let wanted = wanted_files(&options.dir, rendered, existing.is_some());
    let (present, files): (Vec<_>, Vec<_>) = wanted
        .into_iter()
        .partition(|file| options.dir.join(&file.path).exists());

    let made_dir = prepare_dir(&options.dir)?;
    if let Err(error) = write_files(&options.dir, &files) {
        // Only a directory this command made is taken away again: an empty one that was there
        // before stays, with whatever was written.
        if made_dir {
            let _ = fs::remove_dir_all(&options.dir);
        }
        return Err(error);
    }
    let mut manifest_changes = Vec::new();
    if let Some((path, text, changes)) = manifest_update {
        fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        manifest_changes = changes;
    }

    // An application that exists keeps its version control as it is, or as it is not.
    let git = if options.git && existing.is_none() {
        git_init(&options.dir)
    } else {
        Git::Skipped
    };
    Ok(Created {
        project,
        dir: options.dir.clone(),
        written: files.into_iter().map(|file| file.path).collect(),
        kept: present.len(),
        existing: existing.is_some(),
        git,
        platforms: shape.platforms,
        deeplink: shape.deeplink,
        manifest_changes,
    })
}

/// An application's manifest with what `shape` adds to it, and each addition said: the template's
/// region of each platform it did not have (`had`), its list of platforms when it keeps one
/// (`listed`), the links that open it, its description. Nothing that is there is changed, except
/// the list, the `deeplink` key and the description that are asked for.
fn update_manifest(
    manifest: &str,
    template: &str,
    had: &[AppPlatform],
    listed: bool,
    shape: &Shape,
) -> (String, Vec<String>) {
    let mut text = manifest.to_string();
    let mut changes = Vec::new();
    let present: Vec<String> = platforms::table_headers(manifest);
    for platform in shape.platforms.iter().filter(|p| !had.contains(p)) {
        let Some(heading) = platforms::manifest_heading(*platform) else {
            continue;
        };
        let Some(region) = platforms::cut_region(template, heading, "# --- ").1 else {
            continue;
        };
        let headers = platforms::table_headers(&region);
        if headers.iter().any(|header| present.contains(header)) {
            changes.push(format!(
                "Cargo.toml has some of {platform}'s tables already, so none was added: compare \
                 it with a new application's (`frus create --platforms {platform} <dir>`)"
            ));
        } else {
            text = platforms::append(&text, &region);
            changes.push(format!("Cargo.toml: the {platform} section"));
        }
    }
    if listed {
        let list = platforms::toml_list(&shape.platforms);
        if let Some(updated) =
            platforms::set_key(&text, "package.metadata.frus", "platforms", &list)
        {
            if updated != text {
                text = updated;
                changes.push(format!("Cargo.toml: platforms = {list}"));
            }
        }
    }
    if let Some(link) = &shape.deeplink {
        let value = toml_string(&link.text());
        let updated = platforms::set_key(&text, "package.metadata.frus", "deeplink", &value)
            .unwrap_or_else(|| {
                platforms::append(
                    &text,
                    &format!("[package.metadata.frus]\ndeeplink = {value}\n"),
                )
            });
        if updated != text {
            text = updated;
            changes.push(format!("Cargo.toml: deeplink = {value}"));
        }
        if shape.platforms.contains(&AppPlatform::Android) && !link.declared_in(&text) {
            text = platforms::append(&text, &link.android_filter());
            changes.push(format!(
                "Cargo.toml: the Android intent filter for {}",
                link.text()
            ));
        }
    }
    if let Some(description) = &shape.description {
        let value = toml_string(description);
        if let Some(updated) = platforms::set_key(&text, "package", "description", &value) {
            if updated != text {
                text = updated;
                changes.push(format!("Cargo.toml: description = {value}"));
            }
        }
    }
    (text, changes)
}

/// The application already in `dir`, if there is one: a `Cargo.toml` with a `[package]`.
fn existing_application(dir: &Path) -> Result<Option<crate::project::Project>, String> {
    let manifest = dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&manifest)
        .map_err(|error| format!("cannot read {}: {error}", manifest.display()))?;
    crate::project::Project::from_manifest(dir, &text)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "{} has no [package]: it is a workspace, not an application. Run frus create in \
                 a new directory, or in one of its applications",
                manifest.display()
            )
        })
}

/// The template's files that belong in `dir`: every one in a new application; in one that
/// exists, not its manifest nor the sample code, and no desktop binary when it has its own.
fn wanted_files(dir: &Path, files: Vec<RenderedFile>, existing: bool) -> Vec<RenderedFile> {
    // Sample code goes in only where there is no code: beside someone's `src/lib.rs`, a
    // `counter.rs` would be a stray and its test would not build.
    let has_code = existing || dir.join("src").join("lib.rs").exists();
    // A second binary of the same name is a build error; of another name, a second program.
    let src = dir.join("src");
    let has_binary = src.join("main.rs").exists()
        || fs::read_dir(src.join("bin")).is_ok_and(|mut entries| entries.next().is_some());
    files
        .into_iter()
        .filter(|file| !(existing && file.path == "Cargo.toml"))
        .filter(|file| !(has_code && SAMPLE_CODE.contains(&file.path.as_str())))
        .filter(|file| !(has_binary && file.path.starts_with("src/bin/")))
        .collect()
}

/// The name a directory gives an application: its last component, `.` included.
fn name_of(dir: &Path) -> Result<String, String> {
    // A trailing `..` is refused as written, before the path is made absolute: Windows
    // resolves it there and Unix keeps it, so the answer would depend on the platform.
    if matches!(
        dir.components().next_back(),
        Some(std::path::Component::ParentDir)
    ) {
        return Err(format!(
            "cannot tell a name from {}: pass --name",
            dir.display()
        ));
    }
    let absolute = std::path::absolute(dir)
        .map_err(|error| format!("cannot resolve {}: {error}", dir.display()))?;
    // `components` drops a trailing `.`.
    let last = absolute.components().next_back();
    match last {
        Some(std::path::Component::Normal(name)) => name
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{} is not a UTF-8 name: pass --name", dir.display())),
        _ => Err(format!(
            "cannot tell a name from {}: pass --name",
            dir.display()
        )),
    }
}

/// The checkout's root, made absolute, once it is known to hold the `frus` facade.
fn check_checkout(path: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(path)
        .map_err(|error| format!("--frus-path {}: {error}", path.display()))?;
    let manifest = root.join("crates").join("frus").join("Cargo.toml");
    let is_facade = fs::read_to_string(&manifest)
        .is_ok_and(|text| text.lines().any(|line| line.trim() == "name = \"frus\""));
    if !is_facade {
        return Err(format!(
            "--frus-path {}: not a checkout of frus (no crates/frus/Cargo.toml)",
            path.display()
        ));
    }
    Ok(root)
}

/// Makes the directory, or checks that the one there is a directory. True when it was made
/// here.
fn prepare_dir(dir: &Path) -> Result<bool, String> {
    if dir.exists() {
        if !dir.is_dir() {
            return Err(format!("{} exists and is not a directory", dir.display()));
        }
        Ok(false)
    } else {
        fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        Ok(true)
    }
}

fn write_files(dir: &Path, files: &[RenderedFile]) -> Result<(), String> {
    for file in files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        fs::write(&path, &file.bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// `git init`, unless the directory already sits in a repository, as `cargo new` does.
fn git_init(dir: &Path) -> Git {
    let inside = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output();
    match inside {
        Err(error) => return Git::Failed(format!("git could not be run: {error}")),
        Ok(output) if output.status.success() && output.stdout.starts_with(b"true") => {
            return Git::InsideRepository;
        }
        Ok(_) => {}
    }
    match Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir)
        .output()
    {
        Ok(output) if output.status.success() => Git::Initialised,
        Ok(output) => Git::Failed(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Git::Failed(format!("git could not be run: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::new("my-app", "com.acme").unwrap()
    }

    fn file<'a>(files: &'a [RenderedFile], path: &str) -> &'a str {
        let file = files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "no {path} in {:?}",
                    files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )
            });
        std::str::from_utf8(&file.bytes).unwrap()
    }

    /// A directory of its own under the system's temporary one, gone when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("frus-cli-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn options(dir: PathBuf) -> Options {
        Options {
            dir,
            name: None,
            org: DEFAULT_ORG.to_string(),
            frus_path: None,
            git: false,
            platforms: None,
            deeplink: None,
            description: None,
            template: Template::App,
        }
    }

    #[test]
    fn names_that_build_everywhere() {
        for name in ["app", "my_app", "my-app", "app2", "a_b-c"] {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
        let refused = [
            ("", "empty"),
            ("2app", "not a valid name"),
            ("_app", "not a valid name"),
            ("-app", "not a valid name"),
            ("my app", "not a valid name"),
            ("my.app", "not a valid name"),
            ("café", "not a valid name"),
            ("MyApp", "lowercase, `myapp`"),
            ("fn", "Rust keyword"),
            ("self", "Rust keyword"),
            ("frus", "taken"),
            ("frus-widgets", "taken"),
            ("frus_widgets", "taken"),
            ("test", "taken"),
            ("proc-macro", "taken"),
            ("wasm-bindgen", "taken"),
            ("con", "device name"),
            ("lpt1", "device name"),
        ];
        for (name, why) in refused {
            let error = check_name(name).expect_err(name);
            assert!(error.contains(why), "{name}: {error}");
        }
        assert!(check_name(&"a".repeat(64)).is_ok());
        assert!(check_name(&"a".repeat(65)).unwrap_err().contains("64"));
    }

    #[test]
    fn organisations_android_accepts() {
        for org in ["com.example", "io.github.someone", "acme", "com.a_b2"] {
            assert_eq!(check_org(org, "app"), Ok(()), "{org}");
        }
        let refused = [
            ("", "not a valid organisation"),
            ("com.", "not a valid organisation"),
            (".com", "not a valid organisation"),
            ("com..acme", "not a valid organisation"),
            ("Com.Acme", "not a valid organisation"),
            ("com.2acme", "not a valid organisation"),
            ("com.my-co", "not a valid organisation"),
            ("com.new", "Java keyword"),
        ];
        for (org, why) in refused {
            let error = check_org(org, "app").expect_err(org);
            assert!(error.contains(why), "{org}: {error}");
        }
        // The crate's name ends the package, so it is held to the same rule.
        assert!(check_org("com.acme", "class")
            .unwrap_err()
            .contains("Java keyword"));
        assert!(Project::new("int", "com.acme").is_err());
    }

    #[test]
    fn the_crate_name_is_the_name_with_underscores() {
        let project = project();
        assert_eq!(project.project_name, "my-app");
        assert_eq!(project.crate_name, "my_app");
        assert_eq!(project.android_package(), "com.acme.my_app");
    }

    #[test]
    fn rendering_fills_in_every_value() {
        let files = render(&project(), None).unwrap();
        let manifest = file(&files, "Cargo.toml");
        assert!(manifest.contains("name = \"my-app\""));
        assert!(manifest.contains("package = \"com.acme.my_app\""));
        assert!(manifest.contains("label = \"my-app\""));
        assert!(file(&files, "src/bin/my-app.rs").contains("my_app::run()"));
        assert!(file(&files, "web/index.html").contains("./pkg/my_app.js"));
        assert!(file(&files, "src/lib.rs").contains(".title(\"my-app\")"));
        // The tests live in `tests/`, apart from the code, and reach the
        // application as the library it is (milestone 658).
        assert!(file(&files, "tests/counter_test.rs").contains("use my_app::CounterState;"));

        for rendered in &files {
            assert!(!rendered.path.contains("{{"), "{}", rendered.path);
            assert!(!rendered.path.ends_with(".liquid"), "{}", rendered.path);
            if rendered.path.ends_with(".png") {
                continue;
            }
            assert!(
                !rendered.bytes.windows(2).any(|w| w == b"\r\n"),
                "{}",
                rendered.path
            );
            if !is_verbatim(&rendered.path) {
                let text = std::str::from_utf8(&rendered.bytes).unwrap();
                assert!(
                    !text.contains("{{"),
                    "{} keeps template syntax",
                    rendered.path
                );
            }
        }
    }

    /// **The template shows the permissions table, declaring nothing**: uncommented as it is
    /// written, it builds a bundle that says why and an Android manifest that names each one
    /// (milestone 744).
    #[test]
    fn the_template_s_permissions_table_works_as_written() {
        let files = render(&project(), None).unwrap();
        let manifest = file(&files, "Cargo.toml");
        let root = Path::new("my-app");
        let app = crate::project::Project::from_manifest(root, manifest).unwrap();
        assert_eq!(app.permissions().unwrap(), []);

        let uncommented = manifest
            .lines()
            .map(|line| {
                let bare = line.strip_prefix("# ").unwrap_or(line);
                if bare.starts_with("[package.metadata.frus.permissions]")
                    || ["camera", "microphone", "location", "notifications"]
                        .iter()
                        .any(|key| bare.starts_with(&format!("{key} = ")))
                {
                    bare
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let app = crate::project::Project::from_manifest(root, &uncommented).unwrap();
        let declared = app.permissions().unwrap();
        assert_eq!(declared.len(), 4);
        let plist = crate::bundle::info_plist(&app, "my-app", &declared);
        assert!(plist.contains("<string>Scans the code on your ticket.</string>"));
        assert!(plist.contains("<string>com.acme.my-app</string>"));
        let wanted = crate::permissions::android_permissions(&declared);
        let added = crate::permissions::android_additions(&uncommented, &wanted)
            .unwrap()
            .unwrap();
        assert_eq!(added.matches("uses_permission").count(), 5, "{added}");
    }

    /// **A debug run is smooth**: `frus run` builds in debug, and without this the renderer,
    /// the text shaper and frus run unoptimised, at several times a frame's budget. Only the
    /// dependencies are optimised, so the application's own crate stays quick to rebuild
    /// and to hot-reload (milestone 772).
    #[test]
    fn a_debug_build_optimises_the_dependencies() {
        let files = render(&project(), None).unwrap();
        let manifest = file(&files, "Cargo.toml");
        let section = manifest
            .split("[profile.dev.package.\"*\"]")
            .nth(1)
            .expect("the dependencies' debug profile");
        let body = section.split("\n[").next().unwrap();
        assert!(body.contains("\nopt-level = 3\n"), "{body}");
        assert!(!manifest.contains("\n[profile.dev]\n"));
    }

    #[test]
    fn rendering_leaves_out_the_template_config() {
        let files = render(&project(), None).unwrap();
        assert!(files.iter().all(|file| file.path != TEMPLATE_CONFIG));
        assert_eq!(files.len(), FILES.len() - 1);
    }

    #[test]
    fn the_icons_and_the_workflows_are_copied_as_they_are() {
        let files = render(&project(), None).unwrap();
        let icon = files
            .iter()
            .find(|f| f.path == "res/mipmap-hdpi/ic_launcher.png")
            .unwrap();
        let original = FILES
            .iter()
            .find(|f| f.path == "res/mipmap-hdpi/ic_launcher.png")
            .unwrap();
        assert_eq!(icon.bytes, original.bytes);
        assert!(icon.bytes.starts_with(b"\x89PNG"));
        // GitHub's own `${{ … }}` survives.
        assert!(file(&files, ".github/workflows/release.yml")
            .contains("${{ needs.meta.outputs.name }}"));
    }

    #[test]
    fn a_checkout_replaces_the_published_frus() {
        let checkout = Path::new("/src/frus");
        let files = render(&project(), Some(checkout)).unwrap();
        let manifest = file(&files, "Cargo.toml");
        let facade = toml_string(&checkout.join("crates").join("frus").display().to_string());
        assert!(
            manifest.contains(&format!("\nfrus = {{ path = {facade} }}\n")),
            "{manifest}"
        );
        // The published requirement is kept in a comment, to go back to.
        assert!(
            manifest.contains("# published crates instead: frus = \"0."),
            "{manifest}"
        );
        assert_eq!(
            manifest
                .lines()
                .filter(|l| l.starts_with("frus = "))
                .count(),
            1
        );
    }

    #[test]
    fn toml_strings_escape_what_they_must() {
        assert_eq!(toml_string(r"C:\src\frus"), r#""C:\\src\\frus""#);
        assert_eq!(toml_string("a\"b"), r#""a\"b""#);
        assert_eq!(toml_string("a\tb"), r#""a\u0009b""#);
    }

    #[test]
    fn windows_verbatim_prefix_is_dropped() {
        assert_eq!(display_path(Path::new(r"\\?\D:\src\frus")), r"D:\src\frus");
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\host\share")),
            r"\\?\UNC\host\share"
        );
        assert_eq!(display_path(Path::new("/src/frus")), "/src/frus");
    }

    #[test]
    fn a_name_comes_from_the_directory() {
        assert_eq!(name_of(Path::new("some/where/my_app")).unwrap(), "my_app");
        assert_eq!(name_of(Path::new("my_app/.")).unwrap(), "my_app");
        let current = std::env::current_dir().unwrap();
        let expected = current.file_name().unwrap().to_str().unwrap();
        assert_eq!(name_of(Path::new(".")).unwrap(), expected);
        assert!(name_of(Path::new("my_app/.."))
            .unwrap_err()
            .contains("--name"));
    }

    #[test]
    fn create_writes_the_application() {
        let temp = TempDir::new("writes");
        let dir = temp.0.join("hello_app");
        let created = create(&options(dir.clone())).unwrap();
        assert_eq!(created.project.crate_name, "hello_app");
        assert_eq!(created.git, Git::Skipped);
        assert_eq!(created.written.len(), FILES.len() - 1);
        assert_eq!(created.kept, 0);
        assert!(!created.existing);
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("package = \"com.example.hello_app\""));
        assert!(dir.join("src/bin/hello_app.rs").is_file());
        assert!(dir.join("res/mipmap-xxxhdpi/ic_launcher.png").is_file());
        assert!(dir.join(".github/workflows/ci.yml").is_file());
        assert!(!dir.join(TEMPLATE_CONFIG).exists());
    }

    #[test]
    fn create_fills_an_empty_directory() {
        let temp = TempDir::new("empty");
        let mut opts = options(temp.0.clone());
        opts.name = Some("given".into());
        let created = create(&opts).unwrap();
        assert_eq!(created.project.project_name, "given");
        assert!(temp.0.join("src/bin/given.rs").is_file());
    }

    #[test]
    fn create_writes_around_what_a_directory_holds() {
        let temp = TempDir::new("full");
        fs::write(temp.0.join("notes.txt"), "mine").unwrap();
        fs::write(temp.0.join("README.md"), "my readme").unwrap();
        let mut opts = options(temp.0.clone());
        opts.name = Some("app".into());
        let created = create(&opts).unwrap();
        assert!(!created.existing);
        assert_eq!(created.kept, 1, "the README was there");
        assert!(!created.written.contains(&"README.md".to_string()));
        // Nothing that was there is touched; the rest of the application is written.
        assert_eq!(
            fs::read_to_string(temp.0.join("notes.txt")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(temp.0.join("README.md")).unwrap(),
            "my readme"
        );
        assert!(temp.0.join("Cargo.toml").is_file());
        assert!(temp.0.join("src/counter.rs").is_file());
    }

    /// `frus create .` in an application brings back what was deleted, and nothing else:
    /// its manifest, its code and its binary stay as they are, and no sample code is added.
    #[test]
    fn create_in_an_application_restores_only_what_is_missing() {
        let temp = TempDir::new("restore");
        let first = create(&options(temp.0.join("shop"))).unwrap();
        let dir = first.dir;
        let manifest = fs::read_to_string(dir.join("Cargo.toml"))
            .unwrap()
            .replace("com.example.shop", "com.acme.shop");
        fs::write(dir.join("Cargo.toml"), &manifest).unwrap();
        fs::write(dir.join("src/lib.rs"), "// mine").unwrap();
        fs::remove_file(dir.join("src/counter.rs")).unwrap();
        fs::remove_file(dir.join("tests/counter_test.rs")).unwrap();
        fs::remove_dir_all(dir.join("web")).unwrap();
        fs::remove_file(dir.join("res/mipmap-hdpi/ic_launcher.png")).unwrap();

        // Under another directory name, and with the default organisation asked for.
        let moved = temp.0.join("elsewhere");
        fs::rename(&dir, &moved).unwrap();
        let mut opts = options(moved.clone());
        opts.git = true;
        let again = create(&opts).unwrap();
        assert!(again.existing);
        assert_eq!(again.project.project_name, "shop");
        assert_eq!(again.project.org, "com.acme", "the manifest's organisation");
        assert_eq!(
            again.git,
            Git::Skipped,
            "an application keeps its git as it is"
        );
        assert_eq!(
            again.written,
            ["res/mipmap-hdpi/ic_launcher.png", "web/index.html"]
        );
        assert!(fs::read_to_string(moved.join("web/index.html"))
            .unwrap()
            .contains("./pkg/shop.js"));
        assert_eq!(
            fs::read_to_string(moved.join("Cargo.toml")).unwrap(),
            manifest
        );
        assert_eq!(
            fs::read_to_string(moved.join("src/lib.rs")).unwrap(),
            "// mine"
        );
        assert!(!moved.join("src/counter.rs").exists());
        assert!(!moved.join("tests/counter_test.rs").exists());

        // Run again: nothing is missing.
        let third = create(&options(moved.clone())).unwrap();
        assert!(third.written.is_empty());
        assert_eq!(third.kept, again.kept + again.written.len());
    }

    #[test]
    fn create_in_an_application_keeps_its_names() {
        let temp = TempDir::new("names");
        let dir = temp.0.join("app");
        fs::create_dir_all(dir.join("src/bin")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"my-app\"\n\n[lib]\nname = \"core_of_app\"\n",
        )
        .unwrap();
        fs::write(dir.join("src/bin/tool.rs"), "fn main() {}").unwrap();
        let created = create(&options(dir.clone())).unwrap();
        assert_eq!(created.project.crate_name, "core_of_app");
        // No package in the manifest: cargo-apk's own `rust.<lib>` is the one Android gets,
        // and that is what the README written now says.
        assert_eq!(created.project.org, "rust");
        assert!(fs::read_to_string(dir.join("web/index.html"))
            .unwrap()
            .contains("./pkg/core_of_app.js"));
        // It has a binary: none is added beside it.
        assert!(created
            .written
            .iter()
            .all(|path| !path.starts_with("src/bin/")));

        let mut opts = options(dir.clone());
        opts.name = Some("other".into());
        assert!(create(&opts).unwrap_err().contains("does not match"));
    }

    #[test]
    fn create_refuses_a_workspace() {
        let temp = TempDir::new("workspace");
        fs::write(
            temp.0.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\n",
        )
        .unwrap();
        let mut opts = options(temp.0.clone());
        opts.name = Some("app".into());
        assert!(create(&opts).unwrap_err().contains("workspace"));
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }

    #[test]
    fn create_checks_everything_before_writing() {
        let temp = TempDir::new("checks");
        let dir = temp.0.join("Bad Name");
        assert!(create(&options(dir.clone()))
            .unwrap_err()
            .contains("not a valid name"));
        assert!(!dir.exists(), "a refused name leaves no directory behind");

        let dir = temp.0.join("app");
        let mut opts = options(dir.clone());
        opts.frus_path = Some(temp.0.join("nowhere"));
        assert!(create(&opts).unwrap_err().contains("--frus-path"));
        assert!(!dir.exists());

        // A directory that exists and is not a checkout.
        opts.frus_path = Some(temp.0.clone());
        assert!(create(&opts)
            .unwrap_err()
            .contains("not a checkout of frus"));
        assert!(!dir.exists());
    }

    #[test]
    fn create_refuses_a_file_in_the_way() {
        let temp = TempDir::new("file");
        let path = temp.0.join("app");
        fs::write(&path, "").unwrap();
        assert!(create(&options(path))
            .unwrap_err()
            .contains("not a directory"));
    }

    /// **`--platforms` writes only what those platforms need** (milestone 791): no `web/`, no
    /// desktop program, no Android tables, the README and the workflows without them, and the
    /// list in the manifest for `frus run` to read back.
    #[test]
    fn a_new_application_has_only_its_platforms() {
        let temp = TempDir::new("platforms");
        let dir = temp.0.join("phone_app");
        let mut opts = options(dir.clone());
        opts.platforms = Some(vec![AppPlatform::Android]);
        let created = create(&opts).unwrap();
        assert_eq!(created.platforms, [AppPlatform::Android]);
        assert!(!dir.join("web").exists());
        assert!(!dir.join("src/bin").exists());
        assert!(dir.join("res/mipmap-hdpi/ic_launcher.png").is_file());
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(!manifest.contains("wasm-bindgen"), "{manifest}");
        assert!(!manifest.contains("[profile.web-release]"));
        assert!(manifest.contains("[package.metadata.android]"));
        assert!(manifest.contains("\nplatforms = [\"android\"]\n"));
        assert!(
            manifest.contains("[profile.release]"),
            "the shared regions stay"
        );
        let app = crate::project::Project::from_manifest(&dir, &manifest).unwrap();
        assert_eq!(app.app_platforms(), Some(vec![AppPlatform::Android]));
        assert!(app.check_platform(AppPlatform::Web).is_err());
        let readme = fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(readme.contains("## Android") && !readme.contains("## Web"));
        let ci = fs::read_to_string(dir.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("  android:") && !ci.contains("  web:"));
        assert!(!ci.contains("windows-latest"), "{ci}");

        let mut opts = options(temp.0.join("site"));
        opts.platforms = Some(vec![AppPlatform::Web]);
        create(&opts).unwrap();
        let manifest = fs::read_to_string(temp.0.join("site/Cargo.toml")).unwrap();
        assert!(
            !platforms::table_headers(&manifest)
                .iter()
                .any(|header| header.contains("package.metadata.android")),
            "{manifest}"
        );
        assert!(manifest.contains("[profile.web-release]"));
        assert!(!temp.0.join("site/res").exists());
    }

    /// **`frus create --platforms web .` adds the web to an application**: its files, its region
    /// of the manifest appended, the list — and nothing that was there changes.
    #[test]
    fn a_platform_is_added_to_an_application() {
        let temp = TempDir::new("add-platform");
        let dir = temp.0.join("shop");
        let mut opts = options(dir.clone());
        opts.platforms = Some(vec![AppPlatform::Android]);
        create(&opts).unwrap();
        let before = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        fs::write(dir.join("src/lib.rs"), "// mine").unwrap();

        let mut opts = options(dir.clone());
        opts.platforms = Some(vec![AppPlatform::Web]);
        let added = create(&opts).unwrap();
        assert!(added.existing);
        assert_eq!(added.platforms, [AppPlatform::Android, AppPlatform::Web]);
        assert_eq!(added.written, ["web/index.html"]);
        assert_eq!(
            added.manifest_changes,
            [
                "Cargo.toml: the web section",
                "Cargo.toml: platforms = [\"android\", \"web\"]"
            ]
        );
        let after = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        // Everything that was there is there, the list aside.
        let unlisted = |text: &str| {
            text.replace(
                "platforms = [\"android\", \"web\"]",
                "platforms = [\"android\"]",
            )
        };
        assert!(unlisted(&after).starts_with(&before), "{after}");
        assert!(after.contains("[profile.web-release]"));
        assert!(after.contains("wasm-bindgen = \"0.2\""));
        assert_eq!(
            fs::read_to_string(dir.join("src/lib.rs")).unwrap(),
            "// mine"
        );
        let app = crate::project::Project::from_manifest(&dir, &after).unwrap();
        assert!(app.has_web_release);
        assert!(app.check_platform(AppPlatform::Web).is_ok());

        // Asked again: nothing to add.
        let again = create(&opts).unwrap();
        assert!(again.written.is_empty() && again.manifest_changes.is_empty());
        assert_eq!(fs::read_to_string(dir.join("Cargo.toml")).unwrap(), after);

        // `frus create .` restores only what the application's platforms need.
        fs::remove_dir_all(dir.join("web")).unwrap();
        let restored = create(&options(dir.clone())).unwrap();
        assert_eq!(restored.written, ["web/index.html"]);
        assert!(!dir.join("src/bin").exists(), "no desktop: no program");
    }

    /// **`--deeplink` declares the links that open the application**: the intent filter cargo-apk
    /// writes into Android's manifest, and the manifest's own record for the other platforms.
    #[test]
    fn deeplinks_are_declared() {
        let temp = TempDir::new("deeplink");
        let dir = temp.0.join("tides");
        let mut opts = options(dir.clone());
        opts.deeplink = Some(Deeplink::Scheme("tides".into()));
        opts.description = Some("Tides & \"currents\"".into());
        let created = create(&opts).unwrap();
        assert_eq!(created.deeplink, Some(Deeplink::Scheme("tides".into())));
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("data = [{ scheme = \"tides\" }]"),
            "{manifest}"
        );
        assert!(manifest.contains("\ndeeplink = \"tides\"\n"));
        assert!(manifest.contains("description = \"Tides & \\\"currents\\\"\""));
        let page = fs::read_to_string(dir.join("web/index.html")).unwrap();
        assert!(page.contains("content=\"Tides &amp; &quot;currents&quot;\""));
        let app = crate::project::Project::from_manifest(&dir, &manifest).unwrap();
        assert_eq!(app.deeplink.as_deref(), Some("tides"));
        assert_eq!(app.platforms.as_ref().map(Vec::len), Some(5));

        // To an application that has none: added, once.
        let plain = temp.0.join("plain");
        create(&options(plain.clone())).unwrap();
        let mut opts = options(plain.clone());
        opts.deeplink = Some(Deeplink::Https {
            host: "example.com".into(),
        });
        let added = create(&opts).unwrap();
        assert_eq!(
            added.manifest_changes.len(),
            2,
            "{:?}",
            added.manifest_changes
        );
        let manifest = fs::read_to_string(plain.join("Cargo.toml")).unwrap();
        assert_eq!(manifest.matches("intent_filter").count(), 1);
        assert!(manifest.contains("deeplink = \"https://example.com\""));
        assert!(create(&opts).unwrap().manifest_changes.is_empty());
    }

    /// **`--template package` and `--template plugin`** (milestone 791): a library crate each,
    /// every value filled in — the plugin's Java class at its package's path.
    #[test]
    fn packages_and_plugins_are_made() {
        let temp = TempDir::new("library");
        let mut opts = options(temp.0.join("shop_widgets"));
        opts.template = Template::Package;
        opts.description = Some("Shop widgets".into());
        let created = create(&opts).unwrap();
        assert!(created.platforms.is_empty());
        let dir = &created.dir;
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("name = \"shop_widgets\""));
        assert!(manifest.contains("description = \"Shop widgets\""));
        assert!(!manifest.contains("package.metadata.android"));
        assert!(fs::read_to_string(dir.join("tests/tag_test.rs"))
            .unwrap()
            .contains("use shop_widgets::Tag;"));
        assert!(dir.join("examples/gallery.rs").is_file());

        let mut opts = options(temp.0.join("battery-level"));
        opts.template = Template::Plugin;
        opts.org = "com.acme".into();
        let created = create(&opts).unwrap();
        assert_eq!(created.project.class_name(), "BatteryLevel");
        let java = created
            .dir
            .join("android/java/com/acme/battery_level/BatteryLevelPlugin.java");
        let java = fs::read_to_string(java).unwrap();
        assert!(
            java.starts_with("package com.acme.battery_level;"),
            "{java}"
        );
        assert!(java.contains("public final class BatteryLevelPlugin"));
        let android = fs::read_to_string(created.dir.join("src/android.rs")).unwrap();
        assert!(android.contains("\"com.acme.battery_level.BatteryLevelPlugin\""));

        // Not over a crate that is there, and not with an application's options.
        let mut again = options(created.dir.clone());
        again.template = Template::Plugin;
        assert!(create(&again).unwrap_err().contains("Cargo.toml already"));
        let mut odd = options(temp.0.join("other"));
        odd.template = Template::Package;
        odd.platforms = Some(vec![AppPlatform::Web]);
        assert!(create(&odd).unwrap_err().contains("--platforms"));
    }

    #[test]
    fn create_against_this_checkout() {
        let temp = TempDir::new("checkout");
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut opts = options(temp.0.join("app"));
        opts.frus_path = Some(checkout.clone());
        create(&opts).unwrap();
        let manifest = fs::read_to_string(temp.0.join("app").join("Cargo.toml")).unwrap();
        let facade = fs::canonicalize(&checkout)
            .unwrap()
            .join("crates")
            .join("frus");
        let expected = toml_string(&display_path(&facade));
        assert!(
            manifest.contains(&format!("frus = {{ path = {expected} }}")),
            "{manifest}"
        );
    }

    /// The CSS rule a page gives `selector`, between its braces.
    fn css_rule<'a>(page: &'a str, selector: &str) -> Option<&'a str> {
        let start = page.find(&format!("{selector} {{"))?;
        let body = &page[start..];
        let open = body.find('{')? + 1;
        let close = body.find('}')?;
        Some(&body[open..close])
    }

    /// Every page this checkout serves an application from, beside the template's (milestone
    /// 749): they are copies of one page and must keep the canvas sized the same way.
    #[test]
    fn every_page_fills_the_window_it_is_given() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut pages = vec![
            checkout.join("crates/frus-cli/template/web/index.html"),
            checkout.join("crates/frus-hello/web/index.html"),
            checkout.join("frus-examples/forge/frontend/forge/web/index.html"),
        ];
        for example in fs::read_dir(checkout.join("frus-examples")).unwrap() {
            let page = example.unwrap().path().join("web").join("index.html");
            if page.exists() {
                pages.push(page);
            }
        }
        assert!(pages.len() >= 11, "{pages:?}");
        for path in pages {
            let page = fs::read_to_string(&path).unwrap();
            let canvas = css_rule(&page, "canvas")
                .unwrap_or_else(|| panic!("{}: no canvas rule", path.display()));
            // 100vh is the height with a phone browser's address bar hidden, which a page
            // that does not scroll never hides: the bottom of the application was cut off.
            assert!(!canvas.contains("vh"), "{}: {canvas}", path.display());
            for wanted in ["position: fixed", "inset: 0", "width: 100%", "height: 100%"] {
                assert!(canvas.contains(wanted), "{}: {canvas}", path.display());
            }
            // The status box, fixed too, stays over the canvas that comes after it.
            for status in ["#status", "#loading"] {
                if let Some(rule) = css_rule(&page, status) {
                    assert!(rule.contains("z-index"), "{}: {rule}", path.display());
                }
            }
        }
    }

    /// Every page lays itself under a notch and the rounded corners (milestone 751):
    /// without `viewport-fit=cover`, a browser reports every safe-area inset as zero and
    /// letterboxes the page instead, so the shell has nothing to give the application.
    #[test]
    fn every_page_asks_for_the_whole_screen_and_its_safe_areas() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut pages = vec![
            checkout.join("crates/frus-cli/template/web/index.html"),
            checkout.join("crates/frus-hello/web/index.html"),
            checkout.join("crates/frus-demo/web/index.html"),
            checkout.join("frus-examples/forge/frontend/forge/web/index.html"),
        ];
        for example in fs::read_dir(checkout.join("frus-examples")).unwrap() {
            let page = example.unwrap().path().join("web").join("index.html");
            if page.exists() {
                pages.push(page);
            }
        }
        assert!(pages.len() >= 12, "{pages:?}");
        for path in pages {
            let page = fs::read_to_string(&path).unwrap();
            let viewport = page
                .lines()
                .find(|line| line.contains("name=\"viewport\""))
                .unwrap_or_else(|| panic!("{}: no viewport meta", path.display()));
            for wanted in [
                "width=device-width",
                "initial-scale=1",
                "viewport-fit=cover",
            ] {
                assert!(viewport.contains(wanted), "{}: {viewport}", path.display());
            }
        }
    }
}
