// Couch launcher for Steam / Steam Link (see README.md). Steam only adds .exe files as non-Steam games, so this
// small exe draws the menu and reads the controller (XInput), and launcher.ps1 does the launching, the version
// builds and the session check (launcher.ps1 <entry> [label] -NoPrompt). The entry list comes from launcher.ps1.
//   D-pad / left stick = move, A = select, B = back. Arrow keys / Enter / Esc also work.
//   SkateLauncher.exe <entry> [label] [@version]  runs an entry directly (one-click Steam shortcuts).
//   Versions (versions.json): rust-* entries run the selected version (version.txt) or the one named with @id.
//   SkateLauncher.exe version-next / version-prev / version-set <id> / notes  pick a version or show what to test.
// Build: C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /out:SkateLauncher.exe SkateLauncher.cs
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

static class SkateLauncher {
    [StructLayout(LayoutKind.Sequential)]
    struct XState { public uint Packet; public ushort Buttons; public byte LT, RT; public short LX, LY, RX, RY; }
    [DllImport("xinput1_4.dll")] static extern int XInputGetState(int i, out XState s);
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int h);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct FontInfo { public int Size; public int Index; public short W, H; public int Family, Weight;
                      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Face; }
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetCurrentConsoleFontEx(IntPtr h, bool max, ref FontInfo f);

    const ushort UP = 0x0001, DOWN = 0x0002, A = 0x1000, B = 0x2000;
    static string Here = AppDomain.CurrentDomain.BaseDirectory;
    // Same rule as launcher.ps1: state files live next to the exe, except for the copy in the repository's tools\
    // folder, which keeps them in .local\steam-launcher\ so the checkout stays clean.
    static string StateDir() {
        string here = Path.GetFullPath(Here).TrimEnd('\\');
        string repo = Path.GetFullPath(Path.Combine(here, "..", ".."));
        if (here.StartsWith(Path.Combine(repo, "tools") + "\\", StringComparison.OrdinalIgnoreCase))
            return Path.Combine(repo, ".local", "steam-launcher");
        return here;
    }
    static string LastSessionFile() { return Path.Combine(StateDir(), "last_session.txt"); }

    static ushort Pad() {
        for (int i = 0; i < 4; i++) {
            XState s;
            try {
                if (XInputGetState(i, out s) == 0) {
                    ushort b = s.Buttons;
                    if (s.LY > 16000) b |= UP; if (s.LY < -16000) b |= DOWN;
                    return b;
                }
            } catch { return 0; }
        }
        return 0;
    }

    // One input: "up", "down", "ok", "back".
    static string Input() {
        ushort prev = Pad(); int held = 0;
        while (true) {
            while (Console.KeyAvailable) {
                var k = Console.ReadKey(true).Key;
                if (k == ConsoleKey.UpArrow || k == ConsoleKey.W) return "up";
                if (k == ConsoleKey.DownArrow || k == ConsoleKey.S) return "down";
                if (k == ConsoleKey.Enter || k == ConsoleKey.Spacebar) return "ok";
                if (k == ConsoleKey.Escape || k == ConsoleKey.Backspace) return "back";
            }
            ushort b = Pad(); ushort pressed = (ushort)(b & ~prev);
            if ((pressed & A) != 0) return "ok";
            if ((pressed & B) != 0) return "back";
            if ((pressed & UP) != 0) return "up";
            if ((pressed & DOWN) != 0) return "down";
            if ((b & (UP | DOWN)) != 0 && b == prev) { if (++held > 8) return (b & UP) != 0 ? "up" : "down"; } else held = 0;
            prev = b;
            Thread.Sleep(40);
        }
    }

    static void Header(string title) {
        Console.Clear();
        Console.ForegroundColor = ConsoleColor.Cyan;
        Console.WriteLine(); Console.WriteLine("  " + title); Console.WriteLine();
        Console.ResetColor();
    }

    static int Menu(string title, string[] items, string[] hints) {
        int i = 0;
        while (true) {
            Header(title);
            for (int n = 0; n < items.Length; n++) {
                if (n == i) { Console.BackgroundColor = ConsoleColor.Yellow; Console.ForegroundColor = ConsoleColor.Black; Console.WriteLine("  > " + items[n] + " "); Console.ResetColor(); }
                else Console.WriteLine("    " + items[n]);
            }
            Console.WriteLine();
            if (hints != null && i < hints.Length && hints[i] != "") { Console.ForegroundColor = ConsoleColor.DarkGray; Console.WriteLine("  " + hints[i]); Console.ResetColor(); }
            Console.WriteLine();
            Console.ForegroundColor = ConsoleColor.DarkGray; Console.WriteLine("  D-pad: move    A: select    B: back"); Console.ResetColor();
            string inp = Input();
            if (inp == "up") i = (i - 1 + items.Length) % items.Length;
            else if (inp == "down") i = (i + 1) % items.Length;
            else if (inp == "ok") return i;
            else return -1;
        }
    }

    static void Message(string text, ConsoleColor color) {
        Header("Skate launcher");
        Console.ForegroundColor = color;
        foreach (var l in text.Split('\n')) Console.WriteLine("  " + l.TrimEnd('\r'));
        Console.ResetColor(); Console.WriteLine();
        Console.ForegroundColor = ConsoleColor.DarkGray; Console.WriteLine("  A / B: back to the menu"); Console.ResetColor();
        Focus(); Input();
    }

    static void Focus() { var h = GetConsoleWindow(); ShowWindow(h, 3); SetForegroundWindow(h); }

    static string RunningGames() {
        string r = "";
        foreach (var n in new[] { "skate3", "skate3rust" }) if (Process.GetProcessesByName(n).Length > 0) r += (r == "" ? "" : ", ") + n + ".exe";
        return r;
    }

    // Waits until no game runs; false if B / Esc cancels.
    static bool WaitNoGame() {
        while (true) {
            string g = RunningGames();
            if (g == "") return true;
            Header("Skate launcher");
            Console.ForegroundColor = ConsoleColor.Yellow; Console.WriteLine("  Already running: " + g); Console.ResetColor();
            Console.WriteLine("  Only one game at a time. Close it, or wait here: this continues by itself.");
            Console.WriteLine(); Console.ForegroundColor = ConsoleColor.DarkGray; Console.WriteLine("  B: back to the menu"); Console.ResetColor();
            for (int t = 0; t < 25; t++) {
                while (Console.KeyAvailable) { var k = Console.ReadKey(true).Key; if (k == ConsoleKey.Escape || k == ConsoleKey.Backspace) return false; }
                if ((Pad() & B) != 0) return false;
                Thread.Sleep(40);
            }
        }
    }

    static string Ps(string args, bool capture) {
        var psi = new ProcessStartInfo("powershell.exe", "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + args)
            { UseShellExecute = false, WorkingDirectory = Here, RedirectStandardOutput = capture };
        using (var p = Process.Start(psi)) {
            string o = capture ? p.StandardOutput.ReadToEnd() : "";
            p.WaitForExit();
            return o.TrimEnd();
        }
    }

    // id, name, status, selected flag per version (launcher.ps1 versions).
    static string[][] Versions() {
        var rows = new System.Collections.Generic.List<string[]>();
        foreach (var l in Ps("versions", true).Split('\n')) { var f = l.TrimEnd('\r').Split('\t'); if (f.Length >= 3) rows.Add(f); }
        return rows.ToArray();
    }

    static string VerArg(string version) { return string.IsNullOrEmpty(version) ? "" : " -Version " + version; }

    static void Launch(string entry, string label, string name) {
        if (!WaitNoGame()) return;
        Header("Skate launcher");
        Console.WriteLine("  Starting: " + name);
        Console.WriteLine();
        string args = "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + entry
                    + (string.IsNullOrEmpty(label) ? "" : " " + label) + " -NoPrompt";
        var psi = new ProcessStartInfo("powershell.exe", args) { UseShellExecute = false, WorkingDirectory = Here };
        using (var p = Process.Start(psi)) p.WaitForExit();
        string last = LastSessionFile();
        string text = File.Exists(last) ? File.ReadAllText(last) : "No session result.";
        bool bad = text.Contains("MALFORMED") || text.Contains("No new") || text.Contains("Not started") || text.Contains("panic(s)");
        Message("Session ended.\n\n" + text, bad ? ConsoleColor.Yellow : ConsoleColor.Green);
    }

    // id, name, hint, ask-for-a-label flag per entry (launcher.ps1 entries: the engine modes plus the recomp modes
    // from config.json).
    static string[][] Entries() {
        var rows = new System.Collections.Generic.List<string[]>();
        foreach (var l in Ps("entries", true).Split('\n')) { var f = l.TrimEnd('\r').Split('\t'); if (f.Length >= 4) rows.Add(f); }
        return rows.ToArray();
    }
    static readonly string[] Labels = { "session", "ride", "grind", "bail", "push", "emit", "carve", "water", "marker" };

    static void SetupConsole() {
        try { Console.Title = "Skate launcher"; } catch { }
        try {
            var f = new FontInfo { Size = Marshal.SizeOf(typeof(FontInfo)), W = 0, H = 32, Family = 54, Weight = 400, Face = "Consolas" };
            SetCurrentConsoleFontEx(GetStdHandle(-11), false, ref f);
        } catch { }
        Focus();
    }

    // Shows a message for a few seconds (a key or button closes it sooner): direct mode needs no input, because
    // Steam's virtual controller isn't visible to a console program.
    static void Timed(string text, ConsoleColor color, int seconds) {
        Header("Skate launcher");
        Console.ForegroundColor = color;
        foreach (var l in text.Split('\n')) Console.WriteLine("  " + l.TrimEnd('\r'));
        Console.ResetColor(); Console.WriteLine();
        Focus();
        for (int s = seconds; s > 0; s--) {
            Console.ForegroundColor = ConsoleColor.DarkGray; Console.Write("\r  Closing in " + s + " s   "); Console.ResetColor();
            for (int t = 0; t < 25; t++) {
                if (Console.KeyAvailable || (Pad() & (A | B)) != 0) return;
                Thread.Sleep(40);
            }
        }
    }

    // Direct mode (one Steam shortcut per entry): no menu, no waiting for buttons.
    static int Direct(string entry, string label, string version) {
        if (entry == "version-next" || entry == "version-prev" || entry == "version-set" || entry == "notes") {
            string o = Ps(entry + (label == "" ? "" : " " + label) + VerArg(version), true);
            Timed(o, ConsoleColor.White, 15);
            return 0;
        }
        string name = null;
        foreach (var e in Entries()) if (e[0] == entry) name = e[1];
        if (name == null) { Timed("Unknown entry: " + entry + "\nCheck the shortcut's launch options (see README.md).", ConsoleColor.Red, 15); return 2; }
        string g = RunningGames();
        if (g != "") { Timed("Not started: already running " + g + ".\nOnly one game at a time.", ConsoleColor.Yellow, 12); return 3; }
        if (entry.StartsWith("rust-")) Timed("Starting: " + name + "\n\n" + Ps("notes" + VerArg(version), true), ConsoleColor.White, 8);
        Header("Skate launcher");
        Console.WriteLine("  Starting: " + name + (label != "" ? "  (" + label + ")" : ""));
        string args = "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + entry
                    + (label == "" ? "" : " " + label) + " -NoPrompt" + VerArg(version);
        var psi = new ProcessStartInfo("powershell.exe", args) { UseShellExecute = false, WorkingDirectory = Here };
        using (var p = Process.Start(psi)) p.WaitForExit();
        string last = LastSessionFile();
        string text = File.Exists(last) ? File.ReadAllText(last) : "No session result.";
        bool bad = text.Contains("MALFORMED") || text.Contains("No new") || text.Contains("Not started") || text.Contains("panic(s)");
        Timed("Session ended.\n\n" + text, bad ? ConsoleColor.Yellow : ConsoleColor.Green, 10);
        return 0;
    }

    // A session closed with Steam's "Exit game" skips its check (the launcher closes with the game): check it now.
    static void Pending() {
        Header("Skate launcher");
        var psi = new ProcessStartInfo("powershell.exe", "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" pending")
            { UseShellExecute = false, WorkingDirectory = Here };
        int code;
        using (var p = Process.Start(psi)) { p.WaitForExit(); code = p.ExitCode; }
        if (code != 10) return;
        string last = LastSessionFile();
        string text = File.Exists(last) ? File.ReadAllText(last) : "";
        bool bad = text.Contains("MALFORMED") || text.Contains("No new");
        Timed("Previous session (closed before its check):\n\n" + text, bad ? ConsoleColor.Yellow : ConsoleColor.Green, 8);
    }

    static int Main(string[] argv) {
        SetupConsole();
        Pending();
        if (argv.Length > 0) {
            string ver = "", lbl = "", ent = "";
            foreach (var a in argv) {
                if (a.StartsWith("@")) ver = a.Substring(1);
                else if (ent == "") ent = a; else if (lbl == "") lbl = a;
            }
            return Direct(ent, lbl, ver);
        }
        while (true) {
            var vers = Versions(); string curName = "?";
            foreach (var v in vers) if (v.Length > 3 && v[3] == "*") curName = v[1];
            int top = Menu("Skate launcher", new[] { "Rust engine: " + curName, "Choose version", "What to test (this version)", "Recomp (Skate 3 retail)", "Last session result", "Exit" },
                           new[] { "Our engine, the selected version.", "Any version in versions.json.", "", "The recompiled retail game, with or without tracing (config.json).", "", "" });
            if (top == -1 || top == 5) return 0;
            if (top == 1) {
                var vnames = new string[vers.Length]; var vstat = new string[vers.Length];
                for (int n = 0; n < vers.Length; n++) { vnames[n] = vers[n][1]; vstat[n] = vers[n][2]; }
                int vpick = Menu("Choose version", vnames, vstat);
                if (vpick >= 0) Ps("version-set " + vers[vpick][0], true);
                continue;
            }
            if (top == 2) { Message(Ps("notes", true), ConsoleColor.White); continue; }
            top = top == 3 ? 1 : top == 4 ? 2 : 0;
            if (top == 2) {
                string last = LastSessionFile();
                Message(File.Exists(last) ? File.ReadAllText(last) : "No session yet.", ConsoleColor.White);
                continue;
            }
            string prefix = top == 0 ? "rust-" : "recomp-";
            var keys = new System.Collections.Generic.List<string[]>();
            foreach (var e in Entries()) if (e[0].StartsWith(prefix)) keys.Add(e);
            if (keys.Count == 0) { Message("No recomp entries: add them to config.json (see README.md).", ConsoleColor.Yellow); continue; }
            var names = new string[keys.Count]; var hints = new string[keys.Count];
            for (int n = 0; n < keys.Count; n++) { names[n] = keys[n][1]; hints[n] = keys[n][2]; }
            while (true) {
                int pick = Menu(top == 0 ? "Rust engine" : "Recomp", names, hints);
                if (pick == -1) break;
                var e = keys[pick]; string label = "";
                if (e[3] == "1") {
                    int l = Menu("Label for the session folder", Labels, new[] { "No label." });
                    if (l == -1) continue;
                    label = Labels[l] == "session" ? "" : Labels[l];
                }
                Launch(e[0], label, e[1]);
            }
        }
    }
}
