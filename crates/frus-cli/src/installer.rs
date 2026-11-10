// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus build installer` (milestone 790): the desktop program in the form its system installs.
//!
//! - **Windows**: a setup program made with NSIS, which installs for the current user — into
//!   `%LOCALAPPDATA%\Programs`, with a Start menu entry and an uninstaller in the system's list —
//!   so it needs no administrator, to be made or to be installed. NSIS is fetched the first
//!   time (about 2.5 MB, a zip that runs where it is unpacked). MSIX was set aside: a package
//!   installs only once its certificate is trusted, which takes an administrator or a paid one.
//! - **Linux**: a `.deb`, written here (an `ar` archive of two tarballs, made with the system's
//!   `tar`): the program in `/usr/bin`, a desktop entry and the icon.
//! - **macOS**: the `.app` bundle in a disk image, made with `hdiutil`.
//!
//! Each carries the name people see (the flavor's) and the version (`--build-name`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cargo::{self, Mode};
use crate::project::Project;

/// The NSIS release fetched on Windows.
const NSIS_VERSION: &str = "3.11";

/// Makes the installer for this system, from the program `exe`; returns its path.
pub(crate) fn make(project: &Project, mode: Mode, exe: &Path) -> Result<PathBuf, String> {
    let dir = exe
        .parent()
        .ok_or("the program has no directory")?
        .join("installer");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if cfg!(windows) {
        windows(project, exe, &dir)
    } else if cfg!(target_os = "macos") {
        macos(project, exe, &dir)
    } else if cfg!(target_os = "linux") {
        linux(project, mode, exe, &dir)
    } else {
        Err(format!(
            "frus build installer makes Windows, macOS and Linux installers, not {}'s",
            std::env::consts::OS
        ))
    }
}

/// A name for files: the display name's letters and digits, `-` between words.
fn file_stem(project: &Project) -> String {
    let mut stem = String::new();
    for c in project.display_name().chars() {
        if c.is_ascii_alphanumeric() {
            stem.push(c);
        } else if !stem.ends_with('-') && !stem.is_empty() {
            stem.push('-');
        }
    }
    let stem = stem.trim_end_matches('-').to_string();
    if stem.is_empty() {
        project.package.clone()
    } else {
        stem
    }
}

// --- Windows ---

fn windows(project: &Project, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let makensis = nsis()?;
    // DirectX's fast shader compiler, beside the program as `frus build desktop` puts it.
    if let Some(bin) = exe.parent() {
        crate::setup::place_dxc(bin);
    }
    let extra: Vec<PathBuf> = crate::setup::DXC_FILES
        .iter()
        .filter_map(|f| exe.parent().map(|d| d.join(f)))
        .filter(|p| p.is_file())
        .collect();
    let out = dir.join(format!(
        "{}-{}-setup.exe",
        file_stem(project),
        project.version_name()
    ));
    let script = dir.join("installer.nsi");
    std::fs::write(&script, nsis_script(project, exe, &extra, &out)).map_err(|e| e.to_string())?;
    cargo::run(
        Command::new(makensis)
            .args(["/V2", "/INPUTCHARSET", "UTF8"])
            .arg(&script),
    )?;
    Ok(out)
}

/// NSIS's version field holds four numbers: `1.2.3` → `1.2.3.0`, with the build number last
/// when there is one.
fn four_part_version(name: &str, number: Option<u32>) -> String {
    let mut parts: Vec<u32> = name
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    parts.resize(3, 0);
    parts.push(number.unwrap_or(0).min(65535));
    parts
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

/// A value inside an NSIS string: `$` and `"` escaped.
fn nsis_text(text: &str) -> String {
    text.replace('$', "$$").replace('"', "$\\\"")
}

pub(crate) fn nsis_script(project: &Project, exe: &Path, extra: &[PathBuf], out: &Path) -> String {
    let name = nsis_text(&project.display_name());
    let version = nsis_text(&project.version_name());
    let exe_name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}.exe", project.package));
    // The key the system's list of programs knows it by: the application id.
    let key = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        project.android_package
    );
    let mut files = format!("  File \"{}\"\n", exe.display());
    for file in extra {
        files.push_str(&format!("  File \"{}\"\n", file.display()));
    }
    let mut removes = format!("  Delete \"$INSTDIR\\{exe_name}\"\n");
    for file in extra {
        if let Some(n) = file.file_name() {
            removes.push_str(&format!("  Delete \"$INSTDIR\\{}\"\n", n.to_string_lossy()));
        }
    }
    format!(
        r#"; Written by `frus build installer`: a per-user install, no administrator needed.
Unicode true
SetCompressor /SOLID lzma
Name "{name}"
OutFile "{out}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\{name}"
InstallDirRegKey HKCU "{key}" "InstallLocation"
VIProductVersion "{four}"
VIAddVersionKey "ProductName" "{name}"
VIAddVersionKey "FileDescription" "{name} setup"
VIAddVersionKey "ProductVersion" "{version}"
VIAddVersionKey "FileVersion" "{version}"
VIAddVersionKey "LegalCopyright" ""

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
{files}  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\{name}.lnk" "$INSTDIR\{exe_name}"
  WriteRegStr HKCU "{key}" "DisplayName" "{name}"
  WriteRegStr HKCU "{key}" "DisplayVersion" "{version}"
  WriteRegStr HKCU "{key}" "DisplayIcon" "$INSTDIR\{exe_name}"
  WriteRegStr HKCU "{key}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "{key}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "{key}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "{key}" "NoModify" 1
  WriteRegDWORD HKCU "{key}" "NoRepair" 1
SectionEnd

Section "Uninstall"
{removes}  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\{name}.lnk"
  DeleteRegKey HKCU "{key}"
SectionEnd
"#,
        out = out.display(),
        four = four_part_version(&project.version_name(), project.build_number()),
    )
}

