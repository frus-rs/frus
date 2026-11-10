// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Windows packaging beyond the setup program (milestone 790): signing with `signtool`, and
//! the MSIX package `makeappx` makes.
//!
//! Both tools come with the Windows SDK. When it is not installed, the SDK's build tools are
//! fetched from NuGet into `~/.frus/tools` (about 22 MB, once), which hold the same programs.
//!
//! **Signing** is configured once, for every Windows file `frus build installer` makes: the
//! program, the setup program and the MSIX package. A certificate file (`.pfx`) or the
//! thumbprint of one in the user's certificate store; its password comes from the environment
//! only, never from `Cargo.toml`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::project::Project;

/// The NuGet package of the SDK's build tools fetched when no Windows SDK is installed.
const BUILD_TOOLS_VERSION: &str = "10.0.26100.6584";
/// The SDK version its `bin/` directory is named after.
const BUILD_TOOLS_BIN: &str = "10.0.26100.0";

/// The table of `Cargo.toml` that configures signing.
pub(crate) const SIGNING_SECTION: &str = "package.metadata.frus.windows.signing";

/// How Windows files are signed, when they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Signing {
    /// A certificate file, `.pfx`: `FRUS_WINDOWS_CERTIFICATE` or `certificate`.
    pub certificate: Option<PathBuf>,
    /// Its password: `FRUS_WINDOWS_CERTIFICATE_PASSWORD` only.
    pub password: Option<String>,
    /// Or a certificate of the user's store, by thumbprint:
    /// `FRUS_WINDOWS_CERTIFICATE_THUMBPRINT` or `thumbprint`.
    pub thumbprint: Option<String>,
    /// A timestamp server, so that the signature outlives the certificate:
    /// `FRUS_WINDOWS_TIMESTAMP_URL` or `timestamp_url`.
    pub timestamp_url: Option<String>,
    /// The MSIX publisher, which must be the certificate's subject: `FRUS_WINDOWS_PUBLISHER` or
    /// `publisher`. Read from the certificate when not given.
    pub publisher: Option<String>,
}

/// The signing configured for `project`: the environment first, then `Cargo.toml`'s
/// `[package.metadata.frus.windows.signing]`. `None` when nothing is configured.
pub(crate) fn signing(project: &Project) -> Result<Option<Signing>, String> {
    signing_from(project, |name| {
        std::env::var(name).ok().filter(|v| !v.trim().is_empty())
    })
}

fn signing_from(
    project: &Project,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Option<Signing>, String> {
    let manifest = |key: &str| {
        project
            .windows_signing
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    let either = |variable: &str, key: &str| env(variable).or_else(|| manifest(key));
    if manifest("password").is_some() {
        return Err(format!(
            "[{SIGNING_SECTION}] has a `password`: a password does not belong in Cargo.toml. \
             Set FRUS_WINDOWS_CERTIFICATE_PASSWORD in the environment instead"
        ));
    }
    let signing = Signing {
        certificate: either("FRUS_WINDOWS_CERTIFICATE", "certificate").map(|path| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                project.root.join(path)
            }
        }),
        password: env("FRUS_WINDOWS_CERTIFICATE_PASSWORD"),
        thumbprint: either("FRUS_WINDOWS_CERTIFICATE_THUMBPRINT", "thumbprint")
            .map(|t| t.replace([' ', ':'], "").to_uppercase()),
        timestamp_url: either("FRUS_WINDOWS_TIMESTAMP_URL", "timestamp_url"),
        publisher: either("FRUS_WINDOWS_PUBLISHER", "publisher"),
    };
    match (&signing.certificate, &signing.thumbprint) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            Err("Windows signing names both a certificate file and a thumbprint: keep one".into())
        }
        (Some(path), None) if !path.is_file() => Err(format!(
            "the signing certificate {} does not exist",
            path.display()
        )),
        _ => Ok(Some(signing)),
    }
}

