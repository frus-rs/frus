// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The platforms an application is made for** (milestone 791), and the links that open it.
//!
//! `frus create --platforms android,web` writes only what those need, and records them in the
//! manifest:
//!
//! ```toml
//! [package.metadata.frus]
//! platforms = ["android", "web"]
//! ```
//!
//! `frus run` and `frus build` read the list back: `-d all` runs on the devices of these
//! platforms, and a platform left out is refused with the command that adds it
//! (`frus create --platforms web .`). A manifest without the list — every application made before
//! it existed — is made for all of them.
//!
//! The template's manifest is written in **regions**, each starting at a `# --- <title> ---`
//! comment: the Android one is cut from an application without Android, and is what is appended to
//! one that adds Android later. The same goes for the README's `## <platform>` sections and the
//! GitHub workflows' jobs.

use std::fmt;

/// A platform an application can be made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AppPlatform {
    Android,
    Web,
    Windows,
    Macos,
    Linux,
}

impl AppPlatform {
    /// Every platform frus builds for, in the order they are listed.
    pub(crate) const ALL: [AppPlatform; 5] = [
        AppPlatform::Android,
        AppPlatform::Web,
        AppPlatform::Windows,
        AppPlatform::Macos,
        AppPlatform::Linux,
    ];

    /// The word the manifest and `--platforms` use.
    pub(crate) fn word(self) -> &'static str {
        match self {
            AppPlatform::Android => "android",
            AppPlatform::Web => "web",
            AppPlatform::Windows => "windows",
            AppPlatform::Macos => "macos",
            AppPlatform::Linux => "linux",
        }
    }

    pub(crate) fn from_word(word: &str) -> Option<AppPlatform> {
        AppPlatform::ALL.into_iter().find(|p| p.word() == word)
    }

    pub(crate) fn is_desktop(self) -> bool {
        matches!(
            self,
            AppPlatform::Windows | AppPlatform::Macos | AppPlatform::Linux
        )
    }

    /// The desktop this tool runs on: what `frus run` without `-d` starts.
    pub(crate) fn host() -> AppPlatform {
        if cfg!(windows) {
            AppPlatform::Windows
        } else if cfg!(target_os = "macos") {
            AppPlatform::Macos
        } else {
            AppPlatform::Linux
        }
    }
}

