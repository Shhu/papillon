#!/bin/sh
# Builds Papillon for Windows from WSL: a single self-contained .exe (the C
# runtime is linked in, see .cargo/config.toml). Uses the Windows Rust
# toolchain of the Windows user, and builds into their profile folder
# (.papillon-target), outside WSL's file system.
set -e
cd "$(dirname "$0")/../.."
. packaging/windows/paths.sh
"$WIN_HOME_WSL/.cargo/bin/cargo.exe" build --release --target-dir "$WIN_TARGET"
printf 'Ready: %s\n' "$WIN_TARGET\\release\\papillon.exe"
