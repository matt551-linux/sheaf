#!/usr/bin/env bash
# Runs inside the sheaf-linux-x64 container. Expects /src (repo with a
# prebuilt build/ directory and src-tauri/pdfium/linux-x64/libpdfium.so)
# and /run/sheaf.key (updater private key).
set -euo pipefail
export TAURI_SIGNING_PRIVATE_KEY="$(cat /run/sheaf.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
git config --global --add safe.directory '*'
rm -rf /work/sheaf
git clone -q /src /work/sheaf
cd /work/sheaf
cp -r /src/build ./build
mkdir -p src-tauri/pdfium/linux-x64 src-tauri/resources
cp /src/src-tauri/pdfium/linux-x64/libpdfium.so src-tauri/pdfium/linux-x64/
cp src-tauri/pdfium/linux-x64/libpdfium.so src-tauri/resources/
cargo tauri build --target x86_64-unknown-linux-gnu --config '{"build":{"beforeBuildCommand":""}}'
mkdir -p /src/dist-linux
find src-tauri/target -path '*release/bundle/*' -type f \
  \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.sig' \) -exec cp {} /src/dist-linux/ \;
ls -la /src/dist-linux
