#!/usr/bin/env bash
# Source this in git-bash on the Windows ARM64 dev box before running cargo/tauri:
#   source scripts/dev-env-windows.sh
# It puts MSVC (arm64 host) and the Windows SDK tools (rc.exe) on PATH.
MSVC_ROOT="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207"
SDK_ROOT="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0"
case "$(uname -m)" in
  aarch64|arm64) HOST=arm64 ;;
  *) HOST=x64 ;;
esac
export PATH="$MSVC_ROOT/bin/Host$HOST/$HOST:$SDK_ROOT/$HOST:$HOME/.cargo/bin:$PATH"
