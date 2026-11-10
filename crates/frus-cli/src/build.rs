// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus build <target>`: what you ship, for one platform, in release (or `--profile`,
//! `--debug`), as a flavor and a version when asked (milestone 790).

use std::path::Path;
use std::process::ExitCode;

use crate::cargo::{self, Mode};
use crate::project::Project;

/// What `frus build` was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    pub target: Target,
    pub mode: Mode,
    /// `--split-per-abi`: one APK per processor.
    pub split_per_abi: bool,
    /// `--format`: the kind of installer, this system's default when `None`.
    pub format: Option<crate::installer::Format>,
    /// `--flavor`, `--define`, `--build-name`, `--build-number` (milestone 790).
    pub variant: crate::variant::Variant,
}

/// What `frus build` makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// The Android package.
    Apk,
    /// The Android App Bundle a store takes.
    AppBundle,
    /// The desktop program in the system's installer: a setup program on Windows, a `.deb`
    /// on Linux, a disk image on macOS.
    Installer,
    /// The site: `web/`, with the release build in `web/pkg/`.
    Web,
    /// The desktop program, for the system this runs on.
    Desktop,
}

impl Target {
    /// The platform the target is built for (milestone 791).
    pub(crate) fn platform(self) -> crate::platforms::AppPlatform {
        match self {
            Target::Apk | Target::AppBundle => crate::platforms::AppPlatform::Android,
            Target::Web => crate::platforms::AppPlatform::Web,
            Target::Desktop | Target::Installer => crate::platforms::AppPlatform::host(),
        }
    }

    pub(crate) fn parse(word: &str) -> Result<Target, String> {
        match word {
            "apk" | "android" => Ok(Target::Apk),
            "appbundle" | "aab" => Ok(Target::AppBundle),
            "installer" => Ok(Target::Installer),
            "web" => Ok(Target::Web),
            "desktop" => Ok(Target::Desktop),
            "windows" | "linux" | "macos" if word == std::env::consts::OS => Ok(Target::Desktop),
            "windows" | "linux" | "macos" => Err(format!(
                "a {word} program is built on {word}: this is {}",
                std::env::consts::OS
            )),
            other => Err(format!(
                "frus build makes `apk`, `appbundle`, `web`, `{}` or `installer`, not `{other}`",
                std::env::consts::OS
            )),
        }
    }
}

