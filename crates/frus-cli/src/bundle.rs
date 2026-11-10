// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The macOS application bundle** (milestone 744): the program in `Name.app/Contents/MacOS/`,
//! beside an `Info.plist` that says what it is and why it asks for what it declares.
//!
//! macOS reads the usage strings from the bundle of the running program, and ends one that asks
//! for the camera, the microphone or the location without them. A bare executable has no bundle,
//! so `frus build` and `frus run` make one on a Mac. Making it is only files: the code is the
//! same on every system, and tested on all of them.

use std::path::{Path, PathBuf};

use crate::permissions::{usage_strings, xml_escape, Declared};
use crate::project::Project;

/// The oldest macOS the bundle says it runs on: what the GPU layer needs.
const MINIMUM_MACOS: &str = "10.15";

/// The bundle's `Info.plist`, for a program called `executable`.
pub(crate) fn info_plist(project: &Project, executable: &str, declared: &[Declared]) -> String {
    // The version people see and the build's number, `--build-name` and `--build-number`
    // when given (milestone 790); the name, a flavor's when it has one.
    let version = project.version_name();
    let build = project
        .build_number()
        .map(|n| n.to_string())
        .unwrap_or_else(|| version.clone());
    let name = project
        .variant
        .flavor
        .as_ref()
        .and_then(|f| f.app_name.clone())
        .unwrap_or_else(|| project.package.clone());
    let identifier = project.bundle_identifier();
    let mut entries: Vec<(&str, Entry)> = vec![
        ("CFBundleDevelopmentRegion", Entry::Text("en")),
        ("CFBundleExecutable", Entry::Text(executable)),
        ("CFBundleIdentifier", Entry::Text(&identifier)),
        ("CFBundleInfoDictionaryVersion", Entry::Text("6.0")),
        ("CFBundleName", Entry::Text(&name)),
        ("CFBundleDisplayName", Entry::Text(&name)),
        ("CFBundlePackageType", Entry::Text("APPL")),
        ("CFBundleShortVersionString", Entry::Text(&version)),
        ("CFBundleVersion", Entry::Text(&build)),
        ("LSMinimumSystemVersion", Entry::Text(MINIMUM_MACOS)),
        ("NSPrincipalClass", Entry::Text("NSApplication")),
        ("NSHighResolutionCapable", Entry::True),
    ];
    // The application's own link scheme (milestone 791): macOS opens `myapp://…` with it.
    let scheme = project
        .deeplink
        .as_deref()
        .and_then(|link| crate::platforms::Deeplink::parse(link).ok())
        .and_then(|link| link.scheme().map(str::to_string));
    let url_types = scheme.map(|scheme| {
        format!(
            "<array>\n\t\t<dict>\n\t\t\t<key>CFBundleURLName</key>\n\t\t\t<string>{}</string>\n\
             \t\t\t<key>CFBundleURLSchemes</key>\n\t\t\t<array>\n\t\t\t\t<string>{}</string>\n\
             \t\t\t</array>\n\t\t</dict>\n\t</array>",
            xml_escape(&identifier),
            xml_escape(&scheme)
        )
    });
    if let Some(url_types) = &url_types {
        entries.push(("CFBundleURLTypes", Entry::Raw(url_types)));
    }
    for (key, reason) in usage_strings(declared) {
        entries.push((key, Entry::Text(reason)));
        // macOS before 10.14 read the location's reason from its older key; the shell accepts
        // either.
        if key == "NSLocationWhenInUseUsageDescription" {
            entries.push(("NSLocationUsageDescription", Entry::Text(reason)));
        }
    }
    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    for (key, entry) in &entries {
        plist.push_str(&format!("\t<key>{}</key>\n", xml_escape(key)));
        match entry {
            Entry::Text(text) => {
                plist.push_str(&format!("\t<string>{}</string>\n", xml_escape(text)))
            }
            Entry::True => plist.push_str("\t<true/>\n"),
            Entry::Raw(xml) => plist.push_str(&format!("\t{xml}\n")),
        }
    }
    plist.push_str("</dict>\n</plist>\n");
    plist
}

