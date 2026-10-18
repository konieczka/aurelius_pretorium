#!/usr/bin/env bash
# What `e2e run` starts (e2e.config.ts): the test databases, the harness serving the app's commands, and the
# frontend's dev server on 1499 with its E2E page (/e2e). Stopping it stops all three.
set -euo pipefail
cd "$(dirname "$0")/.."
e2e/services.sh up
cargo build --quiet --manifest-path src-tauri/Cargo.toml --features e2e --bin e2e_harness
E2E_FIXTURES="$PWD/e2e/fixtures" src-tauri/target/debug/e2e_harness 127.0.0.1:7811 &
harness=$!
VITE_E2E_HARNESS=http://127.0.0.1:7811 VITE_E2E_WORKSPACE="$PWD/e2e/fixtures/shop" node_modules/.bin/vite dev --port 1499 --strictPort &
vite=$!
trap 'kill $harness $vite 2>/dev/null; e2e/services.sh down' EXIT INT TERM
wait $vite