impl fmt::Display for AppPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// `--platforms android,web`: the platforms named, in the usual order, each once. `desktop` is
/// the three desktops.
pub(crate) fn parse_list(text: &str) -> Result<Vec<AppPlatform>, String> {
    let mut found = Vec::new();
    for word in text.split(',').map(|w| w.trim().to_ascii_lowercase()) {
        match word.as_str() {
            "" => continue,
            "desktop" => {
                found.extend([AppPlatform::Windows, AppPlatform::Macos, AppPlatform::Linux])
            }
            "ios" => {
                return Err(
                    "frus does not build for iOS yet: its iOS shell does not exist (issue #19). \
                     Leave `ios` out; `frus create --platforms ios .` will add it once it does"
                        .into(),
                )
            }
            other => match AppPlatform::from_word(other) {
                Some(platform) => found.push(platform),
                None => {
                    return Err(format!(
                        "`{other}` is not a platform: --platforms takes {} (or `desktop`)",
                        words(&AppPlatform::ALL)
                    ))
                }
            },
        }
    }
    if found.is_empty() {
        return Err("--platforms needs at least one platform: `--platforms android,web`".into());
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// `android, web`.
pub(crate) fn words(platforms: &[AppPlatform]) -> String {
    platforms
        .iter()
        .map(|p| p.word())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The manifest's list: `["android", "web"]`.
pub(crate) fn toml_list(platforms: &[AppPlatform]) -> String {
    let items: Vec<String> = platforms
        .iter()
        .map(|p| format!("\"{}\"", p.word()))
        .collect();
    format!("[{}]", items.join(", "))
}

/// The platforms a manifest's list names; unknown words are skipped (a newer frus may know
/// them).
pub(crate) fn from_words(words: &[String]) -> Vec<AppPlatform> {
    let mut found: Vec<AppPlatform> = words
        .iter()
        .filter_map(|w| AppPlatform::from_word(w))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Whether an application whose manifest lists `listed` (None: no list, every platform) is made
/// for `platform`; when it is not, the error says how to add it.
pub(crate) fn check(listed: Option<&[AppPlatform]>, platform: AppPlatform) -> Result<(), String> {
    match listed {
        Some(listed) if !listed.contains(&platform) => Err(format!(
            "this application is not made for {platform}: its Cargo.toml lists {} \
             ([package.metadata.frus] platforms). `frus create --platforms {platform} .` adds it, \
             with the files it needs",
            if listed.is_empty() {
                "none".to_string()
            } else {
                words(listed)
            }
        )),
        _ => Ok(()),
    }
}

// --- Links that open the application ---

/// `--deeplink`: a scheme of the application's own (`myapp://orders/42`), or the `https` links
/// of a host it owns (`https://example.com/orders/42`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Deeplink {
    Scheme(String),
    Https { host: String },
}

/// Schemes that belong to the system or the web, which an application cannot take.
const TAKEN_SCHEMES: &[&str] = &[
    "http",
    "file",
    "content",
    "intent",
    "android-app",
    "data",
    "javascript",
    "about",
    "mailto",
    "tel",
    "sms",
    "geo",
    "market",
    "ftp",
    "blob",
    "ws",
    "wss",
];

impl Deeplink {
    /// `myapp`, `myapp://`, `https://example.com` or `example.com`.
    pub(crate) fn parse(text: &str) -> Result<Deeplink, String> {
        let text = text.trim();
        let (scheme, host) = match text.split_once("://") {
            Some((scheme, rest)) => (
                Some(scheme.to_ascii_lowercase()),
                rest.split(['/', '?', '#']).next().unwrap_or(""),
            ),
            None if text.contains('.') => (None, text),
            None => (Some(text.to_ascii_lowercase()), ""),
        };
        match scheme.as_deref() {
            Some("https") | None => {
                let host = host.to_ascii_lowercase();
                let shaped = host.contains('.')
                    && !host.starts_with(['.', '-'])
                    && !host.ends_with(['.', '-'])
                    && !host.contains("..")
                    && host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
                if !shaped {
                    return Err(format!(
                        "--deeplink {text}: `{host}` is not a host name (`https://example.com`)"
                    ));
                }
                Ok(Deeplink::Https { host })
            }
            Some("http") => Err(format!(
                "--deeplink {text}: links of the web open an application only over https \
                 (`https://{host}`)"
            )),
            Some(scheme) => {
                let shaped = scheme
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase())
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c));
                if !shaped {
                    return Err(format!(
                        "--deeplink {text}: `{scheme}` is not a scheme: a letter, then letters, \
                         digits, `+`, `-` or `.` (`myapp`)"
                    ));
                }
                if TAKEN_SCHEMES.contains(&scheme) {
                    return Err(format!(
                        "--deeplink {text}: `{scheme}` belongs to the system: choose the \
                         application's own (`myapp`)"
                    ));
                }
                Ok(Deeplink::Scheme(scheme.to_string()))
            }
        }
    }

    /// As the manifest records it: `myapp` or `https://example.com`.
    pub(crate) fn text(&self) -> String {
        match self {
            Deeplink::Scheme(scheme) => scheme.clone(),
            Deeplink::Https { host } => format!("https://{host}"),
        }
    }

    /// A link it opens, for the examples: `myapp://home`, `https://example.com/home`.
    pub(crate) fn example(&self) -> String {
        match self {
            Deeplink::Scheme(scheme) => format!("{scheme}://home"),
            Deeplink::Https { host } => format!("https://{host}/home"),
        }
    }

    /// The scheme a desktop registers, when it is one of the application's own.
    pub(crate) fn scheme(&self) -> Option<&str> {
        match self {
            Deeplink::Scheme(scheme) => Some(scheme),
            Deeplink::Https { .. } => None,
        }
    }

    /// cargo-apk's `data` entry for the intent filter.
    fn android_data(&self) -> String {
        match self {
            Deeplink::Scheme(scheme) => format!("{{ scheme = \"{scheme}\" }}"),
            Deeplink::Https { host } => {
                format!("{{ scheme = \"https\", host = \"{host}\" }}")
            }
        }
    }

    /// The intent filter that opens the application with these links, for the manifest.
    pub(crate) fn android_filter(&self) -> String {
        let note = match self {
            Deeplink::Scheme(_) => String::new(),
            Deeplink::Https { host } => format!(
                "# Android opens `https://{host}` links here once the site vouches for the\n\
                 # application: put the `assetlinks.json` that `frus create` printed at\n\
                 # https://{host}/.well-known/assetlinks.json. Until then (and on Android 12 and\n\
                 # later, where the filter cannot ask for the check, see the README), the person\n\
                 # turns it on in the application's settings, \"Open by default\".\n"
            ),
        };
        format!(
            "# The links that open the application (`frus create --deeplink`): `{example}`\n\
             # starts it at the location `/home`.\n\
             {note}\
             [[package.metadata.android.application.activity.intent_filter]]\n\
             actions = [\"android.intent.action.VIEW\"]\n\
             categories = [\"android.intent.category.DEFAULT\", \"android.intent.category.BROWSABLE\"]\n\
             data = [{data}]\n",
            example = self.example(),
            data = self.android_data(),
        )
    }

    /// The same filter as Android's manifest XML, for a manifest frus writes itself (the
    /// hot-reload engine's): there an `https` filter asks Android to check the host.
    pub(crate) fn android_filter_xml(&self) -> String {
        let (verify, data) = match self {
            Deeplink::Scheme(scheme) => (
                String::new(),
                format!("<data android:scheme=\"{scheme}\" />"),
            ),
            Deeplink::Https { host } => (
                " android:autoVerify=\"true\"".to_string(),
                format!("<data android:scheme=\"https\" android:host=\"{host}\" />"),
            ),
        };
        format!(
            "            <intent-filter{verify}>\n\
             \x20               <action android:name=\"android.intent.action.VIEW\" />\n\
             \x20               <category android:name=\"android.intent.category.DEFAULT\" />\n\
             \x20               <category android:name=\"android.intent.category.BROWSABLE\" />\n\
             \x20               {data}\n\
             \x20           </intent-filter>\n"
        )
    }

    /// Whether `manifest` already has an intent filter for these links.
    pub(crate) fn declared_in(&self, manifest: &str) -> bool {
        let data = self.android_data();
        manifest.lines().any(|line| {
            line.trim_start().starts_with("data") && line.contains(&data[2..data.len() - 2])
        })
    }

    /// The `assetlinks.json` the host serves so that Android opens its links in the application
    /// without asking.
    pub(crate) fn assetlinks(&self, package: &str) -> Option<String> {
        let Deeplink::Https { .. } = self else {
            return None;
        };
        Some(format!(
            "[{{\n  \"relation\": [\"delegate_permission/common.handle_all_urls\"],\n  \
             \"target\": {{\n    \"namespace\": \"android_app\",\n    \
             \"package_name\": \"{package}\",\n    \
             \"sha256_cert_fingerprints\": [\"<SHA-256 of your release key>\"]\n  }}\n}}]\n"
        ))
    }
}

