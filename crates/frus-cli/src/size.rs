// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **What the shipped binary is made of** (milestone 791): `frus build <target> --analyze-size`,
//! and `frus size <file>` for a binary already built.
//!
//! The binary is broken down by the crate each piece of code and data comes from — frus itself,
//! the renderer, the text shaper, the standard library, the application — and by the files it
//! carries inside: the fonts frus bundles, the application's own assets. A short table is
//! printed; the whole of it is written as JSON beside the build, for tooling, and
//! `--compare old.json` says what grew and what shrank since.
//!
//! The shipped build has no symbols (`strip = true`), and without names nothing can be told
//! apart, so the analysis builds the same release once more with them kept, in a directory of its
//! own (`target/frus-size/`): the build you ship is left untouched, and the first analysis
//! compiles everything once. Its parts:
//!
//! - **ELF** (the Android `.so`, a Linux program): the symbol table, each function and object
//!   with its size.
//! - **WebAssembly**: each function body of the code section, named by the `name` section.
//! - **Windows**: the linker's map (`/MAP`), each symbol sized up to the next one.
//!
//! A symbol's crate is the first part of its path once demangled (`frus_widgets::…`); an `impl`'s
//! is its type's (`<frus_core::Color as core::fmt::Debug>::fmt` is frus-core's). What has no
//! Rust name is `[C/C++]`, and what no symbol covers — literals, tables the compiler made — is
//! counted as `[unattributed]`. A font is found by its bytes: every file of frus's bundled fonts
//! and of the application's `assets/` that is inside the binary whole is listed with its size.
//!
//! The tool depends on nothing, so the three formats are read here, as far as sizes go.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::build::Target;
use crate::cargo::Mode;
use crate::json::{object, parse, Value};
use crate::project::Project;

/// What `frus size` / `--analyze-size` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Options {
    /// Build this target with symbols and analyse it; or, when None, `file`.
    pub build: Option<Target>,
    /// A binary already built: `.exe` (with its `.map` beside it), `.so`, `.wasm`.
    pub file: Option<PathBuf>,
    /// A report written before, to compare with.
    pub compare: Option<PathBuf>,
}

/// One analysis, as the JSON holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Report {
    /// `windows`, `android`, `web`, or the file's own kind.
    pub label: String,
    pub file: String,
    /// `elf`, `wasm`, `pe`.
    pub format: &'static str,
    /// What the file weighs on the disk.
    pub file_bytes: u64,
    /// What it weighs once its symbols are gone: what ships.
    pub shipped_bytes: u64,
    /// Bytes by crate, largest first.
    pub crates: Vec<(String, u64)>,
    /// Files carried inside the binary: (path, bytes).
    pub assets: Vec<(String, u64)>,
    /// The binary's sections that load: (name, bytes).
    pub sections: Vec<(String, u64)>,
}

/// A piece of the binary with a name and a size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Symbol {
    pub name: String,
    pub size: u64,
}

/// What a binary was read as.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub format: &'static str,
    pub shipped: u64,
    pub sections: Vec<(String, u64)>,
    pub symbols: Vec<Symbol>,
    /// The bytes the symbols could cover: the code and the read-only data.
    pub covered: u64,
}

