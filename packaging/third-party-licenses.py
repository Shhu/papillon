#!/usr/bin/env python3
"""Writes assets/licenses.txt: the license texts of Papillon and of everything
built into its binaries, read from the crates cargo downloaded. Run it after
any change to Cargo.lock; publish.sh refuses a file that is out of date."""
import json
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
names = re.compile(r"^(licen[sc]e|copying|notice|unlicense)", re.I)

normal = set()
for target in ("x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"):
    tree = subprocess.run(
        ["cargo", "tree", "-e", "normal", "--prefix", "none", "--target", target, "-f", "{p}"],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout
    for line in tree.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            normal.add((parts[0], parts[1].lstrip("v")))

meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked"],
    cwd=root, check=True, capture_output=True, text=True,
).stdout)

out = ["Papillon and the works built into it\n"]
seen = {}
for name in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md", "assets/fonts/Inter-OFL.txt"):
    out.append(f"\n{'=' * 72}\n{name}\n{'=' * 72}\n\n{(root / name).read_text()}")

for package in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
    key = (package["name"], package["version"])
    if key not in normal or package["name"] == "papillon":
        continue
    crate = Path(package["manifest_path"]).parent
    texts = sorted(p for p in crate.iterdir() if p.is_file() and names.match(p.name))
    out.append(f"\n{'=' * 72}\n{key[0]} {key[1]}: {package.get('license') or 'see its repository'}\n{'=' * 72}\n")
    if package.get("repository"):
        out.append(f"{package['repository']}\n")
    for text in texts:
        content = text.read_text(errors="replace")
        # The same text word for word: once is enough.
        if content in seen:
            out.append(f"\n{text.name}: same text as {seen[content]}.\n")
        else:
            seen[content] = f"{key[0]} {key[1]}, {text.name}"
            out.append(f"\n{content}")

(root / "assets" / "licenses.txt").write_text("".join(out))
