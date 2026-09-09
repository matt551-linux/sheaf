# Sheaf 0.2.4 release: macOS remaining

## Status as of 2026-09-09 ~03:20 UTC

Windows (x64 + ARM64) and Linux (x64: deb/rpm/AppImage) are built, verified,
and uploaded to the DRAFT GitHub release `v0.2.4`
(https://github.com/matt551-linux/sheaf/releases/tag/v0.2.4). It is
intentionally left as a draft — do not publish until macOS is added.

## Why macOS is blocked

Two separate problems hit in sequence tonight, both now diagnosed:

1. **Homebrew's cargo has no x86_64-apple-darwin rustup target** — universal
   binary (`mac-univ` / `universal-apple-darwin`) build failed. WORKAROUND
   USED: build native `aarch64-apple-darwin` only (Apple Silicon). This
   succeeded — `Sheaf.app` built clean via `pnpm tauri build --target
   aarch64-apple-darwin`.

2. **DMG bundling hangs on SSH** — `bundle_dmg.sh`'s AppleScript
   Finder-styling step needs a TCC-approved interactive GUI session; over a
   bare SSH session (no active login window driving it) it just hangs
   forever instead of failing, and Tauri reports a generic bundle failure.
   Brian approved a plan: rebuild with Parsec providing the interactive
   session so the one-time TCC prompt can be granted.

3. **NEW as of ~22:50 PT: SSH key auth to the Mac stopped working entirely.**
   It worked fine for ~2 hours of debugging tonight (same key, same
   `~/.ssh/id_ed25519_sheaf`), then started failing with
   `Permission denied (publickey,password,keyboard-interactive)` for both
   `brianhaywood` and `root`, both via hostname and raw IP. `ssh -v` shows
   the key is offered correctly and the server flatly rejects it — this is
   not a network/DNS/host-key issue, `authorized_keys` on the Mac itself
   changed or SSH access was revoked (possibly related to the macOS login
   popup Brian dismissed earlier, or a security policy re-lock). I have no
   way to fix this remotely without an existing working session.

## What's already done on the Mac (still there, don't redo)

- Repo: `~/Documents/sheaf`, on `main` at commit `625f997` (0.2.4 bump).
- Signing key present: `~/.tauri/sheaf.key`.
- PDFium fetched: `~/Documents/sheaf/src-tauri/pdfium/mac-univ/` (universal
  binary, works for both build methods).
- Toolchain: Homebrew rust/node/pnpm at `/opt/homebrew/bin`, NOT rustup —
  `export PATH="/opt/homebrew/bin:$PATH"` before any cargo/pnpm command.
- `Sheaf.app` already built once at
  `~/Documents/sheaf/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Sheaf.app`
  (from the earlier successful `pnpm tauri build --target
  aarch64-apple-darwin` run — may be stale if anything changed since).

## To finish in the morning

1. **Restore SSH access first**, or just do everything from a local Terminal
   via Parsec (simpler — sidesteps the whole SSH mystery):
   - Open Parsec, connect to `brians-mac-mini`.
   - Open Terminal.app there.
   - If continuing to want SSH from Albert: check
     `cat ~/.ssh/authorized_keys` for the `albert-sheaf-build@bmhay-pc` key
     (fingerprint `SHA256:oaCbpYbJt7krJ0L3y7an9/bzZcwWkvOBE7wz6TJ25Lw`); if
     missing, the public key is in this repo's session history / re-ask
     Albert to regenerate and hand you the pubkey to paste back in.

2. **Build (from a real logged-in Terminal, not SSH, so any one-time TCC
   permission prompt can actually be answered):**
   ```bash
   export PATH="/opt/homebrew/bin:$PATH"
   cd ~/Documents/sheaf
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/sheaf.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   pnpm tauri build --target aarch64-apple-darwin
   ```
   Watch for a macOS permission dialog (Automation / control Finder) during
   the DMG step — click Allow. This is almost certainly all that was needed;
   the plain `hdiutil create` command worked fine standalone earlier, only
   the AppleScript Finder step failed.

3. **If it still hangs/fails on DMG even with a real GUI session:** skip the
   styled DMG and ship the app directly —
   ```bash
   cd ~/Documents/sheaf/src-tauri/target/aarch64-apple-darwin/release/bundle/macos
   ditto -c -k --sequesterRsrc --keepParent Sheaf.app Sheaf_0.2.4_aarch64.app.zip
   ```
   `.app.tar.gz` or `.app.zip` is an acceptable updater asset per the
   pipeline skill (`\.app\.tar\.gz(\.sig)?$` is explicitly matched by
   `release-manifest.mjs`), just sign it the same way as other bundles:
   ```bash
   node node_modules/@tauri-apps/cli/tauri.js signer sign Sheaf_0.2.4_aarch64.app.zip
   ```
   (Note: verify the manifest regex handles `.zip` — the script only
   explicitly lists `.app.tar.gz`; use `tar czf` instead of `zip` to match
   it exactly if the regex doesn't accept `.zip`.)

4. **Upload to the existing draft** (do NOT create a new release):
   ```bash
   gh release upload v0.2.4 <path-to-dmg-or-app.tar.gz> <path-to-.sig> --clobber -R matt551-linux/sheaf
   ```

5. **Regenerate `latest.json` from ALL platforms** (Windows + Linux already
   uploaded, now add macOS): download the current release assets, merge in
   the new mac file, rerun the manifest script, re-upload:
   ```bash
   cd /c/my-local-code/sheaf   # back on the Windows box
   rm -rf stage && mkdir stage
   gh release download v0.2.4 -D stage --clobber -R matt551-linux/sheaf
   # copy the new mac bundle + .sig into stage/, delete stage/latest.json
   node scripts/release-manifest.mjs stage dist-final v0.2.4 matt551-linux/sheaf
   gh release upload v0.2.4 dist-final/latest.json --clobber -R matt551-linux/sheaf
   ```

6. **Verify, then publish:**
   ```bash
   gh release view v0.2.4 --json isDraft,assets -R matt551-linux/sheaf
   gh release edit v0.2.4 --draft=false -R matt551-linux/sheaf   # only when Brian confirms
   ```

## Already verified tonight (don't redo)

- All 3 platforms pass the full engine test suite (16/16) on the actual
  target hardware: Windows (this box), macOS (`brians-mac-mini`, native
  build), Linux (`omarchy`, native build). Commit `363b020`.
- Windows x64 + ARM64 installers: built, signed, uploaded, verified by hash.
- Linux x64 deb/rpm/AppImage: built, signed (AppImage only), uploaded,
  verified by hash. AppImage was repacked via `fix-appimage.sh` for
  Mesa25+/GLib compatibility (needed `unsquashfs` from
  `~/.local/bin/unsquashfs` on omarchy, PATH wasn't set by default).
- `NO_STRIP=true` is required for AppImage bundling via `release-local.sh`
  on this particular toolchain (linuxdeploy's bundled `strip` can't handle
  `.relr.dyn` relocation sections in modern glibc/webkitgtk builds) — should
  probably be added as a permanent default in `scripts/release-local.sh` for
  `linux-*` targets rather than an ad-hoc env var each time.