pub(crate) fn size(options: &Options) -> Result<ExitCode, String> {
    let (label, file, root) = match options.build {
        Some(target) => {
            let here = std::env::current_dir().map_err(|e| e.to_string())?;
            let project = Project::find(&here)?;
            project.check_platform(target.platform())?;
            let file = build_with_symbols(&project, target)?;
            (
                target.platform().word().to_string(),
                file,
                Some(project.root),
            )
        }
        None => {
            let file = options
                .file
                .clone()
                .ok_or("frus size needs a target (`frus size apk`) or a binary's path")?;
            let root = std::env::current_dir()
                .ok()
                .and_then(|here| Project::find(&here).ok())
                .map(|p| p.root);
            let label = file
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|| "binary".into());
            (label, file, root)
        }
    };
    let report = analyse(&label, &file, root.as_deref())?;
    print!("{}", table(&report));
    let previous = match &options.compare {
        Some(path) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| format!("--compare {}: {e}", path.display()))
                .and_then(|text| {
                    from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
                })?,
        ),
        None => None,
    };
    if let Some(previous) = &previous {
        print!("{}", comparison(previous, &report));
    }
    let dir = match &root {
        Some(root) => Project::find(root)?.target_dir()?.join("frus-size"),
        None => file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let json = numbered(&dir, &label);
    std::fs::write(&json, to_json(&report).to_json() + "\n")
        .map_err(|e| format!("{}: {e}", json.display()))?;
    println!(
        "\nThe whole analysis: {}\nCompare a later one with it: `--compare {}`.\n\
         How to make it smaller: \"What the binary is made of\" in docs/getting-started.md.",
        json.display(),
        json.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// The first `<label>-NN.json` not taken in `dir`, so the earlier ones stay to compare with.
fn numbered(dir: &Path, label: &str) -> PathBuf {
    (1..)
        .map(|n| dir.join(format!("{label}-{n:02}.json")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(format!("{label}.json")))
}

// --- The build with symbols ---

/// The release build of `target`, with its symbols, in `target/frus-size/`: the binary to read.
fn build_with_symbols(project: &Project, target: Target) -> Result<PathBuf, String> {
    let dir = project.target_dir()?.join("frus-size");
    println!(
        "Building {} in release with its symbols, in {} (the shipped build is left as it is; the \
         first analysis compiles everything once)…",
        target.platform(),
        dir.display()
    );
    // A bundle carries the APK's library, an installer the desktop's program.
    match target {
        Target::Desktop | Target::Installer => build_desktop(project, &dir),
        Target::Web => build_web(project, &dir),
        Target::Apk | Target::AppBundle => build_android(project, &dir),
    }
}

fn build_desktop(project: &Project, dir: &Path) -> Result<PathBuf, String> {
    let mut command = Command::new("cargo");
    command
        .current_dir(&project.root)
        .env("CARGO_TARGET_DIR", dir)
        .env("CARGO_PROFILE_RELEASE_STRIP", "none");
    let map = dir.join(format!("{}.map", project.package));
    if cfg!(windows) {
        // The program's symbols are in the linker's map; the program itself is what ships.
        command.args(["rustc", "--release", "--bin", &project.package, "--"]);
        command.arg(format!("-Clink-arg=/MAP:{}", map.display()));
    } else {
        command.args(["build", "--release", "--bin", &project.package]);
    }
    crate::cargo::run(&mut command)?;
    let exe = dir.join("release").join(format!(
        "{}{}",
        project.package,
        std::env::consts::EXE_SUFFIX
    ));
    if cfg!(windows) {
        let beside = exe.with_extension("map");
        std::fs::copy(&map, &beside).map_err(|e| format!("{}: {e}", map.display()))?;
    }
    Ok(exe)
}

fn build_web(project: &Project, dir: &Path) -> Result<PathBuf, String> {
    crate::setup::rust_target("wasm32-unknown-unknown")?;
    let profile = if project.has_web_release {
        "web-release"
    } else {
        "release"
    };
    crate::cargo::run(
        Command::new("cargo")
            .args([
                "build",
                "--lib",
                "--target",
                "wasm32-unknown-unknown",
                "--profile",
                profile,
            ])
            .current_dir(&project.root)
            .env("CARGO_TARGET_DIR", dir)
            .env("CARGO_PROFILE_RELEASE_STRIP", "none")
            .env("CARGO_PROFILE_WEB_RELEASE_STRIP", "none"),
    )?;
    let module = dir
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join(format!("{}.wasm", project.lib));
    // What ships is wasm-bindgen's output, as `frus build web` writes it (it runs no wasm-opt):
    // the bindings' descriptors gone, the exports trimmed. It keeps the `name` section, which
    // the analysis reads, and which does not count as shipped.
    let wasm_bindgen = crate::setup::cargo_tool(
        "wasm-bindgen",
        "--version",
        "wasm-bindgen-cli",
        project.locked_wasm_bindgen().as_deref(),
    )?;
    let out = dir.join("web-pkg");
    crate::cargo::run(
        Command::new(wasm_bindgen)
            .args(["--target", "web", "--no-typescript", "--out-dir"])
            .arg(&out)
            .arg(&module),
    )?;
    Ok(out.join(format!("{}_bg.wasm", project.lib)))
}

fn build_android(project: &Project, dir: &Path) -> Result<PathBuf, String> {
    // cargo-apk, as `frus build apk` runs it, aimed at this directory: the `.so` it links is
    // the one to read, the `.apk` around it is listed too.
    std::env::set_var("CARGO_TARGET_DIR", dir);
    std::env::set_var("CARGO_PROFILE_RELEASE_STRIP", "none");
    let target = project
        .android_targets
        .first()
        .cloned()
        .unwrap_or_else(|| "aarch64-linux-android".into());
    let library = dir
        .join(&target)
        .join("release")
        .join(format!("lib{}.so", project.lib));
    let started = std::time::SystemTime::now();
    match crate::cargo::build_apk(project, Mode::Release, Some(&target)) {
        Ok(apk) => println!("The package: {} ({} kB).", apk.display(), file_kb(&apk)),
        // cargo-apk links the library before it signs the package: with no key to sign a
        // release with, the library is there all the same, and it is what is measured.
        Err(error) => {
            let fresh = std::fs::metadata(&library)
                .and_then(|m| m.modified())
                .is_ok_and(|at| at >= started);
            if !fresh {
                return Err(error);
            }
            println!("The package was not made ({error}); the library was, and is measured.");
        }
    }
    Ok(library)
}

fn file_kb(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) / 1024
}

// --- Reading a binary ---

/// Reads `file` and attributes its bytes.
pub(crate) fn analyse(label: &str, file: &Path, root: Option<&Path>) -> Result<Report, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let parsed = if bytes.starts_with(b"\x7fELF") {
        parse_elf(&bytes)?
    } else if bytes.starts_with(b"\0asm") {
        parse_wasm(&bytes)?
    } else if bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) {
        parse_macho(&bytes)?
    } else if bytes.starts_with(b"MZ") {
        let map = file.with_extension("map");
        let text = std::fs::read_to_string(&map).map_err(|_| {
            format!(
                "{} is a Windows program, whose symbols are in the linker's map, and there is no \
                 {} beside it: `frus build desktop --analyze-size` writes both",
                file.display(),
                map.display()
            )
        })?;
        let mut parsed = parse_msvc_map(&text);
        parsed.shipped = bytes.len() as u64;
        parsed
    } else {
        return Err(format!(
            "{}: not a binary frus reads (an ELF .so or program, a .wasm, a Mach-O program, or a Windows .exe with \
             its .map)",
            file.display()
        ));
    };
    let assets = embedded_assets(&bytes, &asset_files(root));
    Ok(attribute(label, file, bytes.len() as u64, parsed, assets))
}

/// The report: the symbols grouped by crate, the files found inside taken out of what no symbol
/// covers.
fn attribute(
    label: &str,
    file: &Path,
    file_bytes: u64,
    parsed: Parsed,
    assets: Vec<(String, u64)>,
) -> Report {
    let mut by_crate: BTreeMap<String, u64> = BTreeMap::new();
    let mut named = 0;
    for symbol in &parsed.symbols {
        let owner = crate_of(&symbol.name);
        // Anonymous data is what remains once the files found inside are taken out of it: it
        // is counted with what no symbol covers, below.
        if owner == UNATTRIBUTED {
            continue;
        }
        *by_crate.entry(owner).or_default() += symbol.size;
        named += symbol.size;
    }
    let in_assets: u64 = assets.iter().map(|(_, n)| n).sum();
    let rest = parsed
        .covered
        .saturating_sub(named)
        .saturating_sub(in_assets);
    if rest > 0 {
        by_crate.insert(UNATTRIBUTED.into(), rest);
    }
    let mut crates: Vec<(String, u64)> = by_crate.into_iter().collect();
    crates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Report {
        label: label.to_string(),
        file: file.display().to_string(),
        format: parsed.format,
        file_bytes,
        shipped_bytes: parsed.shipped,
        crates,
        assets,
        sections: parsed.sections,
    }
}

const UNATTRIBUTED: &str = "[unattributed]";
const FOREIGN: &str = "[C/C++]";

fn u16_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u64)
}
fn u32_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// A NUL-terminated name at `at`.
fn c_str(b: &[u8], at: usize) -> String {
    let tail = b.get(at..).unwrap_or(&[]);
    let end = tail.iter().position(|&c| c == 0).unwrap_or(tail.len());
    String::from_utf8_lossy(&tail[..end]).into_owned()
}

