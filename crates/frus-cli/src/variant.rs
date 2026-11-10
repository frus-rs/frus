// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! What `frus run` and `frus build` build besides the platform (milestone 790): the mode
//! (`--debug`, `--profile`, `--release`), the flavor (`--flavor`), the values given to the code
//! (`--define`, `--define-from-file`) and the version (`--build-name`, `--build-number`).
//!
//! They reach the code as environment variables of the build (`FRUS_BUILD_MODE`, `FRUS_FLAVOR`,
//! `FRUS_DEFINE_<KEY>`, `FRUS_BUILD_NAME`, `FRUS_BUILD_NUMBER`), which the shell reads at compile
//! time: cargo, cargo-apk and the web build inherit them from this process, and cargo builds
//! again when one changes. The packages carry the rest: Android's application id, label,
//! version name and code, and the installers' names and versions.
//!
//! A flavor is declared once, in the application's `Cargo.toml`:
//!
//! ```toml
//! [package.metadata.frus.flavors.dev]
//! app_name = "My App Dev"            # the name people see
//! application_id_suffix = ".dev"     # installs beside the other flavors on Android
//! resources = "flavors/dev/res"      # Android resources laid over res/ (the icon)
//! features = ["dev"]                 # cargo features it turns on
//!
//! [package.metadata.frus.flavors.dev.defines]
//! API_URL = "https://dev.example.com"
//! ```

use std::path::{Path, PathBuf};

use crate::cargo::Mode;
use crate::project::{string_list, string_value, Project};

/// A flavor, as the manifest declares it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Flavor {
    pub name: String,
    /// `app_name`: the label on Android, the installers' name.
    pub app_name: Option<String>,
    /// `application_id_suffix`: added to Android's application id.
    pub id_suffix: Option<String>,
    /// `resources`: a directory of Android resources laid over the application's.
    pub resources: Option<String>,
    /// `features`: the cargo features it turns on. None: the feature named like the flavor,
    /// when the package has one.
    pub features: Option<Vec<String>>,
    /// `[…flavors.<name>.defines]`.
    pub defines: Vec<(String, String)>,
}

impl Flavor {
    pub(crate) fn named(name: &str) -> Flavor {
        Flavor {
            name: name.to_string(),
            ..Flavor::default()
        }
    }

    /// One `key = value` line of the flavor's table, or of its `defines`.
    pub(crate) fn read(&mut self, defines: bool, key: &str, raw: &str) {
        if defines {
            if let Some(value) = scalar(raw) {
                self.defines
                    .push((key.trim_matches('"').to_string(), value));
            }
            return;
        }
        match key {
            "app_name" => self.app_name = string_value(raw),
            "application_id_suffix" => self.id_suffix = string_value(raw),
            "resources" => self.resources = string_value(raw),
            "features" => self.features = Some(string_list(raw)),
            _ => {}
        }
    }
}

/// A TOML value a define can take: a string, a number, a boolean.
fn scalar(raw: &str) -> Option<String> {
    string_value(raw).or_else(|| {
        let bare = raw.split('#').next()?.trim();
        (!bare.is_empty() && !bare.starts_with(['[', '{'])).then(|| bare.to_string())
    })
}

/// The flags, as given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Variant {
    pub flavor: Option<String>,
    /// `--define` and `--define-from-file`, in the order given: a later one wins.
    pub defines: Vec<Define>,
    pub build_name: Option<String>,
    pub build_number: Option<u32>,
}

/// One `--define` or `--define-from-file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Define {
    Value(String, String),
    File(PathBuf),
}

