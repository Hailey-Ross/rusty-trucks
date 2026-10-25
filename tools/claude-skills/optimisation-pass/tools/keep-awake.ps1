# Wakes the PC from its screensaver and keeps it awake for a timing test (a screensaver that renders, like the ASUS one
# on the laptop's second screen, takes CPU and GPU and spoils the numbers). Sends a 1 px mouse nudge with SendInput (moved back at
# once) every -Every seconds, asks Windows to keep the display and system on, and ends after -Minutes.
# Usage (background): powershell -NoProfile -ExecutionPolicy Bypass -File keep-awake.ps1 [-Minutes 90] [-Every 30]
param([int]$Minutes = 90, [int]$Every = 30)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Awake {
    [StructLayout(LayoutKind.Sequential)] public struct MouseInput { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] public struct Input { public uint type; public MouseInput mi; }
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint count, Input[] inputs, int size);
    [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct LastInput { public uint size; public uint time; }
    [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LastInput info);
    public static uint IdleMs() { var i = new LastInput(); i.size = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.time; }
    static Input Move(int dx) { var i = new Input(); i.type = 0; i.mi.dx = dx; i.mi.flags = 1; return i; }
    // SendInput, not mouse_event: on the laptop mouse_event left the idle timer running (2026-10-24).
    public static uint Nudge() { return SendInput(2, new[] { Move(1), Move(-1) }, Marshal.SizeOf(typeof(Input))); }
}
'@
# ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
[Awake]::SetThreadExecutionState([uint32]2147483651) | Out-Null
$end = (Get-Date).AddMinutes($Minutes)
"KEEP_AWAKE start idle_ms=$([Awake]::IdleMs()) until $($end.ToString('HH:mm'))"
[Awake]::Nudge() | Out-Null
Start-Sleep -Milliseconds 200
"KEEP_AWAKE woke idle_ms=$([Awake]::IdleMs())"
try {
    while ((Get-Date) -lt $end) {
        Start-Sleep -Seconds $Every
        [Awake]::Nudge() | Out-Null
    }
} finally {
    [Awake]::SetThreadExecutionState([uint32]2147483648) | Out-Null
    'KEEP_AWAKE end'
}
