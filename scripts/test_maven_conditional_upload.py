#!/usr/bin/env python3
"""Focused regression proof for conditional Maven metadata publication."""

from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Thread
import unittest

from maven_conditional_upload import PublishError, publish


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

    @property
    def url(self) -> str:
        host, port = self.server.server_address
        return f"http://{host}:{port}/maven-metadata.xml"


if __name__ == "__main__":
    unittest.main()
