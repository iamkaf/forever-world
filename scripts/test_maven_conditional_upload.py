#!/usr/bin/env python3
"""Focused regression proof for conditional Maven metadata publication."""

from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Thread
import unittest

from maven_conditional_upload import PublishError, publish, publish_immutable


def metadata(*versions: str) -> bytes:
    latest = versions[-1]
    rows = "".join(f"      <version>{version}</version>\n" for version in versions)
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<metadata>\n"
        "  <groupId>com.iamkaf.modpacks</groupId>\n"
        "  <artifactId>forever-world</artifactId>\n"
        "  <versioning>\n"
        f"    <latest>{latest}</latest>\n"
        f"    <release>{latest}</release>\n"
        "    <versions>\n"
        f"{rows}"
        "    </versions>\n"
        "  </versioning>\n"
        "</metadata>\n"
    ).encode()


class MetadataHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        self.server.fetch_path = self.path
        self.server.fetch_cache_control = self.headers.get("Cache-Control")
        self.server.fetch_authorization = self.headers.get("Authorization")
        self.send_response(200)
        self.send_header("Content-Type", "application/xml")
        self.send_header("Content-Length", str(len(self.server.content)))
        if self.server.etag is not None:
            self.send_header("ETag", self.server.etag)
        self.end_headers()
        self.wfile.write(self.server.content)

    def do_PUT(self) -> None:
        self.server.put_path = self.path
        self.server.received_if_match = self.headers.get("If-Match")
        if self.server.mutate_before_put:
            self.server.content = self.server.concurrent_content
            self.server.etag = '"concurrent"'
        if self.server.received_if_match != self.server.etag:
            self.send_response(412)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        length = int(self.headers["Content-Length"])
        self.server.content = self.rfile.read(length)
        self.server.etag = '"published"'
        self.send_response(204)
        self.end_headers()

    def log_message(self, format: str, *args: object) -> None:
        pass


class ConditionalUploadTest(unittest.TestCase):
    def setUp(self) -> None:
        self.current = metadata("1.1.1")
        self.prepared = metadata("1.1.1", "1.2.0")
        self.concurrent = metadata("1.1.1", "1.2.1")
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MetadataHandler)
        self.server.content = self.current
        self.server.concurrent_content = self.concurrent
        self.server.etag = '"initial"'
        self.server.mutate_before_put = True
        self.server.received_if_match = None
        self.server.fetch_cache_control = None
        self.server.fetch_authorization = None
        self.server.fetch_path = None
        self.server.put_path = None
        self.thread = Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_concurrent_mutation_refuses_stale_metadata(self) -> None:
        with TemporaryDirectory() as temporary:
            source = Path(temporary) / "maven-metadata.xml"
            source.write_bytes(self.prepared)
            with self.assertRaisesRegex(PublishError, "changed after validation"):
                publish(source, self.url, "release-user", "release-password")
        self.assertEqual(self.server.received_if_match, '"initial"')
        self.assertEqual(self.server.fetch_cache_control, "no-cache")
        self.assertIsNone(self.server.fetch_authorization)
        self.assertEqual(self.server.content, self.concurrent)

    def test_missing_strong_validator_disables_publication(self) -> None:
        self.server.etag = None
        self.server.mutate_before_put = False
        with TemporaryDirectory() as temporary:
            source = Path(temporary) / "maven-metadata.xml"
            source.write_bytes(self.prepared)
            with self.assertRaisesRegex(PublishError, "strong ETag"):
                publish(source, self.url, "release-user", "release-password")
        self.assertIsNone(self.server.received_if_match)
        self.assertEqual(self.server.content, self.current)

    def test_public_read_origin_can_differ_from_authenticated_write_origin(self) -> None:
        self.server.mutate_before_put = False
        with TemporaryDirectory() as temporary:
            source = Path(temporary) / "maven-metadata.xml"
            source.write_bytes(self.prepared)
            publish(
                source,
                self.url_for("/write/maven-metadata.xml"),
                "release-user",
                "release-password",
                self.url_for("/read/maven-metadata.xml"),
            )
        self.assertEqual(self.server.fetch_path, "/read/maven-metadata.xml")
        self.assertEqual(self.server.put_path, "/write/maven-metadata.xml")

    @property
    def url(self) -> str:
        return self.url_for("/maven-metadata.xml")

    def url_for(self, path: str) -> str:
        host, port = self.server.server_address
        return f"http://{host}:{port}{path}"


class ImmutableHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        self.server.fetch_authorization = self.headers.get("Authorization")
        self.server.fetch_user_agent = self.headers.get("User-Agent")
        if self.server.content is None:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(self.server.content)))
        self.end_headers()
        self.wfile.write(self.server.content)

    def do_PUT(self) -> None:
        self.server.put_authorization = self.headers.get("Authorization")
        self.server.put_user_agent = self.headers.get("User-Agent")
        length = int(self.headers["Content-Length"])
        self.server.content = self.rfile.read(length)
        self.send_response(204)
        self.end_headers()

    def log_message(self, format: str, *args: object) -> None:
        pass


class ImmutableUploadTest(unittest.TestCase):
    def setUp(self) -> None:
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), ImmutableHandler)
        self.server.content = None
        self.server.fetch_authorization = None
        self.server.fetch_user_agent = None
        self.server.put_authorization = None
        self.server.put_user_agent = None
        self.thread = Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_missing_file_is_read_publicly_then_uploaded_with_auth(self) -> None:
        with TemporaryDirectory() as temporary:
            source = Path(temporary) / "pack.mrpack"
            source.write_bytes(b"signed release bytes")
            publish_immutable(
                source,
                self.url,
                self.url,
                "release-user",
                "release-password",
            )
        self.assertIsNone(self.server.fetch_authorization)
        self.assertEqual(self.server.fetch_user_agent, "forever-world-release/1")
        self.assertEqual(
            self.server.put_authorization,
            "Basic cmVsZWFzZS11c2VyOnJlbGVhc2UtcGFzc3dvcmQ=",
        )
        self.assertEqual(self.server.put_user_agent, "forever-world-release/1")
        self.assertEqual(self.server.content, b"signed release bytes")

    def test_matching_file_is_not_uploaded_again(self) -> None:
        self.server.content = b"signed release bytes"
        with TemporaryDirectory() as temporary:
            source = Path(temporary) / "pack.mrpack"
            source.write_bytes(self.server.content)
            publish_immutable(
                source,
                self.url,
                self.url,
                "release-user",
                "release-password",
            )
        self.assertIsNone(self.server.put_authorization)

    @property
    def url(self) -> str:
        host, port = self.server.server_address
        return f"http://{host}:{port}/pack.mrpack"


if __name__ == "__main__":
    unittest.main()