/// A 64-bit little-endian ELF: its sections, and its symbol table's functions and objects.
pub(crate) fn parse_elf(b: &[u8]) -> Result<Parsed, String> {
    let bad = || "an ELF file frus cannot read: only 64-bit little-endian ones".to_string();
    if b.get(4) != Some(&2) || b.get(5) != Some(&1) {
        return Err(bad());
    }
    let shoff = u64_at(b, 0x28).ok_or_else(bad)? as usize;
    let shentsize = u16_at(b, 0x3a).ok_or_else(bad)? as usize;
    let shnum = u16_at(b, 0x3c).ok_or_else(bad)? as usize;
    let shstrndx = u16_at(b, 0x3e).ok_or_else(bad)? as usize;
    struct Section {
        name: u64,
        kind: u64,
        flags: u64,
        offset: u64,
        size: u64,
        link: u64,
        entsize: u64,
    }
    let section = |i: usize| -> Option<Section> {
        let at = shoff + i * shentsize;
        Some(Section {
            name: u32_at(b, at)?,
            kind: u32_at(b, at + 4)?,
            flags: u64_at(b, at + 8)?,
            offset: u64_at(b, at + 0x18)?,
            size: u64_at(b, at + 0x20)?,
            link: u32_at(b, at + 0x28)?,
            entsize: u64_at(b, at + 0x38)?,
        })
    };
    let sections: Vec<Section> = (0..shnum).filter_map(section).collect();
    let names = sections.get(shstrndx).ok_or_else(bad)?;
    let name_of = |s: &Section| c_str(b, (names.offset + s.name) as usize);
    const SHF_ALLOC: u64 = 2;
    const SHF_EXECINSTR: u64 = 4;
    const SHT_NOBITS: u64 = 8;
    const SHT_SYMTAB: u64 = 2;
    let mut parsed = Parsed {
        format: "elf",
        ..Parsed::default()
    };
    let mut not_loaded = 0;
    let mut covered_sections = Vec::new();
    for (index, s) in sections.iter().enumerate() {
        let loads = s.flags & SHF_ALLOC != 0;
        if !loads {
            if s.kind != SHT_NOBITS && index != 0 {
                not_loaded += s.size;
            }
            continue;
        }
        let name = name_of(s);
        if s.kind != SHT_NOBITS {
            parsed.sections.push((name.clone(), s.size));
        }
        // Code and read-only data: where the symbols are.
        let readonly = name.starts_with(".rodata") || name.starts_with(".data.rel.ro");
        if s.flags & SHF_EXECINSTR != 0 || readonly {
            parsed.covered += s.size;
            covered_sections.push(index as u64);
        }
    }
    parsed.shipped = (b.len() as u64).saturating_sub(not_loaded);
    let Some(symtab) = sections.iter().find(|s| s.kind == SHT_SYMTAB) else {
        return Ok(parsed);
    };
    let strtab = sections.get(symtab.link as usize).ok_or_else(bad)?;
    let entsize = if symtab.entsize == 0 {
        24
    } else {
        symtab.entsize
    };
    let mut seen = std::collections::HashSet::new();
    for i in 0..symtab.size / entsize {
        let at = (symtab.offset + i * entsize) as usize;
        let (Some(name), Some(info), Some(shndx), Some(value), Some(size)) = (
            u32_at(b, at),
            b.get(at + 4),
            u16_at(b, at + 6),
            u64_at(b, at + 8),
            u64_at(b, at + 16),
        ) else {
            break;
        };
        let kind = info & 0xf;
        // Functions and objects with a size, in the code or the read-only data; an alias of one
        // already counted (the same place) is not counted twice.
        if !(kind == 1 || kind == 2) || size == 0 || !covered_sections.contains(&shndx) {
            continue;
        }
        if !seen.insert(value) {
            continue;
        }
        parsed.symbols.push(Symbol {
            name: c_str(b, (strtab.offset + name) as usize),
            size,
        });
    }
    Ok(parsed)
}

/// An unsigned LEB128 at `*at`, moving past it.
fn leb(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *b.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// A WebAssembly module: each function body, named from the `name` section; the data segments;
/// the custom sections, which do not ship once stripped.
pub(crate) fn parse_wasm(b: &[u8]) -> Result<Parsed, String> {
    let bad = || "a WebAssembly module frus cannot read".to_string();
    let mut parsed = Parsed {
        format: "wasm",
        ..Parsed::default()
    };
    let mut at = 8;
    let mut imported_functions = 0u64;
    let mut bodies: Vec<u64> = Vec::new();
    let mut names: BTreeMap<u64, String> = BTreeMap::new();
    let mut custom = 0;
    while at < b.len() {
        let id = *b.get(at).ok_or_else(bad)?;
        at += 1;
        let size = leb(b, &mut at).ok_or_else(bad)? as usize;
        let start = at;
        let end = start
            .checked_add(size)
            .filter(|&e| e <= b.len())
            .ok_or_else(bad)?;
        let header = (end - start) as u64 + 1 + (at - start) as u64;
        match id {
            0 => {
                let mut p = start;
                let len = leb(b, &mut p).ok_or_else(bad)? as usize;
                let name = String::from_utf8_lossy(b.get(p..p + len).ok_or_else(bad)?).into_owned();
                p += len;
                if name == "name" {
                    read_function_names(&b[p..end], &mut names);
                }
                custom += header;
            }
            2 => {
                // Imports: each imported function takes an index before the module's own.
                let mut p = start;
                let count = leb(b, &mut p).ok_or_else(bad)?;
                for _ in 0..count {
                    for _ in 0..2 {
                        let len = leb(b, &mut p).ok_or_else(bad)? as usize;
                        p += len;
                    }
                    let kind = *b.get(p).ok_or_else(bad)?;
                    p += 1;
                    match kind {
                        0 => {
                            leb(b, &mut p).ok_or_else(bad)?;
                            imported_functions += 1;
                        }
                        1 => {
                            // A table: its element type, then its limits.
                            p += 1;
                            skip_limits(b, &mut p).ok_or_else(bad)?;
                        }
                        2 => skip_limits(b, &mut p).ok_or_else(bad)?,
                        3 => p += 2,
                        _ => return Err(bad()),
                    }
                }
                parsed.sections.push(("import".into(), size as u64));
            }
            10 => {
                let mut p = start;
                let count = leb(b, &mut p).ok_or_else(bad)?;
                for _ in 0..count {
                    let body_start = p;
                    let len = leb(b, &mut p).ok_or_else(bad)? as usize;
                    p += len;
                    bodies.push((p - body_start) as u64);
                }
                parsed.sections.push(("code".into(), size as u64));
                parsed.covered += size as u64;
            }
            11 => {
                parsed.sections.push(("data".into(), size as u64));
                parsed.covered += size as u64;
            }
            other => parsed
                .sections
                .push((wasm_section_name(other).into(), size as u64)),
        }
        at = end;
    }
    parsed.shipped = (b.len() as u64).saturating_sub(custom);
    for (i, size) in bodies.into_iter().enumerate() {
        let index = imported_functions + i as u64;
        let name = names
            .remove(&index)
            .unwrap_or_else(|| format!("function[{index}]"));
        parsed.symbols.push(Symbol { name, size });
    }
    Ok(parsed)
}

fn skip_limits(b: &[u8], p: &mut usize) -> Option<()> {
    let flags = *b.get(*p)?;
    *p += 1;
    leb(b, p)?;
    if flags & 1 != 0 {
        leb(b, p)?;
    }
    Some(())
}

fn wasm_section_name(id: u8) -> &'static str {
    match id {
        1 => "type",
        3 => "function",
        4 => "table",
        5 => "memory",
        6 => "global",
        7 => "export",
        8 => "start",
        9 => "element",
        12 => "data count",
        _ => "other",
    }
}