/// `signtool`'s arguments to sign `file` as `signing` says.
pub(crate) fn signtool_args(signing: &Signing, file: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["sign", "/fd", "SHA256"].map(OsString::from).to_vec();
    if let Some(certificate) = &signing.certificate {
        args.push("/f".into());
        args.push(certificate.into());
        if let Some(password) = &signing.password {
            args.push("/p".into());
            args.push(password.into());
        }
    } else if let Some(thumbprint) = &signing.thumbprint {
        args.push("/sha1".into());
        args.push(thumbprint.into());
    }
    if let Some(url) = &signing.timestamp_url {
        args.extend(["/tr".into(), url.into(), "/td".into(), "SHA256".into()]);
    }
    args.push(file.into());
    args
}

/// Signs `file` with `signtool`.
pub(crate) fn sign(signing: &Signing, file: &Path) -> Result<(), String> {
    let signtool = sdk_tool("signtool.exe")?;
    crate::report::say(&format!(
        "Signing {}…",
        file.file_name().unwrap_or_default().to_string_lossy()
    ));
    let output = Command::new(&signtool)
        .args(signtool_args(signing, file))
        .output()
        .map_err(|e| format!("could not run {}: {e}", signtool.display()))?;
    if output.status.success() {
        return Ok(());
    }
    // signtool's message, without the password it was given.
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if let Some(password) = signing.password.as_deref().filter(|p| !p.is_empty()) {
        text = text.replace(password, "***");
    }
    Err(format!(
        "signtool could not sign {}: {}",
        file.display(),
        text.trim()
    ))
}

/// The certificate's subject, `CN=…`: what an MSIX package's publisher must be.
fn certificate_subject(signing: &Signing) -> Result<String, String> {
    if let Some(publisher) = &signing.publisher {
        return Ok(publisher.clone());
    }
    // Windows PowerShell reads it; the values travel in the environment, not the command line.
    let script = "$ErrorActionPreference = 'Stop'; \
        if ($env:FRUS_SIGN_FILE) { \
          $c = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($env:FRUS_SIGN_FILE, $env:FRUS_SIGN_PASSWORD) \
        } else { \
          $c = Get-ChildItem Cert:\\CurrentUser\\My, Cert:\\LocalMachine\\My | Where-Object Thumbprint -eq $env:FRUS_SIGN_THUMBPRINT | Select-Object -First 1 \
        }; \
        if ($c) { [Console]::Out.Write($c.Subject) }";
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    if let Some(file) = &signing.certificate {
        command.env("FRUS_SIGN_FILE", file);
        command.env(
            "FRUS_SIGN_PASSWORD",
            signing.password.as_deref().unwrap_or(""),
        );
    } else if let Some(thumbprint) = &signing.thumbprint {
        command.env_remove("FRUS_SIGN_FILE");
        command.env("FRUS_SIGN_THUMBPRINT", thumbprint);
    }
    let output = command
        .output()
        .map_err(|e| format!("could not run PowerShell to read the certificate: {e}"))?;
    let subject = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !subject.is_empty() {
        Ok(subject)
    } else {
        Err(format!(
            "could not read the signing certificate's subject ({}); set FRUS_WINDOWS_PUBLISHER \
             to it, `CN=…`",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A program of the Windows SDK: `FRUS_WINDOWS_SDK_BIN`, the newest installed SDK, or the
/// SDK's build tools fetched from NuGet.
pub(crate) fn sdk_tool(name: &str) -> Result<PathBuf, String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    if let Some(dir) = std::env::var_os("FRUS_WINDOWS_SDK_BIN") {
        let tool = PathBuf::from(dir).join(name);
        return if tool.is_file() {
            Ok(tool)
        } else {
            Err(format!(
                "FRUS_WINDOWS_SDK_BIN has no {name}: {}",
                tool.display()
            ))
        };
    }
    let kits = std::env::var_os("ProgramFiles(x86)")
        .map(|p| PathBuf::from(p).join("Windows Kits").join("10").join("bin"));
    if let Some(kits) = kits {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&kits)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.join(arch).join(name).is_file())
                    .collect()
            })
            .unwrap_or_default();
        versions.sort_by_key(|p| version_key(p));
        if let Some(newest) = versions.last() {
            return Ok(newest.join(arch).join(name));
        }
    }
    let root = crate::setup::frus_home()
        .join("tools")
        .join(format!("windows-sdk-build-tools-{BUILD_TOOLS_VERSION}"));
    let tool = root.join("bin").join(BUILD_TOOLS_BIN).join(arch).join(name);
    if tool.is_file() {
        return Ok(tool);
    }
    crate::report::say(&format!(
        "Fetching the Windows SDK build tools {BUILD_TOOLS_VERSION} (makeappx, signtool; about \
         22 MB, once)…"
    ));
    let archive = crate::setup::frus_home()
        .join("tools")
        .join("windows-sdk-build-tools.zip");
    crate::setup::download(
        &format!(
            "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.buildtools/\
             {BUILD_TOOLS_VERSION}/microsoft.windows.sdk.buildtools.{BUILD_TOOLS_VERSION}.nupkg"
        ),
        &archive,
    )?;
    let _ = std::fs::remove_dir_all(&root);
    crate::setup::extract(&archive, &root)?;
    let _ = std::fs::remove_file(&archive);
    if tool.is_file() {
        Ok(tool)
    } else {
        Err(format!(
            "the Windows SDK build tools were unpacked but {} is missing",
            tool.display()
        ))
    }
}

