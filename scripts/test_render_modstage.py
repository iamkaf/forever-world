#!/usr/bin/env python3
"""Focused regression proof for Modstage cache paths."""

from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
import tomllib
import unittest

from render_modstage import render


class RenderModstageTest(unittest.TestCase):
    def test_rendered_artifacts_use_existing_flat_cache_objects(self) -> None:
        files = [
            self.locked_file("mods/shared.jar", "a" * 128, "required", "required"),
            self.locked_file(
                "mods/client-only.jar", "b" * 128, "required", "unsupported"
            ),
            self.locked_file(
                "shaderpacks/shader.zip", "c" * 128, "required", "unsupported"
            ),
        ]
        lock = {
            "pack": {
                "slug": "test-pack",
                "minecraft": "26.2",
                "loader": "fabric",
                "loader_version": "0.19.3",
            },
            "file": files,
        }

        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            output_directory = root / "generated"
            output_directory.mkdir()
            objects = root / ".cache" / "objects"
            objects.mkdir(parents=True)
            for file in files:
                (objects / file["sha512"]).touch()

            rendered = render(lock)
            config = tomllib.loads(rendered)
            artifact_paths = [
                path
                for instance in config["instance"]
                for path in instance["mods"]
                if not path.startswith("maven:")
            ]
            artifact_paths.extend(
                fixture["from"]
                for instance in config["instance"]
                for fixture in instance.get("fixture", [])
            )

            self.assertTrue(artifact_paths)
            self.assertEqual(
                {Path(path).name for path in artifact_paths},
                {file["sha512"] for file in files},
            )
            for path in artifact_paths:
                self.assertTrue((output_directory / path).is_file(), path)
            fixture_destinations = {
                fixture["to"]
                for instance in config["instance"]
                for fixture in instance.get("fixture", [])
            }
            self.assertEqual(
                fixture_destinations,
                {file["path"] for file in files},
            )
            for file in files:
                self.assertNotIn(f"{file['sha512']}/{Path(file['path']).name}", rendered)

    @staticmethod
    def locked_file(path: str, sha512: str, client: str, server: str) -> dict:
        return {
            "path": path,
            "sha512": sha512,
            "env": {"client": client, "server": server},
        }


if __name__ == "__main__":
    unittest.main()
