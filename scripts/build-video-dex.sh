#!/usr/bin/env bash
#
# Compile the video player's Android half (FrusVideo.java, milestone 797) into a dex of its
# own, which frus-shell embeds only with its `video` feature: an application that plays no
# video carries none of it. Only re-run this when the Java file changes; the resulting dex is
# checked in (crates/frus-shell/assets/frus_video.dex) so that `cargo apk` never needs javac.
#
# Usage (inside WSL):  bash scripts/build-video-dex.sh
set -eu

SDK="${ANDROID_HOME:-/root/android-sdk}"
BUILD_TOOLS="$SDK/build-tools/34.0.0"
PLATFORM="$SDK/platforms/android-34/android.jar"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JAVA_DIR="$ROOT/crates/frus-shell/java/dev/frus/video"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

javac --release 8 -cp "$PLATFORM" -d "$OUT_DIR/classes" "$JAVA_DIR"/*.java
mkdir -p "$OUT_DIR/dex"
"$BUILD_TOOLS/d8" --release --min-api 24 --output "$OUT_DIR/dex" \
    "$OUT_DIR"/classes/dev/frus/video/*.class

mkdir -p "$ROOT/crates/frus-shell/assets"
cp "$OUT_DIR/dex/classes.dex" "$ROOT/crates/frus-shell/assets/frus_video.dex"
echo "OK: $(stat -c%s "$ROOT/crates/frus-shell/assets/frus_video.dex") bytes"
