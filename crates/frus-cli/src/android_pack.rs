// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Android packages cargo-apk does not make (milestone 790): an APK with another id,
//! label, version or resources (a flavor, `--build-name`, `--build-number`), one APK per
//! processor (`frus build apk --split-per-abi`), and the App Bundle a store takes
//! (`frus build appbundle`).
//!
//! cargo-apk builds first, as for any APK: it compiles the library for each processor and
//! leaves, beside its APK, the manifest it wrote and the libraries it packaged. They are
//! packaged again here with the SDK's own tools — `aapt2` links the manifest and the resources
//! (in the protocol-buffer form a bundle holds, for one), `aapt` adds the libraries, `zipalign`
//! and `apksigner` finish an APK, and Google's `bundletool` makes the bundle, which `jarsigner`
//! signs. bundletool is fetched the first time (about 30 MB); the JDK it runs on is the one
//! the Android build uses already.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cargo::{self, Mode};
use crate::project::Project;

/// The bundletool release fetched into `~/.frus/tools`.
const BUNDLETOOL_VERSION: &str = "1.18.3";

/// A keystore and its password (the key's is the same, as cargo-apk has it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    pub path: PathBuf,
    pub password: String,
}

/// The key a package of `mode` is signed with, found as cargo-apk finds it: the
/// `CARGO_APK_<PROFILE>_KEYSTORE` and `…_PASSWORD` variables, then the manifest's
/// `[package.metadata.android.signing.<profile>]`. A debug build, and a profile build, fall
/// back to the debug key; so does a release `frus run` installs (`for_device`). A release
/// `frus build` does not: what it makes is published, and must carry the developer's key.
pub(crate) fn signing_key(project: &Project, mode: Mode, for_device: bool) -> Result<Key, String> {
    let profile = mode.cargo_profile();
    let variable = format!(
        "CARGO_APK_{}_KEYSTORE",
        profile.to_uppercase().replace('-', "_")
    );
    if let Some(path) = std::env::var_os(&variable) {
        let password = std::env::var(format!("{variable}_PASSWORD")).map_err(|_| {
            format!("{variable} is set, but not {variable}_PASSWORD: both or neither")
        })?;
        return Ok(Key {
            path: PathBuf::from(path),
            password,
        });
    }
    if let Some((_, path, password)) = project.signing.iter().find(|(p, _, _)| p == profile) {
        return Ok(Key {
            path: project.root.join(path),
            password: password.clone(),
        });
    }
    if mode != Mode::Release || for_device {
        return Ok(Key {
            path: debug_keystore()?,
            password: "android".into(),
        });
    }
    Err(format!(
        "a release is signed with your own key, and {} names none: set {variable} and \
         {variable}_PASSWORD, or add [package.metadata.android.signing.release] with `path` \
         and `keystore_password` to Cargo.toml (see docs/guide/build-variants.md, \"Signing\"). \
         `frus build apk --profile` signs with the debug key, to try a package on a device",
        project.package
    ))
}

/// The SDK's debug key, `~/.android/debug.keystore`, made with keytool as Android's own tools
/// make it when there is none yet.
pub(crate) fn debug_keystore() -> Result<PathBuf, String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("no home directory for the debug key")?;
    let path = PathBuf::from(home).join(".android").join("debug.keystore");
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    cargo::run(
        Command::new(jdk_tool("keytool")?)
            .args(["-genkeypair", "-keystore"])
            .arg(&path)
            .args([
                "-storepass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keypass",
                "android",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
            ]),
    )?;
    Ok(path)
}

/// A program of the JDK the Android build runs on.
fn jdk_tool(name: &str) -> Result<PathBuf, String> {
    let home = crate::setup::java()?;
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    Ok(home.join("bin").join(exe))
}

/// The SDK's packaging tools, from its newest build-tools.
struct Tools {
    aapt: PathBuf,
    aapt2: PathBuf,
    zipalign: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
}