/// The function names of a `name` section's body (its subsection 1).
fn read_function_names(b: &[u8], names: &mut BTreeMap<u64, String>) {
    let mut at = 0;
    while at < b.len() {
        let id = b[at];
        at += 1;
        let Some(size) = leb(b, &mut at) else {
            return;
        };
        let end = at + size as usize;
        if id == 1 {
            let mut p = at;
            let Some(count) = leb(b, &mut p) else {
                return;
            };
            for _ in 0..count {
                let (Some(index), Some(len)) = (leb(b, &mut p), leb(b, &mut p)) else {
                    return;
                };
                let Some(name) = b.get(p..p + len as usize) else {
                    return;
                };
                names.insert(index, String::from_utf8_lossy(name).into_owned());
                p += len as usize;
            }
        }
        at = end;
    }
}

/// The MSVC linker's map: its sections, and every symbol sized up to the next one in its section.
pub(crate) fn parse_msvc_map(text: &str) -> Parsed {
    let mut parsed = Parsed {
        format: "pe",
        ..Parsed::default()
    };
    // ` 0001:00000000 0012a3b4H .text$mn                CODE`
    let mut lengths: BTreeMap<u32, (u64, String)> = BTreeMap::new();
    // `  0001:00000040       _ZN4core3fmt5write17h…E     0000000140001040 f   lib:obj`
    let mut at: BTreeMap<u32, Vec<(u64, String)>> = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(place) = fields.next() else {
            continue;
        };
        let Some((section, offset)) = place.split_once(':') else {
            continue;
        };
        let (Ok(section), Ok(offset)) = (
            u32::from_str_radix(section, 16),
            u64::from_str_radix(offset, 16),
        ) else {
            continue;
        };
        let Some(second) = fields.next() else {
            continue;
        };
        if let Some(length) = second.strip_suffix('H') {
            // A section's piece: its length, and its class.
            if let Ok(length) = u64::from_str_radix(length, 16) {
                let class = fields.nth(1).unwrap_or("").to_string();
                let entry = lengths.entry(section).or_insert((0, class));
                entry.0 = entry.0.max(offset + length);
            }
            continue;
        }
        if fields
            .next()
            .is_some_and(|rva| u64::from_str_radix(rva, 16).is_ok())
        {
            at.entry(section)
                .or_default()
                .push((offset, second.to_string()));
        }
    }
    for (section, (length, class)) in &lengths {
        let name = format!("{section:04}:{}", class.to_ascii_lowercase());
        parsed.sections.push((name, *length));
        if class == "CODE" || class == "DATA" {
            parsed.covered += *length;
        }
        let Some(symbols) = at.get_mut(section) else {
            continue;
        };
        symbols.sort();
        symbols.dedup_by_key(|(offset, _)| *offset);
        for (i, (offset, name)) in symbols.iter().enumerate() {
            let next = symbols.get(i + 1).map_or(*length, |(next, _)| *next);
            let size = next.saturating_sub(*offset);
            if size > 0 && class != "BSS" {
                parsed.symbols.push(Symbol {
                    name: name.clone(),
                    size,
                });
            }
        }
    }
    parsed
}

// --- Which crate ---

/// The crate a symbol's code or data comes from.
pub(crate) fn crate_of(symbol: &str) -> String {
    // The compiler's anonymous constants (`anon.<hash>.<n>`): literals, tables, the bytes of an
    // `include_bytes!` — Rust's data, owned by no name.
    if is_anonymous(symbol) {
        return UNATTRIBUTED.to_string();
    }
    let path = demangle(symbol);
    match path {
        Some(path) => crate_of_path(&path).unwrap_or_else(|| FOREIGN.to_string()),
        None => FOREIGN.to_string(),
    }
}

/// The path of a Rust symbol, `<core::fmt::Write as …>::write_str` or `alloc::vec::…`; None for
/// a symbol with no Rust name.
fn demangle(symbol: &str) -> Option<String> {
    // A Rust name inside another: Mach-O's underscore in front, a jump table of a function
    // (`switch.table._ZN…`), the unwinding data of one on Windows (`$ip2state$_ZN…`).
    if let Some(at) = symbol.find("_ZN") {
        return Some(legacy(&symbol[at + 3..]));
    }
    let v0 = symbol.match_indices("_R").find(|(at, _)| {
        let before = symbol[..*at].chars().next_back();
        let after = symbol[at + 2..].chars().next();
        before.is_none_or(|c| !c.is_ascii_alphanumeric())
            && after.is_some_and(|c| c.is_ascii_uppercase())
    });
    if let Some((at, _)) = v0 {
        return v0_crate(&symbol[at + 2..]).map(|name| format!("{name}::"));
    }
    // A name already demangled (the `name` section of a module): `core::fmt::write::h12…`.
    (symbol.contains("::")).then(|| symbol.to_string())
}

/// Whether a symbol names one of the compiler's anonymous constants.
fn is_anonymous(symbol: &str) -> bool {
    // `.Lanon.…` (ELF), `l_anon.…` and `l___unnamed_1` (Mach-O), `anon.…` (Windows).
    let symbol = symbol.trim_start_matches(['.', 'L', 'l', '_']);
    symbol.starts_with("anon.") || symbol.starts_with("unnamed_") || symbol.starts_with("alloc_")
}

