// Couch launcher for Steam / Steam Link (see README.md): a camcorder-shaped window (docs/hails-additions/41-...) with a
// menu for the controller, the keyboard, the mouse or Steam's desktop layout. launcher.ps1 does the work (versions, builds, launching, session checks) and
// this window shows its output; the entry and version lists come from launcher.ps1 too.
//   D-pad / left stick = move, A = select, B = back. Arrows / Enter / Esc / Space (Steam's desktop layout) work too.
//   SkateLauncher.exe <entry> [label] [@version]  runs an entry directly (one-click Steam shortcuts), shows the
//   result for a few seconds and closes. version-next / version-prev / version-set <id> / notes work the same way.
// Build (build.bat): C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /target:winexe
//   /out:SkateLauncher.exe SkateLauncher.cs   (that compiler is C# 5: no string interpolation or ?.)
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

static class Native {
    [StructLayout(LayoutKind.Sequential)]
    public struct XState { public uint Packet; public ushort Buttons; public byte LT, RT; public short LX, LY, RX, RY; }
    [DllImport("xinput1_4.dll")] public static extern int XInputGetState(int i, out XState s);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AllowSetForegroundWindow(int pid);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool ReleaseCapture();
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [StructLayout(LayoutKind.Sequential)] struct Pt { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct Sz { public int W, H; }
    [StructLayout(LayoutKind.Sequential, Pack = 1)] struct Blend { public byte Op, Flags, Alpha, Format; }
    [DllImport("user32.dll")] static extern bool UpdateLayeredWindow(IntPtr h, IntPtr dst, ref Pt pos, ref Sz size, IntPtr src, ref Pt srcPos, int key, ref Blend b, int flags);
    public static void DeleteHbitmap(IntPtr h) { DeleteObject(h); }
    // Hands a frame with per-pixel alpha to a layered window (the camcorder's outline is its own alpha).
    public static void Push(IntPtr hwnd, Bitmap bmp, int x, int y, byte alpha) {
        IntPtr screen = GetDC(IntPtr.Zero), mem = CreateCompatibleDC(screen), hb = bmp.GetHbitmap(Color.FromArgb(0)), old = SelectObject(mem, hb);
        var pos = new Pt { X = x, Y = y }; var size = new Sz { W = bmp.Width, H = bmp.Height }; var src = new Pt(); var bl = new Blend { Alpha = alpha, Format = 1 };
        UpdateLayeredWindow(hwnd, screen, ref pos, ref size, mem, ref src, 0, ref bl, 2);
        SelectObject(mem, old); DeleteObject(hb); DeleteDC(mem); ReleaseDC(IntPtr.Zero, screen);
    }

    // Windows only lets the foreground owner hand the foreground on; a window Steam or a hidden launcher starts is
    // otherwise left behind the last active one. Joining the foreground thread's input for the call lifts that.
    public static void Front(IntPtr h) {
        if (h == IntPtr.Zero) return;
        if (IsIconic(h)) ShowWindow(h, 9);
        uint fg = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero), me = GetCurrentThreadId();
        bool joined = fg != 0 && fg != me && AttachThreadInput(me, fg, true);
        BringWindowToTop(h); SetForegroundWindow(h);
        if (joined) AttachThreadInput(me, fg, false);
    }
}

// One menu row: greyed rows (On = false) can be selected to read why, but do nothing.
class Item {
    public string Text, Hint; public bool On = true; public Action Run;
    public string Info = ""; public string[] Records = new string[0];   // the play screen's detail panel
    public Item(string text, string hint, bool on, Action run) { Text = text; Hint = hint ?? ""; On = on; Run = run; }
}
class Page {
    public string Title; public List<Item> Items = new List<Item>(); public int Sel;
    public string Body;          // a text page (notes, results) instead of a list
    public Action OnBack;        // null = pop the page
    public string Mono;          // a fixed-width block under the body (a build's first error)
    public int Cols;             // > 0: the items are tiles like the main screen's, Cols wide (0 = full-width rows)
}

// Build progress from the build output (launcher.ps1 marks the build with LAUNCHER_BUILD lines and turns on cargo's own
// "Building [..] n/total" bar for the window). Every line also goes to the build log file it names.
class BuildView {
    static readonly System.Text.RegularExpressions.Regex Ansi = new System.Text.RegularExpressions.Regex(@"\x1B(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1B]*(\x07|\x1B\\)|[@-Z\\-_])");
    static readonly System.Text.RegularExpressions.Regex Bar = new System.Text.RegularExpressions.Regex(@"Building \[[^\]]*\]\s*(\d+)/(\d+)(?::\s*(.*))?");
    static readonly System.Text.RegularExpressions.Regex Compiling = new System.Text.RegularExpressions.Regex(@"Compiling (\S+) v");
    static readonly System.Text.RegularExpressions.Regex ErrStart = new System.Text.RegularExpressions.Regex(@"^error(\[E\d+\])?: ");
    // Cargo waiting for another cargo (USER-ASKS 72: the laptop showed "0 crates compiled" while cargo printed "Blocking
    // waiting for file lock on build directory"); and its index / download stages, which come before any build step.
    static readonly System.Text.RegularExpressions.Regex Lock = new System.Text.RegularExpressions.Regex(@"Blocking waiting for file lock on (.+)$");
    static readonly System.Text.RegularExpressions.Regex Stage = new System.Text.RegularExpressions.Regex(@"^(Updating|Downloading|Downloaded|Locking|Fetching)\b");
    public string Waiting, WaitWho, StageText; public DateTime WaitSince;
    // The fetch before a build (LAUNCHER_WAIT lines, USER-ASKS 72): shown as WAITING once it has lasted 5 s.
    public string PreWait, LastPreWait; public DateTime PreWaitSince;
    public bool FetchWaiting() { return PreWait != null && (DateTime.Now - PreWaitSince).TotalSeconds >= 5; }
    public bool Active, Ended, Ok; public string Name = "", Commit = "", LogPath, Now = "";
    public int Done, Total, Compiled, Warnings; public DateTime Start, End;
    public readonly List<string> Error = new List<string>(); bool inError; string couldNot;
    public readonly List<string> Tail = new List<string>();   // the last clean lines, when there is no error block
    StreamWriter file;

    // The text after the last \r, no ANSI or control characters, tabs as spaces.
    public static string Clean(string s) {
        s = Ansi.Replace(s ?? "", "");
        int r = s.LastIndexOf('\r'); if (r >= 0) s = s.Substring(r + 1);
        var sb = new StringBuilder(s.Length);
        foreach (char c in s) { if (c == '\t') sb.Append("    "); else if (c >= ' ') sb.Append(c); }
        return sb.ToString().TrimEnd();
    }
    // True when the line is a marker or build output (kept out of the plain job log).
    public bool Feed(string raw) {
        if (raw.StartsWith("LAUNCHER_BUILD\tbegin\t")) {
            var f = raw.Split('\t');
            Close(); Active = true; Ended = false; Name = f.Length > 2 ? f[2] : ""; Commit = f.Length > 3 ? f[3] : ""; LogPath = f.Length > 4 ? f[4] : null;
            Done = Total = Compiled = Warnings = 0; Error.Clear(); Tail.Clear(); inError = false; couldNot = null; Now = ""; Start = DateTime.Now; Waiting = null; WaitWho = null; StageText = null;
            try { if (LogPath != null) { Directory.CreateDirectory(Path.GetDirectoryName(LogPath)); file = new StreamWriter(LogPath, false, new UTF8Encoding(false)); } } catch { file = null; }
            if (LastPreWait != null) { Note(LastPreWait); LastPreWait = null; }
            return true;
        }
        if (raw.StartsWith("LAUNCHER_SELF\t")) return true;   // the start check's own markers (b522)
        if (raw.StartsWith("LAUNCHER_WAIT\t")) {
            var f = raw.Split('\t');
            if (f.Length > 4 && f[1] == "begin") { PreWait = "fetching " + f[3] + " from " + f[4]; PreWaitSince = DateTime.Now; }
            else if (f.Length > 3 && f[1] == "end" && PreWait != null) { LastPreWait = (f[3] == "0" ? "fetched" : "fetch failed,") + PreWait.Substring(8) + " in " + Clock(DateTime.Now - PreWaitSince); PreWait = null; }
            return true;
        }
        // The log closes when the build ends, not when the job does: a game session follows a build in the same job, and
        // an open log blocked the after-session send (laptop 2026-10-19).
        if (raw.StartsWith("LAUNCHER_BUILD\tend\t")) { Active = false; Ended = true; End = DateTime.Now; Ok = raw.EndsWith("\t0"); Close(); return true; }
        if (!Active) return false;
        // launcher.ps1 names who holds the build folder (BUILDING-<id>.txt or a cargo process) before cargo blocks.
        if (raw.StartsWith("LAUNCHER_BUILD\twait\t")) {
            var f = raw.Split('\t'); WaitWho = f.Length > 3 ? f[3] : null; Note("holder: " + WaitWho);
            // "hold": launcher.ps1 itself waits (the version's exe is running), so the card shows it as a wait (b521 4c).
            if (f.Length > 4 && f[4] == "hold") { if (Waiting == null) { WaitSince = DateTime.Now; Note("waiting: " + WaitWho); } Waiting = WaitWho; }
            return true;
        }
        if (raw.StartsWith("LAUNCHER_BUILD\twaitend")) { EndWait(); return true; }
        string c = Clean(raw);
        var m = Bar.Match(c);
        if (m.Success) { Done = int.Parse(m.Groups[1].Value); Total = int.Parse(m.Groups[2].Value); if (m.Groups[3].Success) Now = m.Groups[3].Value.Trim(); }
        var cm = Compiling.Match(c);
        bool barOnly = m.Success && !cm.Success;
        if (file != null && !barOnly && c != "") { try { file.WriteLine(Ansi.Replace(raw, "").Replace("\r", "")); } catch { } }
        var lk = Lock.Match(c);
        if (lk.Success) {
            if (Waiting == null) { WaitSince = DateTime.Now; Note("waiting: " + lk.Groups[1].Value.Trim() + (WaitWho != null ? ", " + WaitWho : "")); }
            Waiting = "another build is using the " + lk.Groups[1].Value.Trim();
        }
        else if (m.Success || cm.Success) { EndWait(); StageText = null; }
        else if (Stage.IsMatch(c)) { EndWait(); StageText = c; }
        if (cm.Success) { Compiled++; Now = cm.Groups[1].Value; }
        if (inError) {
            if (c == "" || Error.Count >= 40) inError = false; else Error.Add(c);
        } else if (Error.Count == 0 && ErrStart.IsMatch(c)) {
            if (c.StartsWith("error: could not compile")) couldNot = c; else { inError = true; Error.Add(c); }
        } else if (c.StartsWith("warning: ") && !c.Contains(" generated ") && !c.StartsWith("warning: build failed")) Warnings++;
        if (c != "" && !barOnly) { Tail.Add(c); if (Tail.Count > 15) Tail.RemoveAt(0); }
        return true;
    }
    // The first error block, else cargo's summary and the last lines (e.g. Build.ps1's own failure).
    public List<string> FirstError() {
        if (Error.Count > 0) return Error;
        var l = new List<string>(); if (couldNot != null) l.Add(couldNot); l.AddRange(Tail); return l;
    }
    void EndWait() { if (Waiting != null) Note("waited " + Clock(DateTime.Now - WaitSince)); Waiting = null; }
    // A launcher line in the build log, so a session note shows when, why and how long a build waited.
    public void Note(string text) { if (file != null) { try { file.WriteLine("[launcher " + DateTime.Now.ToString("HH:mm:ss") + "] " + text); file.Flush(); } catch { } } }
    public static string Clock(TimeSpan t) { return ((int)t.TotalMinutes) + ":" + t.Seconds.ToString("00"); }
    public void Close() { if (file != null) { try { file.Dispose(); } catch { } file = null; } }
}

class LauncherForm : Form {    const ushort UP = 0x0001, DOWN = 0x0002, LEFT = 0x0004, RIGHT = 0x0008, A = 0x1000, B = 0x2000;
    int frame;
    static readonly string Here = AppDomain.CurrentDomain.BaseDirectory;
    static readonly string[] Labels = { "session", "ride", "grind", "bail", "push", "emit", "carve", "water", "marker" };
    static readonly string[] GameNames = { "skate3", "skate3rust", "rpcs3" };

    readonly Stack<Page> pages = new Stack<Page>();
    readonly Timer tick = new Timer();
    readonly List<string> log = new List<string>();
    readonly BuildView build = new BuildView(); bool dirty;
    readonly string[] direct; readonly bool afterUpdate;
    Dictionary<string, string> status = new Dictionary<string, string>();
    Process job; string jobTitle; bool jobHidGame; IntPtr gameWindow; DateTime hidAt; int closeAt = -1; DateTime closeTime;
    string waitEntry, waitLabel, waitName;   // waiting for another game to close before starting
    bool cancelArmed, cancelled, cancelledInFetch;  // B pressed once / twice while a build or its fetch waits
    ushort prevPad; int held; string lastAction = ""; bool lastFromPad; DateTime lastActionTime = DateTime.MinValue;
    Font monoFont, titleFont, itemFont, smallFont, heroFont, markFont, markLight, playFont, iconFont, iconSmall, itemBold, smallBold, bodyFont, splashMark, splashLight;