fn tools(project: &Project) -> Result<Tools, String> {
    let home = crate::setup::android_sdk(project.android_target_sdk)?;
    let build_tools = crate::setup::newest_version_dir(&home.join("build-tools"))
        .ok_or("the Android SDK has no build-tools")?;
    let exe = |name: &str| {
        build_tools.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        })
    };
    let tools = Tools {
        aapt: exe("aapt"),
        aapt2: exe("aapt2"),
        zipalign: exe("zipalign"),
        apksigner: build_tools.join(if cfg!(windows) {
            "apksigner.bat"
        } else {
            "apksigner"
        }),
        android_jar: home
            .join("platforms")
            .join(format!("android-{}", project.android_target_sdk))
            .join("android.jar"),
    };
    for tool in [
        &tools.aapt,
        &tools.aapt2,
        &tools.zipalign,
        &tools.apksigner,
        &tools.android_jar,
    ] {
        if !tool.is_file() {
            return Err(format!("the Android SDK lacks {}", tool.display()));
        }
    }
    Ok(tools)
}

/// Android's name for a Rust target's processor.
pub(crate) fn abi_of(target: &str) -> Option<&'static str> {
    Some(match target {
        "aarch64-linux-android" => "arm64-v8a",
        "armv7-linux-androideabi" => "armeabi-v7a",
        "x86_64-linux-android" => "x86_64",
        "i686-linux-android" => "x86",
        _ => return None,
    })
}

/// What a processor adds to the version code of its own APK, `code * 1000 + version code`, as
/// the leading toolkit numbers them: a device given the choice installs the 64-bit one.
fn abi_code(abi: &str) -> u32 {
    match abi {
        "armeabi-v7a" => 1,
        "arm64-v8a" => 2,
        "x86" => 3,
        _ => 4,
    }
}

/// cargo-apk's manifest with what the variant changes: the application id, the version name
/// and code, and the label.
pub(crate) fn edit_manifest(xml: &str, project: &Project, version_code: u32) -> String {
    let mut xml = set_attribute(xml, "<manifest", "package", &project.android_package);
    xml = set_attribute(
        &xml,
        "<manifest",
        "android:versionCode",
        &version_code.to_string(),
    );
    xml = set_attribute(
        &xml,
        "<manifest",
        "android:versionName",
        &project.version_name(),
    );
    if let Some(name) = project
        .variant
        .flavor
        .as_ref()
        .and_then(|f| f.app_name.as_deref())
    {
        xml = set_attribute(&xml, "<application", "android:label", name);
    }
    xml
}

/// `name="value"` on the first `element` tag: replaced where it is, added where it is not.
pub(crate) fn set_attribute(xml: &str, element: &str, name: &str, value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let Some(start) = xml
        .find(&format!("{element} "))
        .or_else(|| xml.find(element))
    else {
        return xml.to_string();
    };
    let end = start + xml[start..].find('>').unwrap_or(xml.len() - start);
    let tag = &xml[start..end];
    let needle = format!(" {name}=\"");
    let new_tag = match tag.find(&needle) {
        Some(at) => {
            let value_start = at + needle.len();
            let value_end = value_start + tag[value_start..].find('"').unwrap_or(0);
            format!("{}{escaped}{}", &tag[..value_start], &tag[value_end..])
        }
        None => {
            let (body, closing) = match tag.strip_suffix('/') {
                Some(body) => (body, "/"),
                None => (tag, ""),
            };
            format!("{body} {name}=\"{escaped}\"{closing}")
        }
    };
    format!("{}{new_tag}{}", &xml[..start], &xml[end..])
}

/// What cargo-apk left in `built` (the directory of its APK), and the work directory beside it.
struct Built<'a> {
    project: &'a Project,
    mode: Mode,
    /// cargo-apk's directory: `AndroidManifest.xml`, `lib/<abi>/*.so`.
    dir: PathBuf,
    /// The processors packaged.
    abis: Vec<&'static str>,
    /// The libraries the package holds, `lib/<abi>/<name>.so`: the folder beside it may also
    /// hold another application's, from a target directory they share.
    packaged: Vec<String>,
    tools: Tools,
    key: Key,
    work: PathBuf,
    /// The resources, compiled: the application's, and the flavor's laid over them.
    resources: Option<PathBuf>,
    overlay: Option<PathBuf>,
}

