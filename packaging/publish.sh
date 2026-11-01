#!/bin/sh
# Publishes a version of Papillon from this computer (no CI): builds the
# Windows .exe and the Flatpak, tags the sources, creates the GitHub release
# with them, where the website's download buttons point, and updates the
# shu-app.com Flatpak repository. The website itself (docs/) is served by GitHub Pages from
# main, so pushing main publishes it. Can be run again after a failure.
# Usage: packaging/publish.sh notes.md   (version taken from Cargo.toml)
set -e
notes=$(realpath "${1:-}" 2>/dev/null || true)
[ -f "$notes" ] || { echo "Usage: $0 notes.md"; exit 1; }
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"

# "Not found" means not published; any other error stops here.
if output=$(gh release view "$tag" --repo Shhu/papillon 2>&1); then
    echo "$tag is already published: bump the version in Cargo.toml"
    exit 1
elif ! echo "$output" | grep -q "release not found"; then
    echo "$output"
    exit 1
fi
[ -z "$(git status --porcelain)" ] || { echo "Commit your changes first"; exit 1; }
[ "$(git branch --show-current)" = main ] || { echo "Publish from main"; exit 1; }
grep -q "<release version=\"$version\"" packaging/flatpak/io.github.Shhu.Papillon.metainfo.xml ||
    { echo "Add $version to the releases of the metainfo file"; exit 1; }
packaging/third-party-licenses.py
[ -z "$(git status --porcelain)" ] || { echo "assets/licenses.txt was out of date: commit it"; exit 1; }
if tagged=$(git rev-parse -q --verify "refs/tags/$tag^{commit}"); then
    [ "$tagged" = "$(git rev-parse HEAD)" ] || { echo "$tag points to another commit"; exit 1; }
fi

packaging/windows/build.sh
packaging/flatpak/build.sh
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
. packaging/windows/paths.sh
cp "$WIN_TARGET_WSL/release/papillon.exe" "$out/"
cp packaging/flatpak/papillon.flatpak "$out/"
cp assets/licenses.txt "$out/papillon-licenses.txt"

git rev-parse -q --verify "refs/tags/$tag" > /dev/null || git tag "$tag"
git push origin main "$tag"
gh release create "$tag" "$out/papillon.exe" "$out/papillon.flatpak" "$out/papillon-licenses.txt" \
    --repo Shhu/papillon --title "Papillon $version" --notes-file "$notes"
echo "Published: https://github.com/Shhu/papillon/releases/tag/$tag"
# The shu-app.com Flatpak repository (shared with the author's other apps)
# takes the new bundle; the release stands even if this fails.
shared="$HOME/dev/shu-app/flatpak/publish.sh"
if [ -x "$shared" ]; then
    "$shared" || echo "Flatpak repository not updated: run $shared again" >&2
else
    echo "Flatpak repository not updated: $shared is missing" >&2
fi
