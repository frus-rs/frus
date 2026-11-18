// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The limitations registry cannot rot** (milestone 818).
//!
//! `LIMITATIONS.md`, at the repository's root, lists every known limitation of frus under a
//! stable id. This test holds it to its own rules: every id is well-formed and unique, none
//! is missing (an entry is marked fixed, never deleted), every entry has its fields and a
//! known status, and every file it cites as evidence exists.
//!
//! It reads the repository, so it is left out of the published crate (`exclude` in the
//! manifest).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The fields every entry carries, in this order.
const FIELDS: [&str; 6] = [
    "Area",
    "Platforms",
    "Status",
    "What you hit",
    "Workaround",
    "Evidence",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> String {
    let path = root().join("LIMITATIONS.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()))
}

/// One entry: its id's number, its title and the lines below its heading.
struct Entry {
    line: usize,
    number: u32,
    id: String,
    title: String,
    body: Vec<String>,
}

/// Parses `### L-NNN Title` headings and the lines up to the next heading. A `### ` heading
/// that is not a well-formed id is a failure, reported with its line.
fn entries(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries: Vec<Entry> = Vec::new();
    let mut problems = Vec::new();
    let mut current: Option<Entry> = None;
    for (index, line) in text.lines().enumerate() {
        let n = index + 1;
        if line.starts_with('#') {
            entries.extend(current.take());
        }
        let Some(heading) = line.strip_prefix("### ") else {
            if let Some(entry) = current.as_mut() {
                entry.body.push(line.to_owned());
            }
            continue;
        };
        let (id, title) = heading.split_once(' ').unwrap_or((heading, ""));
        let number = id
            .strip_prefix("L-")
            .filter(|digits| digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u32>().ok());
        match number {
            Some(number) if number > 0 && !title.trim().is_empty() => {
                current = Some(Entry {
                    line: n,
                    number,
                    id: id.to_owned(),
                    title: title.trim().to_owned(),
                    body: Vec::new(),
                });
            }
            _ => problems.push(format!(
                "line {n}: `{line}` is not `### L-NNN Title` (three or more digits, from 001)"
            )),
        }
    }
    entries.extend(current);
    (entries, problems)
}

/// The value of `- **Field:** value`, with its continuation lines joined.
fn field(body: &[String], name: &str) -> Option<String> {
    let start = format!("- **{name}:**");
    let at = body.iter().position(|l| l.starts_with(&start))?;
    let mut value = body[at][start.len()..].trim().to_owned();
    for line in &body[at + 1..] {
        if line.starts_with("- ") || line.trim().is_empty() {
            break;
        }
        value.push(' ');
        value.push_str(line.trim());
    }
    Some(value)
}

/// `fixed in X.Y.Z`, with a version of three numbers.
fn is_fixed_in(status: &str) -> bool {
    status.strip_prefix("fixed in ").is_some_and(|version| {
        let parts: Vec<&str> = version.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// The targets of `[text](target)` links in `text`.
fn link_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        if let Some(end) = rest.find(')') {
            targets.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    targets
}

/// Every `docs/milestone-NNN.md` named in `text`, linked or not.
fn milestone_mentions(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("docs/milestone-") {
        let tail = &rest[at + "docs/milestone-".len()..];
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && tail[digits.len()..].starts_with(".md") {
            found.push(format!("docs/milestone-{digits}.md"));
        }
        rest = tail;
    }
    found
}

#[test]
fn ids_are_well_formed_unique_and_none_is_missing() {
    let (entries, mut problems) = entries(&registry());
    assert!(!entries.is_empty(), "LIMITATIONS.md lists no entry");

    let mut seen: BTreeMap<u32, usize> = BTreeMap::new();
    for entry in &entries {
        if let Some(first) = seen.insert(entry.number, entry.line) {
            problems.push(format!(
                "line {}: {} is used again (first at line {first}); a new entry takes the \
                 number after the highest",
                entry.line, entry.id
            ));
        }
    }
    let highest = *seen.keys().last().unwrap();
    for number in 1..=highest {
        if !seen.contains_key(&number) {
            problems.push(format!(
                "L-{number:03} is missing: an entry is never deleted, its status becomes \
                 `fixed in X.Y.Z`"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_entry_has_its_fields_and_a_known_status() {
    let (entries, _) = entries(&registry());
    let mut problems = Vec::new();
    for entry in &entries {
        let at = format!("{} ({}, line {})", entry.id, entry.title, entry.line);
        for name in FIELDS {
            match field(&entry.body, name) {
                None => problems.push(format!("{at}: no `- **{name}:**` line")),
                Some(value) if value.is_empty() => {
                    problems.push(format!("{at}: `{name}` is empty"))
                }
                Some(_) => {}
            }
        }
        if let Some(status) = field(&entry.body, "Status") {
            if !matches!(status.as_str(), "open" | "partial") && !is_fixed_in(&status) {
                problems.push(format!(
                    "{at}: status `{status}` is not `open`, `partial` or `fixed in X.Y.Z`"
                ));
            }
        }
        if let Some(evidence) = field(&entry.body, "Evidence") {
            if link_targets(&evidence).is_empty() {
                problems.push(format!(
                    "{at}: the evidence links to nothing (an issue, a milestone, a test)"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_cited_file_exists() {
    let text = registry();
    let root = root();
    let mut cited: Vec<String> = link_targets(&text)
        .into_iter()
        .filter(|t| !t.contains("://") && !t.starts_with('#') && !t.starts_with("mailto:"))
        .map(|t| t.split('#').next().unwrap_or_default().to_owned())
        .collect();
    cited.extend(milestone_mentions(&text));
    cited.sort();
    cited.dedup();
    assert!(
        cited.iter().any(|c| c.starts_with("docs/milestone-")),
        "no milestone is cited; the parser has probably stopped finding links"
    );
    let missing: Vec<&String> = cited
        .iter()
        .filter(|path| !root.join(path.as_str()).exists())
        .collect();
    assert!(
        missing.is_empty(),
        "LIMITATIONS.md cites files that do not exist: {missing:?}"
    );
}

#[test]
fn the_parser_reads_what_the_registry_writes() {
    let sample = "\
## Area

### L-001 A title

- **Area:** text
- **Platforms:** All
- **Status:** fixed in 0.6.0
- **What you hit:** one line
  and another.
- **Workaround:** None.
- **Evidence:** [docs/milestone-818.md](docs/milestone-818.md), docs/milestone-1.md

### L-2 too short
";
    let (entries, problems) = entries(sample);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "L-001");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        field(&entries[0].body, "What you hit").as_deref(),
        Some("one line and another.")
    );
    assert!(is_fixed_in("fixed in 0.6.0"));
    assert!(!is_fixed_in("fixed in 0.6"));
    assert!(!is_fixed_in("fixed soon"));
    assert_eq!(
        milestone_mentions(sample),
        [
            "docs/milestone-818.md",
            "docs/milestone-818.md",
            "docs/milestone-1.md"
        ]
    );
}
