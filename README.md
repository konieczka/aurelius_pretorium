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

## Release build

```sh
bun run tauri build    # src-tauri/target/release/bundle/
```

Distributing outside the Mac App Store needs a Developer ID signature and notarization: set `APPLE_SIGNING_IDENTITY`
(and `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` for notarization) before building. See
<https://v2.tauri.app/distribute/sign/macos/>.