    public LauncherForm(string[] argv) {
        // --after-update: started by the previous exe after it updated itself (shows what changed), not a direct entry.
        afterUpdate = Array.IndexOf(argv, "--after-update") >= 0;
        sameMonitorFlag = Array.IndexOf(argv, "--same-monitor") >= 0;
        direct = Array.FindAll(argv, a => a != "--after-update" && a != "--same-monitor");
        Text = "Skate launcher";
        // The camcorder as its own window (no frame, its outline is the drawing's alpha): 72 % of the work area's height
        // at the camera's 16:10, centred. On the main monitor, or with --same-monitor / the Tools setting on the monitor
        // of the window it was started from.
        var area = StartScreen().WorkingArea;
        int h = Math.Min(area.Height * 72 / 100, area.Width * 72 / 100 * 10 / 16);
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual; ClientSize = new Size(h * 16 / 10, h);
        Location = new Point(area.Left + (area.Width - Width) / 2, area.Top + (area.Height - Height) / 2);
        BackColor = Color.FromArgb(18, 20, 24); ForeColor = Color.White;
        DoubleBuffered = true; KeyPreview = true;
        try { Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
        tick.Interval = 16; tick.Tick += delegate { Poll(); if (needPresent) Present(); };
        // The window paints first; the start check (launcher.ps1 pending: self update, session check) runs behind a card
        // (it froze the start with no window during a slow self-update fetch; b522).
        Load += delegate { tick.Start(); };
        Shown += delegate { Start(); };
        Resize += delegate { MakeFonts(); Invalidate(); };
        MakeFonts();
    }

    void MakeFonts() { MakeFonts(ClientSize.Height); }
    void MakeFonts(int height) {
        float h = Math.Max(480, height);
        foreach (var f in hintFonts.Values) f.Dispose();
        hintFonts.Clear();
        titleFont = new Font("Segoe UI Semibold", h / 24f, GraphicsUnit.Pixel);
        heroFont = new Font("Segoe UI Semibold", h / 17f, GraphicsUnit.Pixel);
        itemFont = new Font("Segoe UI", h / 34f, GraphicsUnit.Pixel);
        itemBold = new Font("Segoe UI Semibold", h / 34f, GraphicsUnit.Pixel);
        smallFont = new Font("Segoe UI", h / 52f, GraphicsUnit.Pixel);
        monoFont = new Font("Consolas", h / 50f, GraphicsUnit.Pixel);
        smallBold = new Font("Segoe UI Semibold", h / 52f, GraphicsUnit.Pixel);
        bodyFont = new Font("Segoe UI", h / 42f, GraphicsUnit.Pixel);
        markFont = new Font("Segoe UI Black", h / 30f, GraphicsUnit.Pixel);
        markLight = new Font("Segoe UI Light", h / 30f, GraphicsUnit.Pixel);
        splashMark = new Font("Segoe UI Black", h / 11f, GraphicsUnit.Pixel);
        splashLight = new Font("Segoe UI Light", h / 11f, GraphicsUnit.Pixel);
        playFont = new Font("Segoe UI Black", h / 26f, GraphicsUnit.Pixel);
        iconFont = new Font("Segoe MDL2 Assets", h / 26f, GraphicsUnit.Pixel);
        iconSmall = new Font("Segoe MDL2 Assets", h / 40f, GraphicsUnit.Pixel);
    }

    public string Bench(int w, int h) {
        snapshotMode = true; RefreshStatus(); pages.Push(MainPage());
        var sb = new StringBuilder();
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb)) using (var g = Graphics.FromImage(bmp)) {
            foreach (float at in new[] { 0.3f, 0.5f, 0.65f, 0.9f, 1f }) {
                snapPanel = at; flipScale = -1; Render(g, w, h);
                var sw = Stopwatch.StartNew();
                for (int i = 0; i < 20; i++) { g.Clear(Color.Transparent); Render(g, w, h); }
                double ms = sw.Elapsed.TotalMilliseconds / 20;
                sw.Restart();
                for (int i = 0; i < 20; i++) { IntPtr hb = bmp.GetHbitmap(Color.FromArgb(0)); Native.DeleteHbitmap(hb); }
                sb.AppendLine(string.Format(System.Globalization.CultureInfo.InvariantCulture, "assembly {0:0.00}: render {1:0.0} ms, GetHbitmap {2:0.0} ms", at, ms, sw.Elapsed.TotalMilliseconds / 20));
            }
        }
        return sb.ToString();
    }

    // Draws one screen into a PNG (the --snapshot mode): main, versions, tools or job (a sample build log).
    public void Snapshot(string file, string screen, int w, int h) {
        // "<screen>#7": the sticker from seed 7; "<screen>@0.4": 40 % through the power-on (the camera unfolding).
        snapshotMode = true; snapPanel = 1;
        int hash = screen.IndexOf('#');
        if (hash > 0) { int.TryParse(screen.Substring(hash + 1), out snapSeed); screen = screen.Substring(0, hash); }
        int at = screen.IndexOf('@');
        if (at > 0) { float.TryParse(screen.Substring(at + 1), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out snapPanel); screen = screen.Substring(0, at); }
        MakeFonts(h);
        RefreshStatus();
        if (screen == "checking") { starting = true; selfFetching = true; startSince = DateTime.Now.AddSeconds(-7); }
        else pages.Push(MainPage());
        if (screen == "versions") pages.Push(VersionPage());
        if (screen == "tools") pages.Push(EntryPage("Tools", "tools"));
        if (screen == "setup") pages.Push(WizardPage());
        if (screen == "play" || screen == "play-traceall") pages.Push(EntryPage("Play: " + St("version"), "rust"));
        if (screen == "play-traceall") pages.Peek().Sel = Math.Min(1, pages.Peek().Items.Count - 1);
        if (screen == "waiting") {
            job = new Process(); jobTitle = "Update and build";
            build.Feed("LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t");
            build.Start = DateTime.Now.AddSeconds(-75);
            build.Feed("LAUNCHER_BUILD\twait\tbuild\thails-additions is being built by the shell since 16:41 (pid 21412)");
            build.Feed("    Blocking waiting for file lock on build directory");
            build.WaitSince = DateTime.Now.AddSeconds(-72);
        }
        if (screen == "fetching") {
            job = new Process(); jobTitle = "Update and build"; jobStart = DateTime.Now.AddSeconds(-44);
            log.Add("  Checking hails-additions on origin...");
            build.Feed("LAUNCHER_WAIT\tbegin\tfetch\thails-additions\torigin");
            build.PreWaitSince = DateTime.Now.AddSeconds(-42);
        }
        if (screen == "build" || screen == "failed") {
            job = new Process(); jobTitle = "Update and build";
            build.Feed("LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t");
            build.Start = DateTime.Now.AddSeconds(-190);
            foreach (var l in new[] { "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`", " --> crates\\skate-game\\src\\physics.rs:81:53", "", "    Building [=======>        ] 312/540: skate-game, bevy_pbr" }) build.Feed(l);
            if (screen == "failed") foreach (var l in new[] { "error[E0308]: mismatched types", "  --> crates\\skate-game\\src\\physics\\grab_view.rs:42:5", "   |", "42 |     let x: u32 = 1.0;", "   |            ---   ^^^ expected `u32`, found floating-point number", "" }) build.Feed(l);
        }
        if (screen == "job") {
            job = new Process(); jobTitle = "Building hails-additions"; jobStart = DateTime.Now.AddSeconds(-74);
            foreach (var l in new[] { "Fetching hails-additions at 94774ce", "   Compiling bevy_render v0.18.1", "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`", "   Compiling skate-game v0.1.0" }) log.Add(l);
        }
        using (var bmp = new Bitmap(w, h)) {
            using (var g = Graphics.FromImage(bmp)) { Render(g, w, h); }

            bmp.Save(file, System.Drawing.Imaging.ImageFormat.Png);
        }
    }

    // ---------- launcher.ps1 ----------
    static ProcessStartInfo Ps(string args) {
        var psi = new ProcessStartInfo("powershell.exe", "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + args) {
            UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Here,
            RedirectStandardOutput = true, RedirectStandardError = true, RedirectStandardInput = true };
        // The window's build view: launcher.ps1 marks the build and turns on cargo's progress bar.
        psi.EnvironmentVariables["SKATE_LAUNCHER_UI"] = "2";
        var oem = Encoding.GetEncoding(System.Globalization.CultureInfo.CurrentCulture.TextInfo.OEMCodePage);
        psi.StandardOutputEncoding = oem; psi.StandardErrorEncoding = oem;
        return psi;
    }
    // Runs a quick command and returns its output (the lists, notes, status).
    static string Run(string args, out int code) {
        using (var p = Process.Start(Ps(args))) {
            p.StandardInput.Close();
            string o = p.StandardOutput.ReadToEnd(); p.WaitForExit(); code = p.ExitCode;
            return o.TrimEnd('\r', '\n');   // not tabs: a last row can end in an empty column
        }
    }
    static string Run(string args) { int c; return Run(args, out c); }
    // The lists behind the main tiles (Versions, What to test, Recomp, RPCS3, Tools): each launcher.ps1 call is a new
    // powershell.exe (1 to 2.5 s on the laptop, 2026-10-19), so they are fetched in the background at start and after
    // every change (version picked, job finished) and the tiles open from memory. A tile pressed before its fetch is
    // done waits only for the rest of it.
    static readonly string[] Prefetched = { "entries", "versions", "notes", "status" };
    static readonly Dictionary<string, Task<string>> cache = new Dictionary<string, Task<string>>();
    static Task<string> Fetch(string args) {
        lock (cache) {
            Task<string> t;
            if (!cache.TryGetValue(args, out t)) { t = Task.Run(() => Run(args)); cache[args] = t; }
            return t;
        }
    }
    static string Listed(string args) {
        if (Array.IndexOf(Prefetched, args) < 0) return Run(args);
        var t = Fetch(args);
        try { return t.GetAwaiter().GetResult(); }
        catch { lock (cache) { Task<string> c; if (cache.TryGetValue(args, out c) && c == t) cache.Remove(args); } throw; }
    }
    static void Prefetch() { foreach (var a in Prefetched) Fetch(a); }
    // Waits for the prefetched lists off the window thread (a failed one is fetched again when read), so the menu is
    // built from memory: waiting on them in the window froze it for seconds, and Windows offered to end it (USER-ASKS 436).
    static void WaitListed() {
        var all = new List<Task>();
        foreach (var a in Prefetched) all.Add(Fetch(a));
        try { Task.WaitAll(all.ToArray()); } catch { }
    }
    void WhenListed(Action then) { Task.Run((Action)WaitListed).ContinueWith(t => BeginInvoke(then)); }
    static void ForgetCache() { lock (cache) cache.Clear(); }
    static List<string[]> Rows(string args, int min) {
        var rows = new List<string[]>();
        foreach (var l in Listed(args).Split('\n')) { var f = l.TrimEnd('\r').Split('\t'); if (f.Length >= min) rows.Add(f); }
        return rows;
    }
    static string StateDir() {
        string here = Path.GetFullPath(Here).TrimEnd('\\');
        string repo = Path.GetFullPath(Path.Combine(here, "..", ".."));
        if (here.StartsWith(Path.Combine(repo, "tools") + "\\", StringComparison.OrdinalIgnoreCase)) return Path.Combine(repo, ".local", "steam-launcher");
        return here;
    }
    bool sameMonitorFlag;
    static string SameMonitorFile() { return Path.Combine(StateDir(), "open-on-launching-monitor.txt"); }
    static bool SameMonitorSetting() { return File.Exists(SameMonitorFile()); }
    Screen StartScreen() {
        if (!sameMonitorFlag && !SameMonitorSetting()) return Screen.PrimaryScreen;
        IntPtr fg = Native.GetForegroundWindow();
        return fg != IntPtr.Zero ? Screen.FromHandle(fg) : Screen.FromPoint(Cursor.Position);
    }
    Item SameMonitorItem() {
        Func<string> text = () => "Open on: " + (SameMonitorSetting() ? "the monitor it was started from" : "the main monitor");
        Item item = null;
        item = new Item(text(), "Where the launcher window opens. --same-monitor on the command line does the same for one start.", true, delegate {
            try { if (SameMonitorSetting()) File.Delete(SameMonitorFile()); else File.WriteAllText(SameMonitorFile(), "on\n"); } catch { }
            item.Text = text();
        });
        return item;
    }
    static string LastSession() { string f = Path.Combine(StateDir(), "last_session.txt"); return File.Exists(f) ? File.ReadAllText(f).Trim() : "No session yet."; }
    static string VerArg(string v) { return string.IsNullOrEmpty(v) ? "" : " -Version " + v; }
    static string RunningGames() {
        string r = "";
        foreach (var n in GameNames) {
            var found = Process.GetProcessesByName(n);
            if (found.Length > 0) r += (r == "" ? "" : ", ") + n + ".exe";
            foreach (var p in found) p.Dispose();
        }
        // A Refresh Game Files run (launcher.ps1 Refresh-GameFiles) is named instead of its exe (USER-ASKS 72).
        string marker = Path.Combine(StateDir(), "REFRESHING.txt");
        if (r != "" && File.Exists(marker)) { try { r = "Refresh game files (" + File.ReadAllText(marker).Trim() + ")"; } catch { } }
        return r;
    }
    // From the prefetched status when `cached` (just fetched with the lists), else a fresh run (after a share toggle).
    void RefreshStatus() { RefreshStatus(false); }
    void RefreshStatus(bool cached) {
        if (!cached) lock (cache) cache.Remove("status");
        status.Clear();
        foreach (var r in Rows("status", 2)) status[r[0]] = r[1];
    }
    string St(string k) { string v; return status.TryGetValue(k, out v) ? v : ""; }

    // ---------- start ----------
    // The launcher updated its own exe (launcher.ps1 wrote restart.txt): start the new one and wait for it hidden, so
    // Steam keeps the process it started (its controller layout and "playing" state) until the new window closes.
    bool RestartIfUpdated(bool showLast) {
        string f = Path.Combine(StateDir(), "restart.txt");
        if (!File.Exists(f)) return false;
        try { File.Delete(f); } catch { }
        string exe = Path.Combine(Here, "SkateLauncher.exe");
        if (!File.Exists(exe)) return false;
        var args = new List<string>(direct);
        if (showLast) args.Add("--after-update");
        if (sameMonitorFlag) args.Add("--same-monitor");
        Process child;
        SkateLauncher.ReleaseInstance();
        try { child = Process.Start(new ProcessStartInfo(exe, string.Join(" ", args.ConvertAll(a => "\"" + a + "\"").ToArray())) { UseShellExecute = false }); } catch { return false; }
        tick.Stop(); Hide();
        child.EnableRaisingEvents = true;
        child.Exited += delegate { BeginInvoke((Action)Close); };
        if (child.HasExited) Close();
        return true;
    }
    bool starting, skipArmed; DateTime startSince;
    // The start check's process, whether its self-update fetch is done (LAUNCHER_SELF) and whether B skipped it.
    readonly object startLock = new object(); Process startProc; volatile bool selfFetching, selfFetched, skipped;
    // The start check's current step (LAUNCHER_STAGE lines), shown under the rolling board.
    volatile string startStage;
    const double SplashMinSeconds = 1.1;
    void Start() {
        Activate(); Native.Front(Handle);
        starting = true; startSince = DateTime.Now; selfFetching = selfFetched = skipped = skipArmed = false; startStage = null; Invalidate();
        // The splash stays at least SplashMinSeconds, so a fast check does not flash it.
        Task.Run(() => {
            int code = RunPending(false);
            // The menu's lists load behind the splash (not when an updated exe takes over, or in direct mode).
            if (direct.Length == 0 && !File.Exists(Path.Combine(StateDir(), "restart.txt"))) { startStage = "Loading the menu"; Prefetch(); WaitListed(); }
            int left = (int)((SplashMinSeconds - (DateTime.Now - startSince).TotalSeconds) * 1000);
            if (left > 0) Task.Delay(left).Wait();
            return code;
        }).ContinueWith(t => BeginInvoke((Action)(() => FinishStart(t.IsFaulted ? 0 : t.Result))));
    }
    // launcher.ps1 pending; killed by B during the fetch, it runs again without the self update so the rest still happens.
    int RunPending(bool noSelf) {
        var psi = Ps("pending");
        if (noSelf) psi.EnvironmentVariables["SKATE_LAUNCHER_NO_SELF"] = "1";
        int code;
        using (var p = Process.Start(psi)) {
            lock (startLock) startProc = p;
            p.StandardInput.Close();
            string line;
            while ((line = p.StandardOutput.ReadLine()) != null) {
                if (line.StartsWith("LAUNCHER_SELF\tfetching")) selfFetching = true;
                else if (line.StartsWith("LAUNCHER_SELF\tfetched")) selfFetched = true;
                else if (line.StartsWith("LAUNCHER_STAGE\t")) startStage = line.Substring(15);
            }
            p.WaitForExit(); code = p.ExitCode;
            lock (startLock) startProc = null;
        }
        return skipped && !noSelf ? RunPending(true) : code;
    }
    // B during the self-update fetch: the first press arms, the second kills the check (never after the fetch, when files copy).
    bool CanSkip() { return starting && selfFetching && !selfFetched && !skipped && (DateTime.Now - startSince).TotalSeconds >= 3; }
    void SkipPress() {
        if (!CanSkip()) return;
        if (!skipArmed) { skipArmed = true; dirty = true; return; }
        lock (startLock) {
            if (startProc == null || selfFetched) return;
            skipped = true;
            try { Process.Start(new ProcessStartInfo("taskkill.exe", "/T /F /PID " + startProc.Id) { CreateNoWindow = true, UseShellExecute = false }); } catch { }
        }
    }
    void FinishStart(int code) {
        starting = false;
        if (RestartIfUpdated(code == 10)) return;
        string ent = "", lbl = "", ver = "";
        foreach (var a in direct) { if (a.StartsWith("@")) ver = a.Substring(1); else if (ent == "") ent = a; else if (lbl == "") lbl = a; }
        if (ent != "") { Direct(ent, lbl, ver); return; }
        Prefetch();
        RefreshStatus(true);
        pages.Push(MainPage());
        if (St("setup_missing") != "") pages.Push(SetupPage());
        if (St("first_setup") != "") pages.Push(WizardPage());
        if (code == 10 || afterUpdate) ShowText(LastSession().Contains("launcher update") || afterUpdate ? "Launcher updated" : "Previous session (closed before its check)", LastSession(), null);
        Invalidate();
    }

    // Direct mode (one Steam shortcut per entry): no menu, the result closes by itself.
    void Direct(string entry, string label, string version) {
        if (entry == "version-next" || entry == "version-prev" || entry == "version-set" || entry == "notes") {
            ShowText("Skate launcher", Run(entry + (label == "" ? "" : " " + label) + VerArg(version)), null);
            CloseIn(15); return;
        }
        string name = null;
        var rows = Rows("entries" + VerArg(version), 7);
        foreach (var e in rows) if (e[0] == entry) name = e[1];
        if (name == null) { ShowText("Unknown entry", "Unknown entry: " + entry + " (" + rows.Count + " entries listed)\nCheck the shortcut's launch options (see README.md).", null); CloseIn(15); return; }
        if (entry.StartsWith("rust-")) log.Add(Run("notes" + VerArg(version)));
        StartJob(entry + (label == "" ? "" : " " + label) + VerArg(version), name);
    }
    void CloseIn(int seconds) { closeAt = seconds; closeTime = DateTime.Now.AddSeconds(seconds); }

    // ---------- pages ----------
    Page MainPage() {
        var p = new Page { Title = "Skate launcher" };
        p.OnBack = delegate { };
        var entries = Rows("entries", 7);
        p.Items.Add(new Item("Play: " + St("version"), "Our engine, the selected version (" + St("version_status") + ").", true, delegate { pages.Push(EntryPage("Play: " + St("version"), "rust")); }));
        p.Items.Add(new Item("Versions", "Pick the build Play starts.", true, delegate { pages.Push(VersionPage()); }));
        p.Items.Add(new Item("What to test", "What changed and what to try.", true, delegate { ShowText("What to test", Listed("notes"), null); }));
        bool recomp = entries.Exists(e => e[4] == "recomp"), retail = entries.Exists(e => e[4] == "retail");
        p.Items.Add(new Item("Recomp", recomp ? "Recompiled retail, traced or not." : "Not set up (see README.md).", recomp, delegate { pages.Push(EntryPage("Recomp", "recomp")); }));
        p.Items.Add(new Item("RPCS3", retail ? "The PS3 game in RPCS3." : "Not set up (see README.md).", retail, delegate { pages.Push(EntryPage("Retail Skate 3 (RPCS3)", "retail")); }));
        p.Items.Add(new Item("Tools", "Game files, builds, logs, recomp, setup.", true, delegate { pages.Push(EntryPage("Tools", "tools")); }));
        p.Items.Add(new Item("Last session", "What was checked and sent.", true, delegate { ShowText("Last session", LastSession(), null); }));
        p.Items.Add(new Item("Exit", "Close the launcher.", true, delegate { Close(); }));
        return p;
    }
    Page EntryPage(string title, string group) {
        // The engine's play screen (Play, Play + trace all, ...): buttons two wide like the main screen (laptop notes
        // 2026-10-21 22:23).
        var p = new Page { Title = title, Cols = group == "rust" ? 2 : 0 };
        foreach (var e in Rows("entries", 7)) {
            if (e[4] != group) continue;
            var row = e;
            bool on = row[5] == "1";
            var item = new Item(row[1], on ? row[2] : row[6], on, delegate {
                if (row[3] == "1") pages.Push(LabelPage(row)); else Begin(row[0], "", row[1]);
            });
            // The detail panel's text (launcher.ps1 entries columns 8 and 9; an older launcher.ps1 has neither).
            if (row.Length > 7) item.Info = row[7];
            if (row.Length > 8 && row[8] != "") item.Records = row[8].Split('|');
            p.Items.Add(item);
        }
        if (group == "rust" || group == "tools") p.Items.Add(ShareItem());
        if (group == "tools") p.Items.Add(new Item("Opt-ins and settings", "The game's opt-ins and settings, the same ones as its Guide / F1 overlay.", true, delegate { pages.Push(GameSettingsPage("Opt-ins")); }));
        if (group == "tools") p.Items.Add(SameMonitorItem());
        if (group == "tools") p.Items.Add(new Item("Set up the launcher", "Game folder, space for builds, the Skate 3 disc, the recomp and RPCS3.", true, delegate { wiz = null; pages.Push(WizardPage()); }));
        if (group == "tools") p.Items.Add(new Item("Clear builds and build caches", "Frees disk space: everything, or one version's build cache or whole build. Asks before deleting.", true, delegate { pages.Push(ClearPage()); }));
        if (p.Items.Count == 0) p.Items.Add(new Item("Nothing here yet", "See README.md to set these up.", false, null));
        return p;
    }
    // Tools > Opt-ins and settings (USER-ASKS 337): launcher.ps1 "game-settings" lists the game's saved opt-ins and settings
    // (settings\*.json, the files the game's overlay writes); pressing a row steps it ("game-setting <key>"). Opt-ins
    // first, the settings on their own page.
    Page GameSettingsPage(string group) {
        var p = new Page { Title = group == "Opt-ins" ? "Opt-ins" : "Settings" };
        foreach (var r in Rows("game-settings", 5)) {
            if (r[1] != group) continue;
            var row = r; var item = new Item(row[2] + ": " + row[3], row[4], true, null);
            item.Run = delegate {
                int code; string o = Run("game-setting " + row[0], out code);
                if (code != 0) { ShowText(row[2], o, null); return; }
                foreach (var n in Rows("game-settings", 5)) if (n[0] == row[0]) { item.Text = n[2] + ": " + n[3]; }
            };
            p.Items.Add(item);
        }
        if (group == "Opt-ins") p.Items.Add(new Item("Settings", "Resolution, render scale, framerate cap, FPS counter, volumes and difficulty.", true, delegate { pages.Push(GameSettingsPage("Settings")); }));
        if (p.Items.Count == 0) p.Items.Add(new Item("No game files yet", "Convert the game files first (Tools > setup).", false, null));
        return p;
    }
    // docs/hails-additions/second-pc-testing.md#share-sessions-via-github
    Item ShareItem() {
        var item = new Item("", "", false, null);
        item.Run = delegate {
            if (St("share").StartsWith("on ")) { Run("share-off"); RefreshStatus(); ShowShare(item); }
            else pages.Push(ConsentPage(item));
        };
        ShowShare(item);
        return item;
    }
    void ShowShare(Item item) {
        string s = St("share");
        bool on = s.StartsWith("on ");
        item.Text = s == "" ? "Share sessions via GitHub" : "Share sessions: " + (on ? "ON" : "OFF");
        item.Hint = s == "" ? "Not in this launcher version." : on ? "After each session the logs are packed and a GitHub issue opens on " + s.Substring(3) + ". Press to turn off."
            : s == "no repo" ? "This version has no GitHub repository to send to." : "Off. Press to read what is shared and turn it on.";
        item.On = s != "" && s != "no repo";
    }
    Page ConsentPage(Item toggle) {
        var p = new Page { Title = "Share sessions via GitHub" };
        p.Items.Add(new Item("Turn it on", "After each session the launcher zips the new logs and opens a GitHub issue on the repository of the version you play, in your browser. You drag the zip in and press Submit; nothing is sent without that.", true,
            delegate { Run("share-on"); RefreshStatus(); ShowShare(toggle); pages.Pop(); }));
        p.Items.Add(new Item("Not now", "Sharing stays off.", true, delegate { pages.Pop(); }));
        p.Items.Add(new Item("What is shared", "Game logs, trace folders and launcher build logs of this PC, with your Windows user name, home folder and PC name replaced. Not shared: saves, game files, recomp sound captures. Issues on public repositories are public. Turn it off here or in Tools at any time.", false, null));
        return p;
    }
    static string Gb(long bytes) { return (bytes / 1073741824.0).ToString("0.0") + " GB"; }
    // A version's build cache = its cargo target folder (it plays on, the next build starts from scratch); its build =
    // that plus bin\ (it builds again at its next launch).
    Page ClearPage() {
        var p = new Page { Title = "Clear builds and build caches" };
        long allCache = 0, allBin = 0;
        var rows = Rows("sizes", 4);
        foreach (var r in rows) { allCache += long.Parse(r[2]); allBin += long.Parse(r[3]); }
        p.Items.Add(new Item("Everything: every build and build cache (" + Gb(allCache + allBin) + ")", "Every version builds again at its next launch.", allCache + allBin > 0,
            delegate { Confirm("Delete every build and build cache?", "clear-build all", "Clear all builds"); }));
        p.Items.Add(new Item("Every build cache (" + Gb(allCache) + ")", "The versions still play; their next builds start from scratch.", allCache > 0,
            delegate { Confirm("Delete every build cache?", "clear-cache all", "Clear all build caches"); }));
        foreach (var r in rows) {
            var row = r; long cache = long.Parse(row[2]), bin = long.Parse(row[3]);
            if (cache + bin == 0) continue;   // never built here: nothing to clear
            p.Items.Add(new Item(row[1] + ": build cache (" + Gb(cache) + ")", cache > 0 ? "It still plays; its next build starts from scratch." : "Nothing to clear.", cache > 0,
                delegate { Confirm("Delete the build cache of " + row[1] + "?", "clear-cache " + row[0], "Clear build cache: " + row[1]); }));
            p.Items.Add(new Item(row[1] + ": whole build (" + Gb(cache + bin) + ")", cache + bin > 0 ? "Build cache and bin\\: it builds again at its next launch." : "Nothing to clear.", cache + bin > 0,
                delegate { Confirm("Delete the whole build of " + row[1] + "?", "clear-build " + row[0], "Clear build: " + row[1]); }));
        }
        return p;
    }
    void Confirm(string question, string command, string title) {
        var p = new Page { Title = question };
        p.Items.Add(new Item("No, keep it", "", true, delegate { pages.Pop(); }));
        p.Items.Add(new Item("Yes, delete", "This can't be undone; the files are build output only.", true, delegate { pages.Pop(); pages.Pop(); log.Clear(); StartJob(command, title); }));
        pages.Push(p);
    }
    Page LabelPage(string[] e) {
        var p = new Page { Title = "Label for the session folder" };
        foreach (var l in Labels) { string lbl = l; p.Items.Add(new Item(l, l == "session" ? "No label." : "", true, delegate { pages.Pop(); Begin(e[0], lbl == "session" ? "" : lbl, e[1]); })); }
        return p;
    }
    Page VersionPage() {
        var p = new Page { Title = "Choose version" };
        foreach (var v in Rows("versions", 3)) {
            var row = v;
            p.Items.Add(new Item((row.Length > 3 && row[3] == "*" ? "> " : "") + row[1], row[2], true, delegate {
                pages.Pop(); switching = true; Invalidate();
                Task.Run(() => Run("version-set " + row[0])).ContinueWith(t => BeginInvoke((Action)RebuildMain)); }));
            if (row.Length > 3 && row[3] == "*") p.Sel = p.Items.Count - 1;
        }
        return p;
    }
    Page SetupPage() {
        var p = new Page { Title = "First-time setup" };
        var sb = new StringBuilder("This PC needs a few tools before it can build and play versions:\n\n");
        foreach (var r in Rows("setup-check", 5))
            sb.Append((r[2] == "1" ? "  ok       " : "  MISSING  ") + r[1] + (r[2] == "1" ? "" : (r[4] == "1" ? "" : " (optional)")) + "\n");
        p.Body = sb.ToString();
        p.Items.Add(new Item("Install what is missing", "Windows asks once for permission (UAC). The C++ build tools take 10 minutes or more.", true, delegate { pages.Pop(); Begin("tool-setup", "", "First-time setup"); }));
        p.Items.Add(new Item("Not now", "The Play entries stay greyed out until the tools are there.", true, delegate { pages.Pop(); }));
        return p;
    }

    // First-time setup (USER-ASKS 461): where the game versions and builds go, the space they get, the Skate 3 disc, the
    // recomp and RPCS3. launcher.ps1 "config-get" reads the values, "config-set" writes the changed ones to config.json.
    Dictionary<string, string> wiz, wizStart;
    static readonly int[] CapChoices = { 0, 50, 100, 150, 200, 300, 500 };
    string W(string k) { string v; return wiz != null && wiz.TryGetValue(k, out v) ? v : ""; }
    static string Or(string a, string b) { return a == "" ? b : a; }
    static string Now(string v, string unset) { return "Now: " + Or(v, unset) + "\n\n"; }
    static string FreeOn(string path) {
        try {
            string root = Path.GetPathRoot(Path.GetFullPath(path));
            return "Free on " + root + ": " + (new DriveInfo(root).AvailableFreeSpace >> 30) + " GB.";
        } catch { return ""; }
    }
    Page WizardPage() {
        if (wiz == null) {
            wiz = new Dictionary<string, string>();
            foreach (var r in Rows("config-get", 2)) wiz[r[0]] = r[1];
            if (W("storage_root") == "") wiz["storage_root"] = Or(W("target_root"), W("default_root"));
            wizStart = new Dictionary<string, string>(wiz);
        }
        var p = new Page { Title = "Set up the launcher" };
        string root = W("storage_root");
        int cap; int.TryParse(W("build_cap_gb"), out cap);
        p.Items.Add(new Item("Game folder", Now(root, "not set") + (W("standalone") == "1" ? "Where the launcher keeps the game versions and their builds" : "Where the launcher keeps the builds") + " (one version with its build takes about 15 to 45 GB). " + FreeOn(root), true, delegate { PickFolder("storage_root", "Choose the folder for the game versions and their builds"); }));
        p.Items.Add(new Item("Space for builds", Now(cap > 0 ? cap + " GB" : "no limit", "") + "Press to change. Before a build would go over it, the launcher deletes the build cache of the version built longest ago. " + FreeOn(root), true, delegate {
            int i = Array.IndexOf(CapChoices, cap);
            wiz["build_cap_gb"] = CapChoices[(i + 1) % CapChoices.Length].ToString(); ReshowWizard();
        }));
        p.Items.Add(new Item("Skate 3 disc", Now(W("disc"), "not set") + "Your Xbox 360 Skate 3 ISO or default.xex. The first play converts the game files from it once.", true, delegate { PickFile("disc", "Choose your Skate 3 ISO or default.xex", "Skate 3 (*.iso, default.xex)|*.iso;default.xex|All files (*.*)|*.*"); }));
        p.Items.Add(new Item("Skate 3 Recompiled", Now(W("recomp.working_dir"), "not set (optional)") + "The skate3recomp game folder (with skate3.exe). Adds the Recomp entries.", true, delegate { PickRecomp(); }));
        p.Items.Add(new Item("RPCS3", Now(W("rpcs3.exe"), "not set (optional)") + "rpcs3.exe, to play retail Skate 3 (PS3) for comparison.", true, delegate { PickFile("rpcs3.exe", "Choose rpcs3.exe", "RPCS3 (rpcs3.exe)|rpcs3.exe|Programs (*.exe)|*.exe"); }));
        p.Items.Add(new Item("RPCS3 Skate 3 game", Now(W("rpcs3.game"), "not set (optional)") + "The folder of your Skate 3 PS3 game (the one with PS3_GAME or EBOOT.BIN).", true, delegate { PickFolder("rpcs3.game", "Choose your Skate 3 PS3 game folder"); }));
        p.Items.Add(new Item("Done", "Saves these settings. Change them later in Tools > Set up the launcher.", true, delegate { SaveWizard(); }));
        p.Items.Add(new Item("Not now", "Saves nothing; this page opens again at the next start.", true, delegate { wiz = null; pages.Pop(); Invalidate(); }));
        p.OnBack = delegate { wiz = null; pages.Pop(); Invalidate(); };
        return p;
    }
    void ReshowWizard() {
        int sel = pages.Peek().Sel; pages.Pop();
        var p = WizardPage(); p.Sel = Math.Min(sel, p.Items.Count - 1); pages.Push(p); Invalidate();
    }
    void PickFolder(string key, string title) {
        using (var d = new FolderBrowserDialog { Description = title, ShowNewFolderButton = true }) {
            if (Directory.Exists(W(key))) d.SelectedPath = W(key);
            if (d.ShowDialog(this) == DialogResult.OK) { wiz[key] = d.SelectedPath; ReshowWizard(); }
        }
    }
    string AskFile(string current, string title, string filter) {
        using (var d = new OpenFileDialog { Title = title, Filter = filter, CheckFileExists = true }) {
            try { if (current != "" && Directory.Exists(Path.GetDirectoryName(current))) d.InitialDirectory = Path.GetDirectoryName(current); } catch { }
            return d.ShowDialog(this) == DialogResult.OK ? d.FileName : null;
        }
    }
    void PickFile(string key, string title, string filter) {
        string f = AskFile(W(key), title, filter);
        if (f != null) { wiz[key] = f; ReshowWizard(); }
    }
    // The recomp's game folder; skate3.exe is taken from it (or a folder under it), else asked for.
    void PickRecomp() {
        string dir;
        using (var d = new FolderBrowserDialog { Description = "Choose your Skate 3 Recompiled game folder" }) {
            if (Directory.Exists(W("recomp.working_dir"))) d.SelectedPath = W("recomp.working_dir");
            if (d.ShowDialog(this) != DialogResult.OK) return;
            dir = d.SelectedPath;
        }
        string exe = Path.Combine(dir, "skate3.exe");
        if (!File.Exists(exe)) {
            exe = null;
            try { var found = Directory.GetFiles(dir, "skate3.exe", SearchOption.AllDirectories); if (found.Length > 0) exe = found[0]; } catch { }
            if (exe == null) exe = AskFile(dir + "\\x", "Choose the recomp's skate3.exe", "skate3.exe|skate3.exe|Programs (*.exe)|*.exe");
            if (exe == null) return;
        }
        wiz["recomp.working_dir"] = dir; wiz["recomp.exe"] = exe; ReshowWizard();
    }
    static string Js(string v) { return "\"" + v.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\""; }
    void SaveWizard() {
        var parts = new List<string>();
        foreach (var k in new[] { "disc", "recomp.exe", "recomp.working_dir", "rpcs3.exe", "rpcs3.game" })
            if (W(k) != wizStart[k]) parts.Add(Js(k) + ": " + Js(W(k)));
        if (W("build_cap_gb") != wizStart["build_cap_gb"]) parts.Add(Js("build_cap_gb") + ": " + W("build_cap_gb"));
        string root = W("storage_root");
        if (root != wizStart["storage_root"] && root != "") {
            parts.Add(Js("storage_root") + ": " + Js(root));
            parts.Add(Js("target_root") + ": " + Js(Path.Combine(root, "builds")));
            parts.Add(Js("build_cap_dir") + ": " + Js(root));
            if (W("standalone") == "1") parts.Add(Js("versions_root") + ": " + Js(Path.Combine(root, "versions")));
        }
        parts.Add(Js("first_setup_done") + ": true");
        string file = Path.Combine(StateDir(), "config-set.json");
        File.WriteAllText(file, "{" + string.Join(", ", parts.ToArray()) + "}", new UTF8Encoding(false));
        string r = Run("config-set \"" + file + "\"");
        wiz = null; pages.Pop();
        if (r.Trim() != "saved") ShowText("Set up the launcher", "The settings could not be saved:\n\n" + r, null);
        RebuildMain();
        Invalidate();
    }
    void RebuildMain() {
        // A version change or a finished job can change every list (entries, versions, notes): fetch them again.
        // The menu is rebuilt when they are in (off the window thread; the newest request wins).
        ForgetCache();
        Prefetch();
        int gen = ++rebuildGen;
        WhenListed(delegate {
            if (gen != rebuildGen) return;
            switching = false;
            RefreshStatus(true);
            var all = new List<Page>(pages); all.Reverse();
            pages.Clear();
            foreach (var pg in all) {
                if (pg.Title == "Skate launcher" && pg.OnBack != null) { var m = MainPage(); m.Sel = Math.Min(pg.Sel, m.Items.Count - 1); pages.Push(m); }
                else pages.Push(pg);
            }
            Invalidate();
        });
    }
    int rebuildGen;
    // A version switch until the menu is rebuilt for it: Play waits for it (no start of the old version).
    bool switching;
    void ShowText(string title, string body, Action back) { ShowText(title, body, back, null); }
    void ShowText(string title, string body, Action back, string mono) {
        var p = new Page { Title = title, Body = body, Mono = mono };
        p.OnBack = back;
        pages.Push(p);
        Invalidate();
    }

    // ---------- jobs ----------
    void Begin(string entry, string label, string name) {
        if (switching) return;
        string g = RunningGames();
        if (g != "" && !entry.StartsWith("tool-")) {
            waitEntry = entry; waitLabel = label; waitName = name;
            ShowText("Waiting", "Already running: " + g + "\nOnly one game at a time. Close it, or wait here: this continues by itself.\n\nB: cancel", delegate { waitEntry = null; pages.Pop(); });
            return;
        }
        log.Clear();
        StartJob(entry + (label == "" ? "" : " " + label), name);
    }
    void StartJob(string args, string title) {
        jobTitle = title; jobHidGame = false; cancelArmed = cancelled = cancelledInFetch = false; gameWindow = IntPtr.Zero; jobStart = DateTime.Now;
        build.Close(); build.LogPath = null; build.Ended = false; build.Active = false; build.PreWait = null; build.LastPreWait = null;
        // Let the game take the foreground from us (Steam gives the foreground window its controller layout).
        Native.AllowSetForegroundWindow(-1);
        job = new Process { StartInfo = Ps(args + " -NoPrompt"), EnableRaisingEvents = true };
        // Build output feeds the build view; other lines the job log. Drawn from the timer, not per line.
        DataReceivedEventHandler add = delegate(object s, DataReceivedEventArgs e) { if (e.Data != null) BeginInvoke((Action)delegate {
            if (!build.Feed(e.Data)) { string c = BuildView.Clean(e.Data); if (c != "") { log.Add(c); if (log.Count > 400) log.RemoveAt(0); } }
            dirty = true; }); };
        job.OutputDataReceived += add; job.ErrorDataReceived += add;
        job.Exited += delegate { BeginInvoke((Action)JobDone); };
        job.Start(); job.StandardInput.Close();
        job.BeginOutputReadLine(); job.BeginErrorReadLine();
        Invalidate();
    }
    void JobDone() {
        job = null;
        if (!Visible) Show();
        if (WindowState == FormWindowState.Minimized) WindowState = FormWindowState.Normal;
        Activate(); Native.Front(Handle);
        string text = LastSession();
        build.Close();
        if (direct.Length == 0 && RestartIfUpdated(true)) return;
        string mono = null;
        if (cancelled && cancelledInFetch) text = "Cancelled after " + BuildView.Clock(DateTime.Now - build.PreWaitSince) + " while " + build.PreWait + ". Nothing was built or changed.";
        else if (cancelled) text = "Build cancelled after " + BuildView.Clock(DateTime.Now - build.Start) + " (it was waiting for another build)." + (build.LogPath != null ? "\nBuild log: " + build.LogPath : "");
        else if (build.LogPath != null && build.Ended) {
            string took = BuildView.Clock(build.End - build.Start);
            if (build.Ok) text += "\n\nBuilt " + build.Name + " at " + build.Commit + " in " + took + (build.Warnings > 0 ? ", " + build.Warnings + " warnings" : "") + ".\nBuild log: " + build.LogPath;
            else { text += "\n\nBuild failed after " + took + ". Build log: " + build.LogPath + "\n\nFirst error:"; mono = string.Join("\n", build.FirstError()); }
        }
        bool bad = text.Contains("MALFORMED") || text.Contains("No new") || text.Contains("Not started") || text.Contains("panic(s)") || text.Contains("failed") || text.Contains("Still missing");
        bad = bad || mono != null;
        if (direct.Length > 0) { ShowText(bad ? "Finished, check this" : "Finished", text, null, mono); CloseIn(10); return; }
        RebuildMain();
        ShowText(bad ? "Finished, check this" : "Finished", text, null, mono);
    }
    // While a job runs: hide once the game's window shows, so it gets the foreground and the controller layout. The
    // first window is the startup splash; the game's own window comes when the splash is done, and gets the front too.
    void WatchGame() {
        if (job == null || (jobHidGame && (DateTime.Now - hidAt).TotalSeconds > 300)) return;
        foreach (var n in GameNames) foreach (var p in Process.GetProcessesByName(n)) {
            IntPtr w;
            using (p) w = p.MainWindowHandle;
            if (w == IntPtr.Zero || w == gameWindow || !Native.IsWindowVisible(w)) continue;
            gameWindow = w; Native.Front(w);
            if (!jobHidGame) { jobHidGame = true; hidAt = DateTime.Now; Hide(); }
            return;
        }
    }

    // ---------- input ----------
    static ushort Pad() {
        for (int i = 0; i < 4; i++) {
            Native.XState s;
            try {
                if (Native.XInputGetState(i, out s) == 0) {
                    ushort b = s.Buttons;
                    if (s.LY > 16000) b |= UP; if (s.LY < -16000) b |= DOWN;
                    if (s.LX < -16000) b |= LEFT; if (s.LX > 16000) b |= RIGHT;
                    return b;
                }
            } catch { return 0; }
        }
        return 0;
    }
    DateTime scannedAt;
    void Poll() {
        if (powerOffAt >= 0 && !poweredOff) {
            if ((float)clock.Elapsed.TotalSeconds - powerOffAt >= OffTime) { poweredOff = true; Close(); return; }
            Invalidate(); return;
        }
        if (closeAt >= 0) {
            int left = (int)Math.Ceiling((closeTime - DateTime.Now).TotalSeconds);
            if (left <= 0) { Close(); return; }
            if (left != closeAt) { closeAt = left; Invalidate(); }
        }
        // Process scans 4 times a second, not every 16 ms frame: each one lists every process on the PC (USER-ASKS 436).
        if ((DateTime.Now - scannedAt).TotalMilliseconds >= 250) {
            scannedAt = DateTime.Now;
            if (waitEntry != null && RunningGames() == "") { string e = waitEntry; waitEntry = null; pages.Pop(); Begin(e, waitLabel, waitName); }
            WatchGame();
        }
        if (dirty || Animating()) { dirty = false; Invalidate(); }
        // The rolling boards and the Play glow: about 30 frames a second while the menu or a job shows.
        if (Visible && ++frame % 2 == 0) Invalidate();
        // Only while the menu is in front: XInput reports the pad to every process, so the game's presses would
        // otherwise drive the menu behind it.
        if (job != null && Form.ActiveForm == this && ((build.Waiting != null && !build.Ended) || build.FetchWaiting())) {
            ushort jb = Pad(); if ((jb & ~prevPad & B) != 0) CancelPress(); prevPad = jb; return;
        }
        if (starting && Form.ActiveForm == this) { ushort sb = Pad(); if ((sb & ~prevPad & B) != 0) SkipPress(); prevPad = sb; return; }
        if (job != null || Form.ActiveForm != this) { prevPad = Pad(); return; }
        ushort b = Pad(), pressed = (ushort)(b & ~prevPad);
        if ((pressed & A) != 0) Act("ok", true);
        else if ((pressed & B) != 0) Act("back", true);
        else if ((pressed & UP) != 0) Act("up", true);
        else if ((pressed & DOWN) != 0) Act("down", true);
        else if ((pressed & LEFT) != 0) Act("left", true);
        else if ((pressed & RIGHT) != 0) Act("right", true);
        else if ((b & (UP | DOWN)) != 0 && b == prevPad) { if (++held > 14 && held % 4 == 0) Act((b & UP) != 0 ? "up" : "down", true); }
        if (b != prevPad) held = 0;
        prevPad = b;
    }
    // Cancelling a build that waits for another one: the first B arms, the second kills the job's process tree
    // (powershell launcher.ps1 -> Build.ps1 -> cargo); a cargo that only waits on a lock loses nothing.
    void CancelPress() {
        if (job == null) return;
        if (!cancelArmed) { cancelArmed = true; dirty = true; return; }
        cancelled = true; cancelledInFetch = build.PreWait != null; build.Note("cancelled after " + BuildView.Clock(DateTime.Now - build.Start));
        try { Process.Start(new ProcessStartInfo("taskkill.exe", "/T /F /PID " + job.Id) { CreateNoWindow = true, UseShellExecute = false }); } catch { }
    }
    protected override bool ProcessCmdKey(ref Message msg, Keys key) {
        if (job != null && ((build.Waiting != null && !build.Ended) || build.FetchWaiting()) && (key == Keys.Escape || key == Keys.Back)) { CancelPress(); return true; }
        if (starting && (key == Keys.Escape || key == Keys.Back)) { SkipPress(); return true; }
        if (job == null) {
            switch (key) {
                case Keys.Up: case Keys.W: Act("up", false); return true;
                case Keys.Down: case Keys.S: Act("down", false); return true;
                case Keys.Left: case Keys.A: Act("left", false); return true;
                case Keys.Right: case Keys.D: Act("right", false); return true;
                case Keys.Enter: Act("ok", false); return true;
                case Keys.Escape: case Keys.Back: case Keys.Space: Act("back", false); return true;
            }
        }
        return base.ProcessCmdKey(ref msg, key);
    }
    // Mouse: hovering selects a row or tile, a left click runs it, a right click goes back. The rectangles come
    // from the last paint (hits), so they always match what is on screen.
    readonly List<KeyValuePair<RectangleF, int>> hits = new List<KeyValuePair<RectangleF, int>>();
    int HitAt(Point pt) {
        foreach (var h in hits) if (h.Key.Contains(pt)) return h.Value;
        return -1;
    }
    // The A / B prompts drawn last paint (Prompt), clickable like the buttons they show.
    readonly List<KeyValuePair<RectangleF, string>> promptHits = new List<KeyValuePair<RectangleF, string>>();
    string hoverPrompt;
    string PromptAt(Point pt) {
        foreach (var h in promptHits) if (h.Key.Contains(pt)) return h.Value;
        return null;
    }
    // A clicked prompt does what its button does in the current state (ProcessCmdKey's Back / Enter).
    void PromptPress(string key) {
        if (key == "B") {
            if (job != null && ((build.Waiting != null && !build.Ended) || build.FetchWaiting())) CancelPress();
            else if (starting) SkipPress();
            else if (job == null) Act("back", false);
        }
        else if (key == "A" && job == null) Act("ok", false);
        // The camcorder's keys: MENU = back, the dial and the zoom rocker move, START/STOP plays, POWER closes.
        else if (key == "MENU") PromptPress("B");
        else if (job == null && (key == "U" || key == "D" || key == "L" || key == "R")) Act(key == "U" ? "up" : key == "D" ? "down" : key == "L" ? "left" : "right", false);
        else if (key == "PLAY" && job == null && !starting && pages.Count > 0) { var p = pages.Peek(); if (IsMain(p) && p.Items.Count > 0) p.Sel = 0; Act("ok", false); }
        else if (key == "POWER") Close();
    }
    protected override void OnMouseMove(MouseEventArgs e) {
        base.OnMouseMove(e);
        string over = PromptAt(e.Location);
        if (over != hoverPrompt) { hoverPrompt = over; Cursor = over != null ? Cursors.Hand : Cursors.Default; Invalidate(); }
        int i = HitAt(e.Location);
        if (job == null && i >= 0 && pages.Count > 0 && pages.Peek().Sel != i) { pages.Peek().Sel = i; Invalidate(); }
    }
    protected override void OnMouseClick(MouseEventArgs e) {
        base.OnMouseClick(e);
        string prompt = e.Button == MouseButtons.Left ? PromptAt(e.Location) : null;
        if (prompt != null) { PromptPress(prompt); return; }
        if (job != null) return;
        if (e.Button == MouseButtons.Right) { Act("back", false); return; }
        if (e.Button != MouseButtons.Left) return;
        int i = HitAt(e.Location);
        if (i >= 0 && pages.Count > 0) { pages.Peek().Sel = i; Act("ok", false); }
        else if (closeAt >= 0) Close();
    }
    // One action; the same action from the OTHER source (pad / keys) within 150 ms is dropped: Steam can send a key
    // for a press that XInput also reports.
    void Act(string a, bool fromPad) {
        if (a == lastAction && fromPad != lastFromPad && (DateTime.Now - lastActionTime).TotalMilliseconds < 150) return;
        lastAction = a; lastFromPad = fromPad; lastActionTime = DateTime.Now;
        if (closeAt >= 0) { Close(); return; }
        if (pages.Count == 0) return;
        var p = pages.Peek();
        if (a == "back") {
            if (p.OnBack != null) p.OnBack();
            else pages.Pop();
            if (pages.Count == 0) pages.Push(MainPage());
        } else if (p.Items.Count > 0) {
            // "ok" first: the main page's grid move used to swallow it, so A / Enter did nothing there (laptop 2026-10-18).
            if (a == "ok") { var it = p.Items[p.Sel]; if (it.On && it.Run != null) it.Run(); }
            // Every screen is one of the camcorder's menu lists (W / T on the zoom rocker step like the jog dial).
            else if (a == "up" || a == "left") p.Sel = (p.Sel - 1 + p.Items.Count) % p.Items.Count;
            else if (a == "down" || a == "right") p.Sel = (p.Sel + 1) % p.Items.Count;
        } else if (a == "ok" && p.Body != null) {
            if (p.OnBack != null) p.OnBack(); else pages.Pop();
        }
        Invalidate();
    }

    // ---------- drawing helpers ----------
    readonly Stopwatch clock = Stopwatch.StartNew();
    DateTime jobStart;
    static float Ease(float x) { x = Math.Max(0, Math.Min(1, x)); float r = 1 - x; return 1 - r * r * r; }
    static bool IsMain(Page p) { return p.Title == "Skate launcher" && p.OnBack != null; }
    // Every list page (no body text, not the main screen) is a grid of buttons two wide with the detail panel beside it
    // (user 2026-10-21: the play screen's layout for the rest of the launcher).
    static int GridCols(Page p) { return p.Cols > 0 ? p.Cols : (p.Body == null && !IsMain(p) ? 2 : 0); }

    static System.Drawing.Drawing2D.GraphicsPath Round(RectangleF r, float rad) {
        var path = new System.Drawing.Drawing2D.GraphicsPath();
        float d = Math.Min(rad * 2, Math.Min(r.Width, r.Height));
        path.AddArc(r.X, r.Y, d, d, 180, 90); path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90); path.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }
    static void Card(Graphics g, RectangleF r, float rad, Color top, Color bottom, Color edge, float edgeW) {
        using (var path = Round(r, rad)) {
            using (var b = new System.Drawing.Drawing2D.LinearGradientBrush(r, top, bottom, 90f)) g.FillPath(b, path);
            if (edgeW > 0) using (var pen = new Pen(edge, edgeW)) g.DrawPath(pen, path);
        }
    }
    static Color Fade(Color c, int alpha) { return Color.FromArgb(alpha, c.R, c.G, c.B); }
    static void Write(Graphics g, string s, Font f, Color c, RectangleF r, StringAlignment h, StringAlignment v, bool wrap) {
        using (var b = new SolidBrush(c)) using (var sf = new StringFormat()) {
            sf.Alignment = h; sf.LineAlignment = v; sf.Trimming = StringTrimming.EllipsisCharacter;
            // Wrapped text shows whole lines only (a half-clipped last line reads as noise).
            sf.FormatFlags = wrap ? StringFormatFlags.LineLimit : StringFormatFlags.NoWrap;
            g.DrawString(s ?? "", f, b, r, sf);
        }
    }

    // The splash's board (splash.rs draw_board), side view, 180 units long, its top-left at (x, y), `len` px long.
    // The board's x while it rolls along a track on a loop (in at the left, out at the right, again), independent of
    // the progress (user 2026-10-18: it should not follow the loading bar).
    static float Rolling(RectangleF track, float boardLen, float t) {
        float span = track.Width + boardLen;
        return track.X - boardLen + (t * track.Width * 0.3f) % span;
    }
    static void Board(Graphics g, float x, float y, float len) {
        var st = g.Save();
        g.TranslateTransform(x, y); g.ScaleTransform(len / 180f, len / 180f);
        const float l = 180f;
        Func<float, PointF[]> deck = delegate(float down) {
            float[,] top = { { 1, 3 }, { 5, 3.6f }, { 11, 5.6f }, { 18, 8.6f }, { 25, 11 }, { 31, 12 }, { l - 33, 12 }, { l - 26, 10.8f }, { l - 18, 7.8f }, { l - 10, 4.4f }, { l - 4, 2.4f }, { l - 1, 2 } };
            var pts = new PointF[12]; for (int i = 0; i < 12; i++) pts[i] = new PointF(top[i, 0], top[i, 1] + down); return pts;
        };
        Func<float, float, PointF[]> outline = delegate(float upper, float lower) {
            var a = deck(upper); var b = deck(lower); Array.Reverse(b);
            var all = new PointF[a.Length + b.Length]; a.CopyTo(all, 0); b.CopyTo(all, a.Length); return all;
        };
        Action<Color, PointF[]> poly = delegate(Color c, PointF[] pts) { using (var br = new SolidBrush(c)) g.FillPolygon(br, pts); };
        Action<Color, float, float, float> disc = delegate(Color c, float cx, float cy, float r) { using (var br = new SolidBrush(c)) g.FillEllipse(br, cx - r, cy - r, 2 * r, 2 * r); };
        using (var sh = new SolidBrush(Color.FromArgb(110, 0, 0, 0))) g.FillEllipse(sh, 10, 32.5f, l - 20, 5);
        var wood = outline(0, 5);
        poly(Color.FromArgb(214, 176, 124), wood);
        using (var pen = new Pen(Color.FromArgb(92, 62, 36), 0.5f)) g.DrawPolygon(pen, wood);
        poly(Color.FromArgb(150, 104, 62), outline(2.6f, 3.4f));
        poly(Color.FromArgb(34, 34, 38), outline(0, 1.4f));
        foreach (float ax in new float[] { 40, l - 42 }) {
            poly(Color.FromArgb(176, 181, 191), new[] { new PointF(ax - 10, 17), new PointF(ax + 10, 17), new PointF(ax + 9, 19.5f), new PointF(ax - 9, 19.5f) });
            poly(Color.FromArgb(176, 181, 191), new[] { new PointF(ax - 6, 19.5f), new PointF(ax + 6, 19.5f), new PointF(ax + 3.5f, 26), new PointF(ax - 3.5f, 26) });
            disc(Color.FromArgb(168, 162, 148), ax, 26.5f, 7.5f); disc(Color.FromArgb(240, 235, 220), ax, 26.5f, 6.9f);
            disc(Color.FromArgb(86, 146, 214), ax, 26.5f, 3.9f); disc(Color.FromArgb(52, 54, 60), ax, 26.5f, 1.7f);
        }
        g.Restore(st);
    }

    // Log lines as a TV can read them: no colour codes, only the text after the last carriage return.
    static readonly System.Text.RegularExpressions.Regex Ansi = new System.Text.RegularExpressions.Regex("\x1B\\[[0-9;?]*[A-Za-z]");
    static string Clean(string s) { s = Ansi.Replace(s ?? "", ""); int r = s.LastIndexOf('\r'); return (r >= 0 ? s.Substring(r + 1) : s).Trim(); }

    // ---------- the camcorder (USER-ASKS 484, 486, 487) ----------
    // The window is a 2000s MiniDV camcorder seen from its LCD side, standing on a ledge the board rolls along, drawn in
    // a 1600 x 1000 unit space into a per-pixel-alpha (layered) window. Layout, keys, timings and the sticker:
    // docs/hails-additions/41-launcher-camcorder-viewfinder.md.
    const float DevW = 1600, DevH = 1000;
    static readonly RectangleF BodyR = new RectangleF(150, 118, 1390, 754), PanelR = new RectangleF(232, 146, 1016, 660),
        ScreenR = new RectangleF(292, 180, 896, 504), LedgeR = new RectangleF(24, 880, 1552, 108);
    static readonly Color Plastic = Color.FromArgb(54, 56, 62), PlasticHi = Color.FromArgb(100, 103, 112), PlasticDark = Color.FromArgb(24, 25, 29),
        Print = Color.FromArgb(200, 202, 208), RecRed = Color.FromArgb(230, 40, 32), OsdOrange = Color.FromArgb(255, 152, 24),
        OsdNavy = Color.FromArgb(205, 14, 26, 74), OsdWhite = Color.FromArgb(242, 242, 234), OsdGrey = Color.FromArgb(132, 136, 146),
        OsdGreen = Color.FromArgb(120, 224, 96), OsdRed = Color.FromArgb(255, 96, 84);
    static readonly Font UTiny = new Font("Tahoma", 13, GraphicsUnit.Pixel), USmall = new Font("Tahoma", 17, FontStyle.Bold, GraphicsUnit.Pixel),
        UKey = new Font("Tahoma", 21, FontStyle.Bold, GraphicsUnit.Pixel), UMark = new Font("Arial Black", 40, FontStyle.Italic, GraphicsUnit.Pixel),
        UModel = new Font("Tahoma", 24, FontStyle.Bold, GraphicsUnit.Pixel), UTag = new Font("Segoe Script", 44, FontStyle.Bold, GraphicsUnit.Pixel), UCover = new Font("Arial Black", 120, FontStyle.Italic, GraphicsUnit.Pixel);
    float devS = 1, devX, devY;   // units -> window pixels, from the last Render
    RectangleF ToWin(RectangleF r) { return new RectangleF(devX + r.X * devS, devY + r.Y * devS, r.Width * devS, r.Height * devS); }
    void Key(RectangleF unit, string key) { promptHits.Add(new KeyValuePair<RectangleF, string>(ToWin(unit), key)); }
    static RectangleF Grow(RectangleF r, float d) { return new RectangleF(r.X - d, r.Y - d, r.Width + 2 * d, r.Height + 2 * d); }
    static float Clamp01(float x) { return Math.Max(0, Math.Min(1, x)); }

    // Power: the whole camera unfolds at start, on one clock (Assembly, seconds 0..OnTotal): the ledge slides up, the
    // body drops onto it, the lens runs out, the handle rises, the keys pop in, the LCD swings out and turns to face
    // you, the screen comes on, the board rolls in. Closing plays it backwards, a little faster (OffSpeed).
    const float OnTotal = 1.75f, OffSpeed = 1.3f, OffTime = OnTotal / OffSpeed + 0.05f;
    float powerOnAt = -1, powerOffAt = -1, snapPanel = -1; bool poweredOff, snapshotMode; int snapSeed = 3;
    float Assembly(float t) {
        if (snapPanel >= 0) return snapPanel * OnTotal;
        if (powerOffAt >= 0) return Math.Max(0, OnTotal - (t - powerOffAt) * OffSpeed);
        return powerOnAt < 0 ? 0 : Math.Min(OnTotal, t - powerOnAt);
    }
    static float Stage(float a, float from, float to) { return Ease((a - from) / (to - from)); }
    static float BackOut(float x, float k) { x = Clamp01(x) - 1; return 1 + (k + 1) * x * x * x + k * x * x; }
    // Every power-on and power-off rolls its own timing, drop, bounce, tilt, overshoot and swing (Vary), so it never
    // plays the same way twice; all of it still lands inside OnTotal. Snapshots use the plain values.
    readonly float[] vary = { 0, 0, 1, 1, 1.4f, 0, 0, 0.8f, 0, 0, 0, 0, 0, 0, 1.6f, 0, 1, 150, 0, 6.5f };
    void Vary(Random r) {
        Func<double, double, float> u = delegate(double lo, double hi) { return (float)(lo + r.NextDouble() * (hi - lo)); };
        vary[0] = u(0, 0.06); vary[1] = u(-0.04, 0.06); vary[2] = u(0.85, 1.15); vary[3] = u(0.75, 1.25); vary[4] = u(0.9, 2.2);
        vary[5] = u(-4, 4); vary[6] = u(-0.05, 0.05); vary[7] = u(0, 1.6); vary[8] = u(-0.05, 0.05);
        for (int i = 9; i <= 13; i++) vary[i] = u(-0.04, 0.04);
        vary[14] = u(1, 2.6); vary[15] = u(-0.05, 0.05); vary[16] = u(0.85, 1.15); vary[17] = u(120, 190); vary[18] = u(0, 1552); vary[19] = u(5.5, 8);
    }
    float PanelFrom { get { return 0.85f + vary[15]; } }
    float PanelTo { get { return PanelFrom + 0.55f * vary[16]; } }
    float PanelOpen(float t) { return Stage(Assembly(t), PanelFrom, PanelTo); }
    float ScreenLevel(float t) { return Clamp01((Assembly(t) - PanelTo) / 0.25f); }
    float WindowAlpha(float t) { return snapshotMode ? 1 : Clamp01(Assembly(t) / 0.12f); }
    bool Animating() { float t = (float)clock.Elapsed.TotalSeconds; return powerOnAt < 0 || t - powerOnAt < OnTotal + 0.1f || powerOffAt >= 0; }

    // The layered window: every frame is drawn into one bitmap and handed to Windows with its alpha (Native.Push).
    protected override CreateParams CreateParams { get { var cp = base.CreateParams; cp.ExStyle |= 0x80000; return cp; } }
    Bitmap frameBmp; bool needPresent = true;
    protected override void OnInvalidated(InvalidateEventArgs e) { base.OnInvalidated(e); needPresent = true; }
    protected override void OnPaint(PaintEventArgs e) { }
    protected override void OnPaintBackground(PaintEventArgs e) { }
    void Present() {
        if (!IsHandleCreated || !Visible) return;
        needPresent = false;
        float t = (float)clock.Elapsed.TotalSeconds;
        if (powerOnAt < 0) { powerOnAt = t; Vary(new Random()); }
        int w = ClientSize.Width, h = ClientSize.Height;
        if (frameBmp == null || frameBmp.Width != w || frameBmp.Height != h) { if (frameBmp != null) frameBmp.Dispose(); frameBmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb); }
        using (var g = Graphics.FromImage(frameBmp)) { g.Clear(Color.Transparent); Render(g, w, h); }
        Native.Push(Handle, frameBmp, Left, Top, (byte)(255 * WindowAlpha(t)));
    }
    protected override void OnFormClosing(FormClosingEventArgs e) {
        if (Visible && !poweredOff && (e.CloseReason == CloseReason.UserClosing || e.CloseReason == CloseReason.None || e.CloseReason == CloseReason.TaskManagerClosing)) {
            e.Cancel = true;
            if (powerOffAt < 0) { powerOffAt = (float)clock.Elapsed.TotalSeconds; Vary(new Random()); if (!tick.Enabled) tick.Start(); Invalidate(); }
            return;
        }
        base.OnFormClosing(e);
    }
    // Drag the camera by its body.
    protected override void OnMouseDown(MouseEventArgs e) {
        base.OnMouseDown(e);
        if (e.Button == MouseButtons.Left && PromptAt(e.Location) == null && HitAt(e.Location) < 0) { Native.ReleaseCapture(); Native.SendMessage(Handle, 0xA1, (IntPtr)2, IntPtr.Zero); }
    }

    void Render(Graphics g, int w, int h) {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
        hits.Clear(); promptHits.Clear();
        float t = (float)clock.Elapsed.TotalSeconds;
        devS = Math.Min(w / DevW, h / DevH); devX = (w - DevW * devS) / 2; devY = (h - DevH * devS) / 2;
        EnsureFlip(t);
        var st = g.Save();
        g.TranslateTransform(devX, devY); g.ScaleTransform(devS, devS);
        float a = Assembly(t);
        var part = g.Save();
        g.TranslateTransform(0, (1 - Stage(a, vary[0], vary[0] + 0.35f)) * 160);
        Ledge(g);
        g.Restore(part);
        float drop = Clamp01((a - 0.12f - vary[1]) / (0.38f * vary[2])), fall = drop >= 1 ? 0 : -(1 - BackOut(drop, vary[4])) * 340 * vary[3];
        if (drop > 0) {
            var cam = g.Save();
            g.TranslateTransform(0, fall);
            // A slight tilt while it drops, settling flat as it lands.
            float tilt = vary[5] * (1 - Ease(drop));
            if (tilt != 0) { g.TranslateTransform(845, 872); g.RotateTransform(tilt); g.TranslateTransform(-845, -872); }
            float lens = Clamp01((a - 0.42f - vary[6]) / 0.3f);
            part = g.Save(); g.TranslateTransform((1 - BackOut(lens, vary[7])) * 170, 0); ShellLens(g); g.Restore(part);
            part = g.Save(); g.TranslateTransform(0, (1 - Stage(a, 0.48f + vary[8], 0.78f + vary[8])) * 120); ShellHandle(g); g.Restore(part);
            ShellBody(g);
            ShellControls(g, a);
            float open = PanelOpen(t);
            if (open >= 1) LcdPanel(g, ScreenLevel(t), t, true); else FlipPanel(g, open, t);
            g.Restore(cam);
        }
        if (a >= 1.3f) RollingBoard(g, t);
        g.Restore(st);
    }

    static void Cylinder(Graphics g, RectangleF r, float rad, bool upright) {
        using (var path = Round(r, rad)) using (var b = new LinearGradientBrush(Grow(r, 1), PlasticDark, PlasticDark, upright ? 0f : 90f)) {
            b.InterpolationColors = new ColorBlend { Colors = new[] { PlasticDark, PlasticHi, Plastic, PlasticDark }, Positions = new[] { 0f, 0.28f, 0.62f, 1f } };
            g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(160, 0, 0, 0), 2)) g.DrawPath(pen, path);
        }
    }
    static void PrintAt(Graphics g, string s, Font f, Color c, float x, float y, float w, StringAlignment a) {
        Write(g, s, f, c, new RectangleF(x, y, w, f.Height * 1.3f), a, StringAlignment.Near, false);
    }
    // A hardware key: lighter while the mouse is over it.
    void HardKey(Graphics g, RectangleF r, string key, string label, float rad) {
        bool over = hoverPrompt == key;
        Card(g, r, rad, over ? Color.FromArgb(132, 136, 146) : Color.FromArgb(92, 95, 104), over ? Color.FromArgb(70, 72, 80) : Color.FromArgb(44, 46, 52), Color.FromArgb(170, 0, 0, 0), 2);
        if (label != null) Write(g, label, UKey, Print, r, StringAlignment.Center, StringAlignment.Center, false);
        Key(r, key);
    }

    // A key popping in (scale with a little overshoot), its own start on the assembly clock.
    bool PopIn(Graphics g, float a, float start, float cx, float cy) {
        float s = BackOut(Clamp01((a - start) / 0.22f), vary[14]);
        if (s <= 0.01f) return false;
        g.TranslateTransform(cx, cy); g.ScaleTransform(s, s); g.TranslateTransform(-cx, -cy);
        return true;
    }
    // The lens: barrel with its focus ring, the hood, a sliver of the front glass (and the sticker when it is on it).
    void ShellLens(Graphics g) {
        Cylinder(g, new RectangleF(52, 262, 150, 356), 18, false);
        using (var pen = new Pen(Color.FromArgb(80, 0, 0, 0), 3)) for (float x = 96; x < 176; x += 11) g.DrawLine(pen, x, 270, x, 610);
        Cylinder(g, new RectangleF(16, 236, 64, 408), 14, false);
        using (var b = new LinearGradientBrush(new RectangleF(8, 260, 12, 362), Color.FromArgb(50, 80, 140), Color.FromArgb(170, 210, 240), 90f)) g.FillRectangle(b, 9, 262, 9, 356);
        PrintAt(g, "12x OPTICAL", UTiny, Print, 84, 590, 118, StringAlignment.Center);
        var spec = StickerSpec();
        if ((int)spec[1] == 1) PlaceSticker(g, spec);
    }
    // The carry handle with its microphone in front and the accessory shoe on top.
    void ShellHandle(Graphics g) {
        Cylinder(g, new RectangleF(300, 48, 150, 58), 26, false);
        using (var b = new SolidBrush(Color.FromArgb(150, 0, 0, 0))) for (int r = 0; r < 3; r++) for (int c = 0; c < 9; c++) g.FillEllipse(b, 318 + c * 13 + (r % 2) * 6, 62 + r * 11, 5, 5);
        Cylinder(g, new RectangleF(430, 40, 92, 96), 20, true);
        Cylinder(g, new RectangleF(1272, 40, 100, 96), 20, true);
        Cylinder(g, new RectangleF(420, 28, 962, 50), 24, false);
        using (var pen = new Pen(Color.FromArgb(70, 0, 0, 0), 2)) for (float x = 560; x < 1250; x += 9) g.DrawLine(pen, x, 38, x, 68);
        Card(g, new RectangleF(820, 18, 140, 12), 3, Color.FromArgb(170, 174, 180), Color.FromArgb(96, 100, 106), Color.Empty, 0);
    }
    // The body: shell, the recess the LCD folds into, the tally lamp, the model print (and the sticker on it).
    void ShellBody(Graphics g) {
        using (var b = new SolidBrush(Color.FromArgb(90, 0, 0, 0))) g.FillEllipse(b, 190, 864, 1330, 26);
        using (var path = Round(BodyR, 64)) {
            using (var b = new LinearGradientBrush(Grow(BodyR, 1), Plastic, Plastic, 90f)) {
                b.InterpolationColors = new ColorBlend { Colors = new[] { PlasticHi, Plastic, Plastic, PlasticDark }, Positions = new[] { 0f, 0.18f, 0.7f, 1f } };
                g.FillPath(b, path);
            }
            using (var pen = new Pen(Color.FromArgb(190, 0, 0, 0), 3)) g.DrawPath(pen, path);
        }
        using (var pen = new Pen(Color.FromArgb(60, 255, 255, 255), 2)) g.DrawLine(pen, 220, 128, 1470, 128);
        // The recess the LCD folds into: jacks and their labels, seen while the panel moves.
        using (var path = Round(Grow(PanelR, -6), 32)) using (var b = new SolidBrush(Color.FromArgb(20, 21, 24))) g.FillPath(b, path);
        string[] jacks = { "DV IN/OUT", "A/V OUT", "S VIDEO", "LANC", "DC IN 8.4V" };
        for (int i = 0; i < jacks.Length; i++) {
            float jx = 330 + i * 170;
            using (var b = new SolidBrush(Color.FromArgb(8, 8, 10))) g.FillEllipse(b, jx + 40, 420, 40, 40);
            using (var pen = new Pen(Color.FromArgb(120, 130, 134, 140), 3)) g.DrawEllipse(pen, jx + 40, 420, 40, 40);
            PrintAt(g, jacks[i], UTiny, Fade(Print, 160), jx, 470, 120, StringAlignment.Center);
        }
        // The tally lamp: lit while recording (a job runs).
        bool rec = job != null;
        if (rec) using (var b = new SolidBrush(Color.FromArgb(90, RecRed))) g.FillEllipse(b, 186, 136, 40, 40);
        using (var b = new SolidBrush(rec ? RecRed : Color.FromArgb(76, 22, 22))) g.FillEllipse(b, 196, 146, 20, 20);
        // Model print under the LCD.
        Write(g, "SKATE", UMark, Print, new RectangleF(250, 812, 200, 56), StringAlignment.Near, StringAlignment.Center, false);
        PrintAt(g, "SK8-1000", UModel, Print, 438, 826, 160, StringAlignment.Near);
        PrintAt(g, "DIGITAL VIDEO CAMERA   120x DIGITAL ZOOM", USmall, Fade(Print, 200), 700, 830, 560, StringAlignment.Far);
        var spec = StickerSpec();
        if ((int)spec[1] == 2) PlaceSticker(g, spec);
    }
    // Controls on the back, each popping in on its own beat: POWER, MENU, the SEL / PUSH EXEC dial, the zoom rocker,
    // START/STOP.
    void ShellControls(Graphics g, float a) {
        var st = g.Save();
        if (PopIn(g, a, 0.6f + vary[9], 1395, 190)) {
        PrintAt(g, "POWER", USmall, Print, 1300, 148, 190, StringAlignment.Near);
        var track = new RectangleF(1300, 174, 190, 42);
        Card(g, track, 21, Color.FromArgb(18, 19, 22), Color.FromArgb(36, 37, 42), Color.FromArgb(170, 0, 0, 0), 2);
        Card(g, new RectangleF(1420, 178, 64, 34), 17, hoverPrompt == "POWER" ? Color.FromArgb(230, 232, 236) : Color.FromArgb(196, 199, 205), Color.FromArgb(110, 114, 120), Color.Empty, 0);
        Key(track, "POWER");
        PrintAt(g, "OFF", UTiny, Print, 1306, 220, 60, StringAlignment.Near);
        PrintAt(g, "ON", UTiny, Print, 1430, 220, 60, StringAlignment.Near);
        using (var b = new SolidBrush(Color.FromArgb(110, 230, 90))) g.FillEllipse(b, 1276, 188, 14, 14);
        }
        g.Restore(st); st = g.Save();
        if (PopIn(g, a, 0.66f + vary[10], 1395, 289)) HardKey(g, new RectangleF(1325, 262, 140, 54), "MENU", "MENU", 27);
        g.Restore(st); st = g.Save();
        if (PopIn(g, a, 0.72f + vary[11], 1395, 420)) {
        PrintAt(g, "SEL/PUSH EXEC", USmall, Print, 1300, 334, 190, StringAlignment.Center);
        float jx0 = 1395, jy0 = 456;
        using (var b = new SolidBrush(Color.FromArgb(28, 29, 33))) g.FillEllipse(b, jx0 - 96, jy0 - 96, 192, 192);
        using (var pen = new Pen(Color.FromArgb(110, 120, 124, 132), 3))
            for (int i = 0; i < 36; i++) { double ang = i * Math.PI / 18; g.DrawLine(pen, jx0 + (float)Math.Cos(ang) * 80, jy0 + (float)Math.Sin(ang) * 80, jx0 + (float)Math.Cos(ang) * 93, jy0 + (float)Math.Sin(ang) * 93); }
        using (var b = new LinearGradientBrush(new RectangleF(jx0 - 72, jy0 - 72, 144, 144), Color.FromArgb(104, 107, 116), Color.FromArgb(40, 42, 48), 90f)) g.FillEllipse(b, jx0 - 72, jy0 - 72, 144, 144);
        Key(new RectangleF(jx0 - 32, jy0 - 32, 64, 64), "A");
        Key(new RectangleF(jx0 - 72, jy0 - 92, 144, 58), "U");
        Key(new RectangleF(jx0 - 72, jy0 + 34, 144, 58), "D");
        using (var b = new SolidBrush(hoverPrompt == "U" ? Color.White : Print)) g.FillPolygon(b, new[] { new PointF(jx0, jy0 - 62), new PointF(jx0 + 16, jy0 - 40), new PointF(jx0 - 16, jy0 - 40) });
        using (var b = new SolidBrush(hoverPrompt == "D" ? Color.White : Print)) g.FillPolygon(b, new[] { new PointF(jx0, jy0 + 62), new PointF(jx0 + 16, jy0 + 40), new PointF(jx0 - 16, jy0 + 40) });
        Card(g, new RectangleF(jx0 - 30, jy0 - 30, 60, 60), 30, hoverPrompt == "A" ? Color.FromArgb(150, 154, 162) : Color.FromArgb(86, 89, 98), Color.FromArgb(46, 48, 54), Color.FromArgb(170, 0, 0, 0), 2);
        Write(g, "EXEC", UTiny, Print, new RectangleF(jx0 - 30, jy0 - 30, 60, 60), StringAlignment.Center, StringAlignment.Center, false);
        }
        g.Restore(st); st = g.Save();
        if (PopIn(g, a, 0.78f + vary[12], 1395, 606)) {
        PrintAt(g, "W        ZOOM        T", USmall, Print, 1300, 566, 190, StringAlignment.Center);
        var rocker = new RectangleF(1300, 592, 190, 54);
        Card(g, rocker, 27, Color.FromArgb(92, 95, 104), Color.FromArgb(44, 46, 52), Color.FromArgb(170, 0, 0, 0), 2);
        var lh = new RectangleF(rocker.X, rocker.Y, rocker.Width / 2, rocker.Height); var rh = new RectangleF(rocker.X + rocker.Width / 2, rocker.Y, rocker.Width / 2, rocker.Height);
        if (hoverPrompt == "L") Card(g, lh, 27, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(30, 255, 255, 255), Color.Empty, 0);
        if (hoverPrompt == "R") Card(g, rh, 27, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(30, 255, 255, 255), Color.Empty, 0);
        using (var pen = new Pen(Color.FromArgb(150, 0, 0, 0), 2)) g.DrawLine(pen, rocker.X + rocker.Width / 2, rocker.Y + 8, rocker.X + rocker.Width / 2, rocker.Bottom - 8);
        Write(g, "W", UKey, Print, lh, StringAlignment.Center, StringAlignment.Center, false);
        Write(g, "T", UKey, Print, rh, StringAlignment.Center, StringAlignment.Center, false);
        Key(lh, "L"); Key(rh, "R");
        }
        g.Restore(st); st = g.Save();
        if (PopIn(g, a, 0.84f + vary[13], 1395, 750)) {
        PrintAt(g, "START/STOP", USmall, Print, 1300, 672, 190, StringAlignment.Center);
        using (var b = new SolidBrush(Color.FromArgb(26, 27, 31))) g.FillEllipse(b, 1331, 700, 128, 128);
        bool overPlay = hoverPrompt == "PLAY";
        using (var b = new LinearGradientBrush(new RectangleF(1345, 714, 100, 100), overPlay ? Color.FromArgb(255, 120, 110) : Color.FromArgb(240, 70, 60), Color.FromArgb(150, 20, 16), 90f)) g.FillEllipse(b, 1345, 714, 100, 100);
        using (var b = new SolidBrush(Color.FromArgb(70, 255, 255, 255))) g.FillEllipse(b, 1366, 726, 40, 22);
        Key(new RectangleF(1331, 700, 128, 128), "PLAY");
        }
        g.Restore(st);
    }

    // The flip-out LCD, open and facing out: a moulded frame with a raised lip, the hinge, the thumb tab, the dark
    // gasket round the glass, the screen, the bezel's speaker, print and keys, and its sticker when it is on the bezel.
    void LcdPanel(Graphics g, float level, float t, bool live) {
        using (var path = Round(PanelR, 36)) {
            using (var b = new LinearGradientBrush(Grow(PanelR, 1), Color.FromArgb(74, 76, 84), Color.FromArgb(34, 35, 40), 90f)) g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(200, 0, 0, 0), 3)) g.DrawPath(pen, path);
        }
        using (var path = Round(Grow(PanelR, -10), 28)) using (var pen = new Pen(Color.FromArgb(30, 255, 255, 255), 2)) g.DrawPath(pen, path);
        Cylinder(g, new RectangleF(212, 300, 32, 350), 12, true);
        using (var pen = new Pen(Color.FromArgb(110, 0, 0, 0), 2)) for (float y = 330; y < 640; y += 60) g.DrawLine(pen, 213, y, 243, y);
        Card(g, new RectangleF(1236, 380, 18, 180), 6, Color.FromArgb(84, 86, 94), Color.FromArgb(40, 41, 46), Color.FromArgb(150, 0, 0, 0), 1);
        using (var pen = new Pen(Color.FromArgb(120, 0, 0, 0), 2)) for (float y = 392; y < 552; y += 10) g.DrawLine(pen, 1239, y, 1251, y);
        PrintAt(g, "OPEN", UTiny, Print, 1190, 356, 60, StringAlignment.Center);
        using (var b = new SolidBrush(Color.FromArgb(14, 14, 16))) g.FillEllipse(b, 1196, 160, 12, 12);
        using (var path = Round(Grow(ScreenR, 13), 10)) using (var b = new SolidBrush(Color.FromArgb(8, 8, 10))) g.FillPath(b, path);
        if (level > 0) {
            var lcd = LcdCanvas(t);
            var st = g.Save();
            g.InterpolationMode = InterpolationMode.NearestNeighbor; g.PixelOffsetMode = PixelOffsetMode.Half;
            g.DrawImage(lcd, ScreenR, new RectangleF(0, 0, lcd.Width, lcd.Height), GraphicsUnit.Pixel);
            g.Restore(st);
            if (level < 1) using (var b = new SolidBrush(Color.FromArgb((int)(255 * (1 - level)), 6, 7, 9))) g.FillRectangle(b, ScreenR);
        } else using (var b = new SolidBrush(Color.FromArgb(16, 17, 20))) g.FillRectangle(b, ScreenR);
        using (var b = new SolidBrush(Color.FromArgb(14, 255, 255, 255))) g.FillPolygon(b, new[] { new PointF(ScreenR.X, ScreenR.Y), new PointF(ScreenR.X + ScreenR.Width * 0.42f, ScreenR.Y), new PointF(ScreenR.X, ScreenR.Y + ScreenR.Height * 0.55f) });
        // Bezel bottom: speaker, print, DISPLAY and the tape keys (rewind, play, fast forward, stop).
        using (var b = new SolidBrush(Color.FromArgb(150, 0, 0, 0))) for (int r = 0; r < 4; r++) for (int c = 0; c < 7; c++) g.FillEllipse(b, 304 + c * 12 + (r % 2) * 6, 716 + r * 12, 6, 6);
        PrintAt(g, "16:9 WIDE LCD", UTiny, Print, 300, 770, 160, StringAlignment.Near);
        HardKey(g, new RectangleF(800, 716, 84, 32), "DISPLAY", null, 8);
        PrintAt(g, "DISPLAY", UTiny, Print, 800, 752, 84, StringAlignment.Center);
        string[] keys = { "L", "A", "R", "MENU" }, names = { "REW", "PLAY", "FF", "STOP" };
        for (int i = 0; i < 4; i++) {
            var r = new RectangleF(912 + i * 66, 716, 56, 32);
            HardKey(g, r, keys[i], null, 8);
            TapeGlyph(g, i, r);
            PrintAt(g, names[i], UTiny, Print, r.X - 6, 752, r.Width + 12, StringAlignment.Center);
        }
        if (!live) promptHits.Clear();
        var spec = StickerSpec();
        if ((int)spec[1] == 0) PlaceSticker(g, spec);
    }
    static void TapeGlyph(Graphics g, int kind, RectangleF r) {
        float cx = r.X + r.Width / 2, cy = r.Y + r.Height / 2, s = 8;
        using (var b = new SolidBrush(Print)) {
            if (kind == 0) { g.FillPolygon(b, new[] { new PointF(cx - 1, cy), new PointF(cx + 9, cy - s), new PointF(cx + 9, cy + s) }); g.FillPolygon(b, new[] { new PointF(cx - 11, cy), new PointF(cx - 1, cy - s), new PointF(cx - 1, cy + s) }); }
            if (kind == 1) g.FillPolygon(b, new[] { new PointF(cx + 9, cy), new PointF(cx - 6, cy - s), new PointF(cx - 6, cy + s) });
            if (kind == 2) { g.FillPolygon(b, new[] { new PointF(cx + 1, cy), new PointF(cx - 9, cy - s), new PointF(cx - 9, cy + s) }); g.FillPolygon(b, new[] { new PointF(cx + 11, cy), new PointF(cx + 1, cy - s), new PointF(cx + 1, cy + s) }); }
            if (kind == 3) g.FillRectangle(b, cx - 7, cy - 7, 14, 14);
        }
    }
    // The panel's closed back cover.
    static void Cover(Graphics g) {
        using (var path = Round(PanelR, 36)) {
            using (var b = new LinearGradientBrush(Grow(PanelR, 1), Color.FromArgb(68, 70, 78), Color.FromArgb(30, 31, 36), 90f)) g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(200, 0, 0, 0), 3)) g.DrawPath(pen, path);
        }
        Write(g, "SKATE", UCover, Color.FromArgb(40, 255, 255, 255), PanelR, StringAlignment.Center, StringAlignment.Center, false);
        using (var pen = new Pen(Color.FromArgb(70, 0, 0, 0), 3)) for (float y = 640; y < 780; y += 16) g.DrawLine(pen, 1000, y, 1200, y);
    }
    // Mid-flip: the cover (first half) or the screen side (second half) drawn into a bitmap and swung on the hinge
    // at the panel's left edge, in vertical strips with perspective, darker while edge-on.
    Bitmap flipCover, flipScreen; Bitmap[] coverStrips, screenStrips; float flipScale = -1;
    const int Strips = 48;
    // A face cut into its vertical strips once, so each strip draws on its own (GDI+ prepares a whole image per call).
    // Each strip carries one pixel of its neighbours on both sides, drawn over them, so the smoothing at its edges
    // blends with real pixels and the strips meet without seams.
    static Bitmap[] Slice(Bitmap shot, out RectangleF[] inner) {
        var parts = new Bitmap[Strips]; inner = new RectangleF[Strips];
        for (int i = 0; i < Strips; i++) {
            int sx = i * shot.Width / Strips, sw = (i + 1) * shot.Width / Strips - sx;
            int x0 = Math.Max(0, sx - 1), x1 = Math.Min(shot.Width, sx + sw + 1);
            parts[i] = shot.Clone(new Rectangle(x0, 0, x1 - x0, shot.Height), PixelFormat.Format32bppPArgb);
            inner[i] = new RectangleF(sx - x0, 0, sw, shot.Height);
        }
        return parts;
    }
    RectangleF[] coverInner, screenInner;
    Bitmap PanelShot(bool cover, float t) {
        var shot = new Bitmap((int)Math.Ceiling(PanelR.Width * devS) + 2, (int)Math.Ceiling(PanelR.Height * devS) + 2, PixelFormat.Format32bppPArgb);
        var keep = new List<KeyValuePair<RectangleF, string>>(promptHits);
        using (var pg = Graphics.FromImage(shot)) {
            pg.SmoothingMode = SmoothingMode.AntiAlias; pg.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
            pg.ScaleTransform(devS, devS); pg.TranslateTransform(-PanelR.X, -PanelR.Y);
            if (cover) Cover(pg); else LcdPanel(pg, 0, t, false);
        }
        promptHits.Clear(); promptHits.AddRange(keep);
        return shot;
    }
    // Both faces are drawn once per size, on the first (still invisible) frame: only the swing changes per frame.
    void EnsureFlip(float t) {
        if (flipScale == devS) return;
        foreach (var old in new[] { flipCover, flipScreen }) if (old != null) old.Dispose();
        foreach (var set in new[] { coverStrips, screenStrips }) if (set != null) foreach (var s in set) s.Dispose();
        flipCover = PanelShot(true, t); flipScreen = PanelShot(false, t); flipScale = devS;
        coverStrips = Slice(flipCover, out coverInner); screenStrips = Slice(flipScreen, out screenInner);
    }
    void FlipPanel(Graphics g, float open, float t) {
        bool cover = open < 0.5f;
        float f = cover ? 1 - open / 0.5f : (open - 0.5f) / 0.5f;
        if (f <= 0.01f) return;
        var shot = cover ? flipCover : flipScreen;
        var keepInterp = g.InterpolationMode; var keepOffset = g.PixelOffsetMode;
        if (f >= 0.999f) {
            g.InterpolationMode = InterpolationMode.NearestNeighbor; g.PixelOffsetMode = PixelOffsetMode.Half;
            g.DrawImage(shot, PanelR.X, PanelR.Y, shot.Width / devS, shot.Height / devS);
            g.InterpolationMode = keepInterp; g.PixelOffsetMode = keepOffset;
            return;
        }
        g.InterpolationMode = InterpolationMode.NearestNeighbor; g.PixelOffsetMode = PixelOffsetMode.HighSpeed;
        double a = (1 - f) * Math.PI / 2;
        float cos = (float)Math.Cos(a), sin = (float)Math.Sin(a), W = PanelR.Width, cy = PanelR.Y + PanelR.Height / 2, dist = W * 2f;
        var parts = cover ? coverStrips : screenStrips;
        float prevX = PanelR.X, lastS = 1;
        var keepMode = g.CompositingQuality;
        g.CompositingQuality = CompositingQuality.HighSpeed;
        var inner = cover ? coverInner : screenInner;
        for (int i = 0; i < Strips; i++) {
            float u1 = (i + 1f) / Strips, um = (i + 0.5f) / Strips;
            float s1 = dist / (dist - 0.45f * u1 * W * sin), sm = dist / (dist - 0.45f * um * W * sin);
            float x1 = PanelR.X + u1 * W * cos * s1, top = cy - PanelR.Height / 2 * sm;
            float kx = (x1 - prevX) / inner[i].Width;
            g.DrawImage(parts[i], new RectangleF(prevX - inner[i].X * kx, top, parts[i].Width * kx, PanelR.Height * sm));
            prevX = x1; lastS = s1;
        }
        g.CompositingQuality = keepMode; g.InterpolationMode = keepInterp; g.PixelOffsetMode = keepOffset;
        int dark = (int)(170 * (1 - f));
        if (dark > 0) using (var b = new SolidBrush(Color.FromArgb(dark, 0, 0, 0)))
            g.FillPolygon(b, new[] { new PointF(PanelR.X, PanelR.Y), new PointF(prevX, cy - PanelR.Height / 2 * lastS), new PointF(prevX, cy + PanelR.Height / 2 * lastS), new PointF(PanelR.X, PanelR.Bottom) });
    }

    // The ledge the camera stands on: worn concrete, angle-iron coping with grind scuffs and wax, a tag.
    static void Ledge(Graphics g) {
        var face = new RectangleF(LedgeR.X, LedgeR.Y + 20, LedgeR.Width, LedgeR.Height - 20);
        using (var path = Round(face, 6)) using (var b = new LinearGradientBrush(Grow(face, 1), Color.FromArgb(160, 156, 148), Color.FromArgb(100, 97, 92), 90f)) g.FillPath(b, path);
        var rnd = new Random(11);
        using (var d1 = new SolidBrush(Color.FromArgb(60, 0, 0, 0))) using (var d2 = new SolidBrush(Color.FromArgb(50, 255, 255, 255)))
            for (int i = 0; i < 260; i++) g.FillRectangle(i % 2 == 0 ? d1 : d2, face.X + (float)rnd.NextDouble() * face.Width, face.Y + 4 + (float)rnd.NextDouble() * (face.Height - 8), 2.2f, 2.2f);
        Write(g, "skate everything", UTag, Color.FromArgb(150, 236, 72, 150), new RectangleF(1040, face.Y + 4, 520, 80), StringAlignment.Near, StringAlignment.Near, false);
        using (var pen = new Pen(Color.FromArgb(110, 236, 72, 150), 2)) foreach (float dx in new float[] { 1080, 1150, 1290, 1400 }) g.DrawLine(pen, dx, face.Y + 58, dx, face.Y + 58 + (dx % 17) + 10);
        var cop = new RectangleF(LedgeR.X - 4, LedgeR.Y, LedgeR.Width + 8, 22);
        using (var b = new LinearGradientBrush(Grow(cop, 1), Color.FromArgb(228, 231, 235), Color.FromArgb(116, 120, 126), 90f)) g.FillRectangle(b, cop);
        using (var pen = new Pen(Color.FromArgb(160, 255, 255, 255), 2)) g.DrawLine(pen, cop.X, cop.Y + 1, cop.Right, cop.Y + 1);
        using (var pen = new Pen(Color.FromArgb(80, 30, 30, 34), 2)) for (int i = 0; i < 26; i++) { float x = cop.X + (float)rnd.NextDouble() * cop.Width; g.DrawLine(pen, x, cop.Y + 4, x + 30 + (float)rnd.NextDouble() * 60, cop.Y + 6 + (float)rnd.NextDouble() * 8); }
        using (var b = new SolidBrush(Color.FromArgb(40, 255, 250, 220))) g.FillEllipse(b, 480, cop.Y + 2, 300, 16);
    }
    // The board rolls along the coping and pops an ollie every few seconds.
    void RollingBoard(Graphics g, float t) {
        const float len = 230;
        float x = LedgeR.X - len + (t * vary[17] + vary[18]) % (LedgeR.Width + len);
        float cycle = t % vary[19], jump = 0, tilt = 0;
        if (cycle < 0.8f) { float q = cycle / 0.8f; jump = (float)Math.Sin(Math.PI * q) * 110; tilt = q < 0.25f ? -18 * q / 0.25f : -18 * (1 - (q - 0.25f) / 0.75f); }
        float y = LedgeR.Y - len * 34f / 180f - jump;
        int sa = (int)(90 * (1 - jump / 200));
        using (var b = new SolidBrush(Color.FromArgb(Math.Max(0, sa), 0, 0, 0))) g.FillEllipse(b, x + len * 0.1f + jump * 0.2f, LedgeR.Y - 4, len * 0.8f - jump * 0.4f, 9);
        var st = g.Save();
        float px = x + len / 2, py = y + len * 17f / 180f;
        g.TranslateTransform(px, py); g.RotateTransform(tilt); g.TranslateTransform(-px, -py);
        Board(g, x, y, len);
        g.Restore(st);
    }

    // ---------- the sticker (USER-ASKS 487) ----------
    // One sticker slapped on the camera years ago, picked once per user at the first start and kept in
    // camcorder-sticker.txt in the state folder: design, zone (0 bezel, 1 lens, 2 body print), place, angle, wear seed.
    // Placeholders: PNGs in the state folder's stickers\ (the game's own stickers, once the asset export finds them:
    // .claude/todo/living-world-stickers.md) replace the parody set below.
    const int StickerCount = 5;
    float[] sticker; Bitmap[] gameStickers;
    float[] StickerSpec() {
        if (sticker != null) return sticker;
        var inv = System.Globalization.CultureInfo.InvariantCulture;
        string f = Path.Combine(StateDir(), "camcorder-sticker.txt");
        if (!snapshotMode) try {
            if (File.Exists(f)) {
                var parts = File.ReadAllText(f).Trim().Split(' ');
                if (parts.Length == 6) return sticker = Array.ConvertAll(parts, x => float.Parse(x, inv));
            }
        } catch { }
        var rnd = snapshotMode ? new Random(snapSeed) : new Random();
        sticker = new float[] { rnd.Next(StickerCount), rnd.Next(3), (float)rnd.NextDouble(), (float)rnd.NextDouble(), (float)(rnd.NextDouble() * 40 - 20), rnd.Next(100000) };
        if (!snapshotMode) try { Directory.CreateDirectory(StateDir()); File.WriteAllText(f, string.Join(" ", Array.ConvertAll(sticker, x => x.ToString(inv)))); } catch { }
        return sticker;
    }
    Bitmap GameSticker(int pick) {
        if (gameStickers == null) {
            var list = new List<Bitmap>();
            try { string dir = Path.Combine(StateDir(), "stickers"); if (Directory.Exists(dir)) foreach (var p in Directory.GetFiles(dir, "*.png")) try { list.Add(new Bitmap(p)); } catch { } } catch { }
            gameStickers = list.ToArray();
        }
        return gameStickers.Length == 0 ? null : gameStickers[pick % gameStickers.Length];
    }
    void PlaceSticker(Graphics g, float[] s) {
        int zone = (int)s[1];
        float cx, cy, scale;
        if (zone == 0) { cx = 480 + s[2] * 250; cy = 750 + s[3] * 14; scale = 0.86f; }
        else if (zone == 1) { cx = 127; cy = 350 + s[3] * 170; scale = 0.8f; }
        else { cx = 610 + s[2] * 150; cy = 836; scale = 0.62f; }
        var st = g.Save();
        g.TranslateTransform(cx, cy); g.RotateTransform(s[4]); g.ScaleTransform(scale, scale);
        var game = GameSticker((int)s[5]);
        if (game != null) {
            float k = 160f / Math.Max(game.Width, game.Height);
            g.DrawImage(game, -game.Width * k / 2, -game.Height * k / 2, game.Width * k, game.Height * k);
        } else DrawSticker(g, (int)s[0], (int)s[5]);
        g.Restore(st);
    }
    // A parody sticker around (0, 0), about 160 x 160 at most, aged: faded, scuffed, chipped, one edge peeling up.
    static void DrawSticker(Graphics g, int design, int seed) {
        var rnd = new Random(seed);
        using (var shape = StickerShape(design)) {
            // The peel: the part past a line near one edge has lifted and folded back over.
            var bounds = shape.GetBounds();
            double pa = rnd.NextDouble() * Math.PI * 2;
            float nx = (float)Math.Cos(pa), ny = (float)Math.Sin(pa);
            float reach = Math.Abs(nx) * bounds.Width / 2 + Math.Abs(ny) * bounds.Height / 2;
            float dist = reach * (0.74f + 0.14f * (float)rnd.NextDouble());
            var p = new PointF(nx * dist, ny * dist);
            var dx = -ny; var dy = nx;
            using (var half = new GraphicsPath()) {
                half.AddPolygon(new[] { new PointF(p.X + dx * 400, p.Y + dy * 400), new PointF(p.X - dx * 400, p.Y - dy * 400), new PointF(p.X - dx * 400 + nx * 400, p.Y - dy * 400 + ny * 400), new PointF(p.X + dx * 400 + nx * 400, p.Y + dy * 400 + ny * 400) });
                using (var kept = new Region(shape)) using (var lifted = new Region(shape)) {
                    kept.Exclude(half); lifted.Intersect(half);
                    var gs = g.Save();
                    g.TranslateTransform(2, 3);
                    using (var b = new SolidBrush(Color.FromArgb(70, 0, 0, 0))) g.FillRegion(b, kept);
                    g.Restore(gs);
                    gs = g.Save();
                    g.SetClip(kept, CombineMode.Intersect);
                    StickerArt(g, design, rnd);
                    // Wear: scratches, grime, chipped bits along the edge.
                    using (var pen = new Pen(Color.FromArgb(110, 255, 255, 250), 1.2f)) for (int i = 0; i < 16; i++) { float x0 = (float)(rnd.NextDouble() * 160 - 80), y0 = (float)(rnd.NextDouble() * 160 - 80); g.DrawLine(pen, x0, y0, x0 + (float)(rnd.NextDouble() * 40 - 20), y0 + (float)(rnd.NextDouble() * 12 - 6)); }
                    using (var b = new SolidBrush(Color.FromArgb(60, 60, 50, 30))) for (int i = 0; i < 40; i++) g.FillEllipse(b, (float)(rnd.NextDouble() * 160 - 80), (float)(rnd.NextDouble() * 160 - 80), 2.5f, 2.5f);
                    using (var b = new SolidBrush(Color.FromArgb(30, 120, 110, 80))) g.FillRectangle(b, -90, -90, 180, 180);
                    g.Restore(gs);
                    using (var pen = new Pen(Color.FromArgb(120, 40, 40, 44), 3)) {
                        for (int i = 0; i < 4; i++) {
                            double ca = rnd.NextDouble() * Math.PI * 2;
                            float ex = (float)Math.Cos(ca) * bounds.Width * 0.47f, ey = (float)Math.Sin(ca) * bounds.Height * 0.47f;
                            var cs = g.Save(); g.SetClip(kept, CombineMode.Intersect);
                            using (var b = new SolidBrush(Color.FromArgb(120, 46, 47, 52))) g.FillPolygon(b, new[] { new PointF(ex - 6, ey - 3), new PointF(ex + 5, ey - 5), new PointF(ex + 7, ey + 4), new PointF(ex - 3, ey + 7) });
                            g.Restore(cs);
                        }
                    }
                    // Old glue where it lifted, then the flap folded back over the line.
                    using (var b = new SolidBrush(Color.FromArgb(60, 210, 196, 150))) g.FillRegion(b, lifted);
                    float c2 = dx * dx - dy * dy, s2 = 2 * dx * dy;
                    var m = new Matrix(c2, s2, s2, -c2, p.X - (c2 * p.X + s2 * p.Y), p.Y - (s2 * p.X - c2 * p.Y));
                    gs = g.Save();
                    g.MultiplyTransform(m);
                    g.TranslateTransform(-nx * 3, -ny * 3);
                    using (var b = new SolidBrush(Color.FromArgb(80, 0, 0, 0))) g.FillRegion(b, lifted);
                    g.TranslateTransform(nx * 3, ny * 3);
                    using (var b = new LinearGradientBrush(new RectangleF(-90, -90, 180, 180), Color.FromArgb(238, 234, 222), Color.FromArgb(196, 190, 176), (float)(pa * 180 / Math.PI))) g.FillRegion(b, lifted);
                    g.Restore(gs);
                    gs = g.Save(); g.SetClip(shape, CombineMode.Intersect);
                    using (var pen = new Pen(Color.FromArgb(90, 0, 0, 0), 1.2f)) g.DrawLine(pen, p.X + dx * 120, p.Y + dy * 120, p.X - dx * 120, p.Y - dy * 120);
                    g.Restore(gs);
                }
            }
        }
    }
    static GraphicsPath StickerShape(int design) {
        var path = new GraphicsPath();
        if (design == 0) { path.AddClosedCurve(new[] { new PointF(-6, -80), new PointF(22, -64), new PointF(40, -10), new PointF(30, 54), new PointF(34, 80), new PointF(-30, 82), new PointF(-30, 30), new PointF(-42, -24), new PointF(-30, -62) }, 0.5f); return path; }
        if (design == 1) { path.AddArc(-80, -56, 160, 70, 180, 180); path.AddLine(80, -21, 80, 40); path.AddLine(80, 40, -80, 40); path.CloseFigure(); return path; }
        if (design == 2) { path.AddEllipse(-72, -72, 144, 144); return path; }
        if (design == 3) { path.AddRectangle(new RectangleF(-80, -30, 160, 60)); return path; }
        path.AddEllipse(-70, -70, 140, 140); return path;
    }
    // A wobbly hand-drawn line (the riso character sticker's look).
    static void Wobble(Graphics g, Pen pen, Random rnd, params PointF[] pts) {
        var w = new PointF[pts.Length];
        for (int i = 0; i < pts.Length; i++) w[i] = new PointF(pts[i].X + (float)(rnd.NextDouble() * 2.4 - 1.2), pts[i].Y + (float)(rnd.NextDouble() * 2.4 - 1.2));
        g.DrawCurve(pen, w, 0.5f);
    }
    static void FitText(Graphics g, string s, string family, FontStyle style, Color c, RectangleF r) {
        float size = r.Height;
        using (var probe = new Font(family, 100, style, GraphicsUnit.Pixel)) { var m = g.MeasureString(s, probe); size = Math.Min(r.Height * 100 / m.Height * 1.15f, r.Width * 100 / m.Width); }
        using (var f = new Font(family, Math.Max(4, size), style, GraphicsUnit.Pixel)) Write(g, s, f, c, new RectangleF(r.X - 20, r.Y - size * 0.1f, r.Width + 40, r.Height + size * 0.3f), StringAlignment.Center, StringAlignment.Center, false);
    }
    static void StickerArt(Graphics g, int design, Random rnd) {
        if (design == 0) {
            // "The filmer": an original character on clear vinyl, purple hand-drawn lines, flat fills printed off the lines.
            using (var b = new SolidBrush(Color.FromArgb(70, 255, 255, 255))) g.FillRectangle(b, -90, -90, 180, 180);
            var line = Color.FromArgb(84, 58, 160);
            Action<Color, PointF[]> fill = delegate(Color c, PointF[] pts) {
                var off = new PointF[pts.Length]; for (int i = 0; i < pts.Length; i++) off[i] = new PointF(pts[i].X + 2.5f, pts[i].Y + 1.5f);
                using (var b = new SolidBrush(c)) g.FillClosedCurve(b, off, FillMode.Alternate, 0.4f);
            };
            var head = new[] { new PointF(-14, -56), new PointF(-2, -66), new PointF(12, -58), new PointF(12, -40), new PointF(0, -32), new PointF(-12, -38) };
            var hoodie = new[] { new PointF(-22, -30), new PointF(18, -32), new PointF(26, 10), new PointF(20, 16), new PointF(-22, 16), new PointF(-28, 6) };
            var pants = new[] { new PointF(-22, 14), new PointF(20, 14), new PointF(24, 64), new PointF(4, 66), new PointF(0, 30), new PointF(-6, 66), new PointF(-26, 64) };
            var arm = new[] { new PointF(12, -26), new PointF(24, -40), new PointF(30, -56), new PointF(22, -60), new PointF(14, -46), new PointF(6, -30) };
            fill(Color.FromArgb(232, 196, 64), hoodie);
            fill(Color.FromArgb(150, 178, 214), pants);
            fill(Color.FromArgb(250, 240, 222), head);
            fill(Color.FromArgb(232, 196, 64), arm);
            using (var b = new SolidBrush(Color.FromArgb(170, 120, 170))) g.FillRectangle(b, -20, 6, 30, 12);
            using (var b = new SolidBrush(Color.FromArgb(150, 196, 80))) { g.FillEllipse(b, -30, 62, 26, 12); g.FillEllipse(b, 2, 62, 26, 12); }
            using (var b = new SolidBrush(Color.FromArgb(58, 58, 66))) g.FillRectangle(b, 14, -70, 30, 18);
            using (var b = new SolidBrush(Color.FromArgb(120, 150, 200))) g.FillEllipse(b, 38, -67, 10, 12);
            using (var pen = new Pen(line, 2.4f)) {
                pen.LineJoin = LineJoin.Round;
                Wobble(g, pen, rnd, head[0], head[1], head[2], head[3], head[4], head[5], head[0]);
                Wobble(g, pen, rnd, new PointF(-14, -56), new PointF(-4, -68), new PointF(10, -62), new PointF(16, -58));
                Wobble(g, pen, rnd, hoodie[0], hoodie[5], hoodie[4], hoodie[3], hoodie[2], hoodie[1]);
                Wobble(g, pen, rnd, pants[0], pants[6], pants[5], pants[4], pants[3], pants[2], pants[1]);
                Wobble(g, pen, rnd, arm[0], arm[1], arm[2], arm[3], arm[4], arm[5]);
                g.DrawRectangle(pen, 14, -70, 30, 18);
                Wobble(g, pen, rnd, new PointF(-2, -48), new PointF(2, -46), new PointF(4, -42));
                Wobble(g, pen, rnd, new PointF(-6, -30), new PointF(-4, 0), new PointF(-2, 12));
                g.DrawEllipse(pen, -30, 62, 26, 12); g.DrawEllipse(pen, 2, 62, 26, 12);
            }
            using (var pen = new Pen(Color.FromArgb(130, 255, 255, 255), 1.5f)) g.DrawPath(pen, StickerShape(0));
            return;
        }
        var cream = Color.FromArgb(240, 234, 218);
        using (var b = new SolidBrush(cream)) g.FillRectangle(b, -90, -90, 180, 180);
        if (design == 1) {
            // "BLICKEY": a workwear-tag parody, an arch-topped badge.
            using (var inner = new GraphicsPath()) {
                inner.AddArc(-72, -50, 144, 60, 180, 180); inner.AddLine(72, -20, 72, 33); inner.AddLine(72, 33, -72, 33); inner.CloseFigure();
                using (var b = new SolidBrush(Color.FromArgb(150, 132, 88))) g.FillPath(b, inner);
            }
            FitText(g, "BLICKEY", "Arial Black", FontStyle.Regular, cream, new RectangleF(-64, -20, 128, 30));
            FitText(g, "SINCE 1922 (PROBABLY)", "Tahoma", FontStyle.Bold, cream, new RectangleF(-56, 14, 112, 12));
        } else if (design == 2) {
            // "SPITBALL WHEELS": a round flame-head wheel badge parody.
            using (var b = new SolidBrush(Color.FromArgb(214, 52, 38))) g.FillEllipse(b, -64, -64, 128, 128);
            using (var b = new SolidBrush(Color.FromArgb(252, 196, 60)))
                for (int i = 0; i < 9; i++) { double a = -Math.PI / 2 + (i - 4) * 0.28; g.FillPolygon(b, new[] { new PointF((float)Math.Cos(a - 0.12) * 26, (float)Math.Sin(a - 0.12) * 26 - 6), new PointF((float)Math.Cos(a) * 54, (float)Math.Sin(a) * 54 - 6), new PointF((float)Math.Cos(a + 0.12) * 26, (float)Math.Sin(a + 0.12) * 26 - 6) }); }
            using (var b = new SolidBrush(cream)) g.FillEllipse(b, -24, -28, 48, 48);
            using (var pen = new Pen(Color.FromArgb(40, 30, 30), 3)) { g.DrawEllipse(pen, -24, -28, 48, 48); g.DrawArc(pen, -10, -6, 20, 14, 10, 160); }
            using (var b = new SolidBrush(Color.FromArgb(40, 30, 30))) { g.FillEllipse(b, -11, -14, 6, 8); g.FillEllipse(b, 5, -14, 6, 8); }
            FitText(g, "SPITBALL WHEELS", "Arial Black", FontStyle.Regular, cream, new RectangleF(-54, 28, 108, 22));
        } else if (design == 3) {
            // "SUPREMELY MID": the box-logo parody.
            using (var b = new SolidBrush(Color.FromArgb(222, 30, 36))) g.FillRectangle(b, -74, -24, 148, 48);
            FitText(g, "Supremely Mid", "Arial", FontStyle.Bold | FontStyle.Italic, Color.White, new RectangleF(-68, -18, 136, 36));
        } else {
            // "THRASHED": the flame-lettering magazine parody on a black disc.
            using (var b = new SolidBrush(Color.FromArgb(20, 20, 22))) g.FillEllipse(b, -62, -62, 124, 124);
            using (var b = new SolidBrush(Color.FromArgb(240, 110, 30)))
                for (int i = 0; i < 7; i++) { float x = -48 + i * 16; g.FillPolygon(b, new[] { new PointF(x - 7, 10), new PointF(x + 2, -30 - (i % 3) * 8), new PointF(x + 9, 10) }); }
            FitText(g, "THRASHED", "Impact", FontStyle.Regular, Color.FromArgb(255, 226, 120), new RectangleF(-54, -12, 108, 30));
            FitText(g, "SKATE AND REST", "Tahoma", FontStyle.Bold, cream, new RectangleF(-40, 22, 80, 12));
        }
    }

    // ---------- the LCD (drawn low-res, scaled up with hard pixels) ----------
    Bitmap lcdBmp, sceneBmp; Font osdBig, osdRow, osdSmall, osdLogo; int osdFontH;
    bool recOn; DateTime recSince; TimeSpan recTotal;
    int lcdW, lcdH;
    void LoHit(RectangleF lo, int index) {
        var u = new RectangleF(ScreenR.X + lo.X * ScreenR.Width / lcdW, ScreenR.Y + lo.Y * ScreenR.Height / lcdH, lo.Width * ScreenR.Width / lcdW, lo.Height * ScreenR.Height / lcdH);
        hits.Add(new KeyValuePair<RectangleF, int>(ToWin(u), index));
    }
    Bitmap LcdCanvas(float t) {
        int k = Math.Max(2, (int)Math.Round(ScreenR.Height * devS / 190f));
        int W = Math.Max(200, (int)(ScreenR.Width * devS / k)), H = Math.Max(112, (int)(ScreenR.Height * devS / k));
        if (lcdBmp == null || lcdBmp.Width != W || lcdBmp.Height != H) { if (lcdBmp != null) lcdBmp.Dispose(); lcdBmp = new Bitmap(W, H, PixelFormat.Format32bppPArgb); }
        if (osdFontH != H) {
            osdFontH = H;
            osdBig = new Font("Tahoma", H / 14f, FontStyle.Bold, GraphicsUnit.Pixel);
            osdRow = new Font("Tahoma", H / 16.5f, FontStyle.Bold, GraphicsUnit.Pixel);
            osdSmall = new Font("Tahoma", H / 20f, GraphicsUnit.Pixel);
            osdLogo = new Font("Arial Black", H / 5.5f, FontStyle.Italic, GraphicsUnit.Pixel);
        }
        lcdW = W; lcdH = H;
        DateTime now = DateTime.Now;
        bool rec = job != null;
        if (rec && !recOn) recSince = now;
        if (!rec && recOn) recTotal += now - recSince;
        recOn = rec;
        using (var o = Graphics.FromImage(lcdBmp)) {
            o.SmoothingMode = SmoothingMode.None;
            o.TextRenderingHint = System.Drawing.Text.TextRenderingHint.SingleBitPerPixelGridFit;
            bool splash = starting && job == null && pages.Count == 0;
            if (splash) { o.Clear(Color.FromArgb(4, 4, 8)); LcdSplash(o, W, H, t); }
            else {
                o.DrawImageUnscaled(Scene(W, H), 0, 0);
                float lh = osdSmall.Height, top = osdRow.Height + 5, bottom = H - lh * 2 - 4;
                LcdStatus(o, W, H, now);
                if (job != null) LcdJob(o, W, top, bottom, t);
                else if (pages.Count > 0) LcdPage(o, pages.Peek(), W, top, bottom, t);
            }
            LcdFoot(o, W, H, now, splash);
        }
        return lcdBmp;
    }
    // What the camera is pointed at: a skate spot at dusk (stairs, a handrail, a ledge), dimmed under the menus.
    Bitmap Scene(int W, int H) {
        if (sceneBmp != null && sceneBmp.Width == W && sceneBmp.Height == H) return sceneBmp;
        if (sceneBmp != null) sceneBmp.Dispose();
        sceneBmp = new Bitmap(W, H, PixelFormat.Format32bppPArgb);
        using (var g = Graphics.FromImage(sceneBmp)) {
            float hz = H * 0.62f;
            using (var b = new LinearGradientBrush(new RectangleF(0, -1, W, hz + 2), Color.FromArgb(34, 44, 96), Color.FromArgb(214, 122, 74), 90f)) g.FillRectangle(b, 0, 0, W, hz);
            using (var b = new SolidBrush(Color.FromArgb(200, 252, 206, 128))) g.FillEllipse(b, W * 0.68f, hz - H * 0.17f, H * 0.16f, H * 0.16f);
            var rnd = new Random(5);
            using (var b = new SolidBrush(Color.FromArgb(44, 34, 54))) for (float x = 0; x < W; ) { float bw = 8 + (float)rnd.NextDouble() * 16, bh = H * (0.05f + (float)rnd.NextDouble() * 0.18f); g.FillRectangle(b, x, hz - bh, bw, bh); x += bw + 1; }
            using (var b = new LinearGradientBrush(new RectangleF(0, hz - 1, W, H - hz + 2), Color.FromArgb(78, 64, 66), Color.FromArgb(30, 26, 30), 90f)) g.FillRectangle(b, 0, hz, W, H - hz);
            // A set of stairs going down to the right with a handrail, and a ledge on the left.
            float sx0 = W * 0.6f, sy0 = H * 0.7f, stepW = W * 0.08f, stepH = H * 0.06f;
            var stairs = new List<PointF> { new PointF(sx0, H), new PointF(sx0, sy0) };
            for (int i = 0; i < 5; i++) { stairs.Add(new PointF(sx0 + (i + 1) * stepW, sy0 + i * stepH)); stairs.Add(new PointF(sx0 + (i + 1) * stepW, sy0 + (i + 1) * stepH)); }
            stairs.Add(new PointF(W, sy0 + 5 * stepH)); stairs.Add(new PointF(W, H));
            using (var b = new SolidBrush(Color.FromArgb(52, 46, 52))) g.FillPolygon(b, stairs.ToArray());
            using (var pen = new Pen(Color.FromArgb(120, 214, 150, 110), 1)) for (int i = 0; i < 5; i++) g.DrawLine(pen, sx0 + i * stepW, sy0 + i * stepH, sx0 + (i + 1) * stepW, sy0 + i * stepH);
            using (var b = new SolidBrush(Color.FromArgb(70, 64, 66))) g.FillRectangle(b, W * 0.05f, H * 0.74f, W * 0.3f, H * 0.07f);
            using (var pen = new Pen(Color.FromArgb(150, 214, 150, 110), 1)) g.DrawLine(pen, W * 0.05f, H * 0.74f, W * 0.35f, H * 0.74f);
            using (var pen = new Pen(Color.FromArgb(170, 170, 172, 182), 1)) { g.DrawLine(pen, sx0 + stepW * 0.5f, sy0 - H * 0.09f, W, sy0 - H * 0.09f + (W - sx0 - stepW * 0.5f) * stepH / stepW); for (int i = 0; i < 4; i++) { float px = sx0 + stepW * (0.5f + i * 1.2f); g.DrawLine(pen, px, sy0 - H * 0.09f + (px - sx0 - stepW * 0.5f) * stepH / stepW, px, sy0 + i * 1.2f * stepH); } }
            using (var b = new SolidBrush(Color.FromArgb(96, 0, 0, 0))) g.FillRectangle(b, 0, 0, W, H);
            using (var b = new SolidBrush(Color.FromArgb(26, 0, 0, 0))) for (int y = 0; y < H; y += 2) g.FillRectangle(b, 0, y, W, 1);
        }
        return sceneBmp;
    }
    static void OsdText(Graphics g, string s, Font f, Color c, float x, float y) {
        using (var black = new SolidBrush(Color.Black))
            for (int dx = -1; dx <= 1; dx++) for (int dy = -1; dy <= 1; dy++) if (dx != 0 || dy != 0) g.DrawString(s, f, black, x + dx, y + dy);
        using (var b = new SolidBrush(c)) g.DrawString(s, f, b, x, y);
    }
    static void OsdPanel(Graphics g, RectangleF r) {
        using (var b = new SolidBrush(OsdNavy)) g.FillRectangle(b, r);
        using (var pen = new Pen(Color.FromArgb(170, 200, 210, 240), 1)) g.DrawRectangle(pen, r.X, r.Y, r.Width - 1, r.Height - 1);
    }
    // Top row: STBY / REC with its blinking dot, SP, the tape counter (runs only while recording), the battery.
    void LcdStatus(Graphics o, int W, int H, DateTime now) {
        TimeSpan tape = recTotal + (recOn ? now - recSince : TimeSpan.Zero);
        float x = 4, y = 2;
        if (recOn) {
            if (now.Millisecond < 600) using (var b = new SolidBrush(RecRed)) o.FillEllipse(b, x, y + osdRow.Height * 0.25f, osdRow.Height * 0.5f, osdRow.Height * 0.5f);
            x += osdRow.Height * 0.6f;
        }
        string mode = recOn ? "REC" : "STBY";
        OsdText(o, mode, osdRow, OsdWhite, x, y); x += o.MeasureString(mode, osdRow).Width + 4;
        OsdText(o, "SP", osdRow, OsdWhite, x, y); x += o.MeasureString("SP", osdRow).Width + 4;
        OsdText(o, string.Format("{0}:{1:00}:{2:00}:{3:00}", (int)tape.TotalHours, tape.Minutes, tape.Seconds, tape.Milliseconds * 30 / 1000), osdRow, OsdWhite, x, y);
        var mins = o.MeasureString("60min", osdSmall);
        float bx = W - 4 - mins.Width - 18, bh = osdRow.Height * 0.55f, by = y + (osdRow.Height - bh) / 2;
        OsdText(o, "60min", osdSmall, OsdWhite, W - 4 - mins.Width, y + 1);
        using (var bl = new SolidBrush(Color.Black)) o.FillRectangle(bl, bx - 1, by - 1, 16, bh + 2);
        using (var wb = new SolidBrush(OsdWhite)) { o.FillRectangle(wb, bx, by, 14, bh); o.FillRectangle(wb, bx + 14, by + bh * 0.3f, 2, bh * 0.4f); }
        using (var bl = new SolidBrush(Color.Black)) o.FillRectangle(bl, bx + 1, by + 1, 12, bh - 2);
        using (var wb = new SolidBrush(OsdWhite)) for (int i = 0; i < 4; i++) o.FillRectangle(wb, bx + 2 + i * 3, by + 2, 2, bh - 4);
    }
    // Bottom: what the keys do now on the left, the date and time stamp on the right.
    void LcdFoot(Graphics o, int W, int H, DateTime now, bool splash) {
        float lh = osdSmall.Height;
        string hint;
        if (closeAt >= 0) hint = "CLOSING IN " + closeAt + " s";
        else if (job != null) hint = ((build.Waiting != null && !build.Ended) || build.FetchWaiting()) ? (cancelArmed ? "[B] AGAIN TO CANCEL" : "[B] CANCEL") : "";
        else if (splash) hint = CanSkip() ? (skipArmed ? "[B] AGAIN TO SKIP" : "[B] SKIP THE CHECK") : "";
        else if (pages.Count > 0 && IsMain(pages.Peek())) hint = "[A] EXEC   START/STOP: PLAY";
        else hint = "[A] EXEC   [B] RET.";
        if (hint != "") OsdText(o, hint, osdSmall, OsdWhite, 4, H - lh - 2);
        var inv = System.Globalization.CultureInfo.InvariantCulture;
        string date = now.ToString("MMM. d yyyy", inv).ToUpperInvariant(), time = now.ToString("h:mm:ss tt", inv);
        float dw = Math.Max(o.MeasureString(date, osdSmall).Width, o.MeasureString(time, osdSmall).Width);
        OsdText(o, date, osdSmall, OsdWhite, W - 4 - dw, H - lh * 2 - 3);
        OsdText(o, time, osdSmall, OsdWhite, W - 4 - dw, H - lh - 2);
    }
    // A menu screen: the title tab, the list with the orange selection bar, the selected row's info beside it; a text
    // page shows its text in a box (and a build's first error under it).
    void LcdPage(Graphics o, Page p, int W, float top, float bottom, float t) {
        const float m = 4;
        string title = IsMain(p) ? "MENU" : p.Title.TrimEnd(':', ' ').ToUpperInvariant();
        var tab = new RectangleF(m, top, Math.Min(W - 2 * m, o.MeasureString(title, osdRow).Width + 6), osdRow.Height + 2);
        OsdPanel(o, tab);
        Write(o, title, osdRow, OsdWhite, new RectangleF(tab.X + 3, tab.Y + 1, tab.Width - 4, tab.Height), StringAlignment.Near, StringAlignment.Near, false);
        float y = tab.Bottom + 2;
        if (p.Body != null) {
            var br = new RectangleF(m, y, W - 2 * m, p.Items.Count > 0 ? (bottom - y) * 0.6f : bottom - y);
            OsdPanel(o, br);
            float textH = br.Height - 6;
            if (p.Mono != null) textH = Math.Min(textH, o.MeasureString(p.Body, osdSmall, (int)(br.Width - 8)).Height + 2);
            Write(o, p.Body, osdSmall, OsdWhite, new RectangleF(br.X + 4, br.Y + 3, br.Width - 8, textH), StringAlignment.Near, StringAlignment.Near, true);
            if (p.Mono != null) {
                var eb = new RectangleF(br.X + 4, br.Y + 4 + textH, br.Width - 8, br.Bottom - 4 - (br.Y + 4 + textH));
                if (eb.Height > osdSmall.Height) {
                    using (var b = new SolidBrush(Color.FromArgb(200, 60, 12, 16))) o.FillRectangle(b, eb);
                    using (var pen = new Pen(OsdRed, 1)) o.DrawRectangle(pen, eb.X, eb.Y, eb.Width - 1, eb.Height - 1);
                    Write(o, p.Mono, osdSmall, OsdWhite, new RectangleF(eb.X + 3, eb.Y + 2, eb.Width - 6, eb.Height - 4), StringAlignment.Near, StringAlignment.Near, true);
                }
            }
            y = br.Bottom + 3;
        }
        if (p.Items.Count == 0) return;
        bool split = p.Body == null;
        float listW = split ? (float)Math.Floor(W * 0.42f) : W - 2 * m, rowH = osdRow.Height + 3;
        int fit = Math.Max(1, (int)((bottom - y - 4) / rowH));
        int first = Math.Max(0, Math.Min(p.Sel - fit / 2, p.Items.Count - fit)), shown = Math.Min(fit, p.Items.Count - first);
        var box = new RectangleF(m, y, listW, shown * rowH + 4);
        OsdPanel(o, box);
        for (int i = first; i < first + shown; i++) {
            var it = p.Items[i]; bool sel = i == p.Sel;
            var r = new RectangleF(box.X + 2, box.Y + 2 + (i - first) * rowH, box.Width - 4, rowH);
            if (sel) using (var b = new SolidBrush(OsdOrange)) o.FillRectangle(b, r);
            string text = split ? ShortName(it.Text) : it.Text;
            bool cur = text.StartsWith("> ");
            if (cur) text = text.Substring(2);
            Write(o, text, osdRow, sel ? Color.Black : it.On ? OsdWhite : OsdGrey, new RectangleF(r.X + 3, r.Y + 1, r.Width - (cur ? 12 : 5), r.Height), StringAlignment.Near, StringAlignment.Near, false);
            if (cur) using (var b = new SolidBrush(sel ? Color.Black : OsdOrange)) o.FillRectangle(b, r.Right - 7, r.Y + rowH / 2 - 2, 4, 4);
            LoHit(r, i);
        }
        using (var b = new SolidBrush(OsdWhite)) {
            float ax = box.Right - 8;
            if (first > 0) o.FillPolygon(b, new[] { new PointF(ax, box.Y - 1), new PointF(ax + 3, box.Y - 4), new PointF(ax + 6, box.Y - 1) });
            if (first + shown < p.Items.Count) o.FillPolygon(b, new[] { new PointF(ax, box.Bottom + 1), new PointF(ax + 3, box.Bottom + 4), new PointF(ax + 6, box.Bottom + 1) });
        }
        if (split) LcdInfo(o, p, new RectangleF(box.Right + 3, y, W - m - box.Right - 3, bottom - y), t);
    }
    void LcdInfo(Graphics o, Page p, RectangleF r, float t) {
        OsdPanel(o, r);
        float x = r.X + 4, w = r.Width - 8, y = r.Y + 3, bottom = r.Bottom - 3, sh = osdSmall.Height;
        var it = p.Items[p.Sel];
        if (IsMain(p) && p.Sel == 0) {
            Write(o, "NOW SELECTED", osdSmall, OsdOrange, new RectangleF(x, y, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); y += sh + 1;
            float vh = Math.Min(o.MeasureString(St("version"), osdBig, (int)w).Height, osdBig.Height * 2.2f);
            Write(o, St("version"), osdBig, OsdWhite, new RectangleF(x, y, w, vh), StringAlignment.Near, StringAlignment.Near, true); y += vh + 1;
            Write(o, St("version_status"), osdSmall, OsdWhite, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f;
            string send = St("send");
            Write(o, "Last session: " + St("last") + (send == "" || send == "off" ? "" : "   Send: " + send), osdSmall, OsdGrey, new RectangleF(x, y, w, bottom - y - sh - 2), StringAlignment.Near, StringAlignment.Near, true);
            if (t % 1.2f < 0.8f) Write(o, "PRESS START/STOP", osdRow, OsdOrange, new RectangleF(x, bottom - osdRow.Height - 1, w, osdRow.Height + 2), StringAlignment.Near, StringAlignment.Near, false);
            return;
        }
        string name = it.Text.StartsWith("> ") ? it.Text.Substring(2) : it.Text;
        float nh = Math.Min(o.MeasureString(name, osdRow, (int)w).Height, osdRow.Height * 2.2f);
        Write(o, name, osdRow, OsdWhite, new RectangleF(x, y, w, nh), StringAlignment.Near, StringAlignment.Near, true); y += nh + 1;
        Write(o, it.On ? "READY" : "NOT AVAILABLE", osdSmall, it.On ? OsdGreen : OsdRed, new RectangleF(x, y, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); y += sh + 2;
        string info = it.Info != "" ? it.Info : it.Hint;
        if (info != "") {
            float ih = Math.Min(o.MeasureString(info, osdSmall, (int)w).Height, bottom - y);
            Write(o, info, osdSmall, OsdWhite, new RectangleF(x, y, w, ih), StringAlignment.Near, StringAlignment.Near, true); y += ih + 2;
        }
        float nameW = w * 0.38f;
        foreach (var rec in it.Records) {
            if (y + sh > bottom) { Write(o, "...", osdSmall, OsdGrey, new RectangleF(x, y - 2, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); break; }
            int cut = rec.IndexOf(": ");
            string a = cut > 0 ? rec.Substring(0, cut) : rec, b = cut > 0 ? rec.Substring(cut + 2) : "";
            if (a.StartsWith("#")) { Write(o, a.Substring(1).ToUpperInvariant() + (b != "" ? "  " + b : ""), osdSmall, OsdOrange, new RectangleF(x, y, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); y += sh + 1; continue; }
            float rh = Math.Min(Math.Max(o.MeasureString(a, osdSmall, (int)nameW).Height, o.MeasureString(b, osdSmall, (int)(w - nameW - 3)).Height), Math.Max(sh, bottom - y));
            Write(o, a, osdSmall, OsdWhite, new RectangleF(x, y, nameW, rh), StringAlignment.Near, StringAlignment.Near, true);
            Write(o, b, osdSmall, OsdGrey, new RectangleF(x + nameW + 3, y, w - nameW - 3, rh), StringAlignment.Near, StringAlignment.Near, true);
            y += rh + 1;
        }
    }
    // A job (fetch, build): its name, the time, a segmented tape bar for cargo's step count, the counts, then the first
    // error or the newest log lines.
    void LcdJob(Graphics o, int W, float top, float bottom, float t) {
        const float m = 4;
        var card = new RectangleF(m, top, W - 2 * m, bottom - top);
        OsdPanel(o, card);
        float x = card.X + 4, w = card.Width - 8, y = card.Y + 3, sh = osdSmall.Height;
        bool isBuild = build.Active || build.Ended, failed = build.Ended && !build.Ok, fetchWait = build.FetchWaiting();
        float k = isBuild && build.Total > 0 ? Math.Min(1f, build.Done / (float)build.Total) : -1;
        var el = DateTime.Now - (isBuild ? build.Start : fetchWait ? build.PreWaitSince : jobStart);
        Write(o, isBuild ? build.Name : jobTitle, osdBig, OsdWhite, new RectangleF(x, y, w * 0.78f, osdBig.Height + 2), StringAlignment.Near, StringAlignment.Near, false);
        Write(o, BuildView.Clock(el), osdRow, OsdWhite, new RectangleF(x, y, w, osdBig.Height + 2), StringAlignment.Far, StringAlignment.Near, false);
        y += osdBig.Height + 1;
        if (isBuild) { Write(o, "at " + build.Commit, osdSmall, OsdGrey, new RectangleF(x, y, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); y += sh + 2; }
        const int segs = 30;
        var bar = new RectangleF(x, y, w, 7);
        using (var pen = new Pen(OsdWhite, 1)) o.DrawRectangle(pen, bar.X, bar.Y, bar.Width - 1, bar.Height - 1);
        float segW = (bar.Width - 2) / segs;
        int on = k >= 0 ? (int)Math.Round(k * segs) : -1, run = (int)(t * 10) % (segs + 5) - 5;
        using (var b = new SolidBrush(failed ? OsdRed : OsdOrange))
            for (int i = 0; i < segs; i++) if (on >= 0 ? i < on : (i >= run && i < run + 5)) o.FillRectangle(b, bar.X + 1 + i * segW + 0.5f, bar.Y + 1, segW - 1, bar.Height - 2);
        y += 10;
        if (isBuild) {
            string count = build.Total > 0 ? build.Done + " / " + build.Total + " build steps" : build.Compiled + " crates compiled";
            if (!build.Ended && build.Waiting != null) count = "Waiting " + BuildView.Clock(DateTime.Now - build.WaitSince) + ": " + build.Waiting;
            else if (!build.Ended && build.Total == 0 && build.Compiled == 0 && build.StageText != null) count = build.StageText;
            if (build.Ended) count = build.Ok ? "Built in " + BuildView.Clock(build.End - build.Start) : "Build failed";
            Write(o, count, osdRow, failed ? OsdRed : OsdWhite, new RectangleF(x, y, w * 0.7f, osdRow.Height + 2), StringAlignment.Near, StringAlignment.Near, false);
            if (build.Warnings > 0) Write(o, build.Warnings + (build.Warnings == 1 ? " warning" : " warnings"), osdSmall, OsdOrange, new RectangleF(x, y, w, osdRow.Height + 2), StringAlignment.Far, StringAlignment.Near, false);
            y += osdRow.Height + 2;
            string line = !build.Ended && build.Waiting != null ? (build.WaitWho ?? "Cargo does not say who; it continues by itself once that build ends.") : (build.Now != "" && !build.Ended ? "Now: " + build.Now : "");
            if (line != "") { Write(o, line, osdSmall, OsdGrey, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f; }
            if (build.Error.Count > 0) {
                var eb = new RectangleF(x, y, w, card.Bottom - 3 - y);
                using (var b = new SolidBrush(Color.FromArgb(200, 60, 12, 16))) o.FillRectangle(b, eb);
                using (var pen = new Pen(OsdRed, 1)) o.DrawRectangle(pen, eb.X, eb.Y, eb.Width - 1, eb.Height - 1);
                Write(o, "First error\n" + string.Join("\n", build.Error), osdSmall, OsdWhite, new RectangleF(eb.X + 3, eb.Y + 2, eb.Width - 6, eb.Height - 4), StringAlignment.Near, StringAlignment.Near, true);
            }
            return;
        }
        var lines = new List<string>();
        for (int i = log.Count - 1; i >= 0 && lines.Count < 12; i--) { string c = Clean(log[i]); if (c != "") lines.Add(c); }
        string status = fetchWait ? "Waiting " + BuildView.Clock(el) + ": " + build.PreWait : lines.Count > 0 ? lines[0] : "Starting...";
        Write(o, status, osdRow, OsdWhite, new RectangleF(x, y, w, osdRow.Height + 2), StringAlignment.Near, StringAlignment.Near, false); y += osdRow.Height + 2;
        if (fetchWait) { Write(o, "git stops by itself if nothing arrives for 30 s; cancelling keeps the last fetched version.", osdSmall, OsdGrey, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f; }
        for (int i = 1; i < lines.Count && y + sh <= card.Bottom - 2; i++) { Write(o, lines[i], osdSmall, OsdGrey, new RectangleF(x, y, w, sh + 1), StringAlignment.Near, StringAlignment.Near, false); y += sh; }
    }
    // Power-on screen: the logo, the model, then the start check's step.
    void LcdSplash(Graphics o, int W, int H, float t) {
        float since = (float)(DateTime.Now - startSince).TotalSeconds;
        Write(o, "SKATE", osdLogo, OsdWhite, new RectangleF(0, H * 0.18f, W, osdLogo.Height * 1.2f), StringAlignment.Center, StringAlignment.Near, false);
        Write(o, "SK8-1000   DIGITAL VIDEO CAMERA", osdSmall, OsdGrey, new RectangleF(0, H * 0.18f + osdLogo.Height * 1.1f, W, osdSmall.Height + 2), StringAlignment.Center, StringAlignment.Near, false);
        string stage = skipped ? "Finishing the start" : (startStage ?? "Checking for a newer launcher");
        Write(o, stage + new string('.', 1 + (int)(t * 2) % 3), osdRow, OsdWhite, new RectangleF(0, H * 0.64f, W, osdRow.Height + 2), StringAlignment.Center, StringAlignment.Near, false);
        if (since >= 3) OsdText(o, BuildView.Clock(DateTime.Now - startSince), osdSmall, OsdGrey, 4, 3);
    }

    // The largest font from `max` down to `minScale` of it (same family and style) that shows all of `s` in `area`
    // (whole lines when wrapping, one line otherwise); cached per family and size.
    readonly Dictionary<string, Font> hintFonts = new Dictionary<string, Font>();
    Font FitFont(Graphics g, string s, SizeF area, Font max, float minScale, bool wrap) {
        if (string.IsNullOrEmpty(s)) return max;
        Font f = max;
        using (var sf = new StringFormat { FormatFlags = wrap ? StringFormatFlags.LineLimit : StringFormatFlags.NoWrap, Trimming = StringTrimming.EllipsisCharacter }) {
            for (int tenths = (int)(max.Size * 10); tenths >= (int)(max.Size * 10 * minScale); tenths -= 5) {
                string key = max.Name + "|" + max.Style + "|" + tenths;
                if (!hintFonts.TryGetValue(key, out f)) hintFonts[key] = f = new Font(max.FontFamily, tenths / 10f, max.Style, GraphicsUnit.Pixel);
                int chars, lines;
                g.MeasureString(s, f, area, sf, out chars, out lines);
                if (chars >= s.Length) break;
            }
        }
        return f;
    }

    // A play option's button label: the panel shows the full name, so the button drops the "Play + " / "Play with the "
    // lead ("Play + trace all" -> "Trace all").
    // Other long names get a short button label too, and a trailing " (...)" note (version names) goes to the panel only.
    static readonly Dictionary<string, string> ShortLabels = new Dictionary<string, string> {
        { "Update and build the selected version", "Update and build" },
        { "Clear builds and build caches", "Clear builds" },
        { "Send the logs now", "Send logs" },
        { "Recomp: get the newest build", "Recomp: newest build" },
        { "Recomp: get the main PC save", "Recomp: main PC save" },
    };
    static string ShortName(string name) {
        string current = name.StartsWith("> ") ? "> " : "";
        name = name.Substring(current.Length);
        string label;
        if (ShortLabels.TryGetValue(name, out label)) return current + label;
        foreach (var lead in new[] { "Play + ", "Play with the ", "Play with " })
            if (name.StartsWith(lead) && name.Length > lead.Length) return current + char.ToUpper(name[lead.Length]) + name.Substring(lead.Length + 1);
        int note = name.IndexOf(" (");
        if (note > 0 && name.EndsWith(")")) name = name.Substring(0, note);
        return current + name;
    }

}

static class SkateLauncher {
    // One launcher per folder (laptop 2026-10-19: two windows were open by accident; one held its build log open and
    // the after-session send failed). The self-update restart releases it before starting the new window.
    public static System.Threading.Mutex Instance;
    public static void ReleaseInstance() {
        if (Instance == null) return;
        try { Instance.ReleaseMutex(); } catch { }
        Instance.Dispose(); Instance = null;
    }
    [STAThread]
    static void Main(string[] argv) {
        Application.EnableVisualStyles();
        // SkateLauncher.exe --snapshot <out.png> [main|versions|tools|job|build|failed|waiting|fetching|checking] [<width>x<height>]: draws one screen (default
        // 1920x1080; the window's own size to check the windowed layout) and exits (checking the look without the window).
        // SkateLauncher.exe --bench <out.txt> [<width>x<height>]: times Render across the power-on and settled frames
        // (ms per frame, plus the hand-over to the layered window) into out.txt.
        if (argv.Length >= 2 && argv[0] == "--bench") {
            var form = new LauncherForm(new string[0]);
            int bw = 1243, bh = 777;
            if (argv.Length > 2) { var wh = argv[2].Split('x'); bw = int.Parse(wh[0]); bh = int.Parse(wh[1]); }
            File.WriteAllText(argv[1], form.Bench(bw, bh));
            return;
        }
        if (argv.Length >= 2 && argv[0] == "--snapshot") {
            var form = new LauncherForm(new string[0]);
            int sw = 1920, sh = 1080;
            if (argv.Length > 3) { var wh = argv[3].Split('x'); sw = int.Parse(wh[0]); sh = int.Parse(wh[1]); }
            form.Snapshot(argv[1], argv.Length > 2 ? argv[2] : "main", sw, sh);
            return;
        }
        string folder = Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory).TrimEnd('\\').ToLowerInvariant().Replace('\\', '/');
        bool first;
        Instance = new System.Threading.Mutex(true, "Local\\SkateLauncher-" + folder, out first);
        if (!first) {
            // Already running: bring its window to the front instead of opening a second one.
            int me = Process.GetCurrentProcess().Id;
            foreach (var name in new[] { "SkateLauncher", "SkateLauncher.old" })
                foreach (var other in Process.GetProcessesByName(name))
                    if (other.Id != me && other.MainWindowHandle != IntPtr.Zero && Native.IsWindowVisible(other.MainWindowHandle)) { Native.Front(other.MainWindowHandle); return; }
            return;
        }
        Application.Run(new LauncherForm(argv));
        ReleaseInstance();
    }
}