// --- Regions of the template's files ---

/// The lines of `text`, each with its `\n`.
fn lines_with_ends(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Cuts the region starting at the line that starts with `heading` and ending before the next
/// line that starts with `next` (the next region's heading), or at the end. Gives the text without
/// it, and the region.
pub(crate) fn cut_region(text: &str, heading: &str, next: &str) -> (String, Option<String>) {
    let lines = lines_with_ends(text);
    let Some(start) = lines.iter().position(|l| l.starts_with(heading)) else {
        return (text.to_string(), None);
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with(next))
        .map_or(lines.len(), |i| start + 1 + i);
    let region: String = lines[start..end].concat();
    let rest: String = lines[..start].concat() + &lines[end..].concat();
    (rest, Some(region))
}

/// The template manifest's region of a platform: its heading. The desktops have none.
pub(crate) fn manifest_heading(platform: AppPlatform) -> Option<&'static str> {
    match platform {
        AppPlatform::Android => Some("# --- Android"),
        AppPlatform::Web => Some("# --- Web"),
        _ => None,
    }
}

/// The table headers in a piece of TOML: `[profile.web-release]`, `[[…uses_permission]]`.
pub(crate) fn table_headers(toml: &str) -> Vec<String> {
    toml.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('['))
        .map(str::to_string)
        .collect()
}

