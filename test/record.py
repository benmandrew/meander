"""Record the river to video: a lossless master, and from it an H.264 MP4
for web pages and an animated WebP for the README, which GitHub shows as
an image.

Usage: python3 test/record.py [render|encode|all] [out_dir]. `render`
serves `web/` (which `make build` must have filled) as the smoke test
does, warms up seed `SEED` or replays it to step `TICK`, hides the panel,
and renders `SECONDS` s at `SPEED` units of simulated time a second to
`master.mkv`. `encode` makes `meander.mp4` and `meander.webp` from the
master, so they can be made again at other settings without rendering.
`all` (the default) does both. Environment variables override the
settings below.

The page runs on a virtual clock: `performance.now` and
`requestAnimationFrame` are replaced, and each frame advances the clock by
1/`FPS` s, waits for the worker's replies, and is screenshotted. Frames
are evenly spaced however long each takes to draw, and the renderer's
time budgets never run out within a frame, so every frame is drawn whole,
as on a machine fast enough for any speed.

The clip is split into `JOBS` segments, each rendered by its own browser
from a replay to the step it starts at, and joined without re-encoding.
"""

import base64
import functools
import http.server
import multiprocessing
import os
import re
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
# Units of simulated time a second of video: 8 is 20 years a second. The
# speed slider bounds it to 0.5–60.
SPEED = setting("SPEED", 8.0)
# The step to start from, counting the warm-up's steps as the panel's Tick
# does. A step is a quarter of a year, so the 2,000-year warm-up ends at
# step 8,000. Unset, the clip starts when the warm-up ends.
TICK = int(os.environ["TICK"]) if "TICK" in os.environ else None
SECONDS = setting("SECONDS", 20.0)
FPS = setting("FPS", 30)
# Browsers rendering at once. On an M1 (8 cores, 4 fast), the default clip
# took 50 s with one, 26 s with four, and 37 s with eight.
JOBS = setting("JOBS", max(1, (os.cpu_count() or 2) // 2))
WIDTH = setting("WIDTH", 1920)
HEIGHT = setting("HEIGHT", 1080)
# The MP4 and the WebP are scaled to their widths from the master.
MP4_WIDTH = setting("MP4_WIDTH", 1920)
CRF = setting("CRF", 20)
WEBP_WIDTH = setting("WEBP_WIDTH", 800)
WEBP_FPS = setting("WEBP_FPS", 15)
WEBP_QUALITY = setting("WEBP_QUALITY", 70)

# Run before the page's own scripts. `__frame(ms)` advances the clock, runs
# the frame callbacks, and resolves once the worker has answered every
# request; `__river` holds the step and time step of the newest reply.
# Stepping requests lose their time budget, so the worker never stops short
# and the river keeps its speed; the warm-up's, which capture the
# centreline, keep theirs so its progress shows.
CLOCK = """
(() => {
  let now = 0, inflight = 0, waiting = [];
  const callbacks = [];
  performance.now = () => now;
  window.requestAnimationFrame = (cb) => callbacks.push(cb);
  const post = Worker.prototype.postMessage;
  Worker.prototype.postMessage = function (msg, transfer) {
    if (!this.counted) {
      this.counted = true;
      this.addEventListener('message', ({ data }) => {
        if (data.snap) window.__river = { steps: data.snap.scalars.steps, dt: data.snap.params.dt };
        if (--inflight === 0) waiting.splice(0).forEach((resolve) => resolve());
      });
    }
    inflight++;
    if (msg && 'budget' in msg && !('capture' in msg)) msg = { ...msg, budget: Infinity };
    return post.call(this, msg, transfer);
  };
  window.__frame = async (ms) => {
    now += ms;
    for (const cb of callbacks.splice(0)) cb(now);
    if (inflight > 0) await new Promise((resolve) => waiting.push(resolve));
  };
})();
"""


class Progress:
    """A bar on stderr with the count done and, once there is a rate to go
    on, the time left. The rate is taken from the first count on, so the
    time the render's browsers take to start does not inflate it."""

    def __init__(self, label, total):
        self.label, self.total = label, total
        self.start = time.monotonic()
        self.first = None
        self.tty = sys.stderr.isatty()
        self.update(0)

    def update(self, done):
        if not self.tty:
            return
        now = time.monotonic()
        if self.first is None and done > 0:
            self.first = (now, done)
        frac = min(1.0, done / self.total)
        bar = "#" * round(30 * frac)
        eta = ""
        if self.first and done > self.first[1]:
            rate = (done - self.first[1]) / (now - self.first[0])
            eta = f"ETA {clock((self.total - done) / rate)}"
        sys.stderr.write(
            f"\r{self.label:<8} [{bar:<30}] {100 * frac:3.0f}% {done}/{self.total} {eta}\033[K"
        )
        sys.stderr.flush()

    def finish(self):
        line = f"{self.label:<8} done in {clock(time.monotonic() - self.start)}"
        sys.stderr.write(f"\r{line}\033[K\n" if self.tty else f"{line}\n")


def clock(seconds):
    m, s = divmod(round(seconds), 60)
    return f"{m}:{s:02d}"


def serve():
    handler = functools.partial(Quiet, directory="web")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}/?seed={SEED}"


def open_page(p, url, bar=None):
    """Open the page on the virtual clock, without the panel, and wait for
    its warm-up or replay, showing it on `bar` if given."""
    browser = launch(p)
    page = browser.new_page(viewport={"width": WIDTH, "height": HEIGHT})
    page.add_init_script(CLOCK)
    page.goto(url)
    page.add_style_tag(content="#panel, #expand { display: none !important; }")
    while not page.evaluate("document.getElementById('overlay').hidden"):
        if bar:
            done = re.search(r"(\d+)%", page.text_content("#overlay") or "")
            bar.update(int(done[1]) if done else 0)
        time.sleep(0.1)
    return browser, page


def segment(tick, frames, path, counter):
    """Replay to step `tick`, render `frames` frames from there into the
    lossless video at `path`, and count each on `counter`."""
    server, base = serve()
    with sync_playwright() as p, tempfile.TemporaryFile() as log:
        browser, page = open_page(p, f"{base}&tick={tick}")
        # The replay ends paused, and its first frame draws the floodplain
        # whole. Play clears that, so the frame comes first; another with
        # no time passing then draws the river as the first screenshot
        # shows it.
        page.evaluate("__frame(0)")
        page.evaluate(
            """(speed) => {
              const input = document.getElementById('p-speed');
              input.value = speed;
              input.dispatchEvent(new Event('input'));
              document.getElementById('play').click();
            }""",
            SPEED,
        )
        page.evaluate("__frame(0)")
        # Lossless RGB, so the encodes start from the pixels as drawn.
        # CDP's screenshot made fast is still lossless, and took 59 ms a
        # frame at 1920x1080 against Playwright's 171 ms.
        cdp = page.context.new_cdp_session(page)
        ffmpeg = subprocess.Popen(
            ["ffmpeg", "-y", "-loglevel", "error", "-f", "image2pipe",
             "-framerate", str(FPS), "-c:v", "png", "-i", "-",
             "-c:v", "libx264rgb", "-qp", "0", "-preset", "ultrafast", path],
            stdin=subprocess.PIPE, stderr=log,
        )
        assert ffmpeg.stdin
        for _ in range(frames):
            shot = cdp.send("Page.captureScreenshot", {"format": "png", "optimizeForSpeed": True})
            ffmpeg.stdin.write(base64.b64decode(shot["data"]))
            page.evaluate(f"__frame({1000 / FPS})")
            with counter.get_lock():
                counter.value += 1
        ffmpeg.stdin.close()
        if ffmpeg.wait():
            log.seek(0)
            sys.exit(f"record: ffmpeg failed\n{log.read().decode()}")
        browser.close()
    server.shutdown()


def render(out):
    frames = round(SECONDS * FPS)
    master = os.path.join(out, "master.mkv")

    # The step the clip starts at, and the steps a frame covers, from one
    # warm-up or replay.
    server, url = serve()
    if TICK is not None:
        url += f"&tick={TICK}"
    with sync_playwright() as p:
        bar = Progress("warm-up", 100)
        browser, page = open_page(p, url, bar)
        river = page.evaluate("__river")
        browser.close()
        bar.update(100)
        bar.finish()
    server.shutdown()
    per_frame = SPEED / FPS / river["dt"]

    # Each segment starts on the step the clip reaches at its first frame.
    jobs = max(1, min(JOBS, frames))
    cuts = [frames * j // jobs for j in range(jobs + 1)]
    counter = multiprocessing.Value("i", 0)
    with tempfile.TemporaryDirectory() as tmp:
        parts = [os.path.join(tmp, f"{j}.mkv") for j in range(jobs)]
        procs = [
            multiprocessing.Process(
                target=segment,
                args=(river["steps"] + round(cuts[j] * per_frame),
                      cuts[j + 1] - cuts[j], parts[j], counter),
            )
            for j in range(jobs)
        ]
        for proc in procs:
            proc.start()
        bar = Progress("render", frames)
        while any(proc.is_alive() for proc in procs):
            bar.update(counter.value)
            time.sleep(0.2)
        if any(proc.exitcode for proc in procs):
            sys.exit("record: a segment failed")
        bar.update(frames)
        bar.finish()

        listing = os.path.join(tmp, "parts.txt")
        with open(listing, "w") as f:
            f.writelines(f"file '{part}'\n" for part in parts)
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
             "-i", listing, "-c", "copy", master],
            check=True,
        )
    print(f"{master}: {size(master)}, {jobs} segments from step {river['steps']}")


