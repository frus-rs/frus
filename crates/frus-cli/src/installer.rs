// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus build installer` (milestone 790): the desktop program in the form its system installs.
//!
//! - **Windows**: a setup program made with NSIS (the default), which installs for the current
//!   user — into `%LOCALAPPDATA%\Programs`, with a Start menu entry and an uninstaller in the
//!   system's list — so it needs no administrator, to be made or to be installed. NSIS is
//!   fetched the first time (about 2.5 MB, a zip that runs where it is unpacked). Or an MSIX
//!   package (`--format msix`), which must be signed (`windows_package`).
//! - **Linux**: a `.deb` (the default), written here (an `ar` archive of two tarballs, made
//!   with the system's `tar`); an `.rpm` (`--format rpm`), made with `rpmbuild`; or an AppImage
//!   (`--format appimage`), made with `appimagetool`, fetched the first time.
//! - **macOS**: the `.app` bundle in a disk image, made with `hdiutil`.
//!
//! Each carries the name people see (the flavor's) and the version (`--build-name`,
//! `--build-number`). On Windows, the program and what is made from it are signed when a
//! certificate is configured.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cargo::{self, Mode};
use crate::project::Project;

/// The NSIS release fetched on Windows.
const NSIS_VERSION: &str = "3.11";
/// The appimagetool release, and the AppImage runtime it puts in front of the image, fetched
/// on Linux the first time.
const APPIMAGETOOL_VERSION: &str = "1.9.1";
const APPIMAGE_RUNTIME_VERSION: &str = "20251108";

/// The kinds of installer `frus build installer --format` makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// A Windows setup program (NSIS).
    Nsis,
    /// A Windows MSIX package.
    Msix,
    /// A Debian or Ubuntu package.
    Deb,
    /// A Fedora, RHEL or openSUSE package.
    Rpm,
    /// A Linux program in one file that runs without installing.
    AppImage,
    /// A macOS disk image.
    Dmg,
}

impl Format {
    pub(crate) fn parse(word: &str) -> Result<Format, String> {
        match word.to_ascii_lowercase().as_str() {
            "nsis" | "exe" | "setup" => Ok(Format::Nsis),
            "msix" => Ok(Format::Msix),
            "deb" => Ok(Format::Deb),
            "rpm" => Ok(Format::Rpm),
            "appimage" => Ok(Format::AppImage),
            "dmg" => Ok(Format::Dmg),
            other => Err(format!(
                "--format is nsis or msix (Windows), deb, rpm or appimage (Linux), dmg (macOS), \
                 not `{other}`"
            )),
        }
    }

    /// The system the installer is for, which is the one it is made on.
    fn system(self) -> &'static str {
        match self {
            Format::Nsis | Format::Msix => "windows",
            Format::Deb | Format::Rpm | Format::AppImage => "linux",
            Format::Dmg => "macos",
        }
    }

    /// The installer made when none is named: NSIS, `.deb`, disk image.
    fn default_for(system: &str) -> Option<Format> {
        match system {
            "windows" => Some(Format::Nsis),
            "linux" => Some(Format::Deb),
            "macos" => Some(Format::Dmg),
            _ => None,
        }
    }

    /// `format`, or this system's default, when it can be made on `system`.
    fn resolve(format: Option<Format>, system: &str) -> Result<Format, String> {
        let format = match format {
            Some(format) => format,
            None => Format::default_for(system).ok_or_else(|| {
                format!(
                    "frus build installer makes Windows, macOS and Linux installers, not \
                     {system}'s"
                )
            })?,
        };
        if format.system() == system {
            Ok(format)
        } else {
            Err(format!(
                "{} is a {} installer, made on {}: this is {system}",
                format.word(),
                format.system(),
                format.system()
            ))
        }
    }

    fn word(self) -> &'static str {
        match self {
            Format::Nsis => "nsis",
            Format::Msix => "msix",
            Format::Deb => "deb",
            Format::Rpm => "rpm",
            Format::AppImage => "appimage",
            Format::Dmg => "dmg",
        }
    }
}