/// Reads one of the flags `frus run` and `frus build` share. `Ok(false)`: not one of them.
/// `value` takes the flag's value (`--flag value` or `--flag=value`).
pub(crate) fn parse_flag(
    flag: &str,
    has_inline_value: bool,
    value: &mut dyn FnMut(&str) -> Result<String, String>,
    variant: &mut Variant,
    mode: &mut Option<Mode>,
) -> Result<bool, String> {
    let set_mode = |mode: &mut Option<Mode>, new: Mode| match mode {
        Some(old) if *old != new => Err(format!(
            "--{} and --{}: one mode at a time",
            old.name(),
            new.name()
        )),
        _ => {
            *mode = Some(new);
            Ok(true)
        }
    };
    match flag {
        "--debug" if !has_inline_value => set_mode(mode, Mode::Debug),
        "--profile" if !has_inline_value => set_mode(mode, Mode::Profile),
        "--release" if !has_inline_value => set_mode(mode, Mode::Release),
        "--flavor" => {
            let name = value(flag)?;
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(format!(
                    "--flavor takes a name of letters, digits, `_` and `-`, not `{name}`"
                ));
            }
            crate::set_once(&mut variant.flavor, name, "--flavor")?;
            Ok(true)
        }
        "--define" | "--dart-define" => {
            let text = value("--define")?;
            let (key, val) = text
                .split_once('=')
                .ok_or_else(|| format!("--define takes KEY=VALUE, not `{text}`"))?;
            check_key(key)?;
            variant
                .defines
                .push(Define::Value(key.to_string(), val.to_string()));
            Ok(true)
        }
        "--define-from-file" => {
            let path = value(flag)?;
            variant.defines.push(Define::File(PathBuf::from(path)));
            Ok(true)
        }
        "--build-name" => {
            let name = value(flag)?;
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(format!(
                    "--build-name takes a version such as 1.2.3, not `{name}`"
                ));
            }
            crate::set_once(&mut variant.build_name, name, "--build-name")?;
            Ok(true)
        }
        "--build-number" => {
            let text = value(flag)?;
            let number: u32 = text
                .parse()
                .ok()
                .filter(|n| (1..=2_100_000_000).contains(n))
                .ok_or_else(|| {
                    format!(
                        "--build-number takes a whole number from 1 to 2100000000 (Android's \
                         largest version code), not `{text}`"
                    )
                })?;
            crate::set_once(&mut variant.build_number, number, "--build-number")?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// A define's key becomes part of an environment variable's name and of the code's
/// `frus::define!("KEY")`: letters, digits and `_`, not starting with a digit.
fn check_key(key: &str) -> Result<(), String> {
    let ok = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "a define's key is letters, digits and `_` (not starting with a digit): `{key}`"
        ))
    }
}

/// What the flags resolve to for one application.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub mode: Option<Mode>,
    pub flavor: Option<Flavor>,
    /// Every define, the flavor's first, then the files', then the flags': later wins.
    pub defines: Vec<(String, String)>,
    /// The cargo features to turn on.
    pub features: Vec<String>,
    /// `--build-name`, when given.
    pub build_name: Option<String>,
    /// `--build-number`, when given.
    pub build_number: Option<u32>,
}

impl Variant {
    /// Applies the flags to `project`: its variant, and its Android application id. Reads the
    /// define files, relative to `here`.
    pub(crate) fn apply(
        &self,
        mode: Mode,
        project: &mut Project,
        here: &Path,
    ) -> Result<(), String> {
        let flavor = match &self.flavor {
            None => None,
            Some(name) => Some(
                project
                    .flavors
                    .iter()
                    .find(|f| &f.name == name)
                    .cloned()
                    .ok_or_else(|| unknown_flavor(name, project))?,
            ),
        };
        let mut defines: Vec<(String, String)> = Vec::new();
        let mut put = |key: String, value: String| {
            defines.retain(|(k, _)| *k != key);
            defines.push((key, value));
        };
        if let Some(flavor) = &flavor {
            for (key, value) in &flavor.defines {
                check_key(key).map_err(|e| format!("flavor `{}`: {e}", flavor.name))?;
                put(key.clone(), value.clone());
            }
        }
        // The files first, then the single values, whatever their order on the line: a value
        // typed on the line is the one meant.
        for define in &self.defines {
            if let Define::File(path) = define {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    here.join(path)
                };
                for (key, value) in read_define_file(&path)? {
                    put(key, value);
                }
            }
        }
        for define in &self.defines {
            if let Define::Value(key, value) = define {
                put(key.clone(), value.clone());
            }
        }
        let features = match &flavor {
            Some(Flavor {
                features: Some(list),
                ..
            }) => list.clone(),
            Some(flavor) if project.features.contains(&flavor.name) => vec![flavor.name.clone()],
            _ => Vec::new(),
        };
        for feature in &features {
            if !project.features.contains(feature) {
                return Err(format!(
                    "the flavor turns on the feature `{feature}`, which {} does not declare in \
                     [features]",
                    project.package
                ));
            }
        }
        if let Some(suffix) = flavor.as_ref().and_then(|f| f.id_suffix.as_deref()) {
            project.android_package.push_str(suffix);
        }
        project.variant = Resolved {
            mode: Some(mode),
            flavor,
            defines,
            features,
            build_name: self.build_name.clone(),
            build_number: self.build_number,
        };
        Ok(())
    }
}