def run(label, total, args, pattern):
    """Run `args` with a bar, counting the lines of its output that match
    `pattern` (the frame number in its first group, if any)."""
    bar = Progress(label, total)
    with subprocess.Popen(
        args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    ) as proc:
        assert proc.stdout
        lines, count = [], 0
        for line in proc.stdout:
            lines.append(line)
            m = re.match(pattern, line)
            if m:
                count = int(m[1]) if m.groups() else count + 1
                bar.update(min(count, total))
    if proc.returncode:
        sys.exit(f"record: {args[0]} failed\n{''.join(lines[-20:])}")
    bar.finish()


def encode(out):
    master = os.path.join(out, "master.mkv")
    if not os.path.exists(master):
        sys.exit(f"record: no {master}; run `render` first")
    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-count_packets", "-select_streams", "v:0",
         "-show_entries", "stream=nb_read_packets,r_frame_rate", "-of", "csv=p=0", master],
        check=True, capture_output=True, text=True,
    ).stdout.strip().split(",")
    rate, frames = probe[0], int(probe[1])
    num, den = map(int, rate.split("/"))
    seconds = frames * den / num

    mp4 = os.path.join(out, "meander.mp4")
    run(
        "mp4", frames,
        ["ffmpeg", "-y", "-loglevel", "error", "-nostats", "-progress", "pipe:1",
         "-i", master, "-an", "-vf", f"scale={MP4_WIDTH}:-2:flags=lanczos",
         "-c:v", "libx264", "-preset", "slow", "-crf", str(CRF),
         "-pix_fmt", "yuv420p", "-movflags", "+faststart", mp4],
        r"frame=(\d+)",
    )

    webp = os.path.join(out, "meander.webp")
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            ["ffmpeg", "-loglevel", "error", "-i", master,
             "-vf", f"fps={WEBP_FPS},scale={WEBP_WIDTH}:-2:flags=lanczos",
             os.path.join(tmp, "%05d.png")],
            check=True,
        )
        pngs = sorted(os.path.join(tmp, f) for f in os.listdir(tmp))
        run(
            "webp", len(pngs),
            ["img2webp", "-v", "-loop", "0", "-lossy", "-q", str(WEBP_QUALITY),
             "-d", str(round(1000 / WEBP_FPS)), *pngs, "-o", webp],
            r"Added frame",
        )

    for path in (mp4, webp):
        print(f"{path}: {size(path)}, {seconds:.1f} s")


def size(path):
    return f"{os.path.getsize(path) / 1e6:.1f} MB"


def main():
    args = sys.argv[1:]
    stage = args.pop(0) if args and args[0] in ("render", "encode", "all") else "all"
    out = args[0] if args else "video"
    os.makedirs(out, exist_ok=True)
    if stage in ("render", "all"):
        render(out)
    if stage in ("encode", "all"):
        encode(out)


if __name__ == "__main__":
    missing = [t for t in ("ffmpeg", "ffprobe", "img2webp") if not shutil.which(t)]
    if missing:
        sys.exit(f"record: needs {', '.join(missing)} on PATH (the dev shell has them)")
    main()
