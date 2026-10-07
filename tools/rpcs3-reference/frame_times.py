"""Effective game frame rate and frame pacing from a gameplay recording.

A game that renders at ~30 fps, recorded at 60 fps, shows every game frame twice. This tool decodes the
recording without frame-rate conversion, marks frames that barely differ from the previous one as
duplicates, and reports how often the picture really changes:

  python frame_times.py VIDEO [--roi x,y,w,h] [--start S] [--duration D] [--dup-threshold 0.5] [--csv out.csv]

Output: recording fps, unique (game) frames, effective game fps, the interval between unique frames
(median, p5/p95, max) and its histogram in recording frames, and the number of hitches (an interval
longer than 1.5 times the median).

--roi            measure only this region (source pixels), e.g. the part of the screen that always moves;
                 a static picture (pause menu, loading screen) reads as duplicates.
--dup-threshold  mean absolute gray difference (0..255) at or below which a frame counts as a duplicate.
                 Encoder noise on a repeated frame is usually far below 0.5; check --csv if unsure.
--csv            per-frame rows: index, time_s, diff, unique (1/0).
"""
import argparse
import csv
import statistics
import sys
from collections import Counter

from videoframes import find_ffmpeg, mean_abs_diff, parse_roi, percentile, read_frames


def analyse(frames, threshold):
    """frames: [(t, bytes)] -> (rows, summary dict)."""
    rows = []
    unique_times = []
    unique_idx = []
    prev = None
    for i, (t, buf) in enumerate(frames):
        d = 255.0 if prev is None else mean_abs_diff(buf, prev)
        uniq = d > threshold
        if uniq:
            unique_times.append(t)
            unique_idx.append(i)
        rows.append((i, t, d, int(uniq)))
        prev = buf
    n = len(frames)
    span = frames[-1][0] - frames[0][0] if n > 1 else 0.0
    rec_fps = (n - 1) / span if span > 0 else 0.0
    intervals_ms = [(b - a) * 1000.0 for a, b in zip(unique_times, unique_times[1:])]
    runs = Counter(b - a for a, b in zip(unique_idx, unique_idx[1:]))
    med = statistics.median(intervals_ms) if intervals_ms else 0.0
    u_span = unique_times[-1] - unique_times[0] if len(unique_times) > 1 else 0.0
    summary = {
        "frames": n,
        "span_s": span,
        "recording_fps": rec_fps,
        "unique_frames": len(unique_times),
        "game_fps": (len(unique_times) - 1) / u_span if u_span > 0 else 0.0,
        "interval_median_ms": med,
        "interval_p5_ms": percentile(intervals_ms, 5),
        "interval_p95_ms": percentile(intervals_ms, 95),
        "interval_max_ms": max(intervals_ms) if intervals_ms else 0.0,
        "hitches": sum(1 for x in intervals_ms if med and x > 1.5 * med),
        "runs": dict(sorted(runs.items())),
    }
    return rows, summary


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("video")
    ap.add_argument("--roi", type=parse_roi)
    ap.add_argument("--start", type=float)
    ap.add_argument("--duration", type=float)
    ap.add_argument("--dup-threshold", type=float, default=0.5)
    ap.add_argument("--csv")
    ap.add_argument("--ffmpeg")
    a = ap.parse_args(argv)
    frames = read_frames(a.video, find_ffmpeg(a.ffmpeg), roi=a.roi, start=a.start, duration=a.duration)
    if len(frames) < 2:
        sys.exit("fewer than 2 frames decoded")
    rows, s = analyse(frames, a.dup_threshold)
    if a.csv:
        with open(a.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["index", "time_s", "diff", "unique"])
            for i, t, d, u in rows:
                w.writerow([i, f"{t:.6f}", f"{d:.3f}", u])
    print(f"frames {s['frames']} over {s['span_s']:.3f} s, recording {s['recording_fps']:.2f} fps")
    print(f"unique frames {s['unique_frames']}, effective game fps {s['game_fps']:.2f}")
    print(f"interval between unique frames: median {s['interval_median_ms']:.1f} ms, "
          f"p5 {s['interval_p5_ms']:.1f}, p95 {s['interval_p95_ms']:.1f}, max {s['interval_max_ms']:.1f}; "
          f"hitches (>1.5x median) {s['hitches']}")
    print("unique-frame spacing in recording frames (spacing: count): "
          + ", ".join(f"{k}: {v}" for k, v in s["runs"].items()))
    return s


if __name__ == "__main__":
    main()
