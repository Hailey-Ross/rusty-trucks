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
using System.Windows.Forms;

static class Native {
    [StructLayout(LayoutKind.Sequential)]
    public struct XState { public uint Packet; public ushort Buttons; public byte LT, RT; public short LX, LY, RX, RY; }
    [DllImport("xinput1_4.dll")] public static extern int XInputGetState(int i, out XState s);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AllowSetForegroundWindow(int pid);
}

// One menu row: greyed rows (On = false) can be selected to read why, but do nothing.
class Item {
    public string Text, Hint; public bool On = true; public Action Run;
    public Item(string text, string hint, bool on, Action run) { Text = text; Hint = hint ?? ""; On = on; Run = run; }
}
class Page {
    public string Title; public List<Item> Items = new List<Item>(); public int Sel;
    public string Body;          // a text page (notes, results) instead of a list
    public Action OnBack;        // null = pop the page
}

class LauncherForm : Form {
    const ushort UP = 0x0001, DOWN = 0x0002, LEFT = 0x0004, RIGHT = 0x0008, A = 0x1000, B = 0x2000;
    static readonly string Here = AppDomain.CurrentDomain.BaseDirectory;
    static readonly string[] Labels = { "session", "ride", "grind", "bail", "push", "emit", "carve", "water", "marker" };
    static readonly string[] GameNames = { "skate3", "skate3rust", "rpcs3" };

    readonly Stack<Page> pages = new Stack<Page>();
    readonly Timer tick = new Timer();
    readonly List<string> log = new List<string>();
    readonly string[] direct;
    Dictionary<string, string> status = new Dictionary<string, string>();
    Process job; string jobTitle; bool jobHidGame; int closeAt = -1; DateTime closeTime;
    string waitEntry, waitLabel, waitName;   // waiting for another game to close before starting
    ushort prevPad; int held; string lastAction = ""; bool lastFromPad; DateTime lastActionTime = DateTime.MinValue;
    Font titleFont, itemFont, smallFont;

