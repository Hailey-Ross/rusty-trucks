"""Self-check for frame_times.py and event_timer.py on synthetic clips made with ffmpeg.

  python selftest.py [--ffmpeg PATH]

Clip 1: a box moving every frame at 30 fps, recorded at 60 fps (every frame twice) -> expect ~30 game fps, spacing 2.
Clip 2: a white box that moves from 1.0 s to 2.0 s, otherwise still -> expect one event, onset ~1.0 s,
duration ~1.0 s. The clips go to a temporary folder that is deleted afterwards.
"""
import argparse
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import event_timer  # noqa: E402
import frame_times  # noqa: E402
from videoframes import find_ffmpeg  # noqa: E402


BOX = "color=black:size=320x180:rate=30:duration=3[bg];color=white:size=20x20:rate=30:duration=3[box];[bg][box]overlay=x='{x}':y=80,fps=60"


def make(ffmpeg, src, out):
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", src,
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p", out], check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ffmpeg")
    a = ap.parse_args()
    ff = find_ffmpeg(a.ffmpeg)
    ok = True
    with tempfile.TemporaryDirectory() as d:
        c1 = os.path.join(d, "cadence.mp4")
        make(ff, BOX.format(x="mod(t*200,300)"), c1)
        s = frame_times.main([c1, "--ffmpeg", ff])
        good = abs(s["game_fps"] - 30) < 1 and abs(s["recording_fps"] - 60) < 1 and max(s["runs"], key=s["runs"].get) == 2
        print("cadence:", "PASS" if good else "FAIL")
        ok &= good

        c2 = os.path.join(d, "event.mp4")
        make(ff, BOX.format(x="if(between(t,1,2),20+(t-1)*200,20)"), c2)
        ev = event_timer.main([c2, "--roi", "0,60,320,60", "--ffmpeg", ff])
        good = len(ev) == 1 and abs(ev[0][2] - 1.0) < 0.07 and abs((ev[0][3] - ev[0][2]) - 1.0) < 0.1
        print("event:", "PASS" if good else "FAIL")
        ok &= good
    print("selftest", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
