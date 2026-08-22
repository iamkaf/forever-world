set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

swatch_bin := env_var_or_default("SWATCH_BIN", "swatch")

default:
    @just --list

check:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 scripts/check_pack_policy.py
    saved_tsconfig=$(mktemp)
    cp tsconfig.json "$saved_tsconfig"
    trap 'cp "$saved_tsconfig" tsconfig.json; rm -f "$saved_tsconfig"' EXIT
    ./teakitw typecheck --timeout 120

_require-swatch:
    #!/usr/bin/env bash
    if ! command -v "{{ swatch_bin }}" >/dev/null 2>&1; then
    echo "swatch is not installed; set SWATCH_BIN to the Swatch executable" >&2
    exit 1
    fi

_swatch-install: _require-swatch
    "{{ swatch_bin }}" install

install: _swatch-install render-modstage

install-locked: _require-swatch
    #!/usr/bin/env bash
    set -euo pipefail
    before=$(sha256sum pack.toml pack.lock.toml overrides.toml release.toml)
    "{{ swatch_bin }}" install
    after=$(sha256sum pack.toml pack.lock.toml overrides.toml release.toml)
    if [[ "$before" != "$after" ]]; then
    echo "swatch install changed the locked pack" >&2
    exit 1
    fi

render-modstage:
    python3 scripts/render_modstage.py

run-client: install
    modstage --config generated/modstage.toml run client forever-world-client --timeout 180s

run-server: install
    modstage --config generated/modstage.toml run server forever-world-server --timeout 180s

run-pair: install
    mkdir -p build/teakit
    ./teakitw pair --no-sync-sdk --node 26.2-fabric --modstage-config generated/modstage.toml --modstage-instance forever-world-pair --test-file tests/teakit/startup.test.ts --timeout 360 --report build/teakit/startup.json

client: run-client

server: run-server

pair: run-pair

artifact-proof: _require-swatch
    python3 scripts/artifact_proof.py prepare

publish-dry: install-locked artifact-proof
