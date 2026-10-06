#!/usr/bin/env bash
# Unattended scripted recomp session: boot via the demo path, play an input script, trace audio.
# usage: recomp_script_run.sh <script.txt> <trace.tsv> [seconds=120] [autostart_ms=4000] [extra recomp flags...]
# Pass ABSOLUTE paths: the runner cd's into the game folder.
# Needs (Git Bash on Windows):
#   RECOMP_EXE      your own research build's skate3.exe (e.g. <recomp checkout>/out/build/clang-relwithdebinfo/skate3.exe)
#   RECOMP_GAME_DIR the folder the recomp runs from (your own install of the game files)
# Env: BACKGROUND=1 (default) | MUTE=true (default) | SHOTS=1 (default; 0 = no screenshots) | SKATE3_TRACE=<categories>
# Refuses to run while any skate3.exe is already running (one game at a time).
set -u
: "${RECOMP_EXE:?set RECOMP_EXE to the skate3.exe of your research build}"
: "${RECOMP_GAME_DIR:?set RECOMP_GAME_DIR to the folder the recomp runs from}"
T="$2"; SCRIPT=$(cygpath -w "$1"); TRACE=$(cygpath -w "$2"); SECONDS_MAX=${3:-120}; AUTO=${4:-4000}; shift 4 2>/dev/null || shift $#
HERE="$(cd "$(dirname "$0")" && pwd)"
export RECOMP_EXE_DIR="$(cygpath -w "$(dirname "$RECOMP_EXE")")"
if tasklist | grep -qi "^skate3.exe"; then echo "a skate3.exe is already running - not starting"; exit 3; fi
rm -f "$T"
cd "$RECOMP_GAME_DIR" || exit 2
SKATE3_BACKGROUND=${BACKGROUND:-1} SKATE3_AUDIO_CAPTURE="$(cygpath -w "${T%.tsv}.f32")" SKATE3_AUDIO_TRACE_FILE="$TRACE" SKATE3_INPUT_SCRIPT="$SCRIPT" SKATE3_INPUT_SCRIPT_AUTOSTART_MS="$AUTO" \
  "$RECOMP_EXE" --skate3_demo_path --skate3_demo_path_signed_in --audio_mute=${MUTE:-true} --fullscreen=false --window_width=960 --window_height=540 "$@" > "${T%.tsv}.out" 2> "${T%.tsv}.err" &
PID=$!
# Screenshots every 2 s (SHOTS=0 disables) into <trace>_shots/, named by unix ms (the trace's CLOCK line aligns them).
if [ "${SHOTS:-1}" != 0 ]; then
  rm -rf "${T%.tsv}_shots" "${T%.tsv}.stop"
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$HERE/screen_loop.ps1")" -Out "$(cygpath -w "${T%.tsv}_shots")" -Stop "$(cygpath -w "${T%.tsv}.stop")" -Every 2 &
fi
for ((i = 0; i < SECONDS_MAX; i++)); do
  sleep 1
  kill -0 $PID 2>/dev/null || { echo "game exited after ${i}s"; break; }
  if grep -q "script end" "$T" 2>/dev/null; then sleep 3; echo "script finished after ${i}s"; break; fi
done
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$HERE/stop_dev_recomp.ps1")" && echo "stopped dev recomp"
touch "${T%.tsv}.stop"
grep "^MARK" "$T" 2>/dev/null | head -60
