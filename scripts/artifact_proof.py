#!/usr/bin/env python3
"""Prepare and verify Forever World's release artifacts without uploading them."""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256, sha512
from pathlib import Path
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "build"
SWATCH_VERSION = "0.1.1"
PREPARE_PROJECT_SENTINEL = 9_223_372_036_854_775_807
CURSEFORGE_AUTHOR = "iamkaf"
PROOF_FILES = {"release-manifest.sigstore.json", "github-provenance.jsonl"}


@dataclass(frozen=True)
class PreparedFile:
    name: str
    kind: str
    destinations: tuple[str, ...]


def fail(message: str) -> None:
    raise RuntimeError(message)


def load_toml(path: Path) -> dict:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def digest(path: Path, algorithm: str) -> str:
    hasher = sha256() if algorithm == "sha256" else sha512()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def pack_data() -> tuple[dict, dict]:
    source = load_toml(ROOT / "pack.toml")
    lock = load_toml(ROOT / "pack.lock.toml")
    if source["pack"] != lock["pack"]:
        fail("pack.toml and pack.lock.toml disagree on pack metadata")
    return source, lock


def content_count(source: dict) -> int:
    return sum(
        len(source.get(section, {}))
        for section in ("mods", "client_mods", "server_mods", "shaders")
    )


def validate_source(tag: str | None) -> dict:
    source, lock = pack_data()
    pack = source["pack"]
    if tag is not None and tag != f"v{pack['version']}":
        fail(f"tag {tag!r} does not match pack version v{pack['version']}")
    if content_count(source) != 48 or len(lock["file"]) != 48:
        fail("Forever World must keep all 48 canonical content identities")
    curseforge = lock.get("curseforge", [])
    if len(curseforge) != 47:
        fail("the CurseForge release must contain exactly 47 mapped files")
    locked_ids = {item["id"] for item in lock["file"]}
    if "presence-footsteps" not in locked_ids:
        fail("Presence Footsteps is missing from the canonical pack")
    mapped_paths = {item["path"] for item in curseforge}
    presence = next(item for item in lock["file"] if item["id"] == "presence-footsteps")
    if presence["path"] in mapped_paths:
        fail("Presence Footsteps must stay excluded from the CurseForge edition")
    if source["publish"]["maven"]["repository"] != "https://z.kaf.sh/releases":
        fail("numbered pack releases must use the Maven releases repository")
    return pack


def release_pack_toml() -> str:
    text = (ROOT / "pack.toml").read_text(encoding="utf-8")
    marker = "curseforge = false"
    if text.count(marker) != 1:
        fail("pack.toml must contain one explicit disabled CurseForge target")
    replacement = (
        "[publish.curseforge]\n"
        f"project = {PREPARE_PROJECT_SENTINEL}\n"
        f'author = "{CURSEFORGE_AUTHOR}"'
    )
    return text.replace(marker, replacement)


def copy_release_source(destination: Path) -> None:
    for name in ("pack.lock.toml", "overrides.toml", "CHANGELOG.md"):
        shutil.copy2(ROOT / name, destination / name)
    (destination / "pack.toml").write_text(release_pack_toml(), encoding="utf-8")
    for name in ("overrides", "client-overrides", "server-overrides"):
        source = ROOT / name
        if source.is_dir():
            shutil.copytree(source, destination / name)


def version_key(version: str) -> tuple[int, int, int, int] | tuple[int, str]:
    parts = version.split(".")
    if len(parts) == 3 and all(part.isdigit() for part in parts):
        return (0, *(int(part) for part in parts))
    return (1, version)


def merge_maven_metadata(directory: Path, source: dict, pack: dict) -> None:
    repository = source["publish"]["maven"]["repository"].rstrip("/")
    group_path = pack["group"].replace(".", "/")
    url = f"{repository}/{group_path}/{pack['slug']}/maven-metadata.xml"
    versions: set[str] = set()
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "forever-world-release/1"})
        with urllib.request.urlopen(request, timeout=30) as response:
            body = response.read(1024 * 1024 + 1)
        if len(body) > 1024 * 1024:
            fail("published Maven metadata is unexpectedly large")
        root = ET.fromstring(body)
        if root.findtext("groupId") != pack["group"]:
            fail("published Maven metadata has the wrong groupId")
        if root.findtext("artifactId") != pack["slug"]:
            fail("published Maven metadata has the wrong artifactId")
        versions.update(
            element.text
            for element in root.findall("./versioning/versions/version")
            if element.text
        )
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
    versions.add(pack["version"])
    ordered = sorted(versions, key=version_key)
    latest = ordered[-1]
    rows = "".join(f"      <version>{version}</version>\n" for version in ordered)
    metadata = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<metadata>\n"
        f"  <groupId>{pack['group']}</groupId>\n"
        f"  <artifactId>{pack['slug']}</artifactId>\n"
        "  <versioning>\n"
        f"    <latest>{latest}</latest>\n"
        f"    <release>{latest}</release>\n"
        "    <versions>\n"
        f"{rows}"
        "    </versions>\n"
        "  </versioning>\n"
        "</metadata>\n"
    )
    (directory / "maven-metadata.xml").write_text(metadata, encoding="utf-8")