/// Makes the installer `format` (this system's default when `None`) from the program `exe`;
/// returns its path.
pub(crate) fn make(
    project: &Project,
    mode: Mode,
    exe: &Path,
    format: Option<Format>,
) -> Result<PathBuf, String> {
    let format = Format::resolve(format, std::env::consts::OS)?;
    let dir = exe
        .parent()
        .ok_or("the program has no directory")?
        .join("installer");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    match format {
        Format::Nsis | Format::Msix => windows(project, exe, &dir, format),
        Format::Dmg => macos(project, exe, &dir),
        Format::Deb => deb(project, mode, exe, &dir),
        Format::Rpm => rpm(project, exe, &dir),
        Format::AppImage => appimage(project, exe, &dir),
    }
}

/// A name for files: the display name's letters and digits, `-` between words.
pub(crate) fn file_stem(project: &Project) -> String {
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

/// The launcher icon the Android build uses — the frus logo unless the application changed
/// it, or a flavor's resources replace it: what every installer shows.
pub(crate) fn launcher_icon(project: &Project) -> Option<PathBuf> {
    let flavor = project
        .variant
        .flavor
        .as_ref()
        .and_then(|f| f.resources.as_deref())
        .map(|res| project.root.join(res));
    let base = project
        .root
        .join(project.android_resources.as_deref().unwrap_or("res"));
    flavor
        .into_iter()
        .chain(std::iter::once(base))
        .map(|res| res.join("mipmap-xxxhdpi").join("ic_launcher.png"))
        .find(|icon| icon.is_file())
}

// --- Windows ---

fn windows(project: &Project, exe: &Path, dir: &Path, format: Format) -> Result<PathBuf, String> {
    // DirectX's fast shader compiler, beside the program as `frus build desktop` puts it.
    if let Some(bin) = exe.parent() {
        crate::setup::place_dxc(bin);
    }
    let extra: Vec<PathBuf> = crate::setup::DXC_FILES
        .iter()
        .filter_map(|f| exe.parent().map(|d| d.join(f)))
        .filter(|p| p.is_file())
        .collect();
    // The program is signed before it is packaged, so that what is installed is signed too.
    let signing = crate::windows_package::signing(project)?;
    if let Some(signing) = &signing {
        crate::windows_package::sign(signing, exe)?;
    }
    let version = four_part_version(&project.version_name(), project.build_number());
    if format == Format::Msix {
        return crate::windows_package::msix(project, exe, &extra, dir, &version);
    }
    let makensis = nsis()?;
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
    if let Some(signing) = &signing {
        crate::windows_package::sign(signing, &out)?;
    }
    Ok(out)
}

/// A Windows version holds four numbers: `1.2.3` → `1.2.3.0`, with the build number last
/// when there is one.
pub(crate) fn four_part_version(name: &str, number: Option<u32>) -> String {
    let mut parts: Vec<u32> = name
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse::<u32>().unwrap_or(0).min(65535))
        .collect();
    parts.resize(3, 0);
    parts.push(number.unwrap_or(0).min(65535));
    parts
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

/// The last part of a Windows path, on whatever system the script is written: `\` and `/`
/// both separate, as they do on Windows (`Path::file_name` elsewhere sees `C:\t\a.dll` as
/// one name). makensis also runs on Linux and macOS, and the script must not depend on it.
fn windows_file_name(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.rsplit(['\\', '/']).next().unwrap_or(&text).to_string()
}

/// A value inside an NSIS string: `$` and `"` escaped.
fn nsis_text(text: &str) -> String {
    text.replace('$', "$$").replace('"', "$\\\"")
}

pub(crate) fn nsis_script(project: &Project, exe: &Path, extra: &[PathBuf], out: &Path) -> String {
    let name = nsis_text(&project.display_name());
    let version = nsis_text(&project.version_name());
    let exe_name = Some(windows_file_name(exe))
        .filter(|n| !n.is_empty())
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
        removes.push_str(&format!(
            "  Delete \"$INSTDIR\\{}\"\n",
            windows_file_name(file)
        ));
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
    cargo::run(&mut dmg_command(project, &stage, &image))?;
    Ok(image)
}

/// `hdiutil create`: the folder `stage` as a compressed, read-only disk image named after the
/// application.
pub(crate) fn dmg_command(project: &Project, stage: &Path, image: &Path) -> Command {
    let mut command = Command::new("hdiutil");
    command
        .args(["create", "-volname"])
        .arg(project.display_name())
        .arg("-srcfolder")
        .arg(stage)
        .args(["-ov", "-format", "UDZO"])
        .arg(image);
    command
}

// --- Linux ---

/// A Linux package's name: lowercase letters, digits, `+`, `-` and `.`.
fn package_name(package: &str) -> String {
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
        package_name(&project.package),
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

/// Writes `bytes` at `path`, making its directories.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// The files every Linux package installs, below `root`: the program in `usr/bin`, the desktop
/// entry, the icon. Returns the paths written, relative to `root`.
fn linux_tree(project: &Project, exe: &Path, root: &Path) -> Result<Vec<String>, String> {
    let program = package_name(&project.package);
    let mut written = Vec::new();
    let bin = format!("usr/bin/{program}");
    write(
        &root.join(&bin),
        &std::fs::read(exe).map_err(|e| e.to_string())?,
    )?;
    make_executable(&root.join(&bin));
    written.push(bin);
    let entry = format!("usr/share/applications/{program}.desktop");
    write(
        &root.join(&entry),
        desktop_entry(project, &program).as_bytes(),
    )?;
    written.push(entry);
    if let Some(icon) = launcher_icon(project) {
        let path = format!("usr/share/icons/hicolor/192x192/apps/{program}.png");
        write(
            &root.join(&path),
            &std::fs::read(&icon).map_err(|e| e.to_string())?,
        )?;
        written.push(path);
    }
    Ok(written)
}

fn deb(project: &Project, _mode: Mode, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let program = package_name(&project.package);
    let stage = dir.join("deb");
    let _ = std::fs::remove_dir_all(&stage);
    let data = stage.join("data");
    let control = stage.join("control");
    linux_tree(project, exe, &data)?;
    let installed_kb = std::fs::metadata(exe)
        .map(|m| m.len() / 1024 + 1)
        .unwrap_or(0);
    write(
        &control.join("control"),
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

/// An rpm version: no `-` (it separates the release), which becomes `~`, sorted before.
fn rpm_version(name: &str) -> String {
    name.replace('-', "~")
}

fn rpm_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "arm" => "armv7hl",
        other => other,
    }
}

/// The spec `rpmbuild` makes the package from: the files already built, copied from `tree`.
pub(crate) fn rpm_spec(project: &Project, tree: &Path, files: &[String]) -> String {
    let mut listed = String::new();
    for file in files {
        listed.push('/');
        listed.push_str(file);
        listed.push('\n');
    }
    format!(
        "# Written by `frus build installer --format rpm`.\n\
         Name: {name}\n\
         Version: {version}\n\
         Release: {release}\n\
         Summary: {summary}\n\
         License: {license}\n\
         \n\
         # The program is already built, and stripped by its release profile.\n\
         %global debug_package %{{nil}}\n\
         %global __os_install_post %{{nil}}\n\
         %global _build_id_links none\n\
         \n\
         %description\n\
         {summary}\n\
         \n\
         %install\n\
         mkdir -p %{{buildroot}}\n\
         cp -a '{tree}'/. %{{buildroot}}/\n\
         \n\
         %files\n\
         {listed}",
        name = package_name(&project.package),
        version = rpm_version(&project.version_name()),
        release = project.build_number().unwrap_or(1),
        summary = project.display_name().replace('%', "%%"),
        license = project.license.as_deref().unwrap_or("Proprietary"),
        tree = tree.display(),
    )
}

fn rpm(project: &Project, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let rpmbuild = which("rpmbuild").ok_or(
        "an .rpm is made with rpmbuild, which is not installed: `sudo dnf install rpm-build` \
         (Fedora, RHEL), `sudo zypper install rpm-build` (openSUSE), `sudo apt install rpm` \
         (Debian, Ubuntu)",
    )?;
    let stage = dir.join("rpm");
    let _ = std::fs::remove_dir_all(&stage);
    let tree = stage.join("tree");
    let files = linux_tree(project, exe, &tree)?;
    let spec = stage.join("package.spec");
    write(&spec, rpm_spec(project, &tree, &files).as_bytes())?;
    let name = format!(
        "{}-{}-{}.{}.rpm",
        package_name(&project.package),
        rpm_version(&project.version_name()),
        project.build_number().unwrap_or(1),
        rpm_arch()
    );
    let out = dir.join(&name);
    let _ = std::fs::remove_file(&out);
    cargo::run(
        Command::new(rpmbuild)
            .args(["-bb", "--quiet"])
            .arg("--define")
            .arg(format!("_topdir {}", stage.join("build").display()))
            .arg("--define")
            .arg(format!("_rpmdir {}", dir.display()))
            .arg("--define")
            .arg("_build_name_fmt %{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}.rpm")
            .arg("--target")
            .arg(rpm_arch())
            .arg(&spec),
    )?;
    if out.is_file() {
        Ok(out)
    } else {
        Err(format!("rpmbuild finished but wrote no {}", out.display()))
    }
}

/// A program on the `PATH`.
fn which(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// The script an AppImage starts: the program, wherever the image was unpacked or mounted.
pub(crate) fn app_run(program: &str) -> String {
    format!(
        "#!/bin/sh\n# Written by `frus build installer --format appimage`.\n\
         HERE=\"$(dirname \"$(readlink -f \"$0\")\")\"\n\
         exec \"$HERE/usr/bin/{program}\" \"$@\"\n"
    )
}

fn appimage(project: &Project, exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let program = package_name(&project.package);
    let app_dir = dir.join(format!("{program}.AppDir"));
    let _ = std::fs::remove_dir_all(&app_dir);
    linux_tree(project, exe, &app_dir)?;
    // What appimagetool looks for at the top: the entry, the icon, and AppRun.
    write(
        &app_dir.join(format!("{program}.desktop")),
        desktop_entry(project, &program).as_bytes(),
    )?;
    let icon = launcher_icon(project)
        .ok_or("no launcher icon (res/mipmap-xxxhdpi/ic_launcher.png) for the AppImage")?;
    std::fs::copy(&icon, app_dir.join(format!("{program}.png"))).map_err(|e| e.to_string())?;
    write(&app_dir.join("AppRun"), app_run(&program).as_bytes())?;
    make_executable(&app_dir.join("AppRun"));

    let (tool, runtime) = appimage_tools()?;
    let out = dir.join(format!(
        "{}-{}-{}.AppImage",
        file_stem(project),
        project.version_name(),
        std::env::consts::ARCH
    ));
    let _ = std::fs::remove_file(&out);
    cargo::run(
        Command::new(tool)
            // appimagetool is itself an AppImage: unpacked and run, it needs no FUSE.
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .env("ARCH", std::env::consts::ARCH)
            .env(
                "VERSION",
                match project.build_number() {
                    Some(n) => format!("{}+{n}", project.version_name()),
                    None => project.version_name(),
                },
            )
            .arg("--no-appstream")
            .arg("--runtime-file")
            .arg(runtime)
            .arg(&app_dir)
            .arg(&out),
    )?;
    Ok(out)
}

/// appimagetool and the runtime, fetched into `~/.frus/tools` the first time.
fn appimage_tools() -> Result<(PathBuf, PathBuf), String> {
    let arch = std::env::consts::ARCH;
    let root = crate::setup::frus_home()
        .join("tools")
        .join(format!("appimagetool-{APPIMAGETOOL_VERSION}"));
    let tool = root.join(format!("appimagetool-{arch}.AppImage"));
    let runtime = root.join(format!("runtime-{APPIMAGE_RUNTIME_VERSION}-{arch}"));
    if !tool.is_file() || !runtime.is_file() {
        crate::report::say(&format!(
            "Fetching appimagetool {APPIMAGETOOL_VERSION}, which makes AppImages (about 16 MB, \
             once)…"
        ));
        crate::setup::download(
            &format!(
                "https://github.com/AppImage/appimagetool/releases/download/\
                 {APPIMAGETOOL_VERSION}/appimagetool-{arch}.AppImage"
            ),
            &tool,
        )?;
        crate::setup::download(
            &format!(
                "https://github.com/AppImage/type2-runtime/releases/download/\
                 {APPIMAGE_RUNTIME_VERSION}/runtime-{arch}"
            ),
            &runtime,
        )?;
        make_executable(&tool);
    }
    Ok((tool, runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Variant;

    fn project() -> Project {
        let manifest = "[package]\nname = \"my_app\"\nversion = \"1.2.0\"\nlicense = \"MIT\"\n\n\
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
    fn each_system_has_its_formats() {
        assert_eq!(Format::resolve(None, "windows"), Ok(Format::Nsis));
        assert_eq!(Format::resolve(None, "linux"), Ok(Format::Deb));
        assert_eq!(Format::resolve(None, "macos"), Ok(Format::Dmg));
        assert_eq!(
            Format::resolve(Some(Format::Msix), "windows"),
            Ok(Format::Msix)
        );
        assert_eq!(
            Format::resolve(Some(Format::AppImage), "linux"),
            Ok(Format::AppImage)
        );
        assert!(Format::resolve(Some(Format::Rpm), "windows")
            .unwrap_err()
            .contains("rpm is a linux installer"));
        assert!(Format::resolve(None, "freebsd").is_err());
        assert_eq!(Format::parse("AppImage"), Ok(Format::AppImage));
        assert_eq!(Format::parse("setup"), Ok(Format::Nsis));
        assert!(Format::parse("pkg").unwrap_err().contains("not `pkg`"));
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
        // The same script on every system: no full path where a file name belongs.
        assert!(!script.contains(r"$INSTDIR\C:"), "{script}");
        assert_eq!(windows_file_name(Path::new(r"C:\t\a.dll")), "a.dll");
        assert_eq!(
            windows_file_name(Path::new("/t/release/app.exe")),
            "app.exe"
        );
    }

    #[test]
    fn versions_take_four_numbers() {
        assert_eq!(four_part_version("1.2.3", Some(42)), "1.2.3.42");
        assert_eq!(four_part_version("2.0", None), "2.0.0.0");
        assert_eq!(four_part_version("1.0.0-beta", None), "1.0.0.0");
        assert_eq!(
            four_part_version("1.70000.0", Some(80000)),
            "1.65535.0.65535"
        );
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
    fn the_rpm_spec_lists_what_it_installs() {
        let project = project();
        let spec = rpm_spec(
            &project,
            Path::new("/tmp/stage/tree"),
            &[
                "usr/bin/my-app".to_string(),
                "usr/share/applications/my-app.desktop".to_string(),
            ],
        );
        for line in [
            "Name: my-app\n",
            "Version: 1.2.0\n",
            "Release: 7\n",
            "Summary: My App $Dev\n",
            "License: MIT\n",
            "%global debug_package %{nil}\n",
            "cp -a '/tmp/stage/tree'/. %{buildroot}/\n",
            "%files\n/usr/bin/my-app\n/usr/share/applications/my-app.desktop\n",
        ] {
            assert!(spec.contains(line), "{line:?} missing from\n{spec}");
        }
        assert_eq!(rpm_version("1.0.0-beta.2"), "1.0.0~beta.2");
    }

    #[test]
    fn the_appimage_starts_the_program() {
        let run = app_run("my-app");
        assert!(run.starts_with("#!/bin/sh\n"));
        assert!(run.contains(r#"exec "$HERE/usr/bin/my-app" "$@""#), "{run}");
    }

    #[test]
    fn the_disk_image_is_named_after_the_application() {
        let project = project();
        let command = dmg_command(
            &project,
            Path::new("/out/installer/dmg"),
            Path::new("/out/installer/My-App-Dev-1.2.0.dmg"),
        );
        assert_eq!(command.get_program(), "hdiutil");
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "create",
                "-volname",
                "My App $Dev",
                "-srcfolder",
                "/out/installer/dmg",
                "-ov",
                "-format",
                "UDZO",
                "/out/installer/My-App-Dev-1.2.0.dmg"
            ]
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