pub(crate) fn build(request: Request) -> Result<ExitCode, String> {
    let Request {
        target,
        mode,
        split_per_abi,
        format,
        variant,
    } = request;
    if split_per_abi && target != Target::Apk {
        return Err("--split-per-abi goes with `frus build apk`".into());
    }
    if format.is_some() && target != Target::Installer {
        return Err("--format goes with `frus build installer`".into());
    }
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut project = Project::find(&here)?;
    project.check_platform(target.platform())?;
    variant.apply(mode, &mut project, &here)?;
    crate::variant::export(&project);
    say_variant(&project, mode);
    match target {
        Target::Apk if split_per_abi => {
            let apk = cargo::cargo_apk(&project, mode, None)?;
            let apks = crate::android_pack::split_per_abi(&project, mode, &apk)?;
            println!();
            for apk in &apks {
                println!("Built {} ({}).", apk.display(), size(apk));
            }
            println!("One per processor: a store, or a device's own, installs the one it runs.");
        }
        Target::AppBundle => {
            let apk = cargo::cargo_apk(&project, mode, None)?;
            let bundle = crate::android_pack::app_bundle(&project, mode, &apk)?;
            println!(
                "\nBuilt {} ({}): the App Bundle to upload to a store, version {} ({}).",
                bundle.display(),
                size(&bundle),
                project.version_name(),
                project.version_code()
            );
        }
        Target::Installer => {
            let exe = cargo::build_desktop(&project, mode)?;
            let installer = crate::installer::make(&project, mode, &exe, format)?;
            println!("\nBuilt {} ({}).", installer.display(), size(&installer));
        }
        Target::Apk => {
            let apk = cargo::build_apk(&project, mode, None)?;
            println!("\nBuilt {} ({}).", apk.display(), size(&apk));
            if mode == Mode::Release {
                println!("Install it with `adb install -r <apk>`; a store wants it signed (see README.md, \"Signing\").");
            }
        }
        Target::Web => {
            let site = cargo::build_web(&project, mode)?;
            let wasm = site.join("pkg").join(format!("{}_bg.wasm", project.lib));
            println!(
                "\nBuilt the site: {} (the application is {}).\n\
                 Put that directory on any static host, and serve the .wasm compressed.\n\
                 Try it here first: `frus run -d web{}`.",
                site.display(),
                size(&wasm),
                if mode == Mode::Release {
                    " --release"
                } else {
                    ""
                }
            );
        }
        Target::Desktop => {
            let exe = cargo::build_desktop(&project, mode)?;
            println!("\nBuilt {} ({}).", exe.display(), size(&exe));
            // On a Mac the application is a bundle: its Info.plist says why it asks for what it
            // declares, and macOS ends one that asks without it (milestone 744).
            if cfg!(target_os = "macos") {
                let dir = exe.parent().unwrap_or(&project.root);
                let declared = project.permissions()?;
                let inside =
                    crate::bundle::make_app(dir, &project.package, &exe, &project, &declared)?;
                println!(
                    "The application: {}.app (open it, or start {}).",
                    dir.join(&project.package).display(),
                    inside.display()
                );
            }
            // Shipped beside the program, DXC makes it start about half a second faster on
            // Windows (milestone 684); without them it uses Windows' own compiler.
            if cfg!(windows) {
                if let Some(dir) = exe.parent() {
                    crate::setup::place_dxc(dir);
                    println!(
                        "Beside it: {} — DirectX 12's shader compiler; ship them with the \
                         program for a faster start.",
                        crate::setup::DXC_FILES.join(", ")
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// What is built, when it is more than the plain build: the mode, the flavor, the version.
fn say_variant(project: &Project, mode: Mode) {
    let variant = &project.variant;
    let mut parts = vec![format!("{} build", mode.name())];
    if let Some(flavor) = &variant.flavor {
        parts.push(format!("flavor {}", flavor.name));
    }
    if variant.build_name.is_some() || variant.build_number.is_some() {
        parts.push(format!(
            "version {}{}",
            project.version_name(),
            project
                .build_number()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default()
        ));
    }
    if !variant.defines.is_empty() {
        let keys: Vec<_> = variant.defines.iter().map(|(k, _)| k.as_str()).collect();
        parts.push(format!("defines {}", keys.join(", ")));
    }
    if parts.len() > 1 || mode == Mode::Profile {
        crate::report::say(&format!("Building: {}.", parts.join("; ")));
    }
}

fn size(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => format!("{:.1} MB", meta.len() as f64 / 1_048_576.0),
        Err(_) => "size unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(Target::parse("apk"), Ok(Target::Apk));
        assert_eq!(Target::parse("android"), Ok(Target::Apk));
        assert_eq!(Target::parse("web"), Ok(Target::Web));
        assert_eq!(Target::parse("appbundle"), Ok(Target::AppBundle));
        assert_eq!(Target::parse("aab"), Ok(Target::AppBundle));
        assert_eq!(Target::parse("installer"), Ok(Target::Installer));
        assert_eq!(Target::parse("desktop"), Ok(Target::Desktop));
        assert_eq!(Target::parse(std::env::consts::OS), Ok(Target::Desktop));
        assert!(Target::parse("ios").unwrap_err().contains("not `ios`"));
        let elsewhere = if cfg!(windows) { "linux" } else { "windows" };
        assert!(Target::parse(elsewhere)
            .unwrap_err()
            .contains("is built on"));
    }
}
