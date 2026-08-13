set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

pack := "cargo run --quiet --"

default:
    @just --list

check:
    cargo fmt -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    ./teakitw typecheck --timeout 120

fmt:
    cargo fmt

resolve:
    {{pack}} resolve

export: resolve
    {{pack}} export

verify: export
    {{pack}} verify

overlay: resolve
    {{pack}} overlay

# Dedicated-server boot of the locked server-side jars. Requires `modstage`.
run-server: overlay
    modstage --config generated/modstage.toml run server forever-world-server --timeout 180s

# Pastel lives in server/. Fetch the binary if this machine does not have it yet.
pastel:
    #!/usr/bin/env bash
    mkdir -p server
    if [[ ! -x server/pastel ]]; then
      (cd server && curl -fsSL https://kaf.sh/pastel/install.sh | sh)
    fi

# Install the exported archive into this repo's server folder.
pastel-install: export pastel
    ./server/pastel install "./dist/$({{pack}} name)" -dir server -yes
    (cd server && ./pastel refresh -dry-run)

# Dedicated server plus client, with TeaKit layered as a test extra.
pair: overlay
    ./teakitw pair \
      --node 26.2-fabric \
      --modstage-config generated/modstage.toml \
      --modstage-instance forever-world-pair \
      --test-file test/teakit/startup.test.ts \
      --timeout 360 \
      --report build/teakit/startup.json

publish-dry: export
    {{pack}} publish --dry-run