def prepared_files(pack: dict) -> list[PreparedFile]:
    slug = pack["slug"]
    version = pack["version"]
    return [
        PreparedFile(
            f"{slug}-{version}.mrpack",
            "modrinth-pack",
            ("github", "maven", "modrinth"),
        ),
        PreparedFile(
            f"{slug}-{version}-curseforge.zip",
            "curseforge-pack",
            ("curseforge", "github"),
        ),
        PreparedFile(
            f"{slug}-{version}.pom",
            "maven-pom",
            ("github", "maven"),
        ),
        PreparedFile(
            "maven-metadata.xml",
            "maven-metadata",
            ("github", "maven"),
        ),
        PreparedFile(
            "release-notes.md",
            "release-notes",
            ("curseforge", "github", "modrinth"),
        ),
    ]


def safe_output(path: Path) -> Path:
    resolved = path.resolve()
    build = BUILD.resolve()
    if build not in resolved.parents:
        fail(f"release output must stay below {build}")
    return resolved


def write_sidecars(directory: Path, files: list[PreparedFile]) -> list[PreparedFile]:
    sidecars = sidecar_files(files)
    for sidecar in sidecars:
        algorithm = sidecar.kind.removesuffix("-checksum")
        parent = directory / sidecar.name.removesuffix(f".{algorithm}")
        (directory / sidecar.name).write_text(
            digest(parent, algorithm), encoding="ascii"
        )
    return sidecars


def sidecar_files(files: list[PreparedFile]) -> list[PreparedFile]:
    sidecars: list[PreparedFile] = []
    for item in files:
        destinations = tuple(
            destination for destination in item.destinations if destination in {"github", "maven"}
        )
        for algorithm in ("sha256", "sha512"):
            sidecars.append(
                PreparedFile(
                    f"{item.name}.{algorithm}",
                    f"{algorithm}-checksum",
                    destinations,
                )
            )
    return sidecars


def aggregate_files() -> list[PreparedFile]:
    return [
        PreparedFile("SHA256SUMS", "sha256-manifest", ("github",)),
        PreparedFile("SHA512SUMS", "sha512-manifest", ("github",)),
    ]


def write_aggregate_checksums(
    directory: Path, files: list[PreparedFile]
) -> list[PreparedFile]:
    result = aggregate_files()
    for item in result:
        algorithm = item.kind.removesuffix("-manifest")
        rows = [
            f"{digest(directory / subject.name, algorithm)}  {subject.name}\n"
            for subject in sorted(files, key=lambda subject: subject.name)
        ]
        (directory / item.name).write_text("".join(rows), encoding="ascii")
    return result


def manifest_entry(directory: Path, item: PreparedFile) -> dict:
    path = directory / item.name
    return {
        "name": item.name,
        "kind": item.kind,
        "bytes": path.stat().st_size,
        "sha256": digest(path, "sha256"),
        "sha512": digest(path, "sha512"),
        "destinations": list(item.destinations),
    }


