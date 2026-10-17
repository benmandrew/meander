"""Development server for `web/`: sends every response with caching off, so
a reload always picks up the latest JS, CSS and WASM.

Development only. The deployed site (`nix build .#web-dist`) must set its
own caching headers on the host, since no-store there would refetch every
module and the WASM on each visit.
"""

import functools
import http.server
import sys


class NoCache(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
handler = functools.partial(NoCache, directory="web")
with http.server.ThreadingHTTPServer(("127.0.0.1", port), handler) as server:
    print(f"Serving web/ at http://127.0.0.1:{port}")
    server.serve_forever()
