#!/usr/bin/env python3
"""Set Pretorium's version everywhere it lives: src-tauri/tauri.conf.json (what the app and the updater use),
package.json, src-tauri/Cargo.toml and Cargo.lock. Edits the version lines in place (formatting kept).

usage: bump_version.py <new-version>     e.g. bump_version.py 0.1.2      (run from anywhere in the repo)
"""
import json, re, subprocess, sys
from pathlib import Path

def semver(v):
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", v)
    if not m:
        sys.exit(f"'{v}' isn't a plain MAJOR.MINOR.PATCH version")
    return tuple(map(int, m.groups()))

def replace_once(path, pattern, repl):
    text = path.read_text()
    new, n = re.subn(pattern, repl, text, count=1, flags=re.M)
    if n != 1:
        sys.exit(f"couldn't find the version in {path}")
    path.write_text(new)

root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip())
if len(sys.argv) != 2:
    sys.exit(__doc__)
new = sys.argv[1].removeprefix("v")
conf = root / "src-tauri/tauri.conf.json"
current = json.loads(conf.read_text())["version"]
if semver(new) <= semver(current):
    sys.exit(f"{new} isn't newer than the current {current}: installed apps only update to a newer version")

replace_once(conf, r'^(  "version": )"[^"]*"', rf'\g<1>"{new}"')
replace_once(root / "package.json", r'^(  "version": )"[^"]*"', rf'\g<1>"{new}"')
replace_once(root / "src-tauri/Cargo.toml", r'^(version = )"[^"]*"', rf'\g<1>"{new}"')  # the [package] one comes first
# Cargo.lock records the crate's own version too.
subprocess.run(["cargo", "update", "--workspace", "--quiet"], cwd=root / "src-tauri", check=True)

lock = (root / "src-tauri/Cargo.lock").read_text()
assert f'name = "fleet_command"\nversion = "{new}"' in lock, "Cargo.lock wasn't updated"
print(f"{current} -> {new}: tauri.conf.json, package.json, Cargo.toml, Cargo.lock")