impl<'a> Built<'a> {
    fn new(
        project: &'a Project,
        mode: Mode,
        apk: &Path,
        targets: &[String],
        for_device: bool,
    ) -> Result<Self, String> {
        let dir = apk
            .parent()
            .ok_or("the APK has no directory")?
            .to_path_buf();
        let abis = targets
            .iter()
            .map(|t| abi_of(t).ok_or_else(|| format!("{t} is not an Android target")))
            .collect::<Result<Vec<_>, _>>()?;
        let packaged = packaged_libraries(apk)?;
        let tools = tools(project)?;
        let key = signing_key(project, mode, for_device)?;
        let work = dir.join("frus");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let mut built = Built {
            project,
            mode,
            dir,
            abis,
            packaged,
            tools,
            key,
            work,
            resources: None,
            overlay: None,
        };
        if let Some(res) = &project.android_resources {
            built.resources = Some(built.compile(&project.root.join(res), "resources.zip")?);
        }
        if let Some(res) = project
            .variant
            .flavor
            .as_ref()
            .and_then(|f| f.resources.as_ref())
        {
            let dir = project.root.join(res);
            if !dir.is_dir() {
                return Err(format!(
                    "the flavor's resources, {}, are not a directory",
                    dir.display()
                ));
            }
            built.overlay = Some(built.compile(&dir, "flavor-resources.zip")?);
        }
        Ok(built)
    }

    fn compile(&self, dir: &Path, name: &str) -> Result<PathBuf, String> {
        let out = self.work.join(name);
        cargo::run(
            Command::new(&self.tools.aapt2)
                .args(["compile", "--dir"])
                .arg(dir)
                .arg("-o")
                .arg(&out),
        )?;
        Ok(out)
    }

