"""Time an on-screen event from a recording: when a screen region starts and stops changing.

  python event_timer.py VIDEO --roi x,y,w,h [--start S] [--duration D] [--threshold 2.0] [--gap 0.1]
                        [--min-length 0.05] [--csv out.csv]

For every frame it measures how much the region changed since the previous frame and since the first
frame (mean absolute gray difference, 0..255). Frames above --threshold are "active"; active runs that
are less than --gap seconds apart are merged into one event. Each event is printed with its onset and
end frame, the times and its duration, e.g. how long an animation, a prop movement, a respawn fade or a
menu transition takes in the retail game.

--roi         region in source pixels (x,y = top left). Pick it in any image viewer from a frame of the
              recording; keep it on the thing that moves and off HUD counters that tick.
--threshold   per-frame change that counts as movement. Raise it if encoder noise makes idle frames active.
--csv         per-frame rows: index, time_s, diff_prev, diff_first, active (1/0).

The onset is the first frame that differs, so the resolution is one recorded frame (16.7 ms at 60 fps) or
one game frame when the game renders slower than the recording.
"""
import argparse
import csv
import sys

from videoframes import find_ffmpeg, mean_abs_diff, parse_roi, read_frames


def find_events(frames, threshold, gap, min_length):
    """frames: [(t, bytes)] -> (rows, [(onset_idx, end_idx, onset_t, end_t)])."""
    rows = []
    first = frames[0][1]
    prev = None
    active = []
    for i, (t, buf) in enumerate(frames):
        dp = 0.0 if prev is None else mean_abs_diff(buf, prev)
        df = mean_abs_diff(buf, first)
        on = dp > threshold
        rows.append((i, t, dp, df, int(on)))
        if on:
            active.append(i)
        prev = buf
    events = []
    for i in active:
        if events and frames[i][0] - frames[events[-1][1]][0] <= gap:
            events[-1][1] = i
        else:
            events.append([i, i])
    out = []
    for a, b in events:
        ta, tb = frames[a][0], frames[b][0]
        if tb - ta >= min_length:
            out.append((a, b, ta, tb))
    return rows, out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("video")
    ap.add_argument("--roi", type=parse_roi, required=True)
    ap.add_argument("--start", type=float)
    ap.add_argument("--duration", type=float)
    ap.add_argument("--threshold", type=float, default=2.0)
    ap.add_argument("--gap", type=float, default=0.1, help="merge active runs closer than this (s)")
    ap.add_argument("--min-length", type=float, default=0.05, help="drop events shorter than this (s)")
    ap.add_argument("--csv")
    ap.add_argument("--ffmpeg")
    a = ap.parse_args(argv)
    frames = read_frames(a.video, find_ffmpeg(a.ffmpeg), size=(64, 64), roi=a.roi, start=a.start, duration=a.duration)
    if len(frames) < 2:
        sys.exit("fewer than 2 frames decoded")
    rows, events = find_events(frames, a.threshold, a.gap, a.min_length)
    if a.csv:
        with open(a.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["index", "time_s", "diff_prev", "diff_first", "active"])
            for i, t, dp, df, on in rows:
                w.writerow([i, f"{t:.6f}", f"{dp:.3f}", f"{df:.3f}", on])
    print(f"{len(frames)} frames, {len(events)} event(s) in roi {a.roi}")
    for n, (fa, fb, ta, tb) in enumerate(events, 1):
        print(f"event {n}: onset frame {fa} at {ta:.3f} s, end frame {fb} at {tb:.3f} s, duration {tb - ta:.3f} s")
    return events


if __name__ == "__main__":
    main()
