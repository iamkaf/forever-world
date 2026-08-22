#!/usr/bin/env python3
"""Publish signed Maven files without overwriting existing release bytes."""

from __future__ import annotations

from base64 import b64encode
from pathlib import Path
import argparse
import os
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

from artifact_proof import verify_maven_update_documents


class PublishError(RuntimeError):
    """The metadata could not be published without weakening release safety."""


def authorization(username: str, password: str) -> str:
    encoded = b64encode(f"{username}:{password}".encode()).decode("ascii")
    return f"Basic {encoded}"


def is_strong_etag(value: str | None) -> bool:
    return bool(
        value
        and not value.startswith("W/")
        and len(value) >= 2
        and value.startswith('"')
        and value.endswith('"')
        and "\r" not in value
        and "\n" not in value
    )


def fetch(destination: str) -> tuple[int, bytes, str | None]:
    request = urllib.request.Request(
        destination,
        headers={
            "Cache-Control": "no-cache",
            "Pragma": "no-cache",
            "User-Agent": "forever-world-release/1",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, response.read(1024 * 1024 + 1), response.headers.get("ETag")
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()
        if status == 404:
            return 404, b"", None
        raise PublishError(f"Maven metadata lookup failed with HTTP {status}") from error


def compare_remote(source: Path, destination: str) -> tuple[int, bool]:
    request = urllib.request.Request(
        destination,
        headers={
            "Cache-Control": "no-cache",
            "Pragma": "no-cache",
            "User-Agent": "forever-world-release/1",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            with source.open("rb") as local:
                while True:
                    remote_block = response.read(1024 * 1024)
                    local_block = local.read(1024 * 1024)
                    if remote_block != local_block:
                        return response.status, False
                    if not remote_block:
                        return response.status, True
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()
        if status == 404:
            return 404, False
        raise PublishError(f"Maven artifact lookup failed with HTTP {status}") from error


def publish_immutable(
    source: Path,
    destination: str,
    read_destination: str,
    username: str,
    password: str,
) -> None:
    status, matches = compare_remote(source, read_destination)
    if status == 200:
        if matches:
            print(f"{source.name} is already published")
            return
        raise PublishError(f"Maven already has different bytes for {source.name}")
    if status != 404:
        raise PublishError(f"Maven artifact lookup failed with HTTP {status}")

    request = urllib.request.Request(
        destination,
        data=source.read_bytes(),
        headers={
            "Authorization": authorization(username, password),
            "Content-Type": "application/octet-stream",
            "User-Agent": "forever-world-release/1",
        },
        method="PUT",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            if response.status not in {200, 201, 204}:
                raise PublishError(
                    f"immutable Maven upload failed with HTTP {response.status}"
                )
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()
        if status == 409:
            _, matches = compare_remote(source, read_destination)
            if matches:
                print(f"{source.name} is already published")
                return
        raise PublishError(f"immutable Maven upload failed with HTTP {status}") from error
    print(f"published immutable Maven file {source.name}")


def publish(
    source: Path,
    destination: str,
    username: str,
    password: str,
    read_destination: str | None = None,
) -> None:
    prepared = source.read_bytes()
    auth = authorization(username, password)
    status, current, etag = fetch(read_destination or destination)
    if len(current) > 1024 * 1024:
        raise PublishError("published Maven metadata is unexpectedly large")

    headers = {
        "Authorization": auth,
        "Cache-Control": "no-cache",
        "Content-Type": "application/xml",
        "User-Agent": "forever-world-release/1",
    }
    if status == 200:
        if current == prepared:
            print("signed Maven metadata is already published")
            return
        if not is_strong_etag(etag):
            raise PublishError(
                "Maven metadata GET did not return a strong ETag; conditional publication is disabled"
            )
        verify_maven_update_documents(prepared, current)
        headers["If-Match"] = etag
    elif status == 404:
        headers["If-None-Match"] = "*"
    else:
        raise PublishError(f"Maven metadata lookup failed with HTTP {status}")

    request = urllib.request.Request(
        destination,
        data=prepared,
        headers=headers,
        method="PUT",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status not in {200, 201, 204}:
                raise PublishError(
                    f"conditional Maven metadata upload failed with HTTP {response.status}"
                )
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()
        if status == 412:
            raise PublishError(
                "Maven metadata changed after validation; conditional upload refused stale bytes"
            ) from error
        raise PublishError(
            f"conditional Maven metadata upload failed with HTTP {status}"
        ) from error
    print("published signed Maven metadata with an atomic precondition")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", required=True)
    parser.add_argument("--read-destination")
    parser.add_argument("--immutable", action="store_true")
    args = parser.parse_args()
    username = os.environ.get("MAVEN_PUBLISH_USERNAME", "")
    password = os.environ.get("MAVEN_PUBLISH_PASSWORD", "")
    if not username or not password:
        print("Maven publication credentials are not configured", file=sys.stderr)
        return 1
    try:
        if args.immutable:
            if not args.read_destination:
                raise PublishError("immutable publication requires --read-destination")
            publish_immutable(
                args.source,
                args.destination,
                args.read_destination,
                username,
                password,
            )
        else:
            publish(
                args.source,
                args.destination,
                username,
                password,
                args.read_destination,
            )
    except (OSError, PublishError, ET.ParseError) as error:
        print(f"Maven metadata publication failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