/// `makensis`, from the NSIS fetched into `~/.frus/tools` the first time.
fn nsis() -> Result<PathBuf, String> {
    let root = crate::setup::frus_home()
        .join("tools")
        .join(format!("nsis-{NSIS_VERSION}"));
    let makensis = root
        .join(format!("nsis-{NSIS_VERSION}"))
        .join("makensis.exe");
    if makensis.is_file() {
        return Ok(makensis);
    }
    crate::report::say(&format!(
        "Fetching NSIS {NSIS_VERSION}, which makes Windows setup programs (about 2.5 MB, once)…"
    ));
    let archive = crate::setup::frus_home().join("tools").join("nsis.zip");
    crate::setup::download(
        &format!(
            "https://downloads.sourceforge.net/project/nsis/NSIS%203/{NSIS_VERSION}/nsis-{NSIS_VERSION}.zip"
        ),
        &archive,
    )?;
    let _ = std::fs::remove_dir_all(&root);
    crate::setup::extract(&archive, &root)?;
    let _ = std::fs::remove_file(&archive);
    if makensis.is_file() {
        Ok(makensis)
    } else {
        Err(format!(
            "NSIS was unpacked but {} is missing",
            makensis.display()
        ))
    }
}

// --- macOS ---

fn macos(project: &Project, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let stage = dir.join("dmg");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let declared = project.permissions()?;
    crate::bundle::make_app(&stage, &project.display_name(), exe, project, &declared)?;
    // The usual shape of a disk image: the application, and a link to drag it onto.
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink("/Applications", stage.join("Applications"));
    }
    let image = dir.join(format!(
        "{}-{}.dmg",
        file_stem(project),
        project.version_name()
    ));
    let _ = std::fs::remove_file(&image);
    cargo::run(
        Command::new("hdiutil")
            .args(["create", "-volname"])
            .arg(project.display_name())
            .arg("-srcfolder")
            .arg(&stage)
            .args(["-ov", "-format", "UDZO"])
            .arg(&image),
    )?;
    Ok(image)
}

// --- Linux ---

/// A Debian package's name: lowercase letters, digits, `+`, `-` and `.`.
fn deb_name(package: &str) -> String {
    package
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "+-.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        other => other,
    }
}

pub(crate) fn deb_control(project: &Project, installed_kb: u64) -> String {
    let version = match project.build_number() {
        Some(n) => format!("{}+{n}", project.version_name()),
        None => project.version_name(),
    };
    format!(
        "Package: {}\nVersion: {version}\nArchitecture: {}\nMaintainer: {} developers\n\
         Installed-Size: {installed_kb}\nSection: misc\nPriority: optional\n\
         Depends: libc6\nRecommends: libvulkan1, libxkbcommon0, libwayland-client0, libx11-6\n\
         Description: {}\n",
        deb_name(&project.package),
        deb_arch(),
        project.display_name(),
        project.display_name(),
    )
}

pub(crate) fn desktop_entry(project: &Project, program: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={program}\nIcon={program}\n\
         Terminal=false\nCategories=Utility;\n",
        project.display_name()
    )
}

