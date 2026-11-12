#!/usr/bin/env bash
#
# Compile frus's Android activity (crates/frus-cli/java/dev/frus/FrusActivity.java, milestone
# 801) into the dex `frus build` and `frus run` add to every package as its classes.dex.
# Only re-run this when the Java file changes; the resulting dex is checked in
# (crates/frus-cli/assets/frus_activity.dex) so that building an application never needs javac.
#
# Usage (WSL, or Git Bash on Windows with ANDROID_HOME set):  bash scripts/build-activity-dex.sh
set -eu

SDK="${ANDROID_HOME:-/root/android-sdk}"
BUILD_TOOLS="$SDK/build-tools/34.0.0"
PLATFORM="$SDK/platforms/android-34/android.jar"
D8="$BUILD_TOOLS/d8"
if [ ! -x "$D8" ] && [ -f "$BUILD_TOOLS/d8.bat" ]; then
    D8="$BUILD_TOOLS/d8.bat"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$ROOT/crates/frus-cli/java/dev/frus/FrusActivity.java"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

javac --release 8 -cp "$PLATFORM" -d "$OUT_DIR/classes" "$SOURCE"
mkdir -p "$OUT_DIR/dex"
"$D8" --release --min-api 24 --output "$OUT_DIR/dex" "$OUT_DIR"/classes/dev/frus/*.class

mkdir -p "$ROOT/crates/frus-cli/assets"
cp "$OUT_DIR/dex/classes.dex" "$ROOT/crates/frus-cli/assets/frus_activity.dex"
echo "OK: $(wc -c < "$ROOT/crates/frus-cli/assets/frus_activity.dex") bytes"
