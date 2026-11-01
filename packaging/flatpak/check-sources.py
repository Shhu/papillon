#!/usr/bin/env python3
"""Fails if cargo-sources.json doesn't hold exactly the crates of Cargo.lock.
File dates can't tell: git gives them whatever the order of a checkout."""
import json
import re
import sys
import tomllib

lock_path, sources_path = sys.argv[1:]
with open(lock_path, "rb") as f:
    locked = {
        (p["name"], p["version"])
        for p in tomllib.load(f)["package"]
        if "source" in p
    }
with open(sources_path) as f:
    sources = set()
    for source in json.load(f):
        found = re.search(r"/crates/([^/]+)/\1-([^/]+)\.crate$", source.get("url", ""))
        if found:
            sources.add(found.groups())
if locked != sources:
    print("missing:", sorted(locked - sources), "extra:", sorted(sources - locked))
    sys.exit(1)
