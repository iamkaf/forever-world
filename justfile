set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

swatch_bin := env_var_or_default("SWATCH_BIN", "swatch")

default:
    @just --list

_require-swatch:
    #!/usr/bin/env bash
    if ! command -v "{{ swatch_bin }}" >/dev/null 2>&1; then
        echo "swatch is not installed; set SWATCH_BIN to the Swatch executable" >&2
        exit 1
    fi

install: _require-swatch
    "{{ swatch_bin }}" install

install-locked: _require-swatch
    #!/usr/bin/env bash
    set -euo pipefail
    before=$(sha256sum pack.toml pack.lock.toml overrides.toml)
    "{{ swatch_bin }}" install
    after=$(sha256sum pack.toml pack.lock.toml overrides.toml)
    if [[ "$before" != "$after" ]]; then
        echo "swatch install changed the locked pack" >&2
        exit 1
    fi

teakit-typecheck:
    scripts/check

artifact-check: install-locked
    "{{ swatch_bin }}" prepare
    "{{ swatch_bin }}" verify

check: install-locked
    scripts/check
    "{{ swatch_bin }}" prepare
    "{{ swatch_bin }}" verify

stage: install-locked
    "{{ swatch_bin }}" stage all

run-client: stage
    modstage --config modstage.toml run client forever-world-client --timeout 180s

run-server: stage
    modstage --config modstage.toml run server forever-world-server --timeout 180s

run-pair: stage
    mkdir -p build/teakit
    ./teakitw pair --no-sync-sdk --node 26.2-fabric --modstage-config modstage.toml --modstage-instance forever-world-pair --test-file tests/teakit/startup.test.ts --timeout 360 --report build/teakit/startup.json

runtime-check: install-locked
    SWATCH_BIN="{{ swatch_bin }}" scripts/check-runtime

run-pair-xvfb: runtime-check

client: run-client

server: run-server

pair: run-pair

publish-dry: install-locked
    "{{ swatch_bin }}" publish --dry-run
