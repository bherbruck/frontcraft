#!/usr/bin/env python3
"""Serves a web build with its precompressed files. Usage: serve_web.py [dir] [port]"""

import http.server
import os
import sys
from functools import partial

ENCODINGS = (("br", ".br"), ("gzip", ".gz"))


class Handler(http.server.SimpleHTTPRequestHandler):
    encoding = None
    stored_type = None

    def send_head(self):
        path = self.translate_path(self.path)
        accepted = self.headers.get("Accept-Encoding", "")
        if os.path.isfile(path):
            for encoding, suffix in ENCODINGS:
                if encoding in accepted and os.path.isfile(path + suffix):
                    self.encoding = encoding
                    self.stored_type = self.guess_type(path)
                    self.path = self.path.split("?")[0] + suffix
                    break
        return super().send_head()

    def guess_type(self, path):
        return self.stored_type or super().guess_type(path)

    def end_headers(self):
        if self.encoding:
            self.send_header("Content-Encoding", self.encoding)
        self.send_header("Vary", "Accept-Encoding")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


if __name__ == "__main__":
    directory = sys.argv[1] if len(sys.argv) > 1 else "bin/web"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8080
    server = http.server.ThreadingHTTPServer(("", port), partial(Handler, directory=directory))
    print(f"Serving {directory} on http://localhost:{port}")
    server.serve_forever()
