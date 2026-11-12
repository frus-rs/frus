#!/usr/bin/env bash
#
# Compile the Android bridge classes (FrusTextBridge.java, the input bridge;
# FrusSystemBars.java, the system bars of #46; FrusClipboard.java, the clipboard of
# #22; FrusNotifications.java, the notifications of milestone 703; FrusPermissions.java,
# the runtime permissions of milestone 730; FrusRestoration.java, the state restoration of
# milestone 753; FrusShare.java, opening addresses and sharing of milestone 775;
# FrusHaptics.java and FrusConnectivity.java, milestone 777; FrusAccessibility.java, the
# screen reader's view of the window, milestone 782; FrusPlatformViews.java and the
# dev.frus.platform interfaces plugins implement, the native views of milestone 794;
# FrusIntents.java, what an intent that opens the application carries, milestone 801) into one
# embedded dex, and the web views of milestone 799 (dev.frus.webview) into a second one.
# Only re-run this when a Java file changes; the resulting dex is checked in
# (crates/frus-shell/assets/frus_input.dex) so that `cargo apk` never needs javac.
#
# Usage (inside WSL):  bash scripts/build-input-dex.sh
set -eu

SDK="${ANDROID_HOME:-/root/android-sdk}"
BUILD_TOOLS="$SDK/build-tools/34.0.0"
PLATFORM="$SDK/platforms/android-34/android.jar"
D8="$BUILD_TOOLS/d8"
if [ ! -x "$D8" ] && [ -f "$BUILD_TOOLS/d8.bat" ]; then
    D8="$BUILD_TOOLS/d8.bat"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JAVA_DIR="$ROOT/crates/frus-shell/java/dev/frus/input"
PLATFORM_DIR="$ROOT/crates/frus-shell/java/dev/frus/platform"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

javac --release 8 -cp "$PLATFORM" -d "$OUT_DIR/classes" "$JAVA_DIR"/*.java "$PLATFORM_DIR"/*.java
mkdir -p "$OUT_DIR/dex"
# API 24, the lowest frus supports: default interface methods (PlatformView) stay as they are,
# so a plugin compiled with or without desugaring finds them.
"$D8" --release --min-api 24 --output "$OUT_DIR/dex" \
    "$OUT_DIR"/classes/dev/frus/input/*.class "$OUT_DIR"/classes/dev/frus/platform/*.class

mkdir -p "$ROOT/crates/frus-shell/assets"
cp "$OUT_DIR/dex/classes.dex" "$ROOT/crates/frus-shell/assets/frus_input.dex"
echo "OK: $(wc -c < "$ROOT/crates/frus-shell/assets/frus_input.dex") bytes"

# The web views (milestone 799), in a dex of their own: only an application built with frus's
# `webview` feature carries it, and loads it as a plugin's factory over the shared dex.
WEB_DIR="$ROOT/crates/frus-shell/java/dev/frus/webview"
javac --release 8 -cp "$PLATFORM:$OUT_DIR/classes" -d "$OUT_DIR/web" "$WEB_DIR"/*.java
mkdir -p "$OUT_DIR/webdex"
"$BUILD_TOOLS/d8" --release --min-api 24 --lib "$PLATFORM" --classpath "$OUT_DIR/classes"     --output "$OUT_DIR/webdex" "$OUT_DIR"/web/dev/frus/webview/*.class
cp "$OUT_DIR/webdex/classes.dex" "$ROOT/crates/frus-shell/assets/frus_webview.dex"
echo "OK: $(stat -c%s "$ROOT/crates/frus-shell/assets/frus_webview.dex") bytes (web views)"
