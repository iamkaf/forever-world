#!/usr/bin/env python3
"""Check Forever World's save-safety and release invariants."""

from __future__ import annotations

from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]

SERVER_JARS = {
    "mods/CreativeCore_FABRIC_v2.14.16_mc26.2.jar",
    "mods/ForgeConfigAPIPort-v26.2.1-mc26.2.x-Fabric.jar",
    "mods/PuzzlesLib-v26.2.0-mc26.2.x-Fabric.jar",
    "mods/amber-fabric-11.1.2+26.2.jar",
    "mods/appleskin-fabric-mc26.2-3.0.10.jar",
    "mods/bonded-fabric-4.1.0+26.2.jar",
    "mods/c2me-fabric-mc26.2-0.4.2-alpha.0.13.jar",
    "mods/fabric-api-0.154.2+26.2.jar",
    "mods/fabric-language-kotlin-1.13.12+kotlin.2.4.0.jar",
    "mods/ferritecore-9.0.0-fabric.jar",
    "mods/happyghastimprovements-fabric-2.1.0+26.2.jar",
    "mods/jei-26.2-fabric-30.9.0.57.jar",
    "mods/kafvalentine-fabric-5.1.0+26.2.jar",
    "mods/konfig-fabric-0.5.0+26.2.jar",
    "mods/liteminer-fabric-4.1.1+26.2.jar",
    "mods/lithium-fabric-0.25.2+mc26.2.jar",
    "mods/mochila-fabric-6.1.0+26.2.jar",
    "mods/mru-1.0.30+26.2-fabric.jar",
    "mods/placeholder-api-3.1.0-beta.1+26.2.jar",
    "mods/sit-fabric-26.1.1-1.5.1.jar",
    "mods/snapshears-fabric-5.1.0+26.2.jar",
    "mods/sound-physics-remastered-fabric-1.5.1+26.2.jar",
    "mods/torchtoss-fabric-5.1.0+26.2.jar",
    "mods/yet_another_config_lib_v3-3.9.5+26.2-fabric.jar",
}

PERSISTENT_CONTENT_JARS = {
    "mods/amber-fabric-11.1.2+26.2.jar",
    "mods/bonded-fabric-4.1.0+26.2.jar",
    "mods/happyghastimprovements-fabric-2.1.0+26.2.jar",
    "mods/kafvalentine-fabric-5.1.0+26.2.jar",
    "mods/konfig-fabric-0.5.0+26.2.jar",
    "mods/liteminer-fabric-4.1.1+26.2.jar",
    "mods/mochila-fabric-6.1.0+26.2.jar",
    "mods/snapshears-fabric-5.1.0+26.2.jar",
    "mods/torchtoss-fabric-5.1.0+26.2.jar",
}


def fail(message: str) -> None:
    raise AssertionError(message)


def assert_equal(actual: object, expected: object, message: str) -> None:
    if actual != expected:
        fail(f"{message}: expected {expected!r}, got {actual!r}")


def load(name: str) -> dict:
    with (ROOT / name).open("rb") as stream:
        return tomllib.load(stream)


def content(pack: dict) -> list[tuple[str, str, str, str, str]]:
    entries: list[tuple[str, str, str, str, str]] = []
    for section, folder, client, server in (
        ("mods", "mods/", "required", "required"),
        ("client_mods", "mods/", "required", "unsupported"),
        ("server_mods", "mods/", "unsupported", "required"),
        ("shaders", "shaderpacks/", "required", "unsupported"),
    ):
        for identifier, version in pack.get(section, {}).items():
            entries.append((identifier, version, folder, client, server))
    return entries


def check() -> None:
    source = load("pack.toml")
    lock = load("pack.lock.toml")
    pack = source["pack"]

    assert_equal(pack["name"], "FOREVER WORLD", "pack name")
    assert_equal(pack["version"], "1.2.0", "pack version")
    assert_equal(pack["minecraft"], "26.2", "Minecraft version")
    assert_equal(pack["loader"], "fabric", "loader")
    assert_equal(pack["loader_version"], "0.19.3", "loader version")

    entries = content(source)
    assert_equal(len(entries), 48, "pack content count")
    assert_equal(lock["version"], 2, "lockfile version")
    assert_equal(lock["pack"], pack, "pack metadata differs between source and lockfile")
    files = lock["file"]
    assert_equal(len(files), len(entries), "lockfile content count")

    teakit = load("teakit.toml")
    nodes = teakit.get("nodes")
    if not isinstance(nodes, dict):
        fail("teakit.toml has no nodes table")
    node = f"{pack['minecraft']}-{pack['loader']}"
    assert_equal(len(nodes), 1, "TeaKit node count")
    if node not in nodes:
        fail(f"TeaKit is missing the pack node {node}")

    by_id = {file["id"]: file for file in files}
    assert_equal(len(by_id), len(files), "lockfile contains duplicate content IDs")
    for identifier, version, folder, client, server in entries:
        file = by_id.get(identifier)
        if file is None:
            fail(f"{identifier} is missing from the lock")
        assert_equal(file["requested_version"], version, f"{identifier} requested version")
        assert_equal(
            file["env"],
            {"client": client, "server": server},
            f"{identifier} side",
        )
        if not file["path"].startswith(folder):
            fail(f"{identifier} must be stored below {folder}, got {file['path']}")

    actual_server = {
        file["path"] for file in files if file["env"]["server"] != "unsupported"
    }
    assert_equal(
        actual_server,
        SERVER_JARS,
        "the dedicated-server jar set changed; review every new jar against the save promise",
    )
    for path in PERSISTENT_CONTENT_JARS:
        if path not in actual_server:
            fail(f"{path} must load on the server; the save already contains its content")

    for file in files:
        path = file["path"]
        if (
            path.startswith("shaderpacks/")
            or "sodium" in path
            or "iris-" in path
            or "kafhud" in path
            or "gentlehurtcam" in path
        ):
            assert_equal(
                file["env"]["server"],
                "unsupported",
                f"{path} must not load on the dedicated server",
            )

    shaders = [file["path"] for file in files if file["path"].startswith("shaderpacks/")]
    assert_equal(shaders, ["shaderpacks/ComplementaryUnbound_r5.7.1.zip"], "bundled shaders")

    mapped = {file["path"] for file in lock.get("curseforge", [])}
    unresolved = [
        file["path"]
        for file in files
        if file["env"]["client"] != "unsupported" and file["path"] not in mapped
    ]
    assert_equal(
        unresolved,
        ["mods/PresenceFootsteps-1.13.3+26.2.jar"],
        "unmapped CurseForge files",
    )
    assert_equal(len(lock.get("curseforge", [])), 47, "CurseForge mapping count")

    publish = source["publish"]
    assert_equal(publish["curseforge"], False, "direct CurseForge publishing")
    assert_equal(
        publish["maven"]["repository"],
        "https://z.kaf.sh/releases",
        "numbered Maven repository",
    )
    assert_equal(
        publish["github"]["repository"],
        "iamkaf/forever-world",
        "GitHub release repository",
    )
    assert_equal(
        publish["modrinth"]["project"],
        "forever-world",
        "Modrinth project",
    )


def main() -> int:
    try:
        check()
    except (AssertionError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        print(f"pack policy failed: {error}", file=sys.stderr)
        return 1
    print("pack policy ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
