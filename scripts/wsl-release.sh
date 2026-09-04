#!/usr/bin/env bash
# Build Sheaf's Linux bundles inside WSL from the Windows checkout.
# Run: wsl -d Ubuntu-24.04 -u root -- bash /mnt/c/my-local-code/sheaf/scripts/wsl-release.sh linux-arm64
set -euo pipefail
target="${1:-linux-arm64}"
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export HOME="${HOME:-/root}"
SRC=/mnt/c/my-local-code/sheaf
mkdir -p "$HOME/.tauri"
cp /mnt/c/Users/bmhay/.tauri/sheaf.key "$HOME/.tauri/sheaf.key"
chmod 600 "$HOME/.tauri/sheaf.key"
if [ ! -d "$HOME/sheaf/.git" ]; then
  git clone -q "$SRC" "$HOME/sheaf"
fi
cd "$HOME/sheaf"
git fetch -q origin && git reset -q --hard origin/main
echo "building $(git log --oneline -1)"
pnpm install --frozen-lockfile 2>&1 | tail -1 || true
test -x node_modules/.bin/tauri
export NO_STRIP=true
set +o pipefail
bash scripts/release-local.sh "$target" 2>&1 | grep -vE '^\s+(Compiling|Downloaded|Fresh|Downloading)' | tail -30
set -o pipefail
mkdir -p "$SRC/dist-linux"
find src-tauri/target -path '*release/bundle/*' -type f \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.sig' \) -exec cp {} "$SRC/dist-linux/" \;
ls -la "$SRC/dist-linux"