/// A 64-bit Mach-O file (a macOS program, or an object): its sections, and each symbol defined
/// in one, sized up to the next symbol of its section — Mach-O's symbol table has no sizes.
pub(crate) fn parse_macho(b: &[u8]) -> Result<Parsed, String> {
    let bad = || "a Mach-O file frus cannot read".to_string();
    let ncmds = u32_at(b, 16).ok_or_else(bad)?;
    struct Section {
        name: String,
        addr: u64,
        size: u64,
        covered: bool,
    }
    let mut sections: Vec<Section> = Vec::new();
    let mut symtab = None;
    let mut not_shipped = 0;
    let mut at = 32;
    for _ in 0..ncmds {
        let cmd = u32_at(b, at).ok_or_else(bad)?;
        let size = u32_at(b, at + 4).ok_or_else(bad)? as usize;
        match cmd {
            // LC_SEGMENT_64: its sections follow it.
            0x19 => {
                let nsects = u32_at(b, at + 64).ok_or_else(bad)?;
                for i in 0..nsects as usize {
                    let s = at + 72 + i * 80;
                    let name = c_str(&b[..s + 16], s);
                    let segment = c_str(&b[..s + 32], s + 16);
                    let addr = u64_at(b, s + 32).ok_or_else(bad)?;
                    let size = u64_at(b, s + 40).ok_or_else(bad)?;
                    let flags = u32_at(b, s + 64).ok_or_else(bad)?;
                    // Code, and the read-only data beside it: where the symbols are.
                    let code = flags & 0x8000_0000 != 0 || name == "__text";
                    let readonly = segment == "__TEXT" || segment == "__DATA_CONST";
                    let zerofill = flags & 0xff == 1;
                    sections.push(Section {
                        name: format!("{segment},{name}"),
                        addr,
                        size,
                        covered: (code || readonly) && !zerofill,
                    });
                }
            }
            // LC_SYMTAB.
            0x2 => {
                let symoff = u32_at(b, at + 8).ok_or_else(bad)? as usize;
                let nsyms = u32_at(b, at + 12).ok_or_else(bad)? as usize;
                let stroff = u32_at(b, at + 16).ok_or_else(bad)? as usize;
                let strsize = u32_at(b, at + 20).ok_or_else(bad)?;
                not_shipped += nsyms as u64 * 16 + strsize;
                symtab = Some((symoff, nsyms, stroff));
            }
            _ => {}
        }
        at += size;
    }
    let mut parsed = Parsed {
        format: "macho",
        shipped: (b.len() as u64).saturating_sub(not_shipped),
        ..Parsed::default()
    };
    for section in &sections {
        parsed.sections.push((section.name.clone(), section.size));
        if section.covered {
            parsed.covered += section.size;
        }
    }
    let Some((symoff, nsyms, stroff)) = symtab else {
        return Ok(parsed);
    };
    // (section index, address, name) of every symbol defined in a section, stabs left out.
    let mut defined: Vec<(usize, u64, String)> = Vec::new();
    for i in 0..nsyms {
        let s = symoff + i * 16;
        let (Some(strx), Some(kind), Some(sect), Some(value)) =
            (u32_at(b, s), b.get(s + 4), b.get(s + 5), u64_at(b, s + 8))
        else {
            break;
        };
        const N_STAB: u8 = 0xe0;
        const N_TYPE: u8 = 0x0e;
        const N_SECT: u8 = 0x0e;
        if kind & N_STAB != 0 || kind & N_TYPE != N_SECT || *sect == 0 {
            continue;
        }
        let name = c_str(b, stroff + strx as usize);
        // The assembler's own labels: a section's start, not a piece of it.
        if name.starts_with("ltmp") {
            continue;
        }
        defined.push((*sect as usize - 1, value, name));
    }
    defined.sort_by_key(|(sect, addr, _)| (*sect, *addr));
    defined.dedup_by_key(|(sect, addr, _)| (*sect, *addr));
    for (i, (sect, addr, name)) in defined.iter().enumerate() {
        let Some(section) = sections.get(*sect).filter(|s| s.covered) else {
            continue;
        };
        let end = defined
            .get(i + 1)
            .filter(|(next, _, _)| next == sect)
            .map_or(section.addr + section.size, |(_, next, _)| *next);
        let size = end.saturating_sub(*addr);
        if size > 0 {
            parsed.symbols.push(Symbol {
                name: name.clone(),
                size,
            });
        }
    }
    Ok(parsed)
}

/// `4core3fmt5write17h0123E` → `core::fmt::write::h0123`, the escapes decoded.
fn legacy(mut rest: &str) -> String {
    let mut parts = Vec::new();
    while let Some(digits) = rest.find(|c: char| !c.is_ascii_digit()).filter(|&n| n > 0) {
        let Ok(len) = rest[..digits].parse::<usize>() else {
            break;
        };
        let Some(part) = rest.get(digits..digits + len) else {
            break;
        };
        parts.push(decode_legacy(part));
        rest = &rest[digits + len..];
    }
    parts.join("::")
}

fn decode_legacy(part: &str) -> String {
    let part = part
        .strip_prefix("_$")
        .map_or(part.to_string(), |r| format!("${r}"));
    part.replace("$LT$", "<")
        .replace("$GT$", ">")
        .replace("$u20$", " ")
        .replace("$RF$", "&")
        .replace("$BP$", "*")
        .replace("$C$", ",")
        .replace("$u5b$", "[")
        .replace("$u5d$", "]")
        .replace("$u7b$", "{")
        .replace("$u7d$", "}")
        .replace("$u27$", "'")
        .replace("..", "::")
}

/// The first crate name of a v0 symbol: `C` then an optional disambiguator, then the length.
fn v0_crate(rest: &str) -> Option<String> {
    let at = rest.find('C')?;
    let mut tail = &rest[at + 1..];
    if let Some(after) = tail.strip_prefix('s') {
        tail = &after[after.find('_')? + 1..];
    }
    let digits = tail.find(|c: char| !c.is_ascii_digit())?;
    let len: usize = tail[..digits].parse().ok()?;
    let name = tail.get(digits..digits + len)?;
    Some(name.trim_start_matches('_').to_string())
}

/// The crate of a demangled path: its first segment, or, for `<A as B>::f`, A's crate (B's when
/// A is a type of no crate: a reference to a primitive, a tuple).
fn crate_of_path(path: &str) -> Option<String> {
    let path = path.trim();
    if let Some(inner) = path.strip_prefix('<') {
        let inner = inner.trim_start_matches(['&', '*', '(', '[', ' ']);
        let inner = inner
            .trim_start_matches("mut ")
            .trim_start_matches("const ")
            .trim_start_matches("dyn ");
        let first = inner
            .split([' ', '>', ',', ';', ')', ']'])
            .next()
            .unwrap_or("");
        if let Some((name, _)) = first.split_once("::") {
            return Some(name.to_string());
        }
        // A primitive type: the trait's crate.
        if let Some(index) = inner.find(" as ") {
            return crate_of_path(&inner[index + 4..]);
        }
        return None;
    }
    let name = path.split("::").next()?;
    let name = name.split('<').next()?.trim();
    let shaped = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    shaped.then(|| name.to_string())
}

// --- What is carried inside ---

/// The files that may be inside the binary whole: the fonts frus bundles, from the frus-text
/// crate the application builds with, and the application's own `assets/`.
fn asset_files(root: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let Some(root) = root else {
        return found;
    };
    if let Some(fonts) = frus_text_assets(root) {
        list_files(&fonts, "frus-text/assets", &mut found);
    }
    list_files(&root.join("assets"), "assets", &mut found);
    found
}

/// The `assets/` of the frus-text crate the application resolves, as cargo knows it.
fn frus_text_assets(root: &Path) -> Option<PathBuf> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let metadata = parse(&text).ok()?;
    let manifest = metadata
        .get("packages")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(Value::as_str) == Some("frus-text"))?
        .get("manifest_path")?
        .as_str()?
        .to_string();
    Some(Path::new(&manifest).parent()?.join("assets"))
}

fn list_files(dir: &Path, shown: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = format!("{shown}/{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            list_files(&path, &name, out);
        } else {
            out.push((name, path));
        }
    }
}

/// The files of `candidates` found whole inside `binary`, with their sizes. Small files are left
/// out: a few bytes match anywhere.
pub(crate) fn embedded_assets(
    binary: &[u8],
    candidates: &[(String, PathBuf)],
) -> Vec<(String, u64)> {
    const SMALLEST: usize = 4096;
    let mut found = Vec::new();
    for (name, path) in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.len() >= SMALLEST && contains(binary, &bytes) {
            found.push((name.clone(), bytes.len() as u64));
        }
    }
    found
}