fn unknown_flavor(name: &str, project: &Project) -> String {
    let declared: Vec<_> = project.flavors.iter().map(|f| f.name.as_str()).collect();
    if declared.is_empty() {
        format!(
            "{} declares no flavor `{name}`: add a [package.metadata.frus.flavors.{name}] table \
             to its Cargo.toml (see docs/guide/build-variants.md)",
            project.package
        )
    } else {
        format!(
            "{} declares no flavor `{name}`; its flavors: {}",
            project.package,
            declared.join(", ")
        )
    }
}

/// The defines a file gives: a JSON object (`{"API_URL": "…", "RETRIES": 3}`) or a `.env` file
/// (`API_URL=…`, `#` comments, values optionally quoted).
pub(crate) fn read_define_file(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("--define-from-file {}: {e}", path.display()))?;
    let is_json = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        || text.trim_start().starts_with('{');
    let pairs = if is_json {
        defines_from_json(&text)
    } else {
        defines_from_env(&text)
    }
    .map_err(|e| format!("--define-from-file {}: {e}", path.display()))?;
    for (key, _) in &pairs {
        check_key(key).map_err(|e| format!("--define-from-file {}: {e}", path.display()))?;
    }
    Ok(pairs)
}

fn defines_from_json(text: &str) -> Result<Vec<(String, String)>, String> {
    use crate::json::Value;
    let text = text.trim_start_matches('\u{feff}');
    match crate::json::parse(text)? {
        Value::Object(members) => members
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(s) => s,
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
                        format!("{}", n as i64)
                    }
                    Value::Number(n) => n.to_string(),
                    _ => return Err(format!("`{key}` is not a string, a number or a boolean")),
                };
                Ok((key, value))
            })
            .collect(),
        _ => Err("the file is not a JSON object".into()),
    }
}

fn defines_from_env(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: KEY=VALUE expected", number + 1))?;
        let value = value.trim();
        let value = match value.chars().next() {
            Some(q @ ('"' | '\'')) if value.len() >= 2 && value.ends_with(q) => {
                value[1..value.len() - 1].to_string()
            }
            _ => value.split(" #").next().unwrap_or(value).trim().to_string(),
        };
        pairs.push((key.trim().to_string(), value));
    }
    Ok(pairs)
}

/// The variables every environment variable this module sets starts with.
const DEFINE_PREFIX: &str = "FRUS_DEFINE_";

/// The environment of every build this process starts, for `project`'s variant: set here, in
/// the process, so that cargo, cargo-apk, the hot-reload engine and the web build all inherit
/// it. Called once, before any build.
pub(crate) fn export(project: &Project) {
    for (name, value) in environment(project) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}

/// What [`export`] sets (`Some`) and clears (`None`).
pub(crate) fn environment(project: &Project) -> Vec<(String, Option<String>)> {
    let variant = &project.variant;
    let mut vars = vec![
        (
            "FRUS_BUILD_MODE".to_string(),
            (variant.mode == Some(Mode::Profile)).then(|| "profile".to_string()),
        ),
        (
            "FRUS_FLAVOR".to_string(),
            variant.flavor.as_ref().map(|f| f.name.clone()),
        ),
        ("FRUS_BUILD_NAME".to_string(), variant.build_name.clone()),
        (
            "FRUS_BUILD_NUMBER".to_string(),
            variant.build_number.map(|n| n.to_string()),
        ),
    ];
    for (key, value) in &variant.defines {
        vars.push((format!("{DEFINE_PREFIX}{key}"), Some(value.clone())));
    }
    // A project made before `frus run --profile` existed has no `[profile.profile]`: cargo is
    // given the same one through its environment.
    if variant.mode == Some(Mode::Profile) && !project.has_profile_profile {
        for (key, value) in PROFILE_PROFILE {
            vars.push((
                format!(
                    "CARGO_PROFILE_PROFILE_{}",
                    key.to_uppercase().replace('-', "_")
                ),
                Some(value.to_string()),
            ));
        }
    }
    vars
}

/// The `profile` cargo profile: a release build that keeps what a profiler reads.
pub(crate) const PROFILE_PROFILE: [(&str, &str); 3] = [
    ("inherits", "release"),
    ("strip", "false"),
    ("debug", "line-tables-only"),
];