/// Replaces the `key = …` line of a table, or adds it after the header. None when the table is
/// not in `manifest`.
pub(crate) fn set_key(manifest: &str, table: &str, key: &str, value: &str) -> Option<String> {
    let header = format!("[{table}]");
    let lines = lines_with_ends(manifest);
    let start = lines.iter().position(|l| l.trim() == header)?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    let newline = if manifest.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let wanted = format!("{key} = {value}{newline}");
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match (start + 1..end).find(|&i| {
        lines[i]
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == key && !lines[i].trim_start().starts_with('#'))
    }) {
        Some(i) => out[i] = wanted,
        None => out.insert(start + 1, wanted),
    }
    Some(out.concat())
}

/// Appends `block` to `manifest`, after a blank line, in the manifest's own line endings.
pub(crate) fn append(manifest: &str, block: &str) -> String {
    let crlf = manifest.contains("\r\n");
    let mut out = manifest.to_string();
    let newline = if crlf { "\r\n" } else { "\n" };
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(newline);
    }
    if !out.is_empty() && !out.ends_with(&format!("{newline}{newline}")) {
        out.push_str(newline);
    }
    let block = block.replace("\r\n", "\n");
    out.push_str(&if crlf {
        block.replace('\n', "\r\n")
    } else {
        block
    });
    out
}

/// The README without the sections of the platforms left out: `## Android`, its table row.
pub(crate) fn trim_readme(readme: &str, platforms: &[AppPlatform]) -> String {
    let desktop = platforms.iter().any(|p| p.is_desktop());
    let mut text = readme.to_string();
    for (title, wanted) in [
        ("Desktop", desktop),
        ("Android", platforms.contains(&AppPlatform::Android)),
        ("Web", platforms.contains(&AppPlatform::Web)),
    ] {
        if wanted {
            continue;
        }
        text = cut_region(&text, &format!("## {title}\n"), "## ").0;
        text = text
            .split_inclusive('\n')
            .filter(|line| !line.starts_with(&format!("| {title} |")))
            .collect();
    }
    text
}

