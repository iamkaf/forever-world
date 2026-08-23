# Contributing to Forever World

This repository is the source for `com.iamkaf.modpacks:forever-world`. [Swatch](https://github.com/iamkaf/swatch) reads `pack.toml` and prepares the pack. `pack.lock.toml` records the exact installed files, including their hashes and download URLs.

## Pack entries

Most entries are one line:

```toml
[client_mods]
sodium = "mc26.2-0.9.1-fabric"
```

`[mods]` loads on both sides. `[client_mods]` stays off the server. `[server_mods]` stays off the client. `[shaders]` contains client shader packs.

## Install and run

Maintainers need Swatch on `PATH`, or can set `SWATCH_BIN` to its executable.

```bash
swatch install
swatch stage all
just run-client
just run-server
just run-pair
```

`swatch install` resolves and downloads the locked files. `swatch stage all` writes complete client and server trees under `build/stage/`. The `just` recipes stage those trees before launching the static Modstage client, server, or TeaKit pair. `just run-pair-xvfb` runs the pair in a background X server.

To check the project without launching Minecraft:

```bash
just check
```

## Versioning

Forever World versions describe what changed in the pack:

- Major: a Minecraft version bump.
- Minor: any mod, resource pack, or shader change.
- Patch: fixes to the glue that do not change those inputs.

## CurseForge mappings

CurseForge files are resolved with Packwiz and pinned in `pack.lock.toml`. Content exceptions under `[publish.curseforge]` refer to stable content IDs, not filenames. Run `swatch install --curseforge` when a changed pack needs new CurseForge mappings. Swatch runs `packwiz` from `PATH`; `PACKWIZ_BIN` can override the command.

## Release checks

`just publish-dry` prepares a publication preview without uploading. Swatch checks the manifest, lockfile, authored files, configured destinations, and prepared artifact hashes.

Run the Release workflow from `main` with the tag matching the version in `pack.toml`, such as `v1.2.0`. It stages the pack and reruns the TeaKit client and dedicated server pair under Xvfb before it prepares any release bytes. It then signs `release.json` through Sigstore, creates GitHub provenance attestations, and verifies both kinds of proof. The `publish` input controls whether that verified release goes to GitHub Releases, Modrinth, CurseForge, and Maven. Pull requests and ordinary pushes never publish.

The `pack-release` environment needs these secrets before publication:

- `MAVEN_PUBLISH_USERNAME` and `MAVEN_PUBLISH_PASSWORD` for numbered releases at `https://z.kaf.sh/releases`.
- `CURSEFORGE_TOKEN` for project `1663962`.
- `MODRINTH_TOKEN` for project `TRgAveYb`.

The workflow uses its GitHub token for GitHub Releases. Swatch prepares once, verifies the same files again in the publication job, and publishes that verified snapshot without rebuilding it.

CurseForge's author API cannot verify an existing upload before creating one. After an ambiguous network failure, inspect the project before retrying.

TeaKit is only for the pair check. It never goes in the pack, and it does not change Fabric Loader 0.19.3.
