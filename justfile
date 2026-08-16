set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

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
