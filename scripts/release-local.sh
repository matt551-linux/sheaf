#!/usr/bin/env bash
# Local release build for one target: fetch PDFium, stage it as a resource,
# build signed bundles. Mirrors the CI build job so local and CI artifacts
# are interchangeable.
#
#   scripts/release-local.sh <pdfium-target> [extra tauri build args]
#   pdfium-target: win-x64 | win-arm64 | mac-univ | linux-x64 | linux-arm64
#
# Needs the updater private key: TAURI_SIGNING_PRIVATE_KEY (contents) or
# TAURI_SIGNING_PRIVATE_KEY_PATH (file, default ~/.tauri/sheaf.key).
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?pdfium target required}"; shift || true

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  keyfile="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/sheaf.key}"
  [ -f "$keyfile" ] || { echo "updater key not found at $keyfile" >&2; exit 1; }
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$keyfile")"
fi
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

node scripts/fetch-pdfium.mjs "$target"
mkdir -p src-tauri/resources
rm -f src-tauri/resources/*pdfium.*
cp src-tauri/pdfium/"$target"/*pdfium.* src-tauri/resources/

# Always name bundles by the explicit Rust target; without it tauri labels
# them by the host default (an ARM64 host still produces "_x64" names).
case "$target" in
  win-x64)     rust_target=x86_64-pc-windows-msvc ;;
  win-arm64)   rust_target=aarch64-pc-windows-msvc ;;
  linux-x64)   rust_target=x86_64-unknown-linux-gnu ;;
  linux-arm64) rust_target=aarch64-unknown-linux-gnu ;;
  mac-univ)    rust_target=universal-apple-darwin ;;
  *) echo "unknown target $target" >&2; exit 1 ;;
esac
pnpm tauri build --target "$rust_target" "$@"
echo "bundles:"
find src-tauri/target -path '*release/bundle/*' -type f \
  \( -name '*.exe' -o -name '*.msi' -o -name '*.dmg' -o -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.tar.gz' -o -name '*.sig' \) \
  -newer src-tauri/resources -print
