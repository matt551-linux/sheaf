#!/usr/bin/env bash
# Repack a Tauri-built AppImage so it runs on current distributions.
#
# linuxdeploy bundles Ubuntu 22.04's libwayland-client, GLib, GStreamer core
# and friends next to the app. On any host with Mesa 25+ / GLib 2.80+
# (Arch, Fedora 42+, Ubuntu 26.04) the host EGL driver calls into the old
# bundled libwayland, eglGetDisplay fails, and WebKitWebProcess aborts
# before the window paints. Upstream: tauri-apps/tauri#15665.
#
# Fix: drop those libraries so the host's ABI-compatible copies are used
# (we already require host WebKitGTK/GTK), and put a shim in front of the
# binary that clears the GST_PLUGIN_* overrides AppRun.wrapped injects (a
# set path REPLACES GStreamer's default search path; the bundled plugin dir
# is empty, so WebKit would find no plugins and abort).
#
#   scripts/fix-appimage.sh <file.AppImage>       (rewritten in place)
set -euo pipefail
src="${1:?AppImage path required}"
abs="$(cd "$(dirname "$src")" && pwd)/$(basename "$src")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "==> extracting $(basename "$abs")"
# --appimage-extract needs the AppImage's own runtime to execute; for a
# foreign-arch file (fixing x64 on an ARM box) read the squashfs directly.
# Type 2 runtime: the payload starts at the end of the ELF (e_shoff +
# e_shnum * e_shentsize).
offset=$(python3 - "$abs" <<'PY'
import struct, sys
with open(sys.argv[1], "rb") as f:
    h = f.read(64)
shoff = struct.unpack_from("<Q", h, 0x28)[0]
shentsize, shnum = struct.unpack_from("<HH", h, 0x3A)
print(shoff + shentsize * shnum)
PY
)
( cd "$work" && unsquashfs -q -n -d squashfs-root -o "$offset" "$abs" >/dev/null )
root="$work/squashfs-root"
lib="$root/usr/lib"

# Fail loudly if the layout changed, rather than shipping an unfixed file.
compgen -G "$lib/libwayland-client.so*" >/dev/null || { echo "libwayland-client not in $lib: bundler layout changed" >&2; exit 1; }

echo "==> removing bundled infra libs (host copies are used instead)"
rm -f "$lib"/libwayland-client.so* "$lib"/libwayland-cursor.so* "$lib"/libwayland-egl.so* "$lib"/libwayland-server.so* \
      "$lib"/libglib-2.0.so* "$lib"/libgio-2.0.so* "$lib"/libgobject-2.0.so* "$lib"/libgmodule-2.0.so* \
      "$lib"/libmount.so* "$lib"/libblkid.so* "$lib"/libselinux.so* "$lib"/libsystemd.so* "$lib"/libpcre2-8.so* \
      "$lib"/libgst*.so* "$lib"/libzstd.so* "$lib"/libelf.so* "$lib"/libffi.so* \
      "$lib"/libEGL.so* "$lib"/libGL.so* "$lib"/libGLX.so* "$lib"/libGLdispatch.so* "$lib"/libgbm.so* "$lib"/libdrm.so*
rm -rf "$lib/gstreamer-1.0"

wrapped="$root/AppRun.wrapped"
grep -aq GST_PLUGIN_SYSTEM_PATH_1_0 "$wrapped" || { echo "AppRun.wrapped no longer sets GST_PLUGIN_SYSTEM_PATH_1_0: re-check fix-appimage.sh" >&2; exit 1; }

# Find the real app binary (the one AppRun.wrapped execs) and shim it.
bin="$root/usr/bin/sheaf"
[ -x "$bin" ] || { echo "app binary not found at usr/bin/sheaf" >&2; exit 1; }
mv "$bin" "$bin.real"
cat > "$bin" <<'SHIM'
#!/bin/sh
# Installed by scripts/fix-appimage.sh: drop bundle-pointing GStreamer path
# overrides so the host GStreamer resolves plugins normally. Values that do
# not point into this AppImage belong to the user and are kept.
appdir="${APPDIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
for var in GST_PLUGIN_SYSTEM_PATH_1_0 GST_PLUGIN_SYSTEM_PATH GST_PLUGIN_PATH_1_0 GST_PLUGIN_PATH GST_PLUGIN_SCANNER GST_PLUGIN_SCANNER_1_0; do
  eval "val=\${$var:-}"
  case "$val" in *"$appdir/"*) unset "$var" ;; esac
done
exec "$(dirname "$0")/sheaf.real" "$@"
SHIM
chmod +x "$bin"

echo "==> repacking"
tool="$work/appimagetool"
# Target arch from the bundled binary, not the host (cross-fixing is fine).
case "$(od -An -tx1 -j18 -N2 "$root/usr/bin/sheaf.real" | tr -d ' ')" in
  3e00) arch=x86_64 ;;
  b700) arch=aarch64 ;;
  *) echo "unknown ELF machine in sheaf.real" >&2; exit 1 ;;
esac
hostarch="$(uname -m)"
curl -fsSL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${hostarch}.AppImage"
chmod +x "$tool"
runtime="$work/runtime-$arch"
curl -fsSL -o "$runtime" "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-${arch}"
( cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$arch" "$tool" -n --runtime-file "$runtime" "$root" "$abs.new" >/dev/null 2>&1 )
mv "$abs.new" "$abs"
chmod +x "$abs"
echo "==> done: $(du -h "$abs" | cut -f1) $(basename "$abs")"
