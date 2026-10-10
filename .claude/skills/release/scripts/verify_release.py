#!/usr/bin/env python3
"""Check a release before (or after) publishing it: what installed apps will download has to be complete, announce the
right version and carry a signature from the updater key in tauri.conf.json, or the update fails (or never shows up).

usage: verify_release.py <tag>               the draft (or published) release, e.g. v0.1.2
       verify_release.py <tag> --endpoint    also: the URL the app polls serves this version (only after publishing)
Exit status 1 if anything is wrong. Read-only: downloads into a temporary folder.
"""
import base64, json, subprocess, sys, tempfile, urllib.request
from pathlib import Path

def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout

if len(sys.argv) < 2:
    sys.exit(__doc__)
tag = sys.argv[1] if sys.argv[1].startswith("v") else "v" + sys.argv[1]
version = tag[1:]
root = Path(run("git", "rev-parse", "--show-toplevel").strip())
conf = json.loads((root / "src-tauri/tauri.conf.json").read_text())
repo = run("gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner").strip()
failed = []

def check(ok, what):
    print(("PASS " if ok else "FAIL ") + what)
    if not ok:
        failed.append(what)

release = json.loads(run("gh", "release", "view", tag, "-R", repo, "--json", "isDraft,assets,body"))
names = [a["name"] for a in release["assets"]]
print(f"{tag} ({'draft' if release['isDraft'] else 'published'}): {', '.join(names)}")
for kind, ok in [(".dmg", any(n.endswith(".dmg") for n in names)), (".app.tar.gz", any(n.endswith(".app.tar.gz") for n in names)),
                 (".app.tar.gz.sig", any(n.endswith(".app.tar.gz.sig") for n in names)), ("latest.json", "latest.json" in names)]:
    check(ok, f"has {kind}")
if failed:
    sys.exit(1)

with tempfile.TemporaryDirectory() as d:
    run("gh", "release", "download", tag, "-R", repo, "-D", d, "-p", "latest.json", "-p", "*.app.tar.gz.sig")
    latest = json.loads(Path(d, "latest.json").read_text())
    sig = next(Path(d).glob("*.sig")).read_text().strip()

platforms = latest.get("platforms", {})
check(latest.get("version") == version, f"latest.json announces {version} (says {latest.get('version')})")
check({"darwin-aarch64", "darwin-x86_64"} <= platforms.keys(), f"latest.json covers Apple Silicon and Intel ({', '.join(sorted(platforms))})")
check(all(f"/download/{tag}/" in p["url"] for p in platforms.values()), f"every download URL points at {tag}")
check(all(p["signature"].strip() == sig for p in platforms.values()), "latest.json signatures are the uploaded .sig")

sig_lines = base64.b64decode(sig).decode().splitlines()
trusted = next((l for l in sig_lines if l.startswith("trusted comment:")), "")
check(f"version:{version}" in trusted, f"signature records version {version} (requireSignedVersion)")
pub_lines = base64.b64decode(conf["plugins"]["updater"]["pubkey"]).decode().splitlines()
key_id = lambda b64: base64.b64decode(b64)[2:10]  # minisign: 2-byte algorithm, then the 8-byte key id
check(key_id(sig_lines[1]) == key_id(pub_lines[1]), "signed with the updater key in tauri.conf.json")
print(f"notes shown in the update prompt: {latest.get('notes', '')!r}")

if "--endpoint" in sys.argv:
    url = conf["plugins"]["updater"]["endpoints"][0]
    with urllib.request.urlopen(url) as r:
        served = json.load(r).get("version")
    check(served == version, f"{url} serves {version} (serves {served})")

sys.exit(1 if failed else 0)
