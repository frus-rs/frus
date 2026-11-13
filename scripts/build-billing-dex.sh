#!/usr/bin/env bash
#
# Compile the in-app purchases' Android half (FrusBilling.java, milestone 807) into a dex of its
# own, which frus-shell embeds only with its `purchases` feature: an application that sells
# nothing carries none of it. Only re-run this when the Java file changes; the resulting dex is
# checked in (crates/frus-shell/assets/frus_billing.dex) so that a build never needs javac.
#
# The class is compiled against Google Play Billing Library's public API (downloaded here, not
# bundled): the library itself is put into the application's package by `frus build`, at the
# version `PLAY_BILLING_MARKER` names in crates/frus-shell/src/purchases/android.rs.
#
# Usage (WSL or Git Bash, with a JDK and the Android SDK):  bash scripts/build-billing-dex.sh
set -eu

BILLING=8.3.0
SDK="${ANDROID_HOME:-/root/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JAVA_DIR="$ROOT/crates/frus-shell/java/dev/frus/billing"
OUT_DIR="$(mktemp -d)"
# Git Bash: the JDK wants Windows paths, separated by semicolons.
SEP=":"
if command -v cygpath >/dev/null 2>&1; then
    OUT_DIR="$(cygpath -m "$OUT_DIR")"
    PLATFORM="$(cygpath -m "$PLATFORM")"
    BUILD_TOOLS="$(cygpath -m "$BUILD_TOOLS")"
    SEP=";"
fi
trap 'rm -rf "$OUT_DIR"' EXIT

curl --fail --silent --location --output "$OUT_DIR/billing.aar" \
    "https://dl.google.com/dl/android/maven2/com/android/billingclient/billing/$BILLING/billing-$BILLING.aar"
(cd "$OUT_DIR" && unzip -q billing.aar classes.jar && mv classes.jar billing.jar)

javac --release 8 -cp "$PLATFORM$SEP$OUT_DIR/billing.jar" -d "$OUT_DIR/classes" "$JAVA_DIR"/*.java
mkdir -p "$OUT_DIR/dex"
java -cp "$BUILD_TOOLS/lib/d8.jar" com.android.tools.r8.D8 --release --min-api 24 \
    --lib "$PLATFORM" --classpath "$OUT_DIR/billing.jar" --output "$OUT_DIR/dex" \
    "$OUT_DIR"/classes/dev/frus/billing/*.class

mkdir -p "$ROOT/crates/frus-shell/assets"
cp "$OUT_DIR/dex/classes.dex" "$ROOT/crates/frus-shell/assets/frus_billing.dex"
echo "OK: $(wc -c < "$ROOT/crates/frus-shell/assets/frus_billing.dex") bytes"
