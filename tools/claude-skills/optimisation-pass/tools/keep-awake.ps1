# Wakes the PC from its screensaver and keeps it awake for a timing test (a screensaver that renders, like the ASUS one
# on the laptop's second screen, takes CPU and GPU and spoils the numbers). Sends a 1 px mouse nudge (moved back at
# once) every -Every seconds, asks Windows to keep the display and system on, and ends after -Minutes.
# Usage (background): powershell -NoProfile -ExecutionPolicy Bypass -File keep-awake.ps1 [-Minutes 90] [-Every 30]
param([int]$Minutes = 90, [int]$Every = 30)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Awake {
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct LastInput { public uint size; public uint time; }
    [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LastInput info);
    public static uint IdleMs() { var i = new LastInput(); i.size = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.time; }
    public static void Nudge() { mouse_event(1, 1, 0, 0, UIntPtr.Zero); mouse_event(1, -1, 0, 0, UIntPtr.Zero); }
}
'@
# ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
[Awake]::SetThreadExecutionState([uint32]2147483651) | Out-Null
$end = (Get-Date).AddMinutes($Minutes)
"KEEP_AWAKE start idle_ms=$([Awake]::IdleMs()) until $($end.ToString('HH:mm'))"
[Awake]::Nudge()
"KEEP_AWAKE woke idle_ms=$([Awake]::IdleMs())"
try {
    while ((Get-Date) -lt $end) {
        Start-Sleep -Seconds $Every
        [Awake]::Nudge()
    }
} finally {
    [Awake]::SetThreadExecutionState([uint32]2147483648) | Out-Null
    'KEEP_AWAKE end'
}
