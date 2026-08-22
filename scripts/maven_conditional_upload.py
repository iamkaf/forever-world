#!/usr/bin/env python3
"""Publish signed Maven metadata without accepting a lost update."""

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


def publish(source: Path, destination: str, username: str, password: str) -> None:
    prepared = source.read_bytes()
    auth = authorization(username, password)
    status, current, etag = fetch(destination)
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
    args = parser.parse_args()
    username = os.environ.get("MAVEN_PUBLISH_USERNAME", "")
    password = os.environ.get("MAVEN_PUBLISH_PASSWORD", "")
    if not username or not password:
        print("Maven publication credentials are not configured", file=sys.stderr)
        return 1
    try:
        publish(args.source, args.destination, username, password)
    except (OSError, PublishError, ET.ParseError) as error:
        print(f"Maven metadata publication failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