    /// The manifest for one package, written into `dir`.
    fn manifest(&self, dir: &Path, version_code: u32) -> Result<PathBuf, String> {
        let written = self.dir.join("AndroidManifest.xml");
        let xml = std::fs::read_to_string(&written)
            .map_err(|e| format!("cargo-apk's manifest, {}: {e}", written.display()))?;
        let path = dir.join("AndroidManifest.xml");
        std::fs::write(&path, edit_manifest(&xml, self.project, version_code))
            .map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// Whether the manifest names frus's activity, whose code the package must then carry: left
    /// beside cargo-apk's package as `classes.dex` (`app_links::finish_package`), and written
    /// there again in case it is not.
    fn has_code(&self) -> Result<bool, String> {
        let written = self.dir.join("AndroidManifest.xml");
        let xml = std::fs::read_to_string(&written).map_err(|e| e.to_string())?;
        if !crate::android_activity::uses_frus_activity(&xml) {
            return Ok(false);
        }
        std::fs::write(self.dir.join("classes.dex"), crate::android_activity::DEX)
            .map_err(|e| format!("classes.dex: {e}"))?;
        Ok(true)
    }

    /// `aapt2 link`: the manifest and the resources, as an APK or (`proto`) as a bundle's.
    fn link(&self, manifest: &Path, out: &Path, proto: bool) -> Result<(), String> {
        let mut link = Command::new(&self.tools.aapt2);
        link.arg("link")
            .arg("-o")
            .arg(out)
            .arg("-I")
            .arg(&self.tools.android_jar)
            .arg("--manifest")
            .arg(manifest);
        if proto {
            link.arg("--proto-format");
        } else if let Some(assets) = &self.project.android_assets {
            link.arg("-A").arg(self.project.root.join(assets));
        }
        if let Some(res) = &self.resources {
            link.arg(res);
        }
        if let Some(overlay) = &self.overlay {
            link.arg("--auto-add-overlay").arg("-R").arg(overlay);
        }
        cargo::run(&mut link)
    }

    /// The libraries of `abi`, as the paths inside the package: `lib/arm64-v8a/libapp.so`.
    fn libraries(&self, abi: &str) -> Result<Vec<String>, String> {
        let prefix = format!("lib/{abi}/");
        let mut found: Vec<String> = self
            .packaged
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect();
        found.sort();
        if found.is_empty() {
            return Err(format!("cargo-apk's package has no library for {abi}"));
        }
        Ok(found)
    }

    /// One APK with the libraries of `abis`, signed.
    fn apk(&self, name: &str, abis: &[&str], version_code: u32) -> Result<PathBuf, String> {
        let dir = self.work.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let manifest = self.manifest(&dir, version_code)?;
        let unaligned = dir.join("unaligned.apk");
        self.link(&manifest, &unaligned, false)?;
        let mut add = Command::new(&self.tools.aapt);
        add.arg("add");
        // As cargo-apk does: a debug build's package is not compressed, to be quicker to make.
        if self.mode == Mode::Debug {
            add.args(["-0", ""]);
        }
        add.arg(&unaligned).current_dir(&self.dir);
        for abi in abis {
            add.args(self.libraries(abi)?);
        }
        // frus's activity, which the manifest names (milestone 801): its code, at the root.
        if self.has_code()? {
            add.arg("classes.dex");
        }
        add.stdout(std::process::Stdio::null());
        cargo::run(&mut add)?;
        let apk = self.dir.join(format!("{name}.apk"));
        cargo::run(
            Command::new(&self.tools.zipalign)
                .args(["-f", "4"])
                .arg(&unaligned)
                .arg(&apk),
        )?;
        cargo::run(
            Command::new(&self.tools.apksigner)
                .args(["sign", "--ks"])
                .arg(&self.key.path)
                .arg("--ks-pass")
                .arg(format!("pass:{}", self.key.password))
                .arg(&apk),
        )?;
        Ok(apk)
    }
}

/// The libraries `apk` holds (`lib/<abi>/<name>.so`): what is packaged again. Not the `lib/`
/// folder cargo-apk leaves beside it, which keeps every application's built into the same
/// target directory — milestone 801 found a new application's package carrying the example
/// built before it.
pub(crate) fn packaged_libraries(apk: &Path) -> Result<Vec<String>, String> {
    let libraries: Vec<String> = zip::names(apk)?
        .into_iter()
        .filter(|name| name.starts_with("lib/") && name.ends_with(".so"))
        .collect();
    if libraries.is_empty() {
        return Err(format!("{} holds no library", apk.display()));
    }
    Ok(libraries)
}

/// The name a package of this build is given: the library's, the flavor's, the processor's.
fn package_name(project: &Project, apk: &Path, abi: Option<&str>) -> String {
    let mut name = apk
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.lib.clone());
    if let Some(flavor) = &project.variant.flavor {
        name.push('-');
        name.push_str(&flavor.name);
    }
    if let Some(abi) = abi {
        name.push('-');
        name.push_str(abi);
    }
    name
}

/// cargo-apk's `apk` packaged again with the variant's id, label, version and resources, for
/// `targets`: the APK to install or ship.
pub(crate) fn repackage(
    project: &Project,
    mode: Mode,
    apk: &Path,
    targets: &[String],
    for_device: bool,
) -> Result<PathBuf, String> {
    let built = Built::new(project, mode, apk, targets, for_device)?;
    let abis = built.abis.clone();
    built.apk(
        &package_name(project, apk, None),
        &abis,
        project.version_code(),
    )
}

/// One APK per processor (`--split-per-abi`), each with its own version code.
pub(crate) fn split_per_abi(
    project: &Project,
    mode: Mode,
    apk: &Path,
) -> Result<Vec<PathBuf>, String> {
    let built = Built::new(project, mode, apk, &project.android_targets, false)?;
    let base = project.version_code();
    built
        .abis
        .iter()
        .map(|abi| {
            let code = abi_code(abi) * 1000 + base;
            built.apk(&package_name(project, apk, Some(abi)), &[abi], code)
        })
        .collect()
}

