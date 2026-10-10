---
name: release
description: Release a new version of Pretorium (this Tauri app) end to end - bump the version, run the checks, commit and push main, build it with the Release GitHub workflow, verify the draft's latest.json and updater signature, and publish it so installed copies auto-update. Use this whenever the user wants to release, ship, publish, cut or put out a version (0.1.2, a patch, "the fix", "a new build"), push an update to users, or redo or recover a failed release, even if they don't say "release" in so many words.
---

# Releasing Pretorium

Installed copies of Pretorium update themselves from this repo's GitHub releases: they poll `latest.json` of the
**latest published** release and install the update archive it points at, if its signature checks out. So a release
is only done when that published `latest.json` announces the new version, signed by the updater key. Everything below
gets there and verifies it, because a broken release fails silently on users' machines.

How it's wired:
- **Release workflow** (`.github/workflows/release.yml`, run by hand). It builds a universal macOS app (Apple Silicon
  and Intel) and signs the update archive with the `TAURI_SIGNING_PRIVATE_KEY` repo secret. It then creates a
  **draft** release with the `.dmg`, `Pretorium_universal.app.tar.gz` (+ `.sig`) and `latest.json`. Drafts aren't
  "latest", so nothing ships until the draft is published.
- **Updater config** (`src-tauri/tauri.conf.json` > `plugins.updater`): the public key, the endpoint, and
  `requireSignedVersion`. That last one means the signature must record the same version that `latest.json`
  announces.
- **Version** lives in `src-tauri/tauri.conf.json` (the one that counts), `package.json`, `src-tauri/Cargo.toml` and
  `Cargo.lock`. Installed apps only take a version newer than their own.

## Steps

### 1. Preflight

```sh
git fetch origin --tags                 # tags are made by the workflow on GitHub: fetch them to compare with
git status -sb                          # on main, in sync with origin
gh secret list                          # TAURI_SIGNING_PRIVATE_KEY must be there
gh release list --limit 3               # the latest published version; any leftover drafts
git log --oneline v<last>..origin/main  # what this release would ship
```

- **Nothing to ship:** if that log is empty, a release would just repeat the last one. Say so and ask what they
  meant instead of bumping. The work may still be in a branch or one of the sibling worktrees
  (`git worktree list`, `git log main..<branch>`) and need merging first.
- **Workflow changes:** the build runs `.github/workflows/release.yml` as it is on `origin/main`. A local, unpushed
  edit to it (e.g. the `notes` input) doesn't count until it's pushed.

- **Uncommitted changes:** releasing builds what's on `origin/main`, not the working tree. Ask whether they belong in
  this release. Leave unrelated files alone (the user keeps untracked notes like `lp.md` in the repo).
- **Committing:** always stage files by name, never `git add -A`.
- **Missing signing secret:** stop and ask the user to add it. Don't upload `~/.tauri/pretorium.key` yourself unless
  they ask.

### 2. Pick the version and write the notes

- **Version:** use the one the user gave. Otherwise propose one from the commits since the last release
  (preflight's log): patch for fixes, minor for features. Confirm it if you're guessing.
- **Notes:** draft a few short, user-facing lines from those commits: what changed for someone using the app, not
  commit-speak. They appear in the app's "Update available" prompt, so keep them brief.

### 3. Bump and check

```sh
python3 .claude/skills/release/scripts/bump_version.py <version>
bun run check && bun test && (cd src-tauri && cargo test)
```

`bump_version.py` updates all four files and refuses a version that isn't newer. Fix any failing check before going
on: a broken build that gets published is what every installed copy updates to.

### 4. Commit and push

Commit the bump with a message like `Release 0.1.2` (followed by the attribution lines, if any are configured), then
`git push origin main`. If the release is for a fix that isn't committed yet, the version bump can go in the same
commit as the fix.

### 5. Build

```sh
gh workflow run release.yml --ref main -f notes="<the notes>"
gh run list --workflow release.yml --limit 1 --json databaseId,url    # the run that just started
gh run watch <id> --exit-status --interval 30                        # takes ~10-20 min: run it in the background
```

- **The notes are fixed at build time.** The workflow writes them into `latest.json`, which is what the update prompt
  shows. Editing the GitHub release afterwards changes the release page only, not the prompt.
- **If the run fails:** read the log (`gh run view <id> --log-failed`) and fix the cause.
- **Leftover draft:** the workflow reuses the tag `v<version>`. Delete a stale draft of the same version first:
  `gh release delete v<version> --yes --cleanup-tag`. Only do this for drafts, never for a published release.

### 6. Verify the draft

```sh
python3 .claude/skills/release/scripts/verify_release.py v<version>
```

This checks that:
- all four files are there
- `latest.json` announces this version for both architectures and points at this tag
- the signature matches the archive, records the version, and comes from the updater key in `tauri.conf.json`

Every line must say PASS. A failure here means installed apps would reject the update or never see it. Don't
publish; find out why. Typical causes are a version mismatch between the config files, or a missing or rotated
signing secret.

### 7. Publish

Publishing is what actually ships the update to everyone, so be sure it's wanted. "Release it" from the user counts;
if they only asked for a build or a draft, stop at step 6 and report.

```sh
gh release edit v<version> --draft=false --latest
python3 .claude/skills/release/scripts/verify_release.py v<version> --endpoint
```

The `--endpoint` run confirms that the URL installed apps poll now serves the new version. GitHub can take a few
seconds, so retry once if it still shows the old version.

### 8. Report

Tell the user:
- the release URL
- the version and its notes
- that the checks passed

Also mention that installed copies pick the update up within a few minutes of their next launch (and every 12 hours),
or right away from **Pretorium → Check for Updates…**. If any step was skipped or didn't pass, say so plainly.

## Things that would break updates for everyone

- **A version that isn't newer** than the published one: apps ignore it.
- **Publishing a release that failed verification.**
- **Changing `plugins.updater.pubkey`, or the `TAURI_SIGNING_PRIVATE_KEY` secret.** Installed copies only trust the
  key they shipped with. Rotating it needs a deliberate migration, not a release step.
- **Deleting or un-publishing the latest release.** The endpoint then serves the previous one, or nothing.

The app isn't code signed (no Apple Developer ID yet), so a freshly downloaded `.dmg` needs "Open Anyway" in System
Settings → Privacy & Security on first launch. Updates through the built-in updater don't. That's expected, not a
release problem.
