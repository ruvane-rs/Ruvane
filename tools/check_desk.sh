#!/bin/sh
set -eu
# Run from the repository root with a Wasm-capable Rust toolchain.
cargo build --offline --locked --bin ruvane
cargo fmt --all --manifest-path apps/desk/Cargo.toml --check
export RUVANE_APPLICATION_REVISION=61
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
desk_target=${CARGO_TARGET_DIR:-target}
case "$desk_target" in /*) ;; *) desk_target="$(pwd)/$desk_target" ;; esac
# Library checks do not run this binary; the CLI builds Desk's own Wasm below.
RUVANE_WASM_FILE="$desk_target/wasm32-unknown-unknown/release/ruvane_browser.wasm"
export RUVANE_WASM_FILE
cargo test --manifest-path apps/desk/Cargo.toml --offline --locked --workspace --all-targets
cargo clippy --manifest-path apps/desk/Cargo.toml --offline --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path apps/desk/Cargo.toml --offline --locked --workspace --no-deps
RUVANE_CLI="$desk_target/debug/ruvane"
export RUVANE_CLI
node --test browser/tests/desk.test.mjs
