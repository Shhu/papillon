#!/bin/sh
# Builds papillon.flatpak from the working tree, from source and offline, the
# way Flathub would build it.
# Prerequisites (once): flatpak, flatpak-builder, and the Flathub remote:
#   flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
# After any change to Cargo.lock, regenerate cargo-sources.json with
# flatpak-cargo-generator.py (github.com/flatpak/flatpak-builder-tools, needs
# the Python modules aiohttp and tomlkit):
#   python3 flatpak-cargo-generator.py ../../Cargo.lock -o cargo-sources.json
set -e
cd "$(dirname "$0")"
[ cargo-sources.json -nt ../../Cargo.lock ] ||
    { echo "Cargo.lock changed: regenerate cargo-sources.json (see above)"; exit 1; }
flatpak-builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --repo=repo build io.github.Shhu.Papillon.yml
flatpak build-bundle repo papillon.flatpak io.github.Shhu.Papillon \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "Package ready: $(pwd)/papillon.flatpak"