/// A GitHub workflow with only the jobs of `platforms`: the `android` and `web` jobs go with
/// their platform, a desktop's matrix entries with that desktop, and a job left with no entry
/// goes too. `needs:` lists follow.
pub(crate) fn trim_workflow(workflow: &str, platforms: &[AppPlatform]) -> String {
    let has = |p: AppPlatform| platforms.contains(&p);
    let mut removed: Vec<&str> = Vec::new();
    if !has(AppPlatform::Android) {
        removed.push("android");
    }
    if !has(AppPlatform::Web) {
        removed.push("web");
    }
    // A matrix entry names its runner: windows-*, macos-*, ubuntu-* (Linux).
    let runner_wanted = |line: &str| {
        let wants = |runner: &str, platform| !line.contains(runner) || has(platform);
        wants("windows-", AppPlatform::Windows)
            && wants("macos-", AppPlatform::Macos)
            && wants("ubuntu-", AppPlatform::Linux)
    };
    let mut lines: Vec<String> = workflow.split_inclusive('\n').map(str::to_string).collect();

    // The desktop job's matrix: `os: [windows-latest, macos-latest]`, or `include:` entries.
    let desktop = job_range(&lines, "desktop");
    if let Some((start, end)) = desktop {
        let mut kept_entries = 0;
        let mut out = Vec::new();
        for line in &lines[start..end] {
            let trimmed = line.trim_start();
            if trimmed.starts_with("os: [") {
                let inside = trimmed
                    .trim_start_matches("os: [")
                    .trim_end()
                    .trim_end_matches(']');
                let kept: Vec<&str> = inside
                    .split(',')
                    .map(str::trim)
                    .filter(|os| runner_wanted(os))
                    .collect();
                kept_entries += kept.len();
                let indent = &line[..line.len() - trimmed.len()];
                out.push(format!("{indent}os: [{}]\n", kept.join(", ")));
            } else if trimmed.starts_with("- { os:") {
                if runner_wanted(trimmed) {
                    kept_entries += 1;
                    out.push(line.clone());
                }
            } else {
                out.push(line.clone());
            }
        }
        if kept_entries == 0 {
            removed.push("desktop");
        } else {
            lines.splice(start..end, out);
        }
    }
    for job in &removed {
        if let Some((start, end)) = job_range(&lines, job) {
            lines.drain(start..end);
        }
    }
    // `needs: [meta, desktop, android, web]` without the jobs that are gone.
    for line in &mut lines {
        let trimmed = line.trim_start();
        if let Some(list) = trimmed.strip_prefix("needs: [") {
            let indent = line[..line.len() - trimmed.len()].to_string();
            let kept: Vec<&str> = list
                .trim_end()
                .trim_end_matches(']')
                .split(',')
                .map(str::trim)
                .filter(|job| !removed.contains(job))
                .collect();
            *line = format!("{indent}needs: [{}]\n", kept.join(", "));
        }
    }
    lines.concat()
}

