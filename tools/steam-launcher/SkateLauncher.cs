// Couch launcher for Steam / Steam Link (see README.md): a full-screen window with a menu for the controller, the
// keyboard or Steam's desktop layout. launcher.ps1 does the work (versions, builds, launching, session checks) and
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
    public string Icon;   // the text the tile's glyph is picked from (null = Text)
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
    Font monoFont, titleFont, itemFont, smallFont, heroFont, markFont, markLight, playFont, iconFont, iconSmall, itemBold, smallBold, bodyFont;

    public LauncherForm(string[] argv) {
        // --after-update: started by the previous exe after it updated itself (shows what changed), not a direct entry.
        afterUpdate = Array.IndexOf(argv, "--after-update") >= 0;
        direct = Array.FindAll(argv, a => a != "--after-update");
        Text = "Skate launcher";
        // A normal window: 70 % of the screen's work area at 16:10, centred, resizable (the layout scales with it).
        var area = Screen.PrimaryScreen.WorkingArea;
        int h = Math.Min(area.Height * 7 / 10, area.Width * 7 / 10 * 10 / 16);
        StartPosition = FormStartPosition.CenterScreen; ClientSize = new Size(h * 16 / 10, h);
        MinimumSize = new Size(800, 540);
        BackColor = Color.FromArgb(18, 20, 24); ForeColor = Color.White;
        DoubleBuffered = true; KeyPreview = true;
        try { Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
        tick.Interval = 16; tick.Tick += delegate { Poll(); };
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
        playFont = new Font("Segoe UI Black", h / 26f, GraphicsUnit.Pixel);
        iconFont = new Font("Segoe MDL2 Assets", h / 26f, GraphicsUnit.Pixel);
        iconSmall = new Font("Segoe MDL2 Assets", h / 40f, GraphicsUnit.Pixel);
    }

    // Draws one screen into a PNG (the --snapshot mode): main, versions, tools or job (a sample build log).
    public void Snapshot(string file, string screen, int w, int h) {
        MakeFonts(h);
        RefreshStatus();
        if (screen == "checking") { starting = true; selfFetching = true; startSince = DateTime.Now.AddSeconds(-7); }
        else pages.Push(MainPage());
        if (screen == "versions") pages.Push(VersionPage());
        if (screen == "tools") pages.Push(EntryPage("Tools", "tools"));
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
            // Twice: the first frame starts the screen-change fade; the second draws it finished.
            using (var g = Graphics.FromImage(bmp)) { Render(g, w, h); screenAt = selAt = -10; Render(g, w, h); }
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
    static readonly string[] Prefetched = { "entries", "versions", "notes" };
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
    static string LastSession() { string f = Path.Combine(StateDir(), "last_session.txt"); return File.Exists(f) ? File.ReadAllText(f).Trim() : "No session yet."; }
    static string VerArg(string v) { return string.IsNullOrEmpty(v) ? "" : " -Version " + v; }
    static string RunningGames() {
        string r = "";
        foreach (var n in GameNames) if (Process.GetProcessesByName(n).Length > 0) r += (r == "" ? "" : ", ") + n + ".exe";
        // A Refresh Game Files run (launcher.ps1 Refresh-GameFiles) is named instead of its exe (USER-ASKS 72).
        string marker = Path.Combine(StateDir(), "REFRESHING.txt");
        if (r != "" && File.Exists(marker)) { try { r = "Refresh game files (" + File.ReadAllText(marker).Trim() + ")"; } catch { } }
        return r;
    }
    void RefreshStatus() {
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
    void Start() {
        Activate(); Native.Front(Handle);
        starting = true; startSince = DateTime.Now; selfFetching = selfFetched = skipped = skipArmed = false; Invalidate();
        Task.Run(() => RunPending(false))
            .ContinueWith(t => BeginInvoke((Action)(() => FinishStart(t.IsFaulted ? 0 : t.Result))));
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
        RefreshStatus();
        pages.Push(MainPage());
        if (St("setup_missing") != "") pages.Push(SetupPage());
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
                Run("version-set " + row[0]); RebuildMain(); pages.Pop(); }));
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
    void RebuildMain() {
        // A version change or a finished job can change every list (entries, versions, notes): fetch them again.
        ForgetCache();
        Prefetch();
        RefreshStatus();
        var all = new List<Page>(pages); all.Reverse();
        pages.Clear();
        foreach (var pg in all) {
            if (pg.Title == "Skate launcher" && pg.OnBack != null) { var m = MainPage(); m.Sel = Math.Min(pg.Sel, m.Items.Count - 1); pages.Push(m); }
            else pages.Push(pg);
        }
    }
    void ShowText(string title, string body, Action back) { ShowText(title, body, back, null); }
    void ShowText(string title, string body, Action back, string mono) {
        var p = new Page { Title = title, Body = body, Mono = mono };
        p.OnBack = back;
        pages.Push(p);
        Invalidate();
    }

    // ---------- jobs ----------
    void Begin(string entry, string label, string name) {
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
            IntPtr w = p.MainWindowHandle;
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
    void Poll() {
        if (closeAt >= 0) {
            int left = (int)Math.Ceiling((closeTime - DateTime.Now).TotalSeconds);
            if (left <= 0) { Close(); return; }
            if (left != closeAt) { closeAt = left; Invalidate(); }
        }
        if (waitEntry != null && RunningGames() == "") { string e = waitEntry; waitEntry = null; pages.Pop(); Begin(e, waitLabel, waitName); }
        WatchGame();
        if (dirty || Animating()) { dirty = false; Invalidate(); }
        // The rolling boards and the Play glow: about 30 frames a second while the menu or a job shows.
        if (Visible && ++frame % 2 == 0 && (job != null || starting || (pages.Count > 0 && IsMain(pages.Peek())))) Invalidate();
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
    protected override void OnMouseMove(MouseEventArgs e) {
        base.OnMouseMove(e);
        int i = HitAt(e.Location);
        if (job == null && i >= 0 && pages.Count > 0 && pages.Peek().Sel != i) { pages.Peek().Sel = i; Invalidate(); }
    }
    protected override void OnMouseClick(MouseEventArgs e) {
        base.OnMouseClick(e);
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
            else if (IsMain(p)) p.Sel = GridMove(p.Sel, a, p.Items.Count);
            else if (GridCols(p) > 0) p.Sel = PageGridMove(p.Sel, a, p.Items.Count, GridCols(p));
            else if (a == "up" || a == "left") p.Sel = (p.Sel - 1 + p.Items.Count) % p.Items.Count;
            else if (a == "down" || a == "right") p.Sel = (p.Sel + 1) % p.Items.Count;
        } else if (a == "ok" && p.Body != null) {
            if (p.OnBack != null) p.OnBack(); else pages.Pop();
        }
        Invalidate();
    }

    // The main page's grid: 0 = the hero's Play card, then tiles in rows of TileCols.
    static int GridMove(int sel, string a, int count) {
        if (a == "left") return Math.Max(0, sel - 1);
        if (a == "right") return Math.Min(count - 1, sel + 1);
        if (a == "down") return sel == 0 ? Math.Min(1, count - 1) : Math.Min(count - 1, sel + TileCols);
        if (a == "up") return sel <= TileCols ? 0 : sel - TileCols;
        return sel;
    }

    // A tile page's grid (Page.Cols wide): down onto a shorter last row lands on its last tile.
    static int PageGridMove(int sel, string a, int count, int cols) {
        if (a == "left") return Math.Max(0, sel - 1);
        if (a == "right") return Math.Min(count - 1, sel + 1);
        if (a == "down") return sel / cols < (count - 1) / cols ? Math.Min(count - 1, sel + cols) : sel;
        if (a == "up") return sel >= cols ? sel - cols : sel;
        return sel;
    }

    // ---------- drawing ----------
    // A dark console-launcher look (big hero card, tiles, button prompts) with the splash's skateboard as the mark.
    static readonly Color Bg1 = Color.FromArgb(16, 18, 24), Bg2 = Color.FromArgb(7, 8, 11);
    static readonly Color Panel = Color.FromArgb(24, 28, 37), PanelHi = Color.FromArgb(33, 39, 52), Stroke = Color.FromArgb(44, 51, 67);
    static readonly Color Ink = Color.FromArgb(242, 244, 248), Dim = Color.FromArgb(138, 147, 166), Faint = Color.FromArgb(78, 85, 100);
    static readonly Color Accent = Color.FromArgb(255, 196, 0), Cool = Color.FromArgb(86, 146, 214);
    readonly Stopwatch clock = Stopwatch.StartNew();
    DateTime jobStart;

    protected override void OnPaint(PaintEventArgs ev) { Render(ev.Graphics, ClientSize.Width, ClientSize.Height); }

    // Screen and selection animations (seconds on `clock`): short and eased out, so input never waits on them.
    string screenKey = ""; int screenDepth, screenDir = 1, selIndex = -1; float screenAt = -1, selAt = -1, selEase = 1;
    static float Ease(float x) { x = Math.Max(0, Math.Min(1, x)); float r = 1 - x; return 1 - r * r * r; }
    bool Animating() { float t = (float)clock.Elapsed.TotalSeconds; return t - screenAt < 0.2f || t - selAt < 0.14f; }
    // The incoming screen fades up from the backdrop (the top bar stays).
    static void Veil(Graphics g, int w, int h, float barH, float ease) {
        if (ease >= 1) return;
        var r = new RectangleF(0, barH + 1, w, h - barH - 1);
        int a = (int)(255 * (1 - ease));
        using (var b = new System.Drawing.Drawing2D.LinearGradientBrush(new RectangleF(0, 0, w, h), Fade(Bg1, a), Fade(Bg2, a), 90f)) g.FillRectangle(b, r);
    }

    // The main page: item 0 is the hero's Play card, the rest tiles in rows of TileCols.
    const int TileCols = 4;
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

    // Windows' own icon font (Segoe MDL2 Assets) per menu entry.
    static string Glyph(string text) {
        string t = text.ToLowerInvariant();
        if (t.StartsWith("play")) return "";
        if (t.StartsWith("versions")) return "";
        if (t.StartsWith("what to test")) return "";
        if (t.StartsWith("recomp")) return "";
        if (t.StartsWith("rpcs3")) return "";
        if (t.StartsWith("tools")) return "";
        if (t.StartsWith("last session")) return "";
        if (t.StartsWith("exit")) return "";
        return "";
    }

    // A, B, X / Y badges like the controller's.
    void Prompt(Graphics g, ref float x, float y, string key, Color c, string label) {
        float d = smallFont.Height * 1.25f;
        using (var b = new SolidBrush(c)) g.FillEllipse(b, x, y, d, d);
        Write(g, key, smallBold, Color.FromArgb(16, 18, 24), new RectangleF(x, y, d, d), StringAlignment.Center, StringAlignment.Center, false);
        x += d + d * 0.35f;
        var size = g.MeasureString(label, smallFont);
        Write(g, label, smallFont, Dim, new RectangleF(x, y, size.Width + 4, d), StringAlignment.Near, StringAlignment.Center, false);
        x += size.Width + d;
    }

    // Log lines as a TV can read them: no colour codes, only the text after the last carriage return.
    static readonly System.Text.RegularExpressions.Regex Ansi = new System.Text.RegularExpressions.Regex("\x1B\\[[0-9;?]*[A-Za-z]");
    static string Clean(string s) { s = Ansi.Replace(s ?? "", ""); int r = s.LastIndexOf('\r'); return (r >= 0 ? s.Substring(r + 1) : s).Trim(); }

    void Render(Graphics g, int w, int h) {
        g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
        hits.Clear();
        var all = new RectangleF(0, 0, w, h);
        using (var b = new System.Drawing.Drawing2D.LinearGradientBrush(all, Bg1, Bg2, 90f)) g.FillRectangle(b, all);
        // A soft glow behind the top left, like a lit backdrop.
        using (var path = new System.Drawing.Drawing2D.GraphicsPath()) {
            path.AddEllipse(-w * 0.25f, -h * 0.55f, w * 0.9f, h * 1.1f);
            using (var pg = new System.Drawing.Drawing2D.PathGradientBrush(path)) {
                pg.CenterColor = Color.FromArgb(40, Cool); pg.SurroundColors = new[] { Color.FromArgb(0, Cool) };
                g.FillPath(pg, path);
            }
        }
        float m = w * 0.045f, t = (float)clock.Elapsed.TotalSeconds;
        float barH = h * 0.085f;
        // Top bar: the board as the mark, the wordmark, the version chip.
        Board(g, m, barH * 0.32f, barH * 1.3f);
        float wx = m + barH * 1.45f;
        var skate = g.MeasureString("SKATE", markFont);
        Write(g, "SKATE", markFont, Ink, new RectangleF(wx, 0, skate.Width + 8, barH), StringAlignment.Near, StringAlignment.Center, false);
        Write(g, "LAUNCHER", markLight, Dim, new RectangleF(wx + skate.Width * 0.92f, 0, w * 0.3f, barH), StringAlignment.Near, StringAlignment.Center, false);
        // The launcher's own commit, and the fork's newer one when it did not update itself (USER-ASKS 116).
        if (St("launcher") != "") {
            float lx = wx + skate.Width * 0.92f + g.MeasureString("LAUNCHER", markLight).Width + smallFont.Height * 0.4f;
            string lv = St("launcher") + (St("launcher_newer") != "" ? "   newer on the fork: " + St("launcher_newer") + " (Tools: Update the launcher)" : "");
            Write(g, lv, smallFont, St("launcher_newer") != "" ? Accent : Faint, new RectangleF(lx, 0, w * 0.45f, barH), StringAlignment.Near, StringAlignment.Center, false);
        }
        string chip = St("version") == "" ? "" : St("version");
        if (chip != "") {
            var cs = g.MeasureString(chip, smallFont);
            var cr = new RectangleF(w - m - cs.Width - smallFont.Height * 1.6f, barH * 0.28f, cs.Width + smallFont.Height * 1.6f, barH * 0.44f);
            Card(g, cr, cr.Height / 2, PanelHi, Panel, Stroke, 1);
            using (var dot = new SolidBrush(Accent)) g.FillEllipse(dot, cr.X + cr.Height * 0.35f, cr.Y + cr.Height * 0.38f, cr.Height * 0.24f, cr.Height * 0.24f);
            Write(g, chip, smallFont, Ink, new RectangleF(cr.X + cr.Height * 0.7f, cr.Y, cr.Width - cr.Height * 0.7f, cr.Height), StringAlignment.Near, StringAlignment.Center, false);
        }
        float top = barH + h * 0.02f, bottom = h - h * 0.085f;
        using (var pen = new Pen(Fade(Stroke, 160), 1)) g.DrawLine(pen, m, barH, w - m, barH);

        // A screen change slides the new screen in a little and fades it up (user 2026-10-19: switching screens was
        // "a little jarring"); a main-page refresh keeps its depth and title, so it does not animate.
        string key = job != null ? "job" : pages.Count + "|" + (pages.Count > 0 ? pages.Peek().Title : "");
        if (key != screenKey) {
            int depth = job != null ? pages.Count + 1 : pages.Count;
            screenDir = depth >= screenDepth ? 1 : -1;
            screenKey = key; screenDepth = depth; screenAt = t;
        }
        float ease = Ease((t - screenAt) / 0.18f);
        var content = g.Save();
        g.TranslateTransform(screenDir * (1 - ease) * w * 0.03f, 0);
        if (job != null) { RenderJob(g, w, h, m, top, bottom, t); g.Restore(content); Veil(g, w, h, barH, ease); return; }
        if (pages.Count == 0) {
            if (starting) {
                var el = DateTime.Now - startSince;
                string text = (skipped ? "Skipped the update check; finishing the start..." : "Checking for launcher updates...") + (el.TotalSeconds >= 3 ? " " + BuildView.Clock(el) : "");
                Write(g, text, itemFont, Dim, new RectangleF(m, top, w - 2 * m, bottom - top), StringAlignment.Center, StringAlignment.Center, false);
                if (CanSkip()) Write(g, skipArmed ? "B again to skip it" : "B: skip the update check", smallFont, Dim, new RectangleF(m, (top + bottom) / 2 + itemFont.Height, w - 2 * m, smallFont.Height * 1.4f), StringAlignment.Center, StringAlignment.Near, false);
            }
            g.Restore(content); return;
        }
        var p = pages.Peek();
        if (p.Sel != selIndex) { selIndex = p.Sel; selAt = t; }
        selEase = Ease((t - selAt) / 0.12f);
        if (IsMain(p)) RenderMain(g, p, w, h, m, top, bottom, t); else RenderPage(g, p, w, h, m, top, bottom);
        g.Restore(content);
        Veil(g, w, h, barH, ease);
        // Footer: the selected entry's hint on the left, the button prompts on the right.
        float fy = bottom + (h - bottom - smallFont.Height * 1.25f) / 2;
        string hint = p.Items.Count > 0 ? p.Items[p.Sel].Hint : "";
        if (closeAt >= 0) hint = "Closing in " + closeAt + " s (any button closes now)";
        if (IsMain(p) || closeAt >= 0) Write(g, hint, smallFont, Dim, new RectangleF(m, fy, w * 0.55f, smallFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Center, false);
        float px = w - m - w * 0.2f;
        if (p.Items.Count > 0) Prompt(g, ref px, fy, "A", Color.FromArgb(108, 196, 74), "Select");
        Prompt(g, ref px, fy, "B", Color.FromArgb(228, 76, 70), "Back");
    }

    void RenderMain(Graphics g, Page p, int w, int h, float m, float top, float bottom, float t) {
        // Hero card: the selected version, the rolling board, the Play card.
        var hero = new RectangleF(m, top, w - 2 * m, (bottom - top) * 0.47f);
        Card(g, hero, h * 0.022f, Color.FromArgb(30, 38, 56), Color.FromArgb(17, 20, 28), Stroke, 1);
        var clip = g.Save();
        using (var path = Round(hero, h * 0.022f)) g.SetClip(path);
        float boardLen = hero.Height * 0.95f, span = hero.Width + boardLen * 0.55f;
        float bx = hero.X - boardLen + (hero.Width * 0.42f + t * hero.Width * 0.05f) % span;
        using (var path = new System.Drawing.Drawing2D.GraphicsPath()) {
            path.AddEllipse(hero.Right - hero.Width * 0.5f, hero.Y - hero.Height * 0.4f, hero.Width * 0.7f, hero.Height * 1.6f);
            using (var pg = new System.Drawing.Drawing2D.PathGradientBrush(path)) {
                pg.CenterColor = Color.FromArgb(34, Accent); pg.SurroundColors = new[] { Color.FromArgb(0, Accent) };
                g.FillPath(pg, path);
            }
        }
        // The ground line the board rolls on.
        float ground = hero.Bottom - hero.Height * 0.1f;
        using (var gp = new Pen(Color.FromArgb(40, 255, 255, 255), 1)) g.DrawLine(gp, hero.X, ground, hero.Right, ground);
        // The wheels' bottom is 34 of the board's 180 units below its top.
        Board(g, bx, ground - boardLen * 0.55f * 34f / 180f, boardLen * 0.55f);
        g.Restore(clip);
        float pad = hero.Height * 0.12f;
        Write(g, "NOW SELECTED", smallBold, Accent, new RectangleF(hero.X + pad, hero.Y + pad, hero.Width * 0.6f, smallFont.Height * 1.2f), StringAlignment.Near, StringAlignment.Near, false);
        // The version name shrinks (to 60 %) before it is cut, so a long name stays readable in the window.
        var vr = new RectangleF(hero.X + pad, hero.Y + pad + smallFont.Height * 1.2f, hero.Width * 0.62f, heroFont.Height * 1.2f);
        var vf = FitFont(g, St("version"), vr.Size, heroFont, 0.6f, false);
        Write(g, St("version"), vf, Ink, new RectangleF(vr.X, vr.Y + (heroFont.Height - vf.Height) / 2, vr.Width, vr.Height), StringAlignment.Near, StringAlignment.Near, false);
        float sy = hero.Y + pad + smallFont.Height * 1.3f + heroFont.Height * 1.15f;
        Write(g, St("version_status"), itemFont, Dim, new RectangleF(hero.X + pad, sy, hero.Width * 0.6f, itemFont.Height * 1.2f), StringAlignment.Near, StringAlignment.Near, false);
        string send = St("send");
        Write(g, "Last session: " + St("last") + (send == "" || send == "off" ? "" : "     Send: " + send), smallFont, Faint, new RectangleF(hero.X + pad, sy + itemFont.Height * 1.3f, hero.Width * 0.6f, smallFont.Height * 2.4f), StringAlignment.Near, StringAlignment.Near, true);
        // The Play card (item 0) on the right of the hero.
        if (p.Items.Count > 0) {
            bool sel = p.Sel == 0;
            // Above the board's lane (ground line at 0.9 of the hero, the board about 0.1 of it tall), so the rolling
            // board passes under the card instead of behind it (user 2026-10-18).
            var pr = new RectangleF(hero.Right - pad - hero.Width * 0.26f, ground - hero.Height * 0.14f - hero.Height * 0.32f, hero.Width * 0.26f, hero.Height * 0.32f);
            hits.Add(new KeyValuePair<RectangleF, int>(pr, 0));
            if (sel) using (var glow = Round(new RectangleF(pr.X - 6, pr.Y - 6, pr.Width + 12, pr.Height + 12), pr.Height * 0.3f)) using (var gb = new SolidBrush(Fade(Accent, 60 + (int)(30 * Math.Sin(t * 3))))) g.FillPath(gb, glow);
            Card(g, pr, pr.Height * 0.24f, sel ? Color.FromArgb(255, 210, 60) : Color.FromArgb(232, 178, 0), sel ? Accent : Color.FromArgb(200, 150, 0), Color.Empty, 0);
            var ink = Color.FromArgb(20, 18, 10);
            Write(g, "", iconFont, ink, new RectangleF(pr.X + pr.Height * 0.25f, pr.Y, pr.Height * 0.6f, pr.Height), StringAlignment.Near, StringAlignment.Center, false);
            Write(g, "PLAY", playFont, ink, new RectangleF(pr.X + pr.Height * 0.85f, pr.Y, pr.Width - pr.Height, pr.Height), StringAlignment.Near, StringAlignment.Center, false);
        }
        // Tiles for everything else.
        float gy = hero.Bottom + h * 0.03f, gap = w * 0.012f;
        int n = p.Items.Count - 1, rows = Math.Max(1, (n + TileCols - 1) / TileCols);
        float tw = (w - 2 * m - gap * (TileCols - 1)) / TileCols, th = Math.Min((bottom - gy - gap * (rows - 1)) / rows, h * 0.15f);
        var hint = PageHintFont(g, p, 1, new RectangleF(0, 0, tw, th), true);
        for (int i = 1; i < p.Items.Count; i++) {
            var it = p.Items[i]; bool sel = i == p.Sel;
            int k = i - 1; var r = new RectangleF(m + (k % TileCols) * (tw + gap), gy + (k / TileCols) * (th + gap), tw, th);
            Tile(g, it, sel, r, true, hint);
            hits.Add(new KeyValuePair<RectangleF, int>(r, i));
        }
    }

    void Tile(Graphics g, Item it, bool sel, RectangleF r, bool icon, Font hint) {
        float rad = r.Height * 0.16f;
        // The selection ring grows out over a few frames instead of snapping on.
        float ring = 1 + 2 * selEase;
        if (sel) Card(g, new RectangleF(r.X - ring, r.Y - ring, r.Width + 2 * ring, r.Height + 2 * ring), rad + ring, it.On ? Accent : Faint, it.On ? Accent : Faint, Color.Empty, 0);
        Card(g, r, rad, sel ? PanelHi : Panel, sel ? Panel : Color.FromArgb(20, 23, 31), sel ? Color.Empty : Stroke, sel ? 0 : 1);
        var ink = it.On ? Ink : Faint;
        float pad = r.Height * 0.2f, tx = r.X + pad;
        if (icon) {
            var ib = new RectangleF(r.X + pad, r.Y + (r.Height - r.Height * 0.5f) / 2, r.Height * 0.5f, r.Height * 0.5f);
            Card(g, ib, ib.Height * 0.28f, Fade(it.On ? (sel ? Accent : Cool) : Faint, 50), Fade(it.On ? (sel ? Accent : Cool) : Faint, 25), Color.Empty, 0);
            Write(g, Glyph(it.Icon ?? it.Text), iconSmall, it.On ? (sel ? Accent : Ink) : Faint, ib, StringAlignment.Center, StringAlignment.Center, false);
            tx = ib.Right + pad * 0.8f;
        }
        float tw = r.Right - tx - pad;
        string title = it.Text;
        if (title.StartsWith("> ")) {
            // The current choice (the version list marks it with "> "): an accent pill instead of the marker.
            title = title.Substring(2);
            var ps = g.MeasureString("CURRENT", smallBold);
            var pill = new RectangleF(r.Right - pad - ps.Width - smallBold.Height * 0.9f, r.Y + r.Height * 0.16f + (itemBold.Height - smallBold.Height * 1.3f) / 2, ps.Width + smallBold.Height * 0.9f, smallBold.Height * 1.3f);
            Card(g, pill, pill.Height / 2, Accent, Accent, Color.Empty, 0);
            Write(g, "CURRENT", smallBold, Color.FromArgb(20, 18, 10), pill, StringAlignment.Center, StringAlignment.Center, false);
            tw = pill.X - tx - pad * 0.5f;
        }
        float ty = r.Y + r.Height * 0.16f, hy = ty + itemBold.Height * 1.15f;
        if (it.Hint == "") {
            // No hint (the play screen's buttons; its panel has the text): the name wraps over two lines, centred.
            Write(g, title, itemBold, ink, new RectangleF(tx, r.Y, tw, r.Height), StringAlignment.Near, StringAlignment.Center, true);
            return;
        }
        Write(g, title, itemBold, ink, new RectangleF(tx, ty, tw, itemBold.Height * 1.15f), StringAlignment.Near, StringAlignment.Near, false);
        // The hint in the space left under the title, in the page's hint font (PageHintFont).
        Write(g, it.Hint, hint, it.On ? Dim : Faint, HintArea(r, icon), StringAlignment.Near, StringAlignment.Near, true);
    }

    // Where Tile draws the hint: under the title, right of the icon.
    RectangleF HintArea(RectangleF r, bool icon) {
        float pad = r.Height * 0.2f, tx = icon ? r.X + pad + r.Height * 0.5f + pad * 0.8f : r.X + pad;
        float hy = r.Y + r.Height * 0.16f + itemBold.Height * 1.15f;
        return new RectangleF(tx, hy, r.Right - tx - pad, r.Bottom - hy - pad * 0.3f);
    }

    // One hint font per page, small enough for its longest hint to show whole (user 2026-10-19: in the window the hints
    // were cut after a few words). Items from `from` on are tiles of `tile`'s size.
    Font PageHintFont(Graphics g, Page p, int from, RectangleF tile, bool icon) {
        var area = HintArea(tile, icon).Size;
        Font f = smallFont;
        for (int i = from; i < p.Items.Count; i++) {
            var x = HintFont(g, p.Items[i].Hint, area);
            if (x.Size < f.Size) f = x;
        }
        return f;
    }

    Font HintFont(Graphics g, string s, SizeF area) { return FitFont(g, s, area, smallFont, 0.7f, true); }

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

    void RenderPage(Graphics g, Page p, int w, int h, float m, float top, float bottom) {
        Write(g, p.Title, titleFont, Ink, new RectangleF(m, top, w - 2 * m, titleFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
        float y = top + titleFont.Height * 1.5f;
        if (p.Body != null) {
            float bodyH = p.Items.Count > 0 ? (bottom - y) * 0.45f : bottom - y - h * 0.01f;
            var br = new RectangleF(m, y, w - 2 * m, bodyH);
            Card(g, br, h * 0.016f, Panel, Color.FromArgb(19, 22, 30), Stroke, 1);
            float pad = h * 0.025f;
            float bodyText = br.Height - 2 * pad;
            if (p.Mono != null) bodyText = Math.Min(bodyText, g.MeasureString(p.Body, smallFont, (int)(br.Width - 2 * pad)).Height + smallFont.Height * 0.5f);
            Write(g, p.Body, smallFont, Color.FromArgb(214, 218, 226), new RectangleF(br.X + pad, br.Y + pad, br.Width - 2 * pad, bodyText), StringAlignment.Near, StringAlignment.Near, true);
            if (p.Mono != null) {
                // A failed build's first error, fixed width so the ^^^ marks stay under the code.
                var eb = new RectangleF(br.X + pad, br.Y + pad + bodyText, br.Width - 2 * pad, br.Height - 2 * pad - bodyText);
                Card(g, eb, h * 0.01f, Color.FromArgb(40, 20, 22), Color.FromArgb(26, 14, 16), Color.FromArgb(120, 228, 76, 70), 1);
                Write(g, p.Mono, monoFont, Color.FromArgb(236, 226, 226), new RectangleF(eb.X + pad * 0.6f, eb.Y + pad * 0.6f, eb.Width - 1.2f * pad, eb.Height - 1.2f * pad), StringAlignment.Near, StringAlignment.Near, true);
            }
            y = br.Bottom + h * 0.025f;
        }
        if (p.Items.Count == 0) return;
        int cols = GridCols(p);
        if (cols > 0) {
            // Buttons like the main screen's tiles, cols wide and as many rows as needed (rows scroll to keep the selected
            // one), on the left; a large panel on the right shows the selected option: what it does and, for the play
            // options, what it records (user 2026-10-21).
            float tgap = w * 0.012f, gridW = (w - 2 * m) * 0.48f;
            float tw = (gridW - tgap * (cols - 1)) / cols, th = h * 0.105f;
            RenderDetail(g, p.Items[p.Sel], new RectangleF(m + gridW + tgap * 2, y, w - 2 * m - gridW - tgap * 2, bottom - y), h);
            int rows = (p.Items.Count + cols - 1) / cols;
            int fitRows = Math.Max(1, (int)((bottom - y + tgap) / (th + tgap)));
            int firstRow = Math.Max(0, Math.Min(p.Sel / cols - fitRows / 2, rows - fitRows));
            var thint = PageHintFont(g, p, 0, new RectangleF(0, 0, tw, th), true);
            for (int i = firstRow * cols; i < p.Items.Count && i < (firstRow + fitRows) * cols; i++) {
                int k = i - firstRow * cols;
                var r = new RectangleF(m + (k % cols) * (tw + tgap), y + (k / cols) * (th + tgap), tw, th);
                var shown = new Item(ShortName(p.Items[i].Text), "", p.Items[i].On, null) { Icon = p.Items[i].Text };
                Tile(g, shown, i == p.Sel, r, true, thint);
                hits.Add(new KeyValuePair<RectangleF, int>(r, i));
            }
            if (rows > fitRows)
                Write(g, (p.Sel + 1) + " / " + p.Items.Count, smallFont, Faint, new RectangleF(m, top, w - 2 * m, titleFont.Height * 1.3f), StringAlignment.Far, StringAlignment.Center, false);
            return;
        }
        // A scrolling list of cards: the selected one stays in view.
        float ch = h * 0.095f, gap = h * 0.014f;
        int fit = Math.Max(1, (int)((bottom - y + gap) / (ch + gap)));
        int first = Math.Max(0, Math.Min(p.Sel - fit / 2, p.Items.Count - fit));
        var hint = PageHintFont(g, p, 0, new RectangleF(0, 0, w - 2 * m, ch), false);
        for (int i = first; i < p.Items.Count && i < first + fit; i++) {
            var r = new RectangleF(m, y, w - 2 * m, ch);
            Tile(g, p.Items[i], i == p.Sel, r, false, hint);
            hits.Add(new KeyValuePair<RectangleF, int>(r, i));
            y += ch + gap;
        }
        if (first > 0 || first + fit < p.Items.Count)
            Write(g, (p.Sel + 1) + " / " + p.Items.Count, smallFont, Faint, new RectangleF(m, top, w - 2 * m, titleFont.Height * 1.3f), StringAlignment.Far, StringAlignment.Center, false);
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

    // The play screen's detail panel: the selected option's name with a status pill, what it does, and what it records.
    // Records come from launcher.ps1 as rows "name: what it is"; "#TITLE: folder" rows start a section.
    void RenderDetail(Graphics g, Item it, RectangleF r, int h) {
        Card(g, r, h * 0.02f, Color.FromArgb(26, 31, 43), Color.FromArgb(18, 21, 29), Stroke, 1);
        float pad = h * 0.04f, x = r.X + pad, cw = r.Width - 2 * pad, y = r.Y + pad * 0.9f;
        string name = it.Text.StartsWith("> ") ? it.Text.Substring(2) : it.Text;
        float nh = Math.Min(g.MeasureString(name, titleFont, (int)cw).Height, titleFont.Height * 2.5f);
        Write(g, name, titleFont, Ink, new RectangleF(x, y, cw, nh), StringAlignment.Near, StringAlignment.Near, true);
        y += nh + titleFont.Height * 0.25f;
        // Status pill: READY, or NOT AVAILABLE with the reason beside it.
        string pill = it.On ? "READY" : "NOT AVAILABLE";
        var ps = g.MeasureString(pill, smallBold);
        var pr = new RectangleF(x, y, ps.Width + smallBold.Height * 0.9f, smallBold.Height * 1.4f);
        Color pc = it.On ? Color.FromArgb(108, 196, 74) : Color.FromArgb(228, 76, 70);
        Card(g, pr, pr.Height / 2, Fade(pc, 40), Fade(pc, 40), Fade(pc, 140), 1);
        Write(g, pill, smallBold, pc, pr, StringAlignment.Center, StringAlignment.Center, false);
        if (!it.On) Write(g, it.Hint, smallFont, Dim, new RectangleF(pr.Right + pad * 0.4f, y, x + cw - pr.Right - pad * 0.4f, pr.Height), StringAlignment.Near, StringAlignment.Center, false);
        y += pr.Height + h * 0.028f;
        string info = it.Info != "" ? it.Info : it.Hint;
        if (info != "") {
            float ih = Math.Min(g.MeasureString(info, bodyFont, (int)cw).Height, r.Bottom - pad - y);
            Write(g, info, bodyFont, Color.FromArgb(200, 205, 215), new RectangleF(x, y, cw, ih), StringAlignment.Near, StringAlignment.Near, true);
            y += ih + h * 0.03f;
        }
        // Sections of "name  what it is" rows with hairlines between them.
        float nameW = cw * 0.36f, dx = x + nameW + pad * 0.5f, dw = x + cw - dx, rowGap = h * 0.008f;
        using (var line = new Pen(Color.FromArgb(38, 255, 255, 255), 1)) {
            foreach (var rec in it.Records) {
                int cut = rec.IndexOf(": ");
                string a = cut > 0 ? rec.Substring(0, cut) : rec, b = cut > 0 ? rec.Substring(cut + 2) : "";
                if (a.StartsWith("#")) {
                    // Section header: TITLE in accent caps, its folder after it.
                    if (y + smallBold.Height * 2.4f > r.Bottom - pad) return;
                    y += h * 0.008f;
                    string head = a.Substring(1).ToUpperInvariant();
                    var hs = g.MeasureString(head, smallBold);
                    Write(g, head, smallBold, Accent, new RectangleF(x, y, hs.Width + 4, smallBold.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
                    if (b != "") Write(g, b, monoFont, Faint, new RectangleF(x + hs.Width + pad * 0.3f, y, cw - hs.Width - pad * 0.3f, smallBold.Height * 1.3f), StringAlignment.Near, StringAlignment.Center, false);
                    y += smallBold.Height * 1.5f;
                    continue;
                }
                float rh = Math.Max(g.MeasureString(a, monoFont, (int)nameW).Height, g.MeasureString(b, smallFont, (int)dw).Height);
                if (y + rh + rowGap > r.Bottom - pad) { Write(g, "...", smallFont, Faint, new RectangleF(x, y, cw, smallFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false); return; }
                g.DrawLine(line, x, y, x + cw, y);
                y += rowGap;
                Write(g, a, monoFont, Ink, new RectangleF(x, y, nameW, rh), StringAlignment.Near, StringAlignment.Near, true);
                Write(g, b, smallFont, Dim, new RectangleF(dx, y, dw, rh), StringAlignment.Near, StringAlignment.Near, true);
                y += rh + rowGap;
            }
        }
    }

    // A build: the version and commit, cargo's own step count as a progress bar with the board riding its head, the
    // crate being built, the warning count and, as soon as it appears, the first error.
    void RenderBuild(Graphics g, int w, int h, RectangleF card, float pad, float t) {
        Write(g, build.Waiting != null && !build.Ended ? "WAITING" : "BUILDING", smallBold, Accent, new RectangleF(card.X + pad, card.Y + pad, card.Width - 2 * pad, smallFont.Height * 1.2f), StringAlignment.Near, StringAlignment.Near, false);
        Write(g, BuildView.Clock(DateTime.Now - build.Start), smallBold, Dim, new RectangleF(card.X + pad, card.Y + pad, card.Width - 2 * pad, smallFont.Height * 1.2f), StringAlignment.Far, StringAlignment.Near, false);
        float y = card.Y + pad + smallFont.Height * 1.3f;
        Write(g, build.Name, heroFont, Ink, new RectangleF(card.X + pad, y, card.Width - 2 * pad, heroFont.Height * 1.25f), StringAlignment.Near, StringAlignment.Near, false);
        y += heroFont.Height * 1.2f;
        Write(g, "at " + build.Commit, itemFont, Dim, new RectangleF(card.X + pad, y, card.Width - 2 * pad, itemFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
        y += itemFont.Height * 2.2f;
        float boardLen = card.Width * 0.12f;
        var track = new RectangleF(card.X + pad, y + boardLen * 0.26f, card.Width - 2 * pad, h * 0.01f);
        Card(g, track, track.Height / 2, Stroke, Stroke, Color.Empty, 0);
        float head;
        if (build.Total > 0) {
            float k = Math.Min(1f, build.Done / (float)build.Total);
            var fill = new RectangleF(track.X, track.Y, Math.Max(track.Height, track.Width * k), track.Height);
            Card(g, fill, track.Height / 2, build.Ended && !build.Ok ? Color.FromArgb(228, 76, 70) : Accent, build.Ended && !build.Ok ? Color.FromArgb(228, 76, 70) : Accent, Color.Empty, 0);
            head = fill.Right;
        } else {
            // No count yet (cargo prints its bar once the build plan is known): a sweeping segment.
            float run = (t * 0.35f) % 1f, sx = track.X + run * track.Width * 1.3f - track.Width * 0.3f;
            var seg = RectangleF.Intersect(track, new RectangleF(sx, track.Y, track.Width * 0.3f, track.Height));
            if (seg.Width > 0) Card(g, seg, track.Height / 2, Accent, Accent, Color.Empty, 0);
            head = seg.Width > 0 ? seg.Right : track.X;
        }
        var clip = g.Save(); g.SetClip(card);
        Board(g, Rolling(track, boardLen, t), track.Y - boardLen * 0.21f, boardLen);
        g.Restore(clip);
        y = track.Bottom + h * 0.03f;
        string count = build.Total > 0 ? build.Done + " / " + build.Total + " build steps" : build.Compiled + " crates compiled";
        if (!build.Ended && build.Waiting != null) count = "Waiting " + BuildView.Clock(DateTime.Now - build.WaitSince) + ": " + build.Waiting;
        else if (!build.Ended && build.Total == 0 && build.Compiled == 0 && build.StageText != null) count = build.StageText;
        if (build.Ended) count = build.Ok ? "Built in " + BuildView.Clock(build.End - build.Start) : "Build failed";
        Write(g, count, itemBold, build.Ended && !build.Ok ? Color.FromArgb(255, 120, 110) : Ink, new RectangleF(card.X + pad, y, build.Warnings > 0 ? card.Width * 0.5f : card.Width - 2 * pad, itemBold.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
        if (build.Warnings > 0) {
            string wtext = build.Warnings + (build.Warnings == 1 ? " warning" : " warnings");
            var ws = g.MeasureString(wtext, smallBold);
            var pill = new RectangleF(card.Right - pad - ws.Width - smallBold.Height, y + (itemBold.Height - smallBold.Height * 1.4f) / 2, ws.Width + smallBold.Height, smallBold.Height * 1.4f);
            Card(g, pill, pill.Height / 2, Fade(Accent, 40), Fade(Accent, 40), Fade(Accent, 120), 1);
            Write(g, wtext, smallBold, Accent, pill, StringAlignment.Center, StringAlignment.Center, false);
        }
        y += itemBold.Height * 1.4f;
        if (!build.Ended && build.Waiting != null) {
            // Who holds the folder (when launcher.ps1 knew) and how to stop waiting.
            string why = (build.WaitWho ?? "Cargo does not say who; it continues by itself once that build ends.") + "\n" + (cancelArmed ? "B again to cancel this build" : "B: cancel");
            Write(g, why, smallFont, Dim, new RectangleF(card.X + pad, y, card.Width - 2 * pad, smallFont.Height * 2.6f), StringAlignment.Near, StringAlignment.Near, true);
        }
        else if (build.Now != "" && !build.Ended) Write(g, "Now: " + build.Now, smallFont, Dim, new RectangleF(card.X + pad, y, card.Width - 2 * pad, smallFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
        y += smallFont.Height * 2f;
        if (build.Error.Count > 0) {
            var eb = new RectangleF(card.X + pad, y, card.Width - 2 * pad, card.Bottom - pad - y);
            Card(g, eb, h * 0.012f, Color.FromArgb(40, 20, 22), Color.FromArgb(26, 14, 16), Color.FromArgb(120, 228, 76, 70), 1);
            float ep = h * 0.016f;
            Write(g, "First error", smallBold, Color.FromArgb(255, 120, 110), new RectangleF(eb.X + ep, eb.Y + ep, eb.Width - 2 * ep, smallBold.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
            Write(g, string.Join("\n", build.Error), monoFont, Color.FromArgb(236, 226, 226), new RectangleF(eb.X + ep, eb.Y + ep + smallBold.Height * 1.5f, eb.Width - 2 * ep, eb.Height - 2 * ep - smallBold.Height * 1.5f), StringAlignment.Near, StringAlignment.Near, true);
        }
    }

    void RenderJob(Graphics g, int w, int h, float m, float top, float bottom, float t) {        var card = new RectangleF(w * 0.12f, top + (bottom - top) * 0.03f, w * 0.76f, (bottom - top) * 0.94f);
        Card(g, card, h * 0.022f, Color.FromArgb(28, 34, 48), Color.FromArgb(16, 19, 26), Stroke, 1);
        float pad = card.Height * 0.07f;
        if (build.Active || build.Ended) { RenderBuild(g, w, h, card, pad, t); return; }
        bool fetchWait = build.FetchWaiting();
        Write(g, fetchWait ? "WAITING" : "RUNNING", smallBold, Accent, new RectangleF(card.X + pad, card.Y + pad, card.Width - 2 * pad, smallFont.Height * 1.2f), StringAlignment.Near, StringAlignment.Near, false);
        var el = DateTime.Now - (fetchWait ? build.PreWaitSince : jobStart);
        Write(g, ((int)el.TotalMinutes) + ":" + el.Seconds.ToString("00"), smallBold, Dim, new RectangleF(card.X + pad, card.Y + pad, card.Width - 2 * pad, smallFont.Height * 1.2f), StringAlignment.Far, StringAlignment.Near, false);
        Write(g, jobTitle, heroFont, Ink, new RectangleF(card.X + pad, card.Y + pad + smallFont.Height * 1.3f, card.Width - 2 * pad, heroFont.Height * 1.25f), StringAlignment.Near, StringAlignment.Near, false);
        // The board rolling along the progress track.
        float ty = card.Y + pad + smallFont.Height * 1.4f + heroFont.Height * 1.5f, boardLen = card.Width * 0.16f;
        var track = new RectangleF(card.X + pad, ty + boardLen * 0.26f, card.Width - 2 * pad, h * 0.008f);
        Card(g, track, track.Height / 2, Stroke, Stroke, Color.Empty, 0);
        float run = (t * 0.35f) % 1f;
        float sx = track.X + run * (track.Width + track.Width * 0.3f) - track.Width * 0.3f;
        var seg = RectangleF.Intersect(track, new RectangleF(sx, track.Y, track.Width * 0.3f, track.Height));
        if (seg.Width > 0) Card(g, seg, track.Height / 2, Accent, Accent, Color.Empty, 0);
        var clip = g.Save(); g.SetClip(card);
        Board(g, Rolling(track, boardLen, t), track.Y - boardLen * 0.21f, boardLen);
        g.Restore(clip);
        // The last line as the status, a few earlier ones below it.
        var lines = new List<string>();
        for (int i = log.Count - 1; i >= 0 && lines.Count < 9; i--) { string c = Clean(log[i]); if (c != "") lines.Add(c); }
        float ly = track.Bottom + boardLen * 0.45f;
        string status = fetchWait ? "Waiting " + BuildView.Clock(el) + ": " + build.PreWait : lines.Count > 0 ? lines[0] : "Starting...";
        Write(g, status, itemFont, Ink, new RectangleF(card.X + pad, ly, card.Width - 2 * pad, itemFont.Height * 1.3f), StringAlignment.Near, StringAlignment.Near, false);
        ly += itemFont.Height * 1.6f;
        if (fetchWait) {
            string why = "git stops by itself if nothing arrives for 30 s; cancelling keeps the last fetched version.\n" + (cancelArmed ? "B again to cancel" : "B: cancel");
            Write(g, why, smallFont, Dim, new RectangleF(card.X + pad, ly, card.Width - 2 * pad, smallFont.Height * 2.6f), StringAlignment.Near, StringAlignment.Near, true);
            ly += smallFont.Height * 2.8f;
        }
        var logBox = new RectangleF(card.X + pad, ly, card.Width - 2 * pad, card.Bottom - pad - ly);
        Card(g, logBox, h * 0.012f, Color.FromArgb(12, 14, 19), Color.FromArgb(12, 14, 19), Fade(Stroke, 140), 1);
        float lh = smallFont.Height * 1.15f, lp = h * 0.015f, yy = logBox.Bottom - lp - lh;
        for (int i = 1; i < lines.Count && yy >= logBox.Y + lp; i++) {
            Write(g, lines[i], smallFont, Fade(Dim, Math.Max(70, 255 - i * 22)), new RectangleF(logBox.X + lp, yy, logBox.Width - 2 * lp, lh), StringAlignment.Near, StringAlignment.Near, false);
            yy -= lh;
        }
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
