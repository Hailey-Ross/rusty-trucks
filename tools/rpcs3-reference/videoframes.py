"""Shared helper: decode a video into small grayscale frames with their exact timestamps.

Uses only ffmpeg (5.1 or newer). It is found from --ffmpeg, the FFMPEG environment variable or PATH.
Frames are passed through as recorded (no frame-rate conversion), so duplicates the game left in the
recording stay visible.
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

PTS = re.compile(r"pts_time:\s*(-?[0-9.]+)")


def find_ffmpeg(explicit=None):
    for cand in (explicit, os.environ.get("FFMPEG"), shutil.which("ffmpeg")):
        if cand and os.path.isfile(cand):
            return cand
    sys.exit("ffmpeg not found: put it on PATH, set FFMPEG or pass --ffmpeg")


def parse_roi(text):
    """'x,y,w,h' in source pixels -> tuple of ints."""
    parts = [int(p) for p in text.split(",")]
    if len(parts) != 4 or parts[2] <= 0 or parts[3] <= 0:
        raise ValueError("--roi needs x,y,w,h with w and h > 0")
    return tuple(parts)


def read_frames(video, ffmpeg, size=(96, 54), roi=None, start=None, duration=None):
    """Returns [(pts_seconds, gray_bytes)] for every frame (or the ROI, scaled to size)."""
    w, h = size
    vf = []
    if roi:
        x, y, rw, rh = roi
        vf.append(f"crop={rw}:{rh}:{x}:{y}")
    vf += [f"scale={w}:{h}:flags=area", "format=gray", "showinfo"]
    cmd = [ffmpeg, "-hide_banner", "-nostdin"]
    if start is not None:
        cmd += ["-ss", str(start)]
    cmd += ["-i", video]
    if duration is not None:
        cmd += ["-t", str(duration)]
    cmd += ["-an", "-vf", ",".join(vf), "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "gray", "-"]
    frame_len = w * h
    frames = []
    with tempfile.TemporaryFile() as err:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=err)
        while True:
            buf = proc.stdout.read(frame_len)
            if len(buf) < frame_len:
                break
            frames.append(buf)
        proc.wait()
        err.seek(0)
        log = err.read().decode("utf-8", "replace")
    if proc.returncode != 0:
        sys.exit("ffmpeg failed:\n" + log[-2000:])
    times = [float(m.group(1)) for line in log.splitlines() if "showinfo" in line for m in [PTS.search(line)] if m]
    if len(times) != len(frames):
        sys.exit(f"ffmpeg gave {len(frames)} frames but {len(times)} timestamps")
    return list(zip(times, frames))


def mean_abs_diff(a, b):
    """Mean absolute difference of two equal-size gray frames, 0..255."""
    return sum(abs(p - q) for p, q in zip(a, b)) / len(a)


def percentile(values, q):
    if not values:
        return 0.0
    s = sorted(values)
    k = (len(s) - 1) * q / 100.0
    lo = int(k)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)