enum Entry<'a> {
    Text(&'a str),
    True,
    /// A value written as XML already: an array.
    Raw(&'a str),
}

/// Makes `dir/name.app` around a copy of `program`, replacing one already there, and returns
/// the path of the program inside it — the one to start, for macOS to find its bundle.
pub(crate) fn make_app(
    dir: &Path,
    name: &str,
    program: &Path,
    project: &Project,
    declared: &[Declared],
) -> Result<PathBuf, String> {
    let app = dir.join(format!("{name}.app"));
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|e| format!("could not replace {}: {e}", app.display()))?;
    }
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos)
        .map_err(|e| format!("could not make {}: {e}", macos.display()))?;
    let executable = program
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.package.clone());
    let inside = macos.join(&executable);
    std::fs::copy(program, &inside)
        .map_err(|e| format!("could not copy the program into {}: {e}", app.display()))?;
    std::fs::write(
        contents.join("Info.plist"),
        info_plist(project, &executable, declared),
    )
    .map_err(|e| format!("could not write the Info.plist of {}: {e}", app.display()))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????")
        .map_err(|e| format!("could not write the PkgInfo of {}: {e}", app.display()))?;
    seal(&app);
    Ok(inside)
}

/// Signs the bundle for this machine (an ad-hoc signature), so that the system takes the
/// `Info.plist` as the program's own. Only on a Mac, and a failure is said, not fatal: an
/// unsigned bundle still runs where it was built.
fn seal(app: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let signed = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(app)
        .output();
    match signed {
        Ok(output) if output.status.success() => {}
        Ok(output) => crate::report::warn(&format!(
            "codesign could not sign {}: {}",
            app.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => crate::report::warn(&format!(
            "codesign could not be run to sign {}: {error}",
            app.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::parse;

    fn project() -> Project {
        let manifest = "[package]\nname = \"tides\"\nversion = \"1.2.0\"\n\n\
            [package.metadata.android]\npackage = \"com.acme.tides_app\"\n\n\
            [package.metadata.frus.permissions]\n\
            camera = \"Scans <codes> & \\\"tickets\\\"\"\n\
            location = 'Shows the tide where you are'\n\
            notifications = true\n";
        Project::from_manifest(Path::new("."), manifest).unwrap()
    }

    /// **A flavor and a version reach the plist** (milestone 790): the flavor's name is the
    /// bundle's, `--build-name` its short version, `--build-number` its build, the id suffix
    /// its identifier's.
    #[test]
    fn the_info_plist_carries_the_variant() {
        let manifest = "[package]
name = \"tides\"
version = \"1.2.0\"

            [package.metadata.android]
package = \"com.acme.tides_app\"

            [package.metadata.frus.flavors.dev]
app_name = \"Tides Dev\"
            application_id_suffix = \".dev\"
";
        let mut project = Project::from_manifest(Path::new("."), manifest).unwrap();
        crate::variant::Variant {
            flavor: Some("dev".into()),
            build_name: Some("2.5.0".into()),
            build_number: Some(77),
            ..crate::variant::Variant::default()
        }
        .apply(crate::cargo::Mode::Release, &mut project, Path::new("."))
        .unwrap();
        let plist = info_plist(&project, "tides", &[]);
        for entry in [
            "	<key>CFBundleName</key>
	<string>Tides Dev</string>
",
            "	<key>CFBundleDisplayName</key>
	<string>Tides Dev</string>
",
            "	<key>CFBundleShortVersionString</key>
	<string>2.5.0</string>
",
            "	<key>CFBundleVersion</key>
	<string>77</string>
",
        ] {
            assert!(
                plist.contains(entry),
                "{entry} missing from
{plist}"
            );
        }
        assert!(
            plist.contains(&format!(
                "<key>CFBundleIdentifier</key>
	<string>{}</string>",
                project.bundle_identifier()
            )),
            "{plist}"
        );
        assert!(project.bundle_identifier().ends_with(".dev"), "{plist}");
    }

    /// **The usage strings are in the plist, escaped**: a reason with `<`, `&` or quotes must
    /// not make the file one macOS cannot read — it would then find no reason and end the
    /// application at the question.
    #[test]
    fn the_info_plist_says_why() {
        let project = project();
        let plist = info_plist(&project, "tides", &project.permissions().unwrap());
        assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist"));
        for entry in [
            "\t<key>CFBundleExecutable</key>\n\t<string>tides</string>\n",
            "\t<key>CFBundleIdentifier</key>\n\t<string>com.acme.tides-app</string>\n",
            "\t<key>CFBundleShortVersionString</key>\n\t<string>1.2.0</string>\n",
            "\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n",
            "\t<key>NSHighResolutionCapable</key>\n\t<true/>\n",
            "\t<key>NSCameraUsageDescription</key>\n\
             \t<string>Scans &lt;codes&gt; &amp; &quot;tickets&quot;</string>\n",
            "\t<key>NSLocationWhenInUseUsageDescription</key>\n\
             \t<string>Shows the tide where you are</string>\n",
            "\t<key>NSLocationUsageDescription</key>\n\
             \t<string>Shows the tide where you are</string>\n",
        ] {
            assert!(plist.contains(entry), "{entry} missing from\n{plist}");
        }
        assert!(!plist.contains("NSMicrophoneUsageDescription"));
        assert!(plist.ends_with("</dict>\n</plist>\n"));
        // Well formed as far as a reader cares: every element closed, nothing raw.
        assert_eq!(
            plist.matches("<key>").count(),
            plist.matches("</key>").count()
        );
        assert_eq!(
            plist.matches("<string>").count(),
            plist.matches("</string>").count()
        );
        assert!(!plist.contains("& "), "{plist}");

        // Nothing declared: no usage string, and the bundle is still whole.
        let none = info_plist(&project, "tides", &parse(&[]).unwrap());
        assert!(!none.contains("UsageDescription"));
        assert!(none.contains("CFBundleExecutable"));
        assert!(!none.contains("CFBundleURLTypes"));

        // The application's own links (milestone 791): macOS hands it `tides://…`.
        let mut linked = project.clone();
        linked.deeplink = Some("tides".into());
        let plist = info_plist(&linked, "tides", &[]);
        assert!(
            plist.contains(
                "<key>CFBundleURLSchemes</key>
			<array>
				<string>tides</string>"
            ),
            "{plist}"
        );
        assert_eq!(
            plist.matches("<dict>").count(),
            plist.matches("</dict>").count()
        );
        assert_eq!(
            plist.matches("<array>").count(),
            plist.matches("</array>").count()
        );
        // A host's https links are not a scheme of the application's.
        linked.deeplink = Some("https://tides.example".into());
        assert!(!info_plist(&linked, "tides", &[]).contains("CFBundleURLTypes"));
    }

    /// **The bundle is laid out as macOS reads it**, made again over an old one, on any system.
    #[test]
    fn the_app_is_made_around_the_program() {
        let project = project();
        let dir = std::env::temp_dir().join(format!("frus-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("tides");
        std::fs::write(&program, b"program").unwrap();
        let declared = project.permissions().unwrap();
        let inside = make_app(&dir, "Tides", &program, &project, &declared).unwrap();
        assert_eq!(
            inside,
            dir.join("Tides.app")
                .join("Contents")
                .join("MacOS")
                .join("tides")
        );
        assert_eq!(std::fs::read(&inside).unwrap(), b"program");
        let contents = dir.join("Tides.app").join("Contents");
        let plist = std::fs::read_to_string(contents.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>tides</string>"));
        assert!(plist.contains("NSCameraUsageDescription"));
        assert_eq!(
            std::fs::read_to_string(contents.join("PkgInfo")).unwrap(),
            "APPL????"
        );

        // Made again: what the old one held is gone.
        std::fs::write(contents.join("stale"), b"x").unwrap();
        make_app(&dir, "Tides", &program, &project, &[]).unwrap();
        assert!(!contents.join("stale").exists());
        let plist = std::fs::read_to_string(contents.join("Info.plist")).unwrap();
        assert!(!plist.contains("UsageDescription"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