/// Whether `needle` is inside `haystack`: its first 64 bytes looked for, then the whole.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    let head = &needle[..needle.len().min(64)];
    let mut from = 0;
    while let Some(i) = haystack[from..]
        .windows(head.len())
        .position(|window| window == head)
    {
        let at = from + i;
        if haystack.get(at..at + needle.len()) == Some(needle) {
            return true;
        }
        from = at + 1;
    }
    false
}

// --- Saying it ---

fn kb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    }
}

/// The report for a person: the totals, the crates that weigh most, the files inside.
pub(crate) fn table(report: &Report) -> String {
    let mut out = format!(
        "\n{} ({}): {} as shipped, without its symbols ({} on the disk now)\n",
        report.file,
        report.format,
        kb(report.shipped_bytes),
        kb(report.file_bytes)
    );
    let total: u64 = report.crates.iter().map(|(_, n)| n).sum::<u64>()
        + report.assets.iter().map(|(_, n)| n).sum::<u64>();
    let share = |n: u64| {
        if total == 0 {
            0.0
        } else {
            n as f64 * 100.0 / total as f64
        }
    };
    const SHOWN: usize = 20;
    let others = (report.crates.len() > SHOWN)
        .then(|| format!("{} other crates", report.crates.len() - SHOWN));
    // One width for both lists: the longest name shown.
    let width = report
        .crates
        .iter()
        .take(SHOWN)
        .chain(&report.assets)
        .map(|(name, _)| name.chars().count())
        .chain(others.as_ref().map(|o| o.len()))
        .max()
        .unwrap_or(0)
        .max(16);
    let row = |name: &str, bytes: u64| {
        format!(
            "  {name:<width$} {:>10}  {:>5.1}%\n",
            kb(bytes),
            share(bytes)
        )
    };
    out.push_str(&format!(
        "\n  {:<width$} {:>10}  {:>6}\n",
        "crate", "size", "share"
    ));
    for (name, bytes) in report.crates.iter().take(SHOWN) {
        out.push_str(&row(name, *bytes));
    }
    if let Some(others) = &others {
        let rest: u64 = report.crates[SHOWN..].iter().map(|(_, n)| n).sum();
        out.push_str(&row(others, rest));
    }
    if !report.assets.is_empty() {
        out.push_str(&format!(
            "\n  {:<width$} {:>10}  {:>6}\n",
            "carried inside", "size", "share"
        ));
        for (name, bytes) in &report.assets {
            out.push_str(&row(name, *bytes));
        }
    }
    out
}

/// What changed since `before`: each crate that grew or shrank by a kilobyte or more.
pub(crate) fn comparison(before: &Report, after: &Report) -> String {
    let delta = |a: u64, b: u64| b as i64 - a as i64;
    let signed = |d: i64| {
        let text = kb(d.unsigned_abs());
        if d >= 0 {
            format!("+{text}")
        } else {
            format!("-{text}")
        }
    };
    let mut out = format!(
        "\nSince {}: {} as shipped ({} then, {} now)\n",
        before.file,
        signed(delta(before.shipped_bytes, after.shipped_bytes)),
        kb(before.shipped_bytes),
        kb(after.shipped_bytes)
    );
    let mut names: Vec<&String> = before
        .crates
        .iter()
        .chain(&after.crates)
        .chain(&before.assets)
        .chain(&after.assets)
        .map(|(n, _)| n)
        .collect();
    names.sort();
    names.dedup();
    let find = |list: &[(String, u64)], name: &str| {
        list.iter().find(|(n, _)| n == name).map_or(0, |(_, b)| *b)
    };
    let mut changes: Vec<(i64, &String)> = names
        .into_iter()
        .map(|name| {
            let a = find(&before.crates, name) + find(&before.assets, name);
            let b = find(&after.crates, name) + find(&after.assets, name);
            (delta(a, b), name)
        })
        .filter(|(d, _)| d.unsigned_abs() >= 1024)
        .collect();
    changes.sort_by(|a, b| {
        b.0.unsigned_abs()
            .cmp(&a.0.unsigned_abs())
            .then(a.1.cmp(b.1))
    });
    if changes.is_empty() {
        out.push_str("  no crate changed by a kilobyte or more\n");
    }
    for (d, name) in changes.iter().take(25) {
        out.push_str(&format!("  {name:<32} {:>11}\n", signed(*d)));
    }
    out
}

fn pairs(list: &[(String, u64)]) -> Value {
    Value::Object(
        list.iter()
            .map(|(name, bytes)| (name.clone(), Value::from(*bytes)))
            .collect(),
    )
}

pub(crate) fn to_json(report: &Report) -> Value {
    object([
        ("label", report.label.as_str().into()),
        ("file", report.file.as_str().into()),
        ("format", report.format.into()),
        ("fileBytes", report.file_bytes.into()),
        ("shippedBytes", report.shipped_bytes.into()),
        ("crates", pairs(&report.crates)),
        ("assets", pairs(&report.assets)),
        ("sections", pairs(&report.sections)),
    ])
}