/// The Android App Bundle (`.aab`) a store takes, signed with the release key.
pub(crate) fn app_bundle(project: &Project, mode: Mode, apk: &Path) -> Result<PathBuf, String> {
    let built = Built::new(project, mode, apk, &project.android_targets, false)?;
    let dir = built.work.join("bundle");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let manifest = built.manifest(&dir, project.version_code())?;
    let linked_apk = dir.join("linked.apk");
    built.link(&manifest, &linked_apk, true)?;
    // Unpacked with the JDK's `jar`, which bundletool needs anyway: aapt2 compresses the
    // manifest whatever it is told, and its `--output-to-dir` fails to make the resource
    // directories on Windows.
    let linked = dir.join("linked");
    if linked.exists() {
        std::fs::remove_dir_all(&linked).map_err(|e| format!("{}: {e}", linked.display()))?;
    }
    std::fs::create_dir_all(&linked).map_err(|e| format!("{}: {e}", linked.display()))?;
    cargo::run(
        Command::new(jdk_tool("jar")?)
            .arg("xf")
            .arg(&linked_apk)
            .current_dir(&linked),
    )?;

    // The base module: the manifest under manifest/, the resource table and the resources as
    // aapt2 wrote them, the libraries under lib/, the assets under assets/.
    let mut entries = Vec::new();
    for path in files_below(&linked) {
        let name = relative_name(&linked, &path);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = match name.as_str() {
            "AndroidManifest.xml" => "manifest/AndroidManifest.xml".to_string(),
            "resources.pb" => name,
            other if other.starts_with("res/") => name,
            other => format!("root/{other}"),
        };
        entries.push((name, bytes));
    }
    for abi in &built.abis {
        for lib in built.libraries(abi)? {
            let bytes = std::fs::read(built.dir.join(&lib)).map_err(|e| e.to_string())?;
            entries.push((lib, bytes));
        }
    }
    if built.has_code()? {
        entries.push((
            "dex/classes.dex".to_string(),
            crate::android_activity::DEX.to_vec(),
        ));
    }
    if let Some(assets) = &project.android_assets {
        let root = project.root.join(assets);
        for path in files_below(&root) {
            let name = relative_name(&root, &path);
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            entries.push((format!("assets/{name}"), bytes));
        }
    }
    let module = dir.join("base.zip");
    zip::write_stored(&module, &entries)?;

    let bundle = built
        .dir
        .join(format!("{}.aab", package_name(project, apk, None)));
    let _ = std::fs::remove_file(&bundle);
    cargo::run(
        Command::new(jdk_tool("java")?)
            .arg("-jar")
            .arg(bundletool()?)
            .arg("build-bundle")
            .arg(format!("--modules={}", module.display()))
            .arg(format!("--output={}", bundle.display())),
    )?;
    sign_bundle(&bundle, &built.key)?;
    Ok(bundle)
}

/// A bundle is signed as a JAR is: with jarsigner, under the key's alias.
fn sign_bundle(bundle: &Path, key: &Key) -> Result<(), String> {
    let list = Command::new(jdk_tool("keytool")?)
        .args(["-J-Duser.language=en", "-list", "-keystore"])
        .arg(&key.path)
        .arg("-storepass")
        .arg(&key.password)
        .output()
        .map_err(|e| format!("could not run keytool: {e}"))?;
    let listing = String::from_utf8_lossy(&list.stdout);
    let alias = key_alias(&listing).ok_or_else(|| {
        format!(
            "{} holds no private key keytool can list:\n{listing}{}",
            key.path.display(),
            String::from_utf8_lossy(&list.stderr)
        )
    })?;
    cargo::run(
        Command::new(jdk_tool("jarsigner")?)
            .args([
                "-sigalg",
                "SHA256withRSA",
                "-digestalg",
                "SHA-256",
                "-keystore",
            ])
            .arg(&key.path)
            .arg("-storepass")
            .arg(&key.password)
            .arg("-keypass")
            .arg(&key.password)
            .arg(bundle)
            .arg(alias)
            .stdout(std::process::Stdio::null()),
    )
}

/// The alias of the first private key in `keytool -list`'s output.
fn key_alias(listing: &str) -> Option<String> {
    listing
        .lines()
        .find(|line| line.contains("PrivateKeyEntry"))
        .and_then(|line| line.split(", ").next())
        .map(str::to_string)
}

