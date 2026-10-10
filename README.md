# Pretorium

A macOS workspace for running a fleet of coding agents (Claude Code, Codex, Antigravity, Grok, OpenCode) side by
side: tiled terminals across workspaces and windows, with a file tree, editor, diffs and git alongside, plus views
for the agents' usage limits, skills and settings.

Built with Tauri 2 (Rust) and SvelteKit (Svelte 5).

## Development

```sh
bun install
bun run tauri dev      # the app, with hot reload
bun test               # frontend unit tests
bun run check          # type check
cd src-tauri && cargo test
```

## Releases and updates

Installed copies update themselves: soon after launch (and twice a day) the app reads `latest.json` from this repo's
latest GitHub release, and "Check for Updates…" in the app menu does the same on demand. An update is only installed
if its signature matches the public key in `tauri.conf.json` (`plugins > updater`) and records the version
`latest.json` announces.

To release:

1. Bump `version` in `src-tauri/tauri.conf.json` (also `package.json` and `src-tauri/Cargo.toml`, to keep them in
   step), commit and push.
2. Run the **Release** workflow (Actions tab). It builds a universal app and creates a draft release with the `.dmg`,
   the signed update archive and `latest.json`.
3. Publish the draft. Installed apps see it from then on.

The workflow needs the repository secret `TAURI_SIGNING_PRIVATE_KEY`: the contents of `~/.tauri/pretorium.key`, the
updater's private key. Keep a backup of it outside this machine: without it, installed copies can never be updated
again.

A local build signs updates when the key is in the environment:

```sh
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/pretorium.key)" bun run tauri build   # src-tauri/target/release/bundle/
```

The app isn't code signed yet, so macOS asks before the first launch of a downloaded copy (System Settings → Privacy
& Security → Open Anyway). Distributing it without that needs a Developer ID signature and notarization: set
`APPLE_SIGNING_IDENTITY` (and `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` for notarization) before building, or as
secrets for the workflow. See <https://v2.tauri.app/distribute/sign/macos/>.