fn linux(project: &Project, _mode: Mode, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let program = deb_name(&project.package);
    let stage = dir.join("deb");
    let _ = std::fs::remove_dir_all(&stage);
    let data = stage.join("data");
    let control = stage.join("control");
    let write = |path: PathBuf, bytes: &[u8]| -> Result<(), String> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    let bin = data.join("usr/bin").join(&program);
    write(bin.clone(), &std::fs::read(exe).map_err(|e| e.to_string())?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
    }
    write(
        data.join("usr/share/applications")
            .join(format!("{program}.desktop")),
        desktop_entry(project, &program).as_bytes(),
    )?;
    // The launcher icon the Android build uses: the frus logo unless the application changed it.
    let icon = project
        .root
        .join(project.android_resources.as_deref().unwrap_or("res"))
        .join("mipmap-xxxhdpi")
        .join("ic_launcher.png");
    if icon.is_file() {
        write(
            data.join("usr/share/icons/hicolor/192x192/apps")
                .join(format!("{program}.png")),
            &std::fs::read(&icon).map_err(|e| e.to_string())?,
        )?;
    }
    let installed_kb = std::fs::metadata(exe)
        .map(|m| m.len() / 1024 + 1)
        .unwrap_or(0);
    write(
        control.join("control"),
        deb_control(project, installed_kb).as_bytes(),
    )?;

    let tar = |from: &Path, to: &Path| {
        cargo::run(
            Command::new("tar")
                .args(["--owner=0", "--group=0", "--numeric-owner", "-czf"])
                .arg(to)
                .arg("-C")
                .arg(from)
                .arg("."),
        )
    };
    tar(&control, &stage.join("control.tar.gz"))?;
    tar(&data, &stage.join("data.tar.gz"))?;
    let deb = dir.join(format!(
        "{program}_{}_{}.deb",
        project.version_name(),
        deb_arch()
    ));
    let members = [
        ("debian-binary".to_string(), b"2.0\n".to_vec()),
        (
            "control.tar.gz".to_string(),
            std::fs::read(stage.join("control.tar.gz")).map_err(|e| e.to_string())?,
        ),
        (
            "data.tar.gz".to_string(),
            std::fs::read(stage.join("data.tar.gz")).map_err(|e| e.to_string())?,
        ),
    ];
    std::fs::write(&deb, ar(&members)).map_err(|e| e.to_string())?;
    Ok(deb)
}

/// The `ar` archive a `.deb` is: a magic line, then each member's 60-byte header and its bytes,
/// padded to an even length.
pub(crate) fn ar(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, bytes) in members {
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            name,
            0,
            0,
            0,
            "100644",
            bytes.len()
        );
        debug_assert_eq!(header.len(), 60);
        out.extend(header.as_bytes());
        out.extend(bytes);
        if bytes.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Variant;

    fn project() -> Project {
        let manifest = "[package]\nname = \"my_app\"\nversion = \"1.2.0\"\n\n\
            [package.metadata.android]\npackage = \"com.acme.my_app\"\n\n\
            [package.metadata.frus.flavors.dev]\napp_name = \"My App $Dev\"\n\
            application_id_suffix = \".dev\"\n";
        let mut project = Project::from_manifest(Path::new("."), manifest).unwrap();
        let variant = Variant {
            flavor: Some("dev".into()),
            build_number: Some(7),
            ..Variant::default()
        };
        variant
            .apply(Mode::Release, &mut project, Path::new("."))
            .unwrap();
        project
    }

    #[test]
    fn the_setup_script_installs_for_the_user() {
        let project = project();
        let script = nsis_script(
            &project,
            Path::new(r"C:\t\release\my_app.exe"),
            &[PathBuf::from(r"C:\t\release\dxil.dll")],
            Path::new(r"C:\t\release\installer\My-App-Dev-1.2.0-setup.exe"),
        );
        assert!(script.contains("RequestExecutionLevel user"));
        assert!(script.contains(r#"Name "My App $$Dev""#), "{script}");
        assert!(script.contains(r#"VIProductVersion "1.2.0.7""#), "{script}");
        assert!(
            script.contains(r"Uninstall\com.acme.my_app.dev"),
            "{script}"
        );
        assert!(script.contains(r#"File "C:\t\release\dxil.dll""#));
        assert!(script.contains(r#"Delete "$INSTDIR\dxil.dll""#));
        assert!(script
            .contains(r#"CreateShortcut "$SMPROGRAMS\My App $$Dev.lnk" "$INSTDIR\my_app.exe""#));
        assert_eq!(file_stem(&project), "My-App-Dev");
    }

    #[test]
    fn versions_take_four_numbers() {
        assert_eq!(four_part_version("1.2.3", Some(42)), "1.2.3.42");
        assert_eq!(four_part_version("2.0", None), "2.0.0.0");
        assert_eq!(four_part_version("1.0.0-beta", None), "1.0.0.0");
    }

    #[test]
    fn the_debian_package_describes_itself() {
        let project = project();
        let control = deb_control(&project, 12);
        assert!(
            control.starts_with("Package: my-app\nVersion: 1.2.0+7\n"),
            "{control}"
        );
        assert!(control.contains("Installed-Size: 12\n"));
        assert!(control.ends_with("Description: My App $Dev\n"));
        let entry = desktop_entry(&project, "my-app");
        assert!(
            entry.contains("Name=My App $Dev\nExec=my-app\nIcon=my-app\n"),
            "{entry}"
        );
    }

    #[test]
    fn an_ar_archive_pads_its_members() {
        let archive = ar(&[
            ("debian-binary".into(), b"2.0\n".to_vec()),
            ("odd".into(), b"abc".to_vec()),
        ]);
        assert!(archive.starts_with(
            b"!<arch>\ndebian-binary   0           0     0     100644  4         `\n2.0\n"
        ));
        assert_eq!(archive.len(), 8 + 60 + 4 + 60 + 3 + 1);
    }
}