/// Google's bundletool, fetched into `~/.frus/tools` the first time.
pub(crate) fn bundletool() -> Result<PathBuf, String> {
    let jar = crate::setup::frus_home()
        .join("tools")
        .join(format!("bundletool-all-{BUNDLETOOL_VERSION}.jar"));
    if jar.is_file() {
        return Ok(jar);
    }
    crate::report::say(&format!(
        "Fetching bundletool {BUNDLETOOL_VERSION}, which makes Android App Bundles (about 30 MB, \
         once)…"
    ));
    let partial = jar.with_extension("part");
    crate::setup::download(
        &format!(
            "https://github.com/google/bundletool/releases/download/{BUNDLETOOL_VERSION}/\
             bundletool-all-{BUNDLETOOL_VERSION}.jar"
        ),
        &partial,
    )?;
    std::fs::rename(&partial, &jar).map_err(|e| e.to_string())?;
    Ok(jar)
}

fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// `path` below `root`, with `/` between its parts, as an archive names it.
fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Just enough of the zip format for a bundle's module: an archive whose entries are stored, not
/// compressed, which bundletool reads.
pub(crate) mod zip {
    use std::path::Path;

    /// An entry: its name in the archive, and its bytes.
    pub(crate) type Entry = (String, Vec<u8>);

    const LOCAL: u32 = 0x0403_4b50;
    const CENTRAL: u32 = 0x0201_4b50;
    const END: u32 = 0x0605_4b50;

    pub(crate) fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn u16_at(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    }

    /// The names of the entries of `archive`, compressed or not, from its central directory.
    pub(crate) fn names(archive: &Path) -> Result<Vec<String>, String> {
        let bytes = std::fs::read(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        names_in(&bytes)
            .ok_or_else(|| format!("{} is not an archive frus can read", archive.display()))
    }

    pub(crate) fn names_in(b: &[u8]) -> Option<Vec<String>> {
        let end = (0..=b.len().checked_sub(22)?)
            .rev()
            .find(|&at| u32_at(b, at) == Some(END))?;
        let count = u16_at(b, end + 10)? as usize;
        let mut at = u32_at(b, end + 16)? as usize;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(b, at)? != CENTRAL {
                return None;
            }
            let name_len = u16_at(b, at + 28)? as usize;
            let extra_len = u16_at(b, at + 30)? as usize;
            let comment_len = u16_at(b, at + 32)? as usize;
            names.push(String::from_utf8_lossy(b.get(at + 46..at + 46 + name_len)?).into_owned());
            at += 46 + name_len + extra_len + comment_len;
        }
        Some(names)
    }

    /// Every entry of `archive`, which must all be stored: what the tests read back.
    #[cfg(test)]
    pub(crate) fn read_stored(archive: &Path) -> Result<Vec<Entry>, String> {
        let bytes = std::fs::read(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        parse(&bytes)
            .ok_or_else(|| format!("{} is not an archive frus can read", archive.display()))?
    }

    #[cfg(test)]
    fn parse(b: &[u8]) -> Option<Result<Vec<Entry>, String>> {
        let end = (0..=b.len().checked_sub(22)?)
            .rev()
            .find(|&at| u32_at(b, at) == Some(END))?;
        let count = u16_at(b, end + 10)? as usize;
        let mut at = u32_at(b, end + 16)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(b, at)? != CENTRAL {
                return None;
            }
            let method = u16_at(b, at + 10)?;
            let size = u32_at(b, at + 20)? as usize;
            let name_len = u16_at(b, at + 28)? as usize;
            let extra_len = u16_at(b, at + 30)? as usize;
            let comment_len = u16_at(b, at + 32)? as usize;
            let local = u32_at(b, at + 42)? as usize;
            let name = String::from_utf8_lossy(b.get(at + 46..at + 46 + name_len)?).into_owned();
            at += 46 + name_len + extra_len + comment_len;
            if method != 0 {
                return Some(Err(format!("{name} is compressed (method {method})")));
            }
            if u32_at(b, local)? != LOCAL {
                return None;
            }
            let data =
                local + 30 + u16_at(b, local + 26)? as usize + u16_at(b, local + 28)? as usize;
            entries.push((name, b.get(data..data + size)?.to_vec()));
        }
        Some(Ok(entries))
    }

