#!/usr/bin/env python3
import http.server
import mimetypes
import os
import socketserver
import sys

PORT = 8000
WORKSPACE_DIR = os.getcwd()

# Ensure WASM files are served with the correct MIME type so that
# WebAssembly.instantiateStreaming() works (browsers reject it otherwise).
mimetypes.add_type("application/wasm", ".wasm")
mimetypes.add_type("application/manifest+json", ".webmanifest")


class StaveloomPlayerHandler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        clean_path = path.split("?", 1)[0].split("#", 1)[0]
        if clean_path == "/":
            clean_path = "/index.html"
        return os.path.join(WORKSPACE_DIR, "web", clean_path.lstrip("/"))

    def end_headers(self):
        if self.path.endswith("sw.js"):
            self.send_header("Service-Worker-Allowed", "/")
        super().end_headers()

    def log_message(self, format, *args):
        # Suppress per-request logs; only errors are interesting
        if args and len(args) >= 2 and str(args[1]) not in ("200", "304"):
            super().log_message(format, *args)


def run():
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", PORT), StaveloomPlayerHandler) as httpd:
        print(f"Serving staveloom Web Player on http://localhost:{PORT}")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down.")
            sys.exit(0)


if __name__ == "__main__":
    run()