def prepare(output: Path, swatch: str) -> None:
    pack = validate_source(None)
    source_manifest, _ = pack_data()
    output = safe_output(output)
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    BUILD.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="release-source-", dir=BUILD) as temporary:
        source = Path(temporary)
        copy_release_source(source)
        completed = subprocess.run(
            [swatch, "publish", "--dry-run"],
            cwd=source,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        )
        print(completed.stdout, end="")
        for destination in ("Modrinth", "CurseForge", "GitHub", "Maven"):
            if f"DRY {destination} " not in completed.stdout:
                fail(f"Swatch did not prepare the {destination} destination")
        base = prepared_files(pack)
        for item in base[:-1]:
            source_path = source / "dist" / item.name
            if not source_path.is_file():
                fail(f"Swatch did not prepare {item.name}")
            shutil.copy2(source_path, output / item.name)
        merge_maven_metadata(output, source_manifest, pack)
        shutil.copy2(ROOT / "CHANGELOG.md", output / "release-notes.md")

    sidecars = write_sidecars(output, base)
    sums = write_aggregate_checksums(output, base + sidecars)
    all_files = base + sidecars + sums
    source, lock = pack_data()
    manifest = {
        "schema_version": 1,
        "generator": f"Forever World artifact proof with Swatch {SWATCH_VERSION}",
        "pack": pack,
        "canonical_content_count": content_count(source),
        "curseforge": {
            "content_count": len(lock["curseforge"]),
            "excluded": [
                {
                    "id": "presence-footsteps",
                    "reason": "No Minecraft 26.2 file is available on CurseForge.",
                }
            ],
        },
        "destinations": {
            "github": source["publish"]["github"],
            "maven": source["publish"]["maven"],
            "curseforge": {"author": CURSEFORGE_AUTHOR, "project": None},
            "modrinth": source["publish"]["modrinth"],
        },
        "artifacts": [
            manifest_entry(output, item)
            for item in sorted(all_files, key=lambda item: item.name)
        ],
    }
    (output / "release-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    verify(output, None, False)


def read_json_member(archive: Path, name: str) -> dict:
    with zipfile.ZipFile(archive) as bundle:
        with bundle.open(name) as stream:
            return json.load(stream)


def verify_archives(directory: Path, pack: dict, lock: dict) -> None:
    slug = pack["slug"]
    version = pack["version"]
    modrinth = read_json_member(directory / f"{slug}-{version}.mrpack", "modrinth.index.json")
    if modrinth["versionId"] != version or modrinth["name"] != pack["name"]:
        fail("Modrinth pack identity does not match pack.toml")
    dependencies = modrinth["dependencies"]
    if dependencies.get("minecraft") != pack["minecraft"]:
        fail("Modrinth pack has the wrong Minecraft version")
    loader_key = f"{pack['loader']}-loader"
    if dependencies.get(loader_key) != pack["loader_version"]:
        fail("Modrinth pack has the wrong loader version")
    if len(modrinth["files"]) != 48:
        fail("Modrinth pack must contain all 48 canonical identities")

    curseforge = read_json_member(
        directory / f"{slug}-{version}-curseforge.zip", "manifest.json"
    )
    if curseforge["version"] != version or curseforge["name"] != pack["name"]:
        fail("CurseForge pack identity does not match pack.toml")
    if curseforge["author"] != CURSEFORGE_AUTHOR:
        fail("CurseForge pack has the wrong author")
    if curseforge["minecraft"]["version"] != pack["minecraft"]:
        fail("CurseForge pack has the wrong Minecraft version")
    expected_loader = f"{pack['loader']}-{pack['loader_version']}"
    loaders = curseforge["minecraft"]["modLoaders"]
    if loaders != [{"id": expected_loader, "primary": True}]:
        fail("CurseForge pack has the wrong loader")
    expected_files = sorted(
        (item["project_id"], item["file_id"], True) for item in lock["curseforge"]
    )
    actual_files = sorted(
        (item["projectID"], item["fileID"], item["required"])
        for item in curseforge["files"]
    )
    if actual_files != expected_files:
        fail("CurseForge pack does not match the 47 locked mappings")


def verify_maven(directory: Path, pack: dict) -> None:
    namespace = {"m": "http://maven.apache.org/POM/4.0.0"}
    pom = ET.parse(directory / f"{pack['slug']}-{pack['version']}.pom").getroot()
    expected = {
        "groupId": pack["group"],
        "artifactId": pack["slug"],
        "version": pack["version"],
    }
    for key, value in expected.items():
        element = pom.find(f"m:{key}", namespace)
        if element is None or element.text != value:
            fail(f"Maven POM has the wrong {key}")
    metadata = ET.parse(directory / "maven-metadata.xml").getroot()
    if metadata.findtext("groupId") != pack["group"]:
        fail("Maven metadata has the wrong groupId")
    if metadata.findtext("artifactId") != pack["slug"]:
        fail("Maven metadata has the wrong artifactId")
    versions = [element.text for element in metadata.findall("./versioning/versions/version")]
    if versions != ["1.1.1", pack["version"]]:
        fail("Maven metadata must retain 1.1.1 and add the numbered release")
    if metadata.findtext("./versioning/latest") != pack["version"]:
        fail("Maven metadata latest version does not match the pack")
    if metadata.findtext("./versioning/release") != pack["version"]:
        fail("Maven metadata release version does not match the pack")


def verify_checksums(directory: Path, entries: dict[str, dict]) -> None:
    for entry in entries.values():
        path = directory / entry["name"]
        if path.stat().st_size != entry["bytes"]:
            fail(f"byte count changed for {entry['name']}")
        for algorithm in ("sha256", "sha512"):
            if digest(path, algorithm) != entry[algorithm]:
                fail(f"{algorithm.upper()} mismatch for {entry['name']}")
    aggregate_subjects = sorted(
        name for name in entries if name not in {"SHA256SUMS", "SHA512SUMS"}
    )
    for algorithm, name in (("sha256", "SHA256SUMS"), ("sha512", "SHA512SUMS")):
        expected = "".join(
            f"{digest(directory / subject, algorithm)}  {subject}\n"
            for subject in aggregate_subjects
        )
        if (directory / name).read_text(encoding="ascii") != expected:
            fail(f"{name} does not describe the prepared files")
    for name in entries:
        if name.endswith(".sha256"):
            parent = directory / name.removesuffix(".sha256")
            if (directory / name).read_text(encoding="ascii") != digest(parent, "sha256"):
                fail(f"raw SHA-256 sidecar mismatch for {parent.name}")
        if name.endswith(".sha512"):
            parent = directory / name.removesuffix(".sha512")
            if (directory / name).read_text(encoding="ascii") != digest(parent, "sha512"):
                fail(f"raw SHA-512 sidecar mismatch for {parent.name}")


def verify(directory: Path, tag: str | None, allow_proof: bool) -> None:
    pack = validate_source(tag)
    source, lock = pack_data()
    manifest_path = directory / "release-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1 or manifest.get("pack") != pack:
        fail("release manifest does not match pack.toml")
    if manifest.get("canonical_content_count") != 48:
        fail("release manifest must record all 48 canonical identities")
    curseforge = manifest.get("curseforge", {})
    excluded = curseforge.get("excluded", [])
    if curseforge.get("content_count") != 47 or excluded != [
        {
            "id": "presence-footsteps",
            "reason": "No Minecraft 26.2 file is available on CurseForge.",
        }
    ]:
        fail("release manifest must record the Presence Footsteps exception")
    expected_destinations = {
        "github": source["publish"]["github"],
        "maven": source["publish"]["maven"],
        "curseforge": {"author": CURSEFORGE_AUTHOR, "project": None},
        "modrinth": source["publish"]["modrinth"],
    }
    if manifest.get("destinations") != expected_destinations:
        fail("release manifest destinations do not match pack.toml")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        fail("release manifest has no artifacts")
    entries = {entry["name"]: entry for entry in artifacts}
    if len(entries) != len(artifacts):
        fail("release manifest contains duplicate artifact names")
    for name in entries:
        if Path(name).name != name:
            fail(f"unsafe artifact name in release manifest: {name}")
    base = prepared_files(pack)
    expected_metadata = {
        item.name: (item.kind, list(item.destinations))
        for item in base + sidecar_files(base) + aggregate_files()
    }
    actual_metadata = {
        name: (entry.get("kind"), entry.get("destinations"))
        for name, entry in entries.items()
    }
    if actual_metadata != expected_metadata:
        fail("release manifest artifact roles or destinations changed")
    expected_files = set(entries) | {"release-manifest.json"}
    if allow_proof:
        expected_files |= PROOF_FILES
    actual_files = {path.name for path in directory.iterdir() if path.is_file()}
    if actual_files != expected_files:
        fail(
            "release directory differs from the signed artifact set: "
            f"expected {sorted(expected_files)}, got {sorted(actual_files)}"
        )
    verify_checksums(directory, entries)
    verify_archives(directory, pack, lock)
    verify_maven(directory, pack)
    notes = (directory / "release-notes.md").read_text(encoding="utf-8")
    if f"## {pack['version']}" not in notes:
        fail("release notes do not describe the pack version")
    print(
        f"{pack['name']} {pack['version']} | Minecraft {pack['minecraft']} | "
        f"{pack['loader'].capitalize()} Loader {pack['loader_version']}"
    )
    print("content: 48 canonical identities; CurseForge: 47, excluding presence-footsteps")
    for entry in artifacts:
        if entry["kind"] in {
            "modrinth-pack",
            "curseforge-pack",
            "maven-pom",
            "maven-metadata",
        }:
            print(f"{entry['name']}  sha256:{entry['sha256']}  sha512:{entry['sha512']}")


def clean_source(tag: str) -> None:
    validate_source(tag)
    completed = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    )
    if completed.stdout:
        fail("release tag must point to a clean source checkout")
    print(f"source is clean and {tag} matches pack.toml")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate-source")
    validate_parser.add_argument("--tag", required=True)
    prepare_parser = subparsers.add_parser("prepare")
    prepare_parser.add_argument("--output", type=Path, default=BUILD / "release")
    prepare_parser.add_argument(
        "--swatch", default=os.environ.get("SWATCH_BIN", "swatch")
    )
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--directory", type=Path, default=BUILD / "release")
    verify_parser.add_argument("--tag")
    verify_parser.add_argument("--allow-proof", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "validate-source":
            clean_source(args.tag)
        elif args.command == "prepare":
            prepare(args.output, args.swatch)
        else:
            verify(args.directory, args.tag, args.allow_proof)
    except (
        ET.ParseError,
        KeyError,
        OSError,
        RuntimeError,
        ValueError,
        subprocess.CalledProcessError,
        zipfile.BadZipFile,
    ) as error:
        print(f"artifact proof failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