    /// `entries` as an archive at `path`, each stored.
    pub(crate) fn write_stored(path: &Path, entries: &[Entry]) -> Result<(), String> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = u32::try_from(out.len()).map_err(|_| "the archive is over 4 GB")?;
            let size = u32::try_from(data.len()).map_err(|_| format!("{name} is over 4 GB"))?;
            let crc = crc32(data);
            let header = |signature: u32, central: bool| {
                let mut h = Vec::new();
                h.extend(signature.to_le_bytes());
                if central {
                    h.extend(20u16.to_le_bytes()); // made by
                }
                h.extend(20u16.to_le_bytes()); // needed to extract
                h.extend(0x0800u16.to_le_bytes()); // names are UTF-8
                h.extend(0u16.to_le_bytes()); // stored
                h.extend(0u32.to_le_bytes()); // time and date
                h.extend(crc.to_le_bytes());
                h.extend(size.to_le_bytes());
                h.extend(size.to_le_bytes());
                h.extend((name.len() as u16).to_le_bytes());
                h.extend(0u16.to_le_bytes()); // extra
                if central {
                    h.extend(0u16.to_le_bytes()); // comment
                    h.extend(0u16.to_le_bytes()); // disk
                    h.extend(0u16.to_le_bytes()); // internal attributes
                    h.extend(0u32.to_le_bytes()); // external attributes
                    h.extend(offset.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            out.extend(header(LOCAL, false));
            out.extend(data);
            central.extend(header(CENTRAL, true));
        }
        let central_at = out.len() as u32;
        out.extend(&central);
        out.extend(END.to_le_bytes());
        out.extend(0u32.to_le_bytes()); // disks
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(central_at.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Variant;

    const WRITTEN: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.acme.app" android:versionCode="16842752" android:versionName="1.0.0"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="34"/><application android:debuggable="false" android:theme="@style/LaunchTheme" android:hasCode="false" android:icon="@mipmap/ic_launcher" android:label="app"><activity android:name="android.app.NativeActivity" android:label="kept"/></application></manifest>"#;

    fn project(flags: &str) -> Project {
        let manifest = "[package]\nname = \"app\"\nversion = \"1.0.0\"\n\n\
             [package.metadata.android]\npackage = \"com.acme.app\"\n\n\
             [package.metadata.frus.flavors.dev]\napp_name = \"App & Dev\"\n\
             application_id_suffix = \".dev\"\n";
        let mut project = Project::from_manifest(Path::new("."), manifest).unwrap();
        let mut variant = Variant::default();
        let mut mode = None;
        let mut words = flags.split_whitespace().map(str::to_string);
        while let Some(flag) = words.next() {
            let mut value = |_: &str| Ok(words.next().unwrap());
            assert!(
                crate::variant::parse_flag(&flag, false, &mut value, &mut variant, &mut mode)
                    .unwrap()
            );
        }
        variant
            .apply(Mode::Release, &mut project, Path::new("."))
            .unwrap();
        project
    }

    #[test]
    fn the_manifest_says_the_variant() {
        let project = project("--flavor dev --build-name 2.1.0 --build-number 42");
        let xml = edit_manifest(WRITTEN, &project, project.version_code());
        assert!(xml.contains(r#" package="com.acme.app.dev""#), "{xml}");
        assert!(xml.contains(r#"android:versionCode="42""#), "{xml}");
        assert!(xml.contains(r#"android:versionName="2.1.0""#), "{xml}");
        assert!(xml.contains(r#"android:label="App &amp; Dev""#), "{xml}");
        // The activity's own label is not the application's.
        assert!(xml.contains(r#"android:label="kept""#), "{xml}");
        assert_eq!(xml.matches("android:versionCode").count(), 1);

        // Without a flavor, the label stays; without a number, cargo-apk's code is derived
        // from the name given.
        let plain = self::project("--build-name 3.0.1");
        let xml = edit_manifest(WRITTEN, &plain, plain.version_code());
        assert!(xml.contains(r#"android:label="app""#), "{xml}");
        assert!(xml.contains(r#" package="com.acme.app""#), "{xml}");
        assert!(xml.contains(&format!(
            "android:versionCode=\"{}\"",
            (1 << 24) | (3 << 16) | 1
        )));
    }

    #[test]
    fn a_missing_attribute_is_added() {
        assert_eq!(
            set_attribute(
                "<a><application/></a>",
                "<application",
                "android:label",
                "x"
            ),
            r#"<a><application android:label="x"/></a>"#
        );
        assert_eq!(
            set_attribute(r#"<manifest a="1">"#, "<manifest", "package", "p"),
            r#"<manifest a="1" package="p">"#
        );
    }

    #[test]
    fn each_processor_numbers_its_own_package() {
        assert_eq!(abi_of("aarch64-linux-android"), Some("arm64-v8a"));
        assert_eq!(abi_of("armv7-linux-androideabi"), Some("armeabi-v7a"));
        assert_eq!(abi_of("x86_64-linux-android"), Some("x86_64"));
        assert_eq!(abi_of("wasm32-unknown-unknown"), None);
        // A device that runs both picks the 64-bit package: its code is higher.
        assert!(abi_code("arm64-v8a") > abi_code("armeabi-v7a"));
        assert!(abi_code("x86_64") > abi_code("x86"));
    }

    #[test]
    fn packages_are_named_after_the_flavor_and_the_processor() {
        let plain = project("");
        let apk = Path::new("target/release/apk/app.apk");
        assert_eq!(package_name(&plain, apk, None), "app");
        let dev = project("--flavor dev");
        assert_eq!(
            package_name(&dev, apk, Some("arm64-v8a")),
            "app-dev-arm64-v8a"
        );
    }

    #[test]
    fn the_alias_is_read_from_keytool() {
        let listing = "Keystore type: PKCS12\nKeystore provider: SUN\n\nYour keystore contains 1 entry\n\n\
                       upload, Nov 9, 2026, PrivateKeyEntry, \nCertificate fingerprint (SHA-256): AB:CD\n";
        assert_eq!(key_alias(listing).as_deref(), Some("upload"));
        assert_eq!(key_alias("nothing"), None);
    }

    #[test]
    fn a_release_needs_its_own_key_and_a_profile_does_not() {
        let project = project("");
        // Unless the environment names one (CI does), a release `frus build` refuses.
        if std::env::var_os("CARGO_APK_RELEASE_KEYSTORE").is_none() {
            let error = signing_key(&project, Mode::Release, false).unwrap_err();
            assert!(error.contains("signing.release"), "{error}");
        }
        let mut signed = project.clone();
        signed
            .signing
            .push(("release".into(), "keys/up.jks".into(), "pw".into()));
        if std::env::var_os("CARGO_APK_RELEASE_KEYSTORE").is_none() {
            assert_eq!(
                signing_key(&signed, Mode::Release, false).unwrap(),
                Key {
                    path: Path::new(".").join("keys/up.jks"),
                    password: "pw".into()
                }
            );
        }
    }

    #[test]
    fn a_stored_archive_reads_back() {
        let dir = std::env::temp_dir().join(format!("frus-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.zip");
        let entries = vec![
            (
                "manifest/AndroidManifest.xml".to_string(),
                b"<manifest/>".to_vec(),
            ),
            (
                "lib/arm64-v8a/libapp.so".to_string(),
                vec![0x7f, b'E', b'L', b'F', 0, 1],
            ),
            ("empty".to_string(), Vec::new()),
        ];
        zip::write_stored(&path, &entries).unwrap();
        assert_eq!(zip::read_stored(&path).unwrap(), entries);
        // The libraries a package holds are read from it, not from a folder beside it.
        assert_eq!(
            packaged_libraries(&path).unwrap(),
            ["lib/arm64-v8a/libapp.so"]
        );
        assert_eq!(zip::crc32(b"123456789"), 0xCBF4_3926);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