    public LauncherForm(string[] argv) {
        direct = argv;
        Text = "Skate launcher";
        FormBorderStyle = FormBorderStyle.None; WindowState = FormWindowState.Maximized;
        BackColor = Color.FromArgb(18, 20, 24); ForeColor = Color.White;
        DoubleBuffered = true; KeyPreview = true;
        try { Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
        tick.Interval = 16; tick.Tick += delegate { Poll(); };
        Load += delegate { Start(); tick.Start(); };
        Resize += delegate { MakeFonts(); Invalidate(); };
        MakeFonts();
    }

    void MakeFonts() {
        float h = Math.Max(480, ClientSize.Height);
        titleFont = new Font("Segoe UI Semibold", h / 22f, GraphicsUnit.Pixel);
        itemFont = new Font("Segoe UI", h / 30f, GraphicsUnit.Pixel);
        smallFont = new Font("Segoe UI", h / 46f, GraphicsUnit.Pixel);
    }

    // ---------- launcher.ps1 ----------
    static ProcessStartInfo Ps(string args) {
        var psi = new ProcessStartInfo("powershell.exe", "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + args) {
            UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Here,
            RedirectStandardOutput = true, RedirectStandardError = true, RedirectStandardInput = true };
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
    static List<string[]> Rows(string args, int min) {
        var rows = new List<string[]>();
        foreach (var l in Run(args).Split('\n')) { var f = l.TrimEnd('\r').Split('\t'); if (f.Length >= min) rows.Add(f); }
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
        return r;
    }
    void RefreshStatus() {
        status.Clear();
        foreach (var r in Rows("status", 2)) status[r[0]] = r[1];
    }
    string St(string k) { string v; return status.TryGetValue(k, out v) ? v : ""; }

    // ---------- start ----------
    void Start() {
        Activate();
        int code; string pending = Run("pending", out code);
        string ent = "", lbl = "", ver = "";
        foreach (var a in direct) { if (a.StartsWith("@")) ver = a.Substring(1); else if (ent == "") ent = a; else if (lbl == "") lbl = a; }
        if (ent != "") { Direct(ent, lbl, ver); return; }
        RefreshStatus();
        pages.Push(MainPage());
        if (St("setup_missing") != "") pages.Push(SetupPage());
        if (code == 10) ShowText("Previous session (closed before its check)", LastSession(), null);
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
        p.Items.Add(new Item("Choose version", "hails-additions, upstream main and the versions in versions.json.", true, delegate { pages.Push(VersionPage()); }));
        p.Items.Add(new Item("What to test (this version)", "", true, delegate { ShowText("What to test", Run("notes"), null); }));
        bool recomp = entries.Exists(e => e[4] == "recomp"), retail = entries.Exists(e => e[4] == "retail");
        p.Items.Add(new Item("Recomp (Skate 3 retail)", recomp ? "The recompiled retail game, with or without tracing." : "Not set up: add recomp_entries to config.json (see README.md).", recomp, delegate { pages.Push(EntryPage("Recomp", "recomp")); }));
        p.Items.Add(new Item("Retail Skate 3 (RPCS3)", retail ? "The PS3 game in RPCS3." : "Not set up: add rpcs3 and rpcs3_entries to config.json (see README.md).", retail, delegate { pages.Push(EntryPage("Retail Skate 3 (RPCS3)", "retail")); }));
        p.Items.Add(new Item("Tools", "Refresh game files, build, send logs, recomp update and save, first-time setup.", true, delegate { pages.Push(EntryPage("Tools", "tools")); }));
        p.Items.Add(new Item("Last session result", "", true, delegate { ShowText("Last session", LastSession(), null); }));
        p.Items.Add(new Item("Exit", "", true, delegate { Close(); }));
        return p;
    }
    Page EntryPage(string title, string group) {
        var p = new Page { Title = title };
        foreach (var e in Rows("entries", 7)) {
            if (e[4] != group) continue;
            var row = e;
            bool on = row[5] == "1";
            p.Items.Add(new Item(row[1], on ? row[2] : row[6], on, delegate {
                if (row[3] == "1") pages.Push(LabelPage(row)); else Begin(row[0], "", row[1]);
            }));
        }
        if (group == "tools") p.Items.Add(new Item("Clear builds and build caches", "Frees disk space: everything, or one version's build cache or whole build. Asks before deleting.", true, delegate { pages.Push(ClearPage()); }));
        if (p.Items.Count == 0) p.Items.Add(new Item("Nothing here yet", "See README.md to set these up.", false, null));
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
        RefreshStatus();
        var all = new List<Page>(pages); all.Reverse();
        pages.Clear();
        foreach (var pg in all) {
            if (pg.Title == "Skate launcher" && pg.OnBack != null) { var m = MainPage(); m.Sel = Math.Min(pg.Sel, m.Items.Count - 1); pages.Push(m); }
            else pages.Push(pg);
        }
    }
    void ShowText(string title, string body, Action back) {
        var p = new Page { Title = title, Body = body };
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
        jobTitle = title; jobHidGame = false;
        // Let the game take the foreground from us (Steam gives the foreground window its controller layout).
        Native.AllowSetForegroundWindow(-1);
        job = new Process { StartInfo = Ps(args + " -NoPrompt"), EnableRaisingEvents = true };
        DataReceivedEventHandler add = delegate(object s, DataReceivedEventArgs e) { if (e.Data != null) BeginInvoke((Action)delegate { log.Add(e.Data); if (log.Count > 400) log.RemoveAt(0); Invalidate(); }); };
        job.OutputDataReceived += add; job.ErrorDataReceived += add;
        job.Exited += delegate { BeginInvoke((Action)JobDone); };
        job.Start(); job.StandardInput.Close();
        job.BeginOutputReadLine(); job.BeginErrorReadLine();
        Invalidate();
    }
    void JobDone() {
        job = null;
        if (!Visible) Show();
        WindowState = FormWindowState.Maximized; Activate(); Native.SetForegroundWindow(Handle);
        string text = LastSession();
        bool bad = text.Contains("MALFORMED") || text.Contains("No new") || text.Contains("Not started") || text.Contains("panic(s)") || text.Contains("failed") || text.Contains("Still missing");
        if (direct.Length > 0) { ShowText(bad ? "Finished, check this" : "Finished", text, null); CloseIn(10); return; }
        RebuildMain();
        ShowText(bad ? "Finished, check this" : "Finished", text, null);
    }
    // While a job runs: hide once the game's window shows, so it gets the foreground and the controller layout.
    void WatchGame() {
        if (job == null || jobHidGame) return;
        foreach (var n in GameNames) foreach (var p in Process.GetProcessesByName(n)) {
            if (p.MainWindowHandle != IntPtr.Zero) { jobHidGame = true; Native.SetForegroundWindow(p.MainWindowHandle); Hide(); return; }
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
        // Only while the menu is in front: XInput reports the pad to every process, so the game's presses would
        // otherwise drive the menu behind it.
        if (job != null || Form.ActiveForm != this) { prevPad = Pad(); return; }
        ushort b = Pad(), pressed = (ushort)(b & ~prevPad);
        if ((pressed & A) != 0) Act("ok", true);
        else if ((pressed & B) != 0) Act("back", true);
        else if ((pressed & UP) != 0) Act("up", true);
        else if ((pressed & DOWN) != 0) Act("down", true);
        else if ((b & (UP | DOWN)) != 0 && b == prevPad) { if (++held > 14 && held % 4 == 0) Act((b & UP) != 0 ? "up" : "down", true); }
        if (b != prevPad) held = 0;
        prevPad = b;
    }
    protected override bool ProcessCmdKey(ref Message msg, Keys key) {
        if (job == null) {
            switch (key) {
                case Keys.Up: case Keys.W: Act("up", false); return true;
                case Keys.Down: case Keys.S: Act("down", false); return true;
                case Keys.Enter: Act("ok", false); return true;
                case Keys.Escape: case Keys.Back: case Keys.Space: Act("back", false); return true;
            }
        }
        return base.ProcessCmdKey(ref msg, key);
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
            if (a == "up") p.Sel = (p.Sel - 1 + p.Items.Count) % p.Items.Count;
            else if (a == "down") p.Sel = (p.Sel + 1) % p.Items.Count;
            else if (a == "ok") { var it = p.Items[p.Sel]; if (it.On && it.Run != null) it.Run(); }
        } else if (a == "ok" && p.Body != null) {
            if (p.OnBack != null) p.OnBack(); else pages.Pop();
        }
        Invalidate();
    }

    // ---------- drawing ----------
    protected override void OnPaint(PaintEventArgs ev) {
        var g = ev.Graphics; g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.ClearTypeGridFit;
        int w = ClientSize.Width, h = ClientSize.Height, x = w / 12, y = h / 14;
        var dim = Color.FromArgb(140, 146, 156); var accent = Color.FromArgb(255, 196, 0);
        if (job != null) {
            g.DrawString("Running: " + jobTitle, titleFont, Brushes.White, x, y);
            y += (int)(titleFont.Height * 1.6);
            int lines = Math.Max(1, (h - y - h / 10) / smallFont.Height);
            int from = Math.Max(0, log.Count - lines);
            using (var b = new SolidBrush(dim)) for (int i = from; i < log.Count; i++) { g.DrawString(log[i], smallFont, b, x, y); y += smallFont.Height; }
            return;
        }
        if (pages.Count == 0) return;
        var p = pages.Peek();
        g.DrawString(p.Title, titleFont, Brushes.White, x, y);
        if (p.Title == "Skate launcher") {
            using (var b = new SolidBrush(dim)) {
                string send = St("send");
                g.DrawString("Version: " + St("version") + "   (" + St("version_status") + ")", smallFont, b, x, y + titleFont.Height * 1.1f);
                g.DrawString("Last: " + St("last") + (send == "" || send == "off" ? "" : "   Send: " + send), smallFont, b, new RectangleF(x, y + titleFont.Height * 1.1f + smallFont.Height, w - 2 * x, smallFont.Height * 2));
            }
            y += smallFont.Height * 3;
        }
        y += (int)(titleFont.Height * 1.7);
        if (p.Body != null) {
            float bodyH = p.Items.Count > 0 ? h / 2.4f : h - y - h / 6f;
            g.DrawString(p.Body, smallFont, Brushes.Gainsboro, new RectangleF(x, y, w - 2 * x, bodyH));
            y += (int)bodyH;
        }
        for (int i = 0; i < p.Items.Count; i++) {
            var it = p.Items[i]; bool sel = i == p.Sel;
            var color = it.On ? Color.White : Color.FromArgb(96, 100, 108);
            if (sel) using (var hb = new SolidBrush(it.On ? accent : Color.FromArgb(70, 74, 82))) g.FillRectangle(hb, x - 16, y - 4, w - 2 * x + 32, itemFont.Height + 8);
            using (var tb = new SolidBrush(sel && it.On ? Color.Black : color)) g.DrawString(it.Text + (it.On || !(it.Hint.StartsWith("Not set up") || it.Hint.StartsWith("Needs")) ? "" : "   (not set up)"), itemFont, tb, x, y);
            y += (int)(itemFont.Height * 1.35);
        }
        string hint = p.Items.Count > 0 ? p.Items[p.Sel].Hint : "";
        using (var b = new SolidBrush(dim)) {
            if (hint != "") g.DrawString(hint, smallFont, b, new RectangleF(x, y + smallFont.Height, w - 2 * x, smallFont.Height * 4));
            string foot = closeAt >= 0 ? "Closing in " + closeAt + " s (any button closes now)" : p.Items.Count > 0 ? "D-pad: move    A: select    B: back" : "A / B: back";
            g.DrawString(foot, smallFont, b, x, h - h / 12);
        }
    }
}

static class SkateLauncher {
    [STAThread]
    static void Main(string[] argv) {
        Application.EnableVisualStyles();
        Application.Run(new LauncherForm(argv));
    }
}
