#!/usr/bin/env bash
# check-license-files.sh — every published crate must ship the licence texts and NOTICE.
#
# cargo packages only what sits in a crate's own directory, so the files at the
# repository root never reach crates.io. Each published crate keeps a copy, and this
# keeps the copies identical to the root. Run with --fix to refresh them.
#
# frus-glyphon and frus-cosmic-text are skipped: each is a modified copy of a third-party
# crate and carries that crate's own licence files.
set -u
cd "$(dirname "$0")/.."

status=0
for manifest in crates/*/Cargo.toml; do
  dir=$(dirname "$manifest")
  [ "$dir" = "crates/frus-glyphon" ] && continue
  [ "$dir" = "crates/frus-cosmic-text" ] && continue
  grep -q '^publish = false' "$manifest" && continue
  for file in LICENSE-MIT LICENSE-APACHE NOTICE; do
    if ! cmp -s "$file" "$dir/$file"; then
      if [ "${1:-}" = "--fix" ]; then
        cp "$file" "$dir/$file"
      else
        echo "$dir/$file is missing or differs from the root $file"
        status=1
      fi
    fi
  done
done
[ "$status" -ne 0 ] && echo "Run: bash scripts/check-license-files.sh --fix"
exit "$status"