/// `10.0.26100.0` as numbers, to find the newest SDK.
fn version_key(dir: &Path) -> Vec<u32> {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

// --- MSIX ---

/// An MSIX identity name: letters, digits, `.` and `-`, 3 to 50 of them.
pub(crate) fn identity_name(id: &str) -> String {
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(50)
        .collect();
    if name.len() < 3 {
        format!("{name}-app")
    } else {
        name
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The package's `AppxManifest.xml`: a desktop program that runs with the user's full rights,
/// as any program a setup program installs.
pub(crate) fn appx_manifest(
    project: &Project,
    publisher: &str,
    version: &str,
    exe_name: &str,
) -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let name = xml(&project.display_name());
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!-- Written by frus build installer, as an MSIX package. -->
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="{identity}" Publisher="{publisher}" Version="{version}" ProcessorArchitecture="{arch}" />
  <Properties>
    <DisplayName>{name}</DisplayName>
    <PublisherDisplayName>{publisher_name}</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="App" Executable="{exe}" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="{name}" Description="{name}" BackgroundColor="transparent"
                          Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png" />
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"#,
        identity = identity_name(&project.android_package),
        publisher = xml(publisher),
        publisher_name = xml(&publisher_display_name(publisher)),
        exe = xml(exe_name),
    )
}

/// `CN=Acme Ltd, O=Acme` → `Acme Ltd`, `CN="Acme, Inc", O=Acme` → `Acme, Inc`: the name the
/// system shows for the publisher.
fn publisher_display_name(subject: &str) -> String {
    let Some(at) = subject.find("CN=") else {
        return subject.to_string();
    };
    let rest = &subject[at + 3..];
    match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or(quoted).to_string(),
        None => rest.split(',').next().unwrap_or(rest).trim().to_string(),
    }
}

