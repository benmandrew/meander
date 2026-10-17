"""Browser smoke test: load the page in headless Chrome, let the river run,
and fail on any page error, or if the channel stops being redrawn.

Usage: python3 test/smoke.py [web_dir]. Serves web_dir (default `web/`,
which `make build` must have filled) on a free port, warms up for
`WARMUP` units of simulated time (50 years), then watches the page for
`RUN` seconds.

It drives the installed Google Chrome, so the dev shell needs no browser
bundle; set `PLAYWRIGHT_BROWSERS_PATH` to use Playwright's own Chromium.
"""

import functools
import http.server
import os
import sys
import threading

from playwright.sync_api import sync_playwright

WARMUP = 20
RUN = 8


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        pass


def launch(p):
    if os.environ.get("PLAYWRIGHT_BROWSERS_PATH"):
        return p.chromium.launch()
    return p.chromium.launch(channel="chrome")


def channel(page):
    # All path data in the top layer, which changes as the channel migrates.
    return page.evaluate(
        "[...document.querySelectorAll('#view path')].map((p) => p.getAttribute('d')).join()"
    )


def main():
    web = sys.argv[1] if len(sys.argv) > 1 else "web"
    handler = functools.partial(Quiet, directory=web)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/?warmup={WARMUP}"

    errors = []
    with sync_playwright() as p:
        browser = launch(p)
        page = browser.new_page(viewport={"width": 1280, "height": 800})
        page.on("pageerror", lambda e: errors.append(f"page error: {e}"))
        page.on(
            "console",
            lambda m: m.type == "error" and errors.append(f"console error: {m.text}"),
        )
        page.goto(url)
        page.wait_for_function(
            "document.getElementById('overlay').hidden", timeout=60_000, polling=100
        )
        page.wait_for_timeout(RUN * 500)
        before = channel(page)
        page.wait_for_timeout(RUN * 500)
        after = channel(page)
        year = page.text_content("#year")
        browser.close()
    server.shutdown()

    if not before:
        errors.append("no channel drawn in #view")
    elif before == after:
        errors.append(f"channel not redrawn over {RUN // 2} s")
    for e in dict.fromkeys(errors):
        print(e, file=sys.stderr)
    if errors:
        sys.exit(1)
    print(f"smoke: ok, {year}")


if __name__ == "__main__":
    main()