pub(crate) fn from_json(text: &str) -> Result<Report, String> {
    let value = parse(text.trim())?;
    let text_of = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let number = |key: &str| value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as u64;
    let list = |key: &str| match value.get(key) {
        Some(Value::Object(members)) => members
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0) as u64))
            .collect(),
        _ => Vec::new(),
    };
    if value.get("crates").is_none() {
        return Err("not a size report: it has no `crates`".into());
    }
    let format = match text_of("format").as_str() {
        "elf" => "elf",
        "wasm" => "wasm",
        "pe" => "pe",
        "macho" => "macho",
        _ => "?",
    };
    Ok(Report {
        label: text_of("label"),
        file: text_of("file"),
        format,
        file_bytes: number("fileBytes"),
        shipped_bytes: number("shippedBytes"),
        crates: list("crates"),
        assets: list("assets"),
        sections: list("sections"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_s_crate_is_the_first_part_of_its_path() {
        assert_eq!(crate_of("_ZN4core3fmt5write17h0123456789abcdefE"), "core");
        assert_eq!(
            crate_of("_ZN12frus_widgets6button6Button5build17h01E"),
            "frus_widgets"
        );
        // An impl: its type's crate.
        assert_eq!(
            crate_of("_ZN64_$LT$frus_core..color..Color$u20$as$u20$core..fmt..Debug$GT$3fmt17h0E"),
            "frus_core"
        );
        assert_eq!(
            crate_of("_ZN67_$LT$alloc..vec..Vec$LT$T$C$A$GT$$u20$as$u20$core..ops..drop..Drop$GT$4drop17h0E"),
            "alloc"
        );
        // A primitive's impl: the trait's crate.
        assert_eq!(
            crate_of("_ZN52_$LT$$RF$T$u20$as$u20$core..fmt..Display$GT$3fmt17h0E"),
            "core"
        );
        // Windows' and Mach-O's underscore in front.
        assert_eq!(crate_of("__ZN3std2io5stdio6_print17h0E"), "std");
        // Demangled already (a module's `name` section).
        assert_eq!(
            crate_of("cosmic_text::buffer::Buffer::shape_until_scroll::h1234"),
            "cosmic_text"
        );
        assert_eq!(
            crate_of("<wgpu_core::device::Device as core::ops::Drop>::drop"),
            "wgpu_core"
        );
        // v0.
        assert_eq!(crate_of("_RNvCs1234_7mycrate3foo"), "mycrate");
        // Inside another name: a jump table, unwinding data.
        assert_eq!(
            crate_of("switch.table._ZN4naga5front3foo17h0E.9116.rel"),
            "naga"
        );
        assert_eq!(
            crate_of("$ip2state$_ZN8frus_gpu8renderer8Renderer4make17h0E"),
            "frus_gpu"
        );
        assert_eq!(crate_of("$ip2state$_RNvCs1234_7mycrate3foo"), "mycrate");
        // The compiler's anonymous constants: nobody's.
        assert_eq!(
            crate_of("anon.864c7b0412b58b40282e4edf17b122e6.294"),
            UNATTRIBUTED
        );
        assert_eq!(crate_of(".Lanon.fad58de7366495db.3"), UNATTRIBUTED);
        // No Rust name.
        assert_eq!(crate_of("memcpy"), FOREIGN);
        assert_eq!(crate_of("__real@3ff0000000000000"), FOREIGN);
        assert_eq!(crate_of("?CreateWindow@@YAXXZ"), FOREIGN);
    }

    /// A tiny ELF, written here: two functions with their sizes, an alias of one, the symbol
    /// table that does not ship.
    fn elf() -> (Vec<u8>, u64) {
        let mut b = vec![0u8; 0x40];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        // .text: 0x40..0x140 (256 bytes of code).
        b.extend(vec![0x90u8; 256]);
        // .strtab
        let strtab_off = b.len();
        let names = b"\0_ZN4core3fmt5write17h0E\0_ZN6my_app3run17h0E\0alias\0";
        b.extend_from_slice(names);
        // .symtab: null + 3 symbols of 24 bytes.
        let symtab_off = b.len();
        let sym = |name: u32, info: u8, shndx: u16, value: u64, size: u64| {
            let mut s = vec![0u8; 24];
            s[..4].copy_from_slice(&name.to_le_bytes());
            s[4] = info;
            s[6..8].copy_from_slice(&shndx.to_le_bytes());
            s[8..16].copy_from_slice(&value.to_le_bytes());
            s[16..24].copy_from_slice(&size.to_le_bytes());
            s
        };
        b.extend(sym(0, 0, 0, 0, 0));
        let at = |name: &[u8]| names.windows(name.len()).position(|w| w == name).unwrap() as u32;
        b.extend(sym(at(b"_ZN4core"), 0x12, 1, 0x40, 200));
        b.extend(sym(at(b"_ZN6my_app"), 0x12, 1, 0x108, 56));
        b.extend(sym(at(b"alias"), 0x12, 1, 0x40, 200));
        // .shstrtab
        let shstr_off = b.len();
        let shnames = b"\0.text\0.symtab\0.strtab\0.shstrtab\0";
        b.extend_from_slice(shnames);
        let shoff = b.len();
        let section =
            |name: u32, kind: u32, flags: u64, off: usize, size: usize, link: u32, ent: u64| {
                let mut s = vec![0u8; 64];
                s[..4].copy_from_slice(&name.to_le_bytes());
                s[4..8].copy_from_slice(&kind.to_le_bytes());
                s[8..16].copy_from_slice(&flags.to_le_bytes());
                s[0x18..0x20].copy_from_slice(&(off as u64).to_le_bytes());
                s[0x20..0x28].copy_from_slice(&(size as u64).to_le_bytes());
                s[0x28..0x2c].copy_from_slice(&link.to_le_bytes());
                s[0x38..0x40].copy_from_slice(&ent.to_le_bytes());
                s
            };
        b.extend(section(0, 0, 0, 0, 0, 0, 0));
        b.extend(section(1, 1, 6, 0x40, 256, 0, 0));
        b.extend(section(7, 2, 0, symtab_off, 96, 3, 24));
        b.extend(section(15, 3, 0, strtab_off, names.len(), 0, 0));
        b.extend(section(23, 3, 0, shstr_off, shnames.len(), 0, 0));
        b[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        b[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        b[0x3c..0x3e].copy_from_slice(&5u16.to_le_bytes());
        b[0x3e..0x40].copy_from_slice(&4u16.to_le_bytes());
        let not_loaded = 96 + names.len() as u64 + shnames.len() as u64;
        (b, not_loaded)
    }

    #[test]
    fn an_elf_symbol_table_is_read() {
        let (bytes, not_loaded) = elf();
        let parsed = parse_elf(&bytes).unwrap();
        assert_eq!(parsed.format, "elf");
        assert_eq!(parsed.sections, [(".text".to_string(), 256)]);
        assert_eq!(parsed.covered, 256);
        assert_eq!(
            parsed.symbols,
            [
                Symbol {
                    name: "_ZN4core3fmt5write17h0E".into(),
                    size: 200
                },
                Symbol {
                    name: "_ZN6my_app3run17h0E".into(),
                    size: 56
                },
            ],
            "the alias at the same place is counted once"
        );
        // What ships: the file without the symbol table, its names and the section names.
        assert_eq!(parsed.shipped, bytes.len() as u64 - not_loaded);
        let report = attribute(
            "android",
            Path::new("lib.so"),
            bytes.len() as u64,
            parsed,
            vec![],
        );
        assert_eq!(
            report.crates,
            [("core".to_string(), 200), ("my_app".to_string(), 56)]
        );
    }

    /// A tiny module: one imported function, two bodies, their names, a data segment.
    fn wasm() -> Vec<u8> {
        let mut b = b"\0asm\x01\0\0\0".to_vec();
        let mut section = |id: u8, body: Vec<u8>| {
            b.push(id);
            b.push(body.len() as u8);
            b.extend(body);
        };
        // import: 1 entry, "m" "f", function of type 0.
        section(2, vec![1, 1, b'm', 1, b'f', 0, 0]);
        // code: 2 bodies, of 4 and 10 bytes (each with its length byte).
        let mut code = vec![2, 3, 0, 0, 0x0b, 9];
        code.extend(vec![0u8; 8]);
        code.push(0x0b);
        section(10, code);
        // data: 20 bytes.
        section(11, vec![0u8; 20]);
        // name: function names 1 and 2.
        let mut names = vec![4, b'n', b'a', b'm', b'e'];
        let entries = [(1u8, "core::fmt::write::h1"), (2u8, "my_app::run::h2")];
        let mut sub = vec![entries.len() as u8];
        for (index, name) in entries {
            sub.push(index);
            sub.push(name.len() as u8);
            sub.extend(name.bytes());
        }
        names.push(1);
        names.push(sub.len() as u8);
        names.extend(sub);
        section(0, names);
        b
    }

    #[test]
    fn a_module_s_functions_are_named_from_its_name_section() {
        let bytes = wasm();
        let parsed = parse_wasm(&bytes).unwrap();
        assert_eq!(
            parsed.symbols,
            [
                Symbol {
                    name: "core::fmt::write::h1".into(),
                    size: 4
                },
                Symbol {
                    name: "my_app::run::h2".into(),
                    size: 10
                },
            ]
        );
        assert!(
            parsed.shipped < bytes.len() as u64,
            "the name section does not ship"
        );
        let report = attribute(
            "web",
            Path::new("a.wasm"),
            bytes.len() as u64,
            parsed,
            vec![],
        );
        assert_eq!(report.crates[0], (UNATTRIBUTED.to_string(), 15 + 20 - 14));
    }

    /// A real Mach-O object, compiled from `fixtures/size-fixture.rs.txt` with
    /// `rustc --crate-type=lib --emit=obj --target x86_64-apple-darwin -C opt-level=2 -C panic=abort`:
    /// no Mac is needed to make one, and none to read it.
    #[test]
    fn a_mach_o_symbol_table_is_read() {
        let bytes = include_bytes!("fixtures/size-x86_64-apple-darwin.o");
        let parsed = parse_macho(bytes).unwrap();
        assert_eq!(parsed.format, "macho");
        assert!(
            parsed
                .sections
                .iter()
                .any(|(name, size)| name == "__TEXT,__text" && *size > 0),
            "{:?}",
            parsed.sections
        );
        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        let add = parsed
            .symbols
            .iter()
            .find(|s| s.name.contains("7fixture3add"))
            .unwrap_or_else(|| panic!("{names:?}"));
        assert!(add.size > 4 && add.size < 200, "{add:?}");
        assert!(
            names.iter().any(|n| n.contains("7fixture3sum")),
            "{names:?}"
        );
        // The table of 64 words, in the read-only data.
        let table = parsed
            .symbols
            .iter()
            .find(|s| s.name.contains("7fixture5TABLE"))
            .unwrap_or_else(|| panic!("{names:?}"));
        assert_eq!(table.size, 256);
        let report = attribute(
            "macos",
            Path::new("fixture.o"),
            bytes.len() as u64,
            parsed,
            vec![],
        );
        assert_eq!(report.crates[0].0, "fixture", "{:?}", report.crates);
        assert!(report.shipped_bytes < bytes.len() as u64);
        assert_eq!(crate_of("l___unnamed_1"), UNATTRIBUTED);
        assert_eq!(crate_of("l_anon.0123.4"), UNATTRIBUTED);
    }

    #[test]
    fn a_windows_map_is_read() {
        let map = " app\n\n Preferred load address is 0000000140000000\n\n\
             Start         Length     Name                   Class\n\
             0001:00000000 00000100H .text$mn                CODE\n\
             0002:00000000 00000040H .rdata                  DATA\n\
             \n  Address         Publics by Value              Rva+Base               Lib:Object\n\n\
             0001:00000000       _ZN4core3fmt5write17h0E    0000000140001000 f   app.o\n\
             0001:00000080       _ZN6my_app3run17h0E        0000000140001080 f   app.o\n\
             0001:000000c0       memcpy                     00000001400010c0 f   vcruntime:memcpy.obj\n\
             0002:00000000       _ZN6my_app5TABLE17h0E      0000000140002000     app.o\n\
             \n entry point at        0001:00000000\n";
        let parsed = parse_msvc_map(map);
        assert_eq!(parsed.covered, 0x140);
        let report = attribute("windows", Path::new("app.exe"), 1000, parsed, vec![]);
        assert_eq!(
            report.crates,
            [
                ("core".to_string(), 0x80),
                ("my_app".to_string(), 0x80),
                (FOREIGN.to_string(), 0x40),
            ]
        );

        // A font inside an anonymous constant is counted once, as the file it is.
        let map = map.replace("_ZN6my_app5TABLE17h0E", "anon.0123456789abcdef.7");
        let parsed = parse_msvc_map(&map);
        let font = vec![("font.ttf".to_string(), 0x30)];
        let report = attribute("windows", Path::new("app.exe"), 1000, parsed, font);
        assert_eq!(
            report.crates,
            [
                ("core".to_string(), 0x80),
                (FOREIGN.to_string(), 0x40),
                ("my_app".to_string(), 0x40),
                (UNATTRIBUTED.to_string(), 0x10),
            ]
        );
    }

    #[test]
    fn a_file_inside_is_found_by_its_bytes() {
        let dir = std::env::temp_dir().join(format!("frus-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let font: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(dir.join("font.ttf"), &font).unwrap();
        std::fs::write(dir.join("other.ttf"), vec![1u8; 5000]).unwrap();
        std::fs::write(dir.join("tiny.txt"), b"abc").unwrap();
        let mut binary = vec![9u8; 100];
        binary.extend(&font);
        binary.extend(b"abc");
        let candidates = vec![
            ("font.ttf".to_string(), dir.join("font.ttf")),
            ("other.ttf".to_string(), dir.join("other.ttf")),
            ("tiny.txt".to_string(), dir.join("tiny.txt")),
        ];
        assert_eq!(
            embedded_assets(&binary, &candidates),
            [("font.ttf".to_string(), 5000)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_report_goes_to_json_and_back_and_is_compared() {
        let before = Report {
            label: "windows".into(),
            file: "a.exe".into(),
            format: "pe",
            file_bytes: 10_000_000,
            shipped_bytes: 9_000_000,
            crates: vec![("frus_widgets".into(), 3_000_000), ("core".into(), 500_000)],
            assets: vec![("frus-text/assets/DejaVuSans.ttf".into(), 759_720)],
            sections: vec![("0001:code".into(), 6_000_000)],
        };
        let back = from_json(&to_json(&before).to_json()).unwrap();
        assert_eq!(back, before);
        let after = Report {
            shipped_bytes: 8_000_000,
            crates: vec![("frus_widgets".into(), 3_100_000), ("core".into(), 500_100)],
            assets: vec![],
            ..before.clone()
        };
        let said = comparison(&before, &after);
        assert!(said.contains("-976.6 kB as shipped"), "{said}");
        assert!(said.contains("frus-text/assets/DejaVuSans.ttf"), "{said}");
        assert!(said.contains("frus_widgets"), "{said}");
        assert!(
            !said.contains("core "),
            "a change under a kilobyte is not listed: {said}"
        );
        let table = table(&before);
        assert!(table.contains("frus_widgets"), "{table}");
        assert!(table.contains("carried inside"), "{table}");
        assert!(from_json("{}").is_err());
    }

    #[test]
    fn reports_are_numbered_so_the_earlier_ones_stay() {
        let dir = std::env::temp_dir().join(format!("frus-size-n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(numbered(&dir, "web"), dir.join("web-01.json"));
        std::fs::write(dir.join("web-01.json"), "{}").unwrap();
        assert_eq!(numbered(&dir, "web"), dir.join("web-02.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