/// `frus build installer --format msix`: the program, its DLLs and its icons in an MSIX
/// package, signed.
pub(crate) fn msix(
    project: &Project,
    exe: &Path,
    extra: &[PathBuf],
    dir: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    let Some(signing) = signing(project)? else {
        return Err(format!(
            "an MSIX package installs only when it is signed, and no certificate is configured. \
             Set FRUS_WINDOWS_CERTIFICATE (a .pfx) and FRUS_WINDOWS_CERTIFICATE_PASSWORD, or \
             FRUS_WINDOWS_CERTIFICATE_THUMBPRINT, or [{SIGNING_SECTION}] in Cargo.toml; see \
             docs/guide/build-variants.md, \"Signing\". `frus build installer` (a setup program) \
             needs none"
        ));
    };
    let publisher = certificate_subject(&signing)?;
    let layout = dir.join("msix");
    let _ = std::fs::remove_dir_all(&layout);
    let assets = layout.join("Assets");
    std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
    let exe_name = exe
        .file_name()
        .ok_or("the program has no name")?
        .to_string_lossy()
        .into_owned();
    for file in std::iter::once(exe).chain(extra.iter().map(PathBuf::as_path)) {
        if let Some(name) = file.file_name() {
            std::fs::copy(file, layout.join(name))
                .map_err(|e| format!("{}: {e}", file.display()))?;
        }
    }
    // The launcher icon the other platforms use: the frus logo unless the application changed it.
    let icon = crate::installer::launcher_icon(project)
        .ok_or("no launcher icon (res/mipmap-xxxhdpi/ic_launcher.png) for the package's logos")?;
    for logo in [
        "StoreLogo.png",
        "Square150x150Logo.png",
        "Square44x44Logo.png",
    ] {
        std::fs::copy(&icon, assets.join(logo)).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        layout.join("AppxManifest.xml"),
        appx_manifest(project, &publisher, version, &exe_name),
    )
    .map_err(|e| e.to_string())?;

    let package = dir.join(format!(
        "{}-{}.msix",
        crate::installer::file_stem(project),
        project.version_name()
    ));
    let _ = std::fs::remove_file(&package);
    let makeappx = sdk_tool("makeappx.exe")?;
    // makeappx prints every file it packs, and its errors in the same stream: kept quiet, and
    // shown when it fails.
    let packed = Command::new(&makeappx)
        .args(["pack", "/o", "/d"])
        .arg(&layout)
        .arg("/p")
        .arg(&package)
        .output()
        .map_err(|e| format!("could not run {}: {e}", makeappx.display()))?;
    if !packed.status.success() {
        let text = String::from_utf8_lossy(&packed.stdout);
        let errors: Vec<&str> = text.lines().filter(|line| line.contains("error")).collect();
        return Err(format!(
            "makeappx could not make the package from {}:
{}",
            layout.display(),
            if errors.is_empty() {
                text.trim().to_string()
            } else {
                errors.join(
                    "
",
                )
            }
        ));
    }
    sign(&signing, &package)?;
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cargo::Mode;
    use crate::variant::Variant;

    fn project(extra: &str) -> Project {
        let manifest = format!(
            "[package]\nname = \"my_app\"\nversion = \"1.2.0\"\n\n\
             [package.metadata.android]\npackage = \"com.acme.my_app\"\n\n\
             [package.metadata.frus.flavors.dev]\napp_name = \"My <App> & Co\"\n\
             application_id_suffix = \".dev\"\n{extra}"
        );
        let mut project = Project::from_manifest(Path::new("C:/work/app"), &manifest).unwrap();
        Variant {
            flavor: Some("dev".into()),
            ..Variant::default()
        }
        .apply(Mode::Release, &mut project, Path::new("C:/work/app"))
        .unwrap();
        project
    }

    #[test]
    fn nothing_configured_signs_nothing() {
        assert_eq!(signing_from(&project(""), |_| None), Ok(None));
    }

    #[test]
    fn the_environment_comes_before_the_manifest() {
        let project = project(&format!(
            "\n[{SIGNING_SECTION}]\nthumbprint = \"ab:cd ef\"\n\
             timestamp_url = \"http://timestamp.example\"\npublisher = \"CN=Acme\"\n"
        ));
        let from_manifest = signing_from(&project, |_| None).unwrap().unwrap();
        assert_eq!(from_manifest.thumbprint.as_deref(), Some("ABCDEF"));
        assert_eq!(
            from_manifest.timestamp_url.as_deref(),
            Some("http://timestamp.example")
        );
        assert_eq!(from_manifest.publisher.as_deref(), Some("CN=Acme"));

        let env = |name: &str| match name {
            "FRUS_WINDOWS_CERTIFICATE_THUMBPRINT" => Some("1234".to_string()),
            "FRUS_WINDOWS_PUBLISHER" => Some("CN=Other".to_string()),
            _ => None,
        };
        let from_env = signing_from(&project, env).unwrap().unwrap();
        assert_eq!(from_env.thumbprint.as_deref(), Some("1234"));
        assert_eq!(from_env.publisher.as_deref(), Some("CN=Other"));
    }

    #[test]
    fn mistakes_are_named() {
        let password = project(&format!("\n[{SIGNING_SECTION}]\npassword = \"x\"\n"));
        assert!(signing_from(&password, |_| None)
            .unwrap_err()
            .contains("does not belong in Cargo.toml"));
        let both = project(&format!(
            "\n[{SIGNING_SECTION}]\nthumbprint = \"AB\"\ncertificate = \"x.pfx\"\n"
        ));
        assert!(signing_from(&both, |_| None)
            .unwrap_err()
            .contains("keep one"));
        let missing = project(&format!(
            "\n[{SIGNING_SECTION}]\ncertificate = \"nowhere.pfx\"\n"
        ));
        assert!(signing_from(&missing, |_| None)
            .unwrap_err()
            .contains("does not exist"));
    }

    #[test]
    fn signtool_is_told_the_key_and_the_timestamp() {
        let file = Signing {
            certificate: Some(PathBuf::from("C:/keys/code.pfx")),
            password: Some("secret".into()),
            timestamp_url: Some("http://ts.example".into()),
            ..Signing::default()
        };
        let args: Vec<String> = signtool_args(&file, Path::new("app.exe"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "sign",
                "/fd",
                "SHA256",
                "/f",
                "C:/keys/code.pfx",
                "/p",
                "secret",
                "/tr",
                "http://ts.example",
                "/td",
                "SHA256",
                "app.exe"
            ]
        );
        let store = Signing {
            thumbprint: Some("ABCD".into()),
            ..Signing::default()
        };
        let args: Vec<String> = signtool_args(&store, Path::new("app.msix"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["sign", "/fd", "SHA256", "/sha1", "ABCD", "app.msix"]);
    }

    #[test]
    fn the_package_manifest_names_the_program() {
        let project = project("");
        let manifest = appx_manifest(
            &project,
            "CN=Acme \"Test\", O=Acme",
            "1.2.0.0",
            "my_app.exe",
        );
        assert!(manifest.contains(r#"<Identity Name="com.acme.my-app.dev" Publisher="CN=Acme &quot;Test&quot;, O=Acme" Version="1.2.0.0""#), "{manifest}");
        assert!(manifest.contains("<DisplayName>My &lt;App&gt; &amp; Co</DisplayName>"));
        assert!(
            manifest.contains("<PublisherDisplayName>Acme &quot;Test&quot;</PublisherDisplayName>"),
            "{manifest}"
        );
        assert!(manifest
            .contains(r#"Executable="my_app.exe" EntryPoint="Windows.FullTrustApplication""#));
        assert!(manifest.contains(r#"<rescap:Capability Name="runFullTrust" />"#));
        // XML forbids `--` inside a comment: makeappx refuses such a manifest.
        for comment in manifest.split("<!--").skip(1) {
            let body = comment.split("-->").next().unwrap();
            assert!(!body.contains("--"), "{body}");
        }
        assert_eq!(identity_name("ab"), "ab-app");
        assert_eq!(publisher_display_name("CN=Acme Ltd, O=Acme"), "Acme Ltd");
        assert_eq!(
            publisher_display_name("CN=\"Acme, Inc\", O=Acme"),
            "Acme, Inc"
        );
    }

    #[test]
    fn sdk_versions_sort_as_numbers() {
        let mut dirs = [
            PathBuf::from("10.0.9200.0"),
            PathBuf::from("10.0.26100.0"),
            PathBuf::from("10.0.17763.0"),
        ];
        dirs.sort_by_key(|p| version_key(p));
        assert_eq!(dirs.last().unwrap(), &PathBuf::from("10.0.26100.0"));
    }
}
