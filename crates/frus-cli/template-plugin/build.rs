//! Built for Android, the plugin carries its Java side as a dex: `android/java` is compiled with
//! the JDK's `javac` against the SDK's `android.jar`, then turned into `classes.dex` by the SDK's
//! `d8`. The library includes it, and loads it at run time into the application's activity.
//!
//! `frus build apk` and `frus run -d android` find the SDK (and install it when it is missing);
//! by hand, set `ANDROID_HOME`, and `JAVA_HOME` if `javac` is not on the `PATH`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=android/java");
    println!("cargo:rerun-if-env-changed=ANDROID_HOME");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let sdk = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .find_map(env::var_os)
        .map(PathBuf::from)
        .expect("building for Android needs the Android SDK: set ANDROID_HOME");
    let build_tools = newest(&sdk.join("build-tools")).expect("the Android SDK has no build-tools");
    let platform = newest(&sdk.join("platforms")).expect("the Android SDK has no platform");
    let javac = env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join("javac"))
        .unwrap_or_else(|| PathBuf::from("javac"));

    let mut sources = Vec::new();
    files(Path::new("android/java"), "java", &mut sources);
    let classes = out.join("classes");
    let _ = std::fs::remove_dir_all(&classes);
    run(Command::new(&javac)
        .args(["--release", "8", "-cp"])
        .arg(platform.join("android.jar"))
        .arg("-d")
        .arg(&classes)
        .args(&sources));

    let mut compiled = Vec::new();
    files(&classes, "class", &mut compiled);
    let d8 = if cfg!(windows) {
        build_tools.join("d8.bat")
    } else {
        build_tools.join("d8")
    };
    run(Command::new(d8)
        .args(["--release", "--output"])
        .arg(&out)
        .args(&compiled));
}

/// The highest version below `dir`: `build-tools/36.1.0`, `platforms/android-36`.
fn newest(dir: &Path) -> Option<PathBuf> {
    let key = |p: &PathBuf| -> Vec<u64> {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse().ok())
            .collect()
    };
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .max_by_key(key)
}

/// Every file below `dir` with the extension `ext`.
fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("could not run {:?}: {e}", command.get_program()));
    assert!(status.success(), "{:?} failed ({status})", command.get_program());
}
