set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

pack := "cargo run --quiet --"

default:
    @just --list

check:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fmt -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    saved_tsconfig=$(mktemp)
    cp tsconfig.json "$saved_tsconfig"
    trap 'cp "$saved_tsconfig" tsconfig.json; rm -f "$saved_tsconfig"' EXIT
    ./teakitw typecheck --timeout 120

fmt:
    cargo fmt

install:
    {{pack}} install

run-client: install
    {{pack}} run client

run-server: install
    {{pack}} run server

# Dedicated server plus client, with TeaKit layered as a test extra.
run-pair: install
    {{pack}} run pair

publish-dry: install
    {{pack}} publish --dry-run

pair: run-pair