impl Project {
    /// The version people see: `--build-name`, or the manifest's without its build metadata.
    pub(crate) fn version_name(&self) -> String {
        if let Some(name) = &self.variant.build_name {
            return name.clone();
        }
        let version = self.version.as_deref().unwrap_or("0.1.0");
        version.split('+').next().unwrap_or(version).to_string()
    }

    /// The number stores order releases by, when one is given: `--build-number`, or the
    /// manifest version's build metadata (`1.2.3+42`).
    pub(crate) fn build_number(&self) -> Option<u32> {
        self.variant.build_number.or_else(|| {
            self.version
                .as_deref()
                .and_then(|v| v.split_once('+'))
                .and_then(|(_, meta)| meta.parse().ok())
        })
    }

    /// Android's version code: the build number, or what cargo-apk derives from the version
    /// (`1 << 24 | major << 16 | minor << 8 | patch`) when there is none.
    pub(crate) fn version_code(&self) -> u32 {
        self.build_number()
            .unwrap_or_else(|| derived_version_code(&self.version_name()))
    }

    /// The name people see: the flavor's `app_name`, the Android label, or the package's name.
    pub(crate) fn display_name(&self) -> String {
        self.variant
            .flavor
            .as_ref()
            .and_then(|f| f.app_name.clone())
            .or_else(|| self.label.clone())
            .unwrap_or_else(|| self.package.clone())
    }

    /// Whether the Android package must say more than cargo-apk writes: another id, label,
    /// version or resources.
    pub(crate) fn android_overrides(&self) -> bool {
        let flavor = self.variant.flavor.as_ref();
        flavor
            .is_some_and(|f| f.app_name.is_some() || f.id_suffix.is_some() || f.resources.is_some())
            || self.variant.build_name.is_some()
            || self.build_number().is_some()
    }

    /// The cargo arguments the variant adds: `--features …`.
    pub(crate) fn feature_args(&self) -> Vec<String> {
        if self.variant.features.is_empty() {
            Vec::new()
        } else {
            vec!["--features".into(), self.variant.features.join(",")]
        }
    }
}

/// cargo-apk's version code for a version: `1.2.3` → `1 << 24 | 1 << 16 | 2 << 8 | 3`.
pub(crate) fn derived_version_code(version: &str) -> u32 {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u32>().unwrap_or(0).min(255));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    1 << 24 | major << 16 | minor << 8 | patch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<(Variant, Option<Mode>), String> {
        let mut variant = Variant::default();
        let mut mode = None;
        let mut args = line.split_whitespace().map(str::to_string);
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let has_inline = inline.is_some();
            let mut value = |flag: &str| match &inline {
                Some(v) => Ok(v.clone()),
                None => args.next().ok_or_else(|| format!("{flag} needs a value")),
            };
            if !parse_flag(&flag, has_inline, &mut value, &mut variant, &mut mode)? {
                return Err(format!("not ours: {flag}"));
            }
        }
        Ok((variant, mode))
    }

    #[test]
    fn the_shared_flags_are_read() {
        let (variant, mode) = parse(
            "--profile --flavor dev --define API_URL=https://x.y/?a=b --define=EMPTY= \
             --define-from-file env/dev.json --build-name 1.2.3 --build-number=42",
        )
        .unwrap();
        assert_eq!(mode, Some(Mode::Profile));
        assert_eq!(variant.flavor.as_deref(), Some("dev"));
        assert_eq!(
            variant.defines,
            [
                Define::Value("API_URL".into(), "https://x.y/?a=b".into()),
                Define::Value("EMPTY".into(), String::new()),
                Define::File("env/dev.json".into()),
            ]
        );
        assert_eq!(variant.build_name.as_deref(), Some("1.2.3"));
        assert_eq!(variant.build_number, Some(42));
        // The leading toolkit's spelling is understood too.
        assert!(parse("--dart-define A=1").is_ok());
        assert_eq!(parse("--release --release").unwrap().1, Some(Mode::Release));
    }

    #[test]
    fn mistakes_are_named() {
        let error = |line| parse(line).unwrap_err();
        assert!(error("--debug --release").contains("one mode at a time"));
        assert!(error("--profile --release").contains("one mode at a time"));
        assert!(error("--define NOVALUE").contains("KEY=VALUE"));
        assert!(error("--define 1A=x").contains("letters, digits"));
        assert!(error("--define A-B=x").contains("letters, digits"));
        assert!(error("--build-number 0").contains("whole number"));
        assert!(error("--build-number x").contains("whole number"));
        assert!(error("--build-number 2100000001").contains("whole number"));
        assert!(error("--flavor a --flavor b").contains("given twice"));
        assert!(error("--flavor a.b").contains("--flavor takes"));
        assert!(error("--flavor").contains("needs a value"));
        assert!(error("--profile=yes").contains("not ours"));
    }

    const MANIFEST: &str = r#"