/// The lines of the job `name` under `jobs:` (two spaces in), with the comment lines just above
/// it, up to the next job.
fn job_range(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let header = format!("  {name}:");
    let at = lines.iter().position(|l| l.trim_end() == header)?;
    let mut start = at;
    while start > 0 && lines[start - 1].starts_with("  #") {
        start -= 1;
    }
    let mut end = at + 1;
    while end < lines.len() {
        let line = &lines[end];
        let is_next_job =
            line.starts_with("  ") && !line.starts_with("   ") && !line.trim().is_empty();
        if is_next_job {
            break;
        }
        end += 1;
    }
    // The comment lines just above the next job are that job's.
    while end > at + 1 && lines[end - 1].starts_with("  #") {
        end -= 1;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use AppPlatform::*;

    #[test]
    fn the_list_is_read_in_any_order_and_spelling() {
        assert_eq!(parse_list("web,android").unwrap(), [Android, Web]);
        assert_eq!(parse_list(" Android , WEB ,web").unwrap(), [Android, Web]);
        assert_eq!(parse_list("desktop").unwrap(), [Windows, Macos, Linux]);
        assert_eq!(
            parse_list("linux,desktop").unwrap(),
            [Windows, Macos, Linux]
        );
        assert!(parse_list("ios").unwrap_err().contains("iOS"));
        assert!(parse_list("android,symbian")
            .unwrap_err()
            .contains("`symbian` is not a platform"));
        assert!(parse_list(",").unwrap_err().contains("at least one"));
        assert_eq!(toml_list(&[Android, Web]), r#"["android", "web"]"#);
    }

    #[test]
    fn a_platform_left_out_is_refused_with_the_way_to_add_it() {
        assert!(check(None, Web).is_ok(), "no list: every platform");
        assert!(check(Some(&[Android, Web]), Web).is_ok());
        let error = check(Some(&[Android]), Web).unwrap_err();
        assert!(error.contains("not made for web"), "{error}");
        assert!(error.contains("frus create --platforms web ."), "{error}");
    }

    #[test]
    fn deeplinks_are_a_scheme_or_a_host() {
        assert_eq!(
            Deeplink::parse("myapp").unwrap(),
            Deeplink::Scheme("myapp".into())
        );
        assert_eq!(
            Deeplink::parse("MyApp://").unwrap(),
            Deeplink::Scheme("myapp".into())
        );
        let host = Deeplink::Https {
            host: "example.com".into(),
        };
        assert_eq!(Deeplink::parse("https://Example.com/a?b").unwrap(), host);
        assert_eq!(Deeplink::parse("example.com").unwrap(), host);
        assert_eq!(host.text(), "https://example.com");
        assert!(Deeplink::parse("http://example.com")
            .unwrap_err()
            .contains("https"));
        assert!(Deeplink::parse("file").unwrap_err().contains("system"));
        assert!(Deeplink::parse("2app")
            .unwrap_err()
            .contains("not a scheme"));
        assert!(Deeplink::parse("https://localhost")
            .unwrap_err()
            .contains("not a host"));
        assert!(Deeplink::parse("https://a..b").is_err());
    }

    #[test]
    fn the_intent_filter_is_what_cargo_apk_reads() {
        let scheme = Deeplink::Scheme("myapp".into());
        let filter = scheme.android_filter();
        assert!(filter.contains("[[package.metadata.android.application.activity.intent_filter]]"));
        assert!(
            filter.contains("data = [{ scheme = \"myapp\" }]"),
            "{filter}"
        );
        assert!(filter.contains("android.intent.category.BROWSABLE"));
        assert!(scheme.declared_in(&filter));
        assert!(!Deeplink::Scheme("other".into()).declared_in(&filter));
        assert!(scheme.assetlinks("com.acme.app").is_none());

        let host = Deeplink::Https {
            host: "acme.com".into(),
        };
        let filter = host.android_filter();
        assert!(filter.contains("data = [{ scheme = \"https\", host = \"acme.com\" }]"));
        assert!(filter.contains("/.well-known/assetlinks.json"));
        let json = host.assetlinks("com.acme.app").unwrap();
        assert!(
            json.contains("\"package_name\": \"com.acme.app\""),
            "{json}"
        );
        assert!(json.contains("handle_all_urls"));
    }

    #[test]
    fn regions_are_cut_from_heading_to_heading() {
        let text = "a\n# --- Web ---\nw1\nw2\n# --- Android ---\nx\n";
        let (rest, region) = cut_region(text, "# --- Web", "# --- ");
        assert_eq!(rest, "a\n# --- Android ---\nx\n");
        assert_eq!(region.unwrap(), "# --- Web ---\nw1\nw2\n");
        let (rest, region) = cut_region(text, "# --- Android", "# --- ");
        assert_eq!(rest, "a\n# --- Web ---\nw1\nw2\n");
        assert_eq!(region.unwrap(), "# --- Android ---\nx\n");
        assert_eq!(cut_region(text, "# --- Nope", "# --- ").1, None);
    }

    #[test]
    fn a_key_is_set_in_its_table() {
        let manifest = "[package]\nname = \"a\"\n\n[package.metadata.frus]\nplatforms = [\"web\"]\n\n[x]\ny = 1\n";
        let set = set_key(
            manifest,
            "package.metadata.frus",
            "platforms",
            r#"["android", "web"]"#,
        )
        .unwrap();
        assert!(set.contains("[package.metadata.frus]\nplatforms = [\"android\", \"web\"]\n\n[x]"));
        let added = set_key(manifest, "package.metadata.frus", "deeplink", "\"myapp\"").unwrap();
        assert!(added.contains("[package.metadata.frus]\ndeeplink = \"myapp\"\nplatforms"));
        assert!(set_key(manifest, "nope", "a", "1").is_none());
        let crlf = manifest.replace('\n', "\r\n");
        let set = set_key(&crlf, "package.metadata.frus", "deeplink", "\"m\"").unwrap();
        assert!(!set.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn appended_blocks_follow_a_blank_line_in_the_file_s_endings() {
        assert_eq!(append("a = 1\n", "[b]\nc = 2\n"), "a = 1\n\n[b]\nc = 2\n");
        assert_eq!(append("a = 1", "[b]\n"), "a = 1\n\n[b]\n");
        assert_eq!(append("a = 1\r\n\r\n", "[b]\n"), "a = 1\r\n\r\n[b]\r\n");
    }

    #[test]
    fn the_readme_loses_the_sections_of_the_platforms_left_out() {
        let readme =
            "# app\n\n| Desktop | x |\n| Android | y |\n| Web | z |\n\n## Desktop\n\nd\n\n\
                      ## Android\n\na\n\n## Web\n\nw\n\n## Permissions\n\np\n";
        let out = trim_readme(readme, &[Web]);
        assert_eq!(
            out,
            "# app\n\n| Web | z |\n\n## Web\n\nw\n\n## Permissions\n\np\n"
        );
        assert_eq!(trim_readme(readme, &AppPlatform::ALL), readme);
    }

    const CI: &str = "jobs:\n  check:\n    runs-on: ubuntu-24.04\n\n  desktop:\n    strategy:\n      matrix:\n        os: [windows-latest, macos-latest]\n    steps:\n      - run: cargo build\n\n  web:\n    runs-on: ubuntu-24.04\n\n  # The library.\n  android:\n    runs-on: ubuntu-24.04\n";

    #[test]
    fn a_workflow_keeps_the_jobs_of_its_platforms() {
        assert_eq!(trim_workflow(CI, &AppPlatform::ALL), CI);
        let windows_only = trim_workflow(CI, &[Windows]);
        assert!(
            windows_only.contains("os: [windows-latest]\n"),
            "{windows_only}"
        );
        assert!(!windows_only.contains("  web:"));
        assert!(!windows_only.contains("android"), "{windows_only}");
        assert!(windows_only.contains("  check:"));
        let android = trim_workflow(CI, &[Android]);
        assert!(!android.contains("desktop:"), "{android}");
        assert!(android.contains("  # The library.\n  android:"));

        let release = "jobs:\n  desktop:\n    strategy:\n      matrix:\n        include:\n          - { os: windows-latest, target: a }\n          - { os: ubuntu-24.04, target: b }\n          - { os: macos-latest, target: c }\n\n  web:\n    x: 1\n\n  publish:\n    needs: [meta, desktop, android, web]\n";
        let linux = trim_workflow(release, &[Linux, Web]);
        assert!(linux.contains("target: b"));
        assert!(!linux.contains("target: a") && !linux.contains("target: c"));
        assert!(linux.contains("needs: [meta, desktop, web]\n"), "{linux}");
        let web = trim_workflow(release, &[Web]);
        assert!(web.contains("needs: [meta, web]\n"), "{web}");
        assert!(!web.contains("desktop:"));
    }

    /// The template's own files have the regions and jobs these functions look for: a change to
    /// the template that loses one is caught here, not in an application.
    #[test]
    fn the_template_has_the_regions_this_cuts() {
        let text = |path: &str| {
            let file = crate::template::FILES
                .iter()
                .find(|f| f.path == path)
                .unwrap();
            std::str::from_utf8(file.bytes)
                .unwrap()
                .replace("\r\n", "\n")
        };
        let manifest = text("Cargo.toml.liquid");
        for platform in [Android, Web] {
            let heading = manifest_heading(platform).unwrap();
            let region = cut_region(&manifest, heading, "# --- ").1.unwrap();
            assert!(!table_headers(&region).is_empty(), "{platform}");
        }
        assert!(manifest.contains(&format!("\nplatforms = {}\n", toml_list(&AppPlatform::ALL))));
        for workflow in [".github/workflows/ci.yml", ".github/workflows/release.yml"] {
            let all = text(workflow);
            let none_but_linux = trim_workflow(&all, &[Linux]);
            assert!(!none_but_linux.contains("  android:"), "{workflow}");
            assert!(!none_but_linux.contains("  web:"), "{workflow}");
            assert!(!none_but_linux.contains("windows-latest"), "{workflow}");
        }
        let readme = text("README.md");
        let trimmed = trim_readme(&readme, &[Web]);
        assert!(!trimmed.contains("## Android"));
        assert!(trimmed.contains("## Web"));
    }
}
