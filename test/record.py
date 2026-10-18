"""Record the river to video: an H.264 MP4 for web pages and an animated
WebP for the README, which GitHub shows as an image.

Usage: python3 test/record.py [out_dir]. Serves `web/` (which `make build`
must have filled) as the smoke test does, warms up seed `SEED` or replays
it to step `TICK`, hides the panel, and records `SECONDS` s at `SPEED`
units of simulated time a second. Environment variables override the
settings below.

Playwright records each page to a VP8 WebM; ffmpeg cuts the warm-up from
its start and re-encodes it, and img2webp packs frames for the WebP.
"""

import functools
import http.server
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time

from playwright.sync_api import sync_playwright

from smoke import Quiet, launch


def setting(name, default):
    return type(default)(os.environ.get(name, default))


SEED = setting("SEED", 1)
# Units of simulated time a second: 8 is 20 years a second.
SPEED = setting("SPEED", 8.0)
# The step to start from, counting the warm-up's steps as the panel's Tick
# does. A step is a quarter of a year, so the 2,000-year warm-up ends at
# step 8,000. Unset, the clip starts when the warm-up ends.
TICK = int(os.environ["TICK"]) if "TICK" in os.environ else None
SECONDS = setting("SECONDS", 20.0)
WIDTH = setting("WIDTH", 1280)
HEIGHT = setting("HEIGHT", 720)
# The MP4 keeps the recorded size; the WebP is scaled to `WEBP_WIDTH` at
# `WEBP_FPS` frames a second to keep it small. On seed 1's first 20 s the
# WebP is 3.4 MB; quality 30 made it 40% smaller but showed blocks on the
# bars. A lower frame rate made it larger, as each frame differs more from
# the last.
CRF = setting("CRF", 28)
WEBP_WIDTH = setting("WEBP_WIDTH", 800)
WEBP_FPS = setting("WEBP_FPS", 15)
WEBP_QUALITY = setting("WEBP_QUALITY", 60)


def record(url, video_dir):
    """Record the page at `url` and return the WebM's path and the seconds
    of warm-up at its start."""
    with sync_playwright() as p:
        browser = launch(p)
        context = browser.new_context(
            viewport={"width": WIDTH, "height": HEIGHT},
            record_video_dir=video_dir,
            record_video_size={"width": WIDTH, "height": HEIGHT},
        )
        start = time.monotonic()
        page = context.new_page()
        page.goto(url)
        page.add_style_tag(content="#panel, #expand { display: none !important; }")
        page.wait_for_function(
            "document.getElementById('overlay').hidden", timeout=300_000, polling=100
        )
        # A replay to `TICK` ends paused.
        page.evaluate(
            """([speed, replay]) => {
              const input = document.getElementById('p-speed');
              input.value = speed;
              input.dispatchEvent(new Event('input'));
              if (replay) document.getElementById('play').click();
            }""",
            [SPEED, TICK is not None],
        )
        # The page settles for a moment after the overlay lifts.
        page.wait_for_timeout(500)
        skip = time.monotonic() - start
        page.wait_for_timeout(SECONDS * 1000)
        assert page.video
        path = page.video.path()
        context.close()
        browser.close()
    return path, skip


def run(*args):
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "video"
    os.makedirs(out, exist_ok=True)
    handler = functools.partial(Quiet, directory="web")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/?seed={SEED}"
    if TICK is not None:
        url += f"&tick={TICK}"

    with tempfile.TemporaryDirectory() as tmp:
        webm, skip = record(url, tmp)
        server.shutdown()
        cut = ("-ss", f"{skip:.2f}", "-t", f"{SECONDS:.2f}", "-i", webm)

        mp4 = os.path.join(out, "meander.mp4")
        run(
            "ffmpeg", "-y", "-loglevel", "error", *cut, "-an",
            "-c:v", "libx264", "-preset", "slow", "-crf", str(CRF),
            "-pix_fmt", "yuv420p", "-movflags", "+faststart", mp4,
        )

        frames = os.path.join(tmp, "frames")
        os.mkdir(frames)
        run(
            "ffmpeg", "-loglevel", "error", *cut,
            "-vf", f"fps={WEBP_FPS},scale={WEBP_WIDTH}:-2:flags=lanczos",
            os.path.join(frames, "%05d.png"),
        )
        webp = os.path.join(out, "meander.webp")
        pngs = sorted(os.path.join(frames, f) for f in os.listdir(frames))
        run(
            "img2webp", "-loop", "0", "-lossy", "-q", str(WEBP_QUALITY),
            "-d", str(round(1000 / WEBP_FPS)), *pngs, "-o", webp,
        )

    for path in (mp4, webp):
        print(f"{path}: {os.path.getsize(path) / 1e6:.1f} MB")


if __name__ == "__main__":
    if not shutil.which("ffmpeg") or not shutil.which("img2webp"):
        sys.exit("record: needs ffmpeg and img2webp on PATH (the dev shell has both)")
    main()