[package]
name = "app"
version = "1.4.2+17"

[features]
dev = []
staging = []
"mock-api" = []

[package.metadata.android]
package = "com.acme.app"

[package.metadata.android.application]
label = "Acme"

[package.metadata.frus.flavors.dev]
app_name = "Acme Dev"
application_id_suffix = ".dev"
resources = "flavors/dev/res"
features = ["dev", "mock-api"]

[package.metadata.frus.flavors.dev.defines]
API_URL = "https://dev.acme.com"  # the test server
RETRIES = 3
VERBOSE = true

[package.metadata.frus.flavors.staging]

[package.metadata.frus.flavors.prod]
"#;

    fn project() -> Project {
        Project::from_manifest(Path::new("."), MANIFEST).unwrap()
    }

    #[test]
    fn flavors_are_read_from_the_manifest() {
        let project = project();
        let names: Vec<_> = project.flavors.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["dev", "staging", "prod"]);
        let dev = &project.flavors[0];
        assert_eq!(dev.app_name.as_deref(), Some("Acme Dev"));
        assert_eq!(dev.id_suffix.as_deref(), Some(".dev"));
        assert_eq!(dev.resources.as_deref(), Some("flavors/dev/res"));
        assert_eq!(
            dev.features.as_deref(),
            Some(&["dev".to_string(), "mock-api".to_string()][..])
        );
        assert_eq!(
            dev.defines,
            [
                ("API_URL".to_string(), "https://dev.acme.com".to_string()),
                ("RETRIES".to_string(), "3".to_string()),
                ("VERBOSE".to_string(), "true".to_string()),
            ]
        );
        assert_eq!(project.features, ["dev", "staging", "mock-api"]);
        assert_eq!(project.label.as_deref(), Some("Acme"));
    }

    #[test]
    fn a_flavor_changes_the_id_the_name_the_features_and_the_defines() {
        let mut project = project();
        let (variant, _) =
            parse("--flavor dev --define API_URL=https://mine --define X=1").unwrap();
        variant
            .apply(Mode::Debug, &mut project, Path::new("."))
            .unwrap();
        assert_eq!(project.android_package, "com.acme.app.dev");
        assert_eq!(project.display_name(), "Acme Dev");
        assert_eq!(project.feature_args(), ["--features", "dev,mock-api"]);
        // The flag wins over the flavor's value, and keeps its own place.
        assert_eq!(
            project.variant.defines,
            [
                ("RETRIES".to_string(), "3".to_string()),
                ("VERBOSE".to_string(), "true".to_string()),
                ("API_URL".to_string(), "https://mine".to_string()),
                ("X".to_string(), "1".to_string()),
            ]
        );
        assert!(project.android_overrides());

        // A flavor without a features list turns on the feature of its name, if there is one.
        let mut staging = self::project();
        let (variant, _) = parse("--flavor staging").unwrap();
        variant
            .apply(Mode::Release, &mut staging, Path::new("."))
            .unwrap();
        assert_eq!(staging.feature_args(), ["--features", "staging"]);
        assert_eq!(staging.android_package, "com.acme.app");
        assert_eq!(staging.display_name(), "Acme");
        let mut prod = self::project();
        let (variant, _) = parse("--flavor prod").unwrap();
        variant
            .apply(Mode::Release, &mut prod, Path::new("."))
            .unwrap();
        assert!(prod.feature_args().is_empty());

        let mut unknown = self::project();
        let (variant, _) = parse("--flavor qa").unwrap();
        let error = variant
            .apply(Mode::Debug, &mut unknown, Path::new("."))
            .unwrap_err();
        assert!(error.contains("dev, staging, prod"), "{error}");
    }

    #[test]
    fn the_environment_carries_the_variant() {
        let mut project = project();
        let (variant, _) = parse("--flavor dev --build-name 2.0.0 --build-number 7").unwrap();
        variant
            .apply(Mode::Profile, &mut project, Path::new("."))
            .unwrap();
        let env = environment(&project);
        let get = |name: &str| {
            env.iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{name} is not in {env:?}"))
        };
        assert_eq!(get("FRUS_BUILD_MODE").as_deref(), Some("profile"));
        assert_eq!(get("FRUS_FLAVOR").as_deref(), Some("dev"));
        assert_eq!(get("FRUS_BUILD_NAME").as_deref(), Some("2.0.0"));
        assert_eq!(get("FRUS_BUILD_NUMBER").as_deref(), Some("7"));
        assert_eq!(
            get("FRUS_DEFINE_API_URL").as_deref(),
            Some("https://dev.acme.com")
        );
        // No `[profile.profile]` in this manifest: cargo gets it from the environment.
        assert_eq!(
            get("CARGO_PROFILE_PROFILE_INHERITS").as_deref(),
            Some("release")
        );

        // A plain debug build clears what an earlier variant may have left.
        let mut plain = self::project();
        Variant::default()
            .apply(Mode::Debug, &mut plain, Path::new("."))
            .unwrap();
        let env = environment(&plain);
        assert!(env
            .iter()
            .all(|(n, v)| v.is_none() || n.starts_with(DEFINE_PREFIX)));
        assert!(!env.iter().any(|(n, _)| n.starts_with("CARGO_PROFILE")));
    }

    #[test]
    fn versions_come_from_the_flags_then_the_manifest() {
        let mut project = project();
        Variant::default()
            .apply(Mode::Release, &mut project, Path::new("."))
            .unwrap();
        assert_eq!(project.version_name(), "1.4.2");
        assert_eq!(project.build_number(), Some(17));
        assert_eq!(project.version_code(), 17);
        let (variant, _) = parse("--build-name 3.0.1").unwrap();
        variant
            .apply(Mode::Release, &mut project, Path::new("."))
            .unwrap();
        assert_eq!(project.version_name(), "3.0.1");
        assert_eq!(project.version_code(), 17, "the manifest's number stays");
        project.version = Some("3.0.1".into());
        assert_eq!(project.version_code(), derived_version_code("3.0.1"));
        assert_eq!(derived_version_code("3.0.1"), (1 << 24) | (3 << 16) | 1);
        assert_eq!(derived_version_code("0.1.0-beta.2"), (1 << 24) | (1 << 8));
    }

    #[test]
    fn define_files_are_json_or_env() {
        let json = defines_from_json(
            "\u{feff}{\"API_URL\": \"https://a\", \"RETRIES\": 3, \"RATE\": 0.5, \"ON\": true}",
        )
        .unwrap();
        assert_eq!(
            json,
            [
                ("API_URL".to_string(), "https://a".to_string()),
                ("RETRIES".to_string(), "3".to_string()),
                ("RATE".to_string(), "0.5".to_string()),
                ("ON".to_string(), "true".to_string()),
            ]
        );
        assert!(defines_from_json("[1]").is_err());
        assert!(defines_from_json("{\"A\": [1]}").is_err());
        let env = defines_from_env(
            "# comment\n\nAPI_URL=https://a?b=c\nexport NAME=\"Ada Lovelace\"\nQUOTED='x # y'\nTRAIL=v # note\n",
        )
        .unwrap();
        assert_eq!(
            env,
            [
                ("API_URL".to_string(), "https://a?b=c".to_string()),
                ("NAME".to_string(), "Ada Lovelace".to_string()),
                ("QUOTED".to_string(), "x # y".to_string()),
                ("TRAIL".to_string(), "v".to_string()),
            ]
        );
        assert!(defines_from_env("JUSTAWORD").is_err());
    }

    #[test]
    fn a_file_then_the_flags_decide() {
        let dir = std::env::temp_dir().join(format!("frus-defines-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dev.env"), "API_URL=from-file\nONLY_FILE=1\n").unwrap();
        let mut project = project();
        let (variant, _) = parse("--define API_URL=from-flag --define-from-file dev.env").unwrap();
        variant.apply(Mode::Debug, &mut project, &dir).unwrap();
        let get = |k: &str| {
            project
                .variant
                .defines
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("API_URL"), Some("from-flag"));
        assert_eq!(get("ONLY_FILE"), Some("1"));
        let (missing, _) = parse("--define-from-file nope.json").unwrap();
        assert!(missing
            .apply(Mode::Debug, &mut self::project(), &dir)
            .unwrap_err()
            .contains("nope.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
