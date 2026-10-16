//! Startup splash (Windows): a small window of its own, up from the first moment of the process, with a skateboard
//! (no skater) rolling across it and exactly what is loading written under it, the last finished steps dimmed below
//! (like RPCS3's progress lines). It runs on its own thread with its own message loop, so it keeps animating while
//! the game thread is busy (config, skater, physics, world, menus, the audio runtime). The game window is created
//! hidden and shown when the start is done (`startup_loading`), and then the splash closes.
//!
//! Steps are reported from any thread with [`stage`] (a step: "Loading physics") and [`detail`] (the item being
//! read: "sound bank grind_concrete"). Not retail: retail boots into its own movies; this only covers our engine's
//! start. `SKATE_NO_SPLASH=1` turns it off (scripted runs that want no extra window).
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

/// What the splash shows: the current step and item, and the last finished lines (oldest first).
struct Text {
    stage: String,
    detail: String,
    history: Vec<String>,
    started: Option<Instant>,
}

static TEXT: Mutex<Text> = Mutex::new(Text { stage: String::new(), detail: String::new(), history: Vec::new(), started: None });
/// The splash window (0 = none, or closed).
static WINDOW: AtomicIsize = AtomicIsize::new(0);
const HISTORY_LINES: usize = 4;

fn push_history(t: &mut Text, line: String) {
    if !line.is_empty() {
        t.history.push(line);
        let excess = t.history.len().saturating_sub(HISTORY_LINES);
        t.history.drain(..excess);
    }
}

/// A new step ("Loading physics"); the previous step (and its last item) move to the finished lines.
pub(crate) fn stage(label: &str) {
    if let Ok(mut t) = TEXT.lock() {
        if t.stage == label {
            return;
        }
        let previous = std::mem::take(&mut t.detail);
        let previous = if previous.is_empty() { std::mem::take(&mut t.stage) } else { format!("{}: {previous}", std::mem::take(&mut t.stage)) };
        push_history(&mut t, previous);
        t.stage = label.to_owned();
    }
}

/// The item the current step reads now; the previous item moves to the finished lines.
pub(crate) fn detail(text: String) {
    if let Ok(mut t) = TEXT.lock() {
        let previous = std::mem::replace(&mut t.detail, text);
        if !previous.is_empty() {
            push_history(&mut t, previous);
        }
    }
}

/// The splash is up (not closed yet).
pub(crate) fn active() -> bool {
    WINDOW.load(Ordering::Relaxed) != 0
}

/// The line under the board.
fn line(t: &Text) -> String {
    let secs = t.started.map_or(0, |s| s.elapsed().as_secs());
    match (t.stage.is_empty(), t.detail.is_empty()) {
        (true, _) => format!("Starting  ({secs} s)"),
        (false, true) => format!("{}  ({secs} s)", t.stage),
        (false, false) => format!("{}: {}  ({secs} s)", t.stage, t.detail),
    }
}

// Win32 (user32 / gdi32 / kernel32).
type Hwnd = *mut c_void;
type Hdc = *mut c_void;
type Hgdi = *mut c_void;
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: [i32; 2],
    private: u32,
}
#[repr(C)]
struct PaintStruct {
    hdc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    proc_: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: *const c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: *mut c_void,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: Hwnd, menu: *mut c_void, instance: *const c_void, param: *mut c_void) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, ms: u32, func: *mut c_void) -> usize;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn BeginPaint(hwnd: Hwnd, ps: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, ps: *const PaintStruct) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn FillRect(hdc: Hdc, rect: *const Rect, brush: Hgdi) -> i32;
    fn DrawTextW(hdc: Hdc, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn LoadCursorW(instance: *const c_void, name: usize) -> *mut c_void;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Hgdi;
    fn CreatePen(style: i32, width: i32, color: u32) -> Hgdi;
    fn DeleteObject(obj: Hgdi) -> i32;
    fn SelectObject(hdc: Hdc, obj: Hgdi) -> Hgdi;
    fn RoundRect(hdc: Hdc, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    fn Ellipse(hdc: Hdc, l: i32, t: i32, r: i32, b: i32) -> i32;
    fn MoveToEx(hdc: Hdc, x: i32, y: i32, old: *mut [i32; 2]) -> i32;
    fn LineTo(hdc: Hdc, x: i32, y: i32) -> i32;
    fn SetTextColor(hdc: Hdc, color: u32) -> u32;
    fn SetBkMode(hdc: Hdc, mode: i32) -> i32;
    fn CreateFontW(h: i32, w: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32, strike: u32, charset: u32, out: u32, clip: u32, quality: u32, pitch: u32, face: *const u16) -> Hgdi;
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn CreateCompatibleBitmap(hdc: Hdc, w: i32, h: i32) -> Hgdi;
    fn BitBlt(dst: Hdc, x: i32, y: i32, w: i32, h: i32, src: Hdc, sx: i32, sy: i32, rop: u32) -> i32;
    fn DeleteDC(hdc: Hdc) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *const c_void;
}

const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_TIMER: u32 = 0x0113;
const WM_NCHITTEST: u32 = 0x0084;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_APPWINDOW: u32 = 0x0004_0000;
const WIDTH: i32 = 560;
const HEIGHT: i32 = 240;

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const BACKGROUND: u32 = rgb(14, 14, 17);
const DECK: u32 = rgb(140, 97, 56);
const TRUCK: u32 = rgb(158, 163, 173);
const WHEEL: u32 = rgb(237, 232, 214);
const WHEEL_MARK: u32 = rgb(115, 110, 102);
const TEXT_BRIGHT: u32 = rgb(217, 217, 217);
const TEXT_DIM: u32 = rgb(110, 110, 115);
const TITLE: u32 = rgb(160, 160, 165);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Show the splash (once per process; nothing with `SKATE_NO_SPLASH=1`).
pub(crate) fn show() {
    if std::env::var("SKATE_NO_SPLASH").is_ok_and(|v| v == "1") || WINDOW.load(Ordering::Relaxed) != 0 {
        return;
    }
    if let Ok(mut t) = TEXT.lock() {
        t.started = Some(Instant::now());
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("splash".into()).spawn(move || {
        // SAFETY: plain Win32 window creation and a message loop owned by this thread; every pointer passed is to
        // a live local (class, strings, message) for the duration of the call.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("Skate3RustSplash");
            let wc = WndClassExW {
                size: std::mem::size_of::<WndClassExW>() as u32,
                style: 0,
                proc_: wnd_proc,
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: std::ptr::null_mut(),
                cursor: LoadCursorW(std::ptr::null(), 32512),
                background: std::ptr::null_mut(),
                menu_name: std::ptr::null(),
                class_name: class.as_ptr(),
                icon_small: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
            let (sw, sh) = (GetSystemMetrics(0), GetSystemMetrics(1));
            let title = wide("Skate 3 Rust Engine");
            let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP | WS_VISIBLE, (sw - WIDTH) / 2, (sh - HEIGHT) / 2, WIDTH, HEIGHT, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());
            let _ = ready_tx.send(());
            if hwnd.is_null() {
                return;
            }
            WINDOW.store(hwnd as isize, Ordering::Relaxed);
            SetTimer(hwnd, 1, 33, std::ptr::null_mut());
            let mut msg: Msg = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            WINDOW.store(0, Ordering::Relaxed);
        }
    });
    if spawned.is_ok() {
        // The window is up before the slow start begins (bounded: never hold the start for it).
        let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(2));
    }
}

/// Close the splash (any thread; nothing when it is not up).
pub(crate) fn close() {
    let hwnd = WINDOW.swap(0, Ordering::Relaxed);
    if hwnd != 0 {
        // SAFETY: posting a message to a window handle; harmless if the window is already gone.
        unsafe {
            PostMessageW(hwnd as Hwnd, WM_CLOSE, 0, 0);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: called by the system on the splash thread with a valid window handle.
    unsafe {
        match msg {
            WM_TIMER => {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                0
            }
            WM_ERASEBKGND => 1,
            // Drag it anywhere.
            WM_NCHITTEST => 2,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// One frame, drawn into a memory bitmap and copied over (no flicker).
unsafe fn paint(hwnd: Hwnd) {
    // SAFETY: GDI calls on DCs and objects created here and released before return.
    unsafe {
        let mut ps: PaintStruct = std::mem::zeroed();
        let screen = BeginPaint(hwnd, &mut ps);
        let mut client = Rect::default();
        GetClientRect(hwnd, &mut client);
        let (w, h) = (client.right, client.bottom);
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old_bitmap = SelectObject(dc, bitmap);

        let bg = CreateSolidBrush(BACKGROUND);
        FillRect(dc, &client, bg);
        DeleteObject(bg);

        let (text, history, started) = match TEXT.lock() {
            Ok(t) => (line(&t), t.history.clone(), t.started),
            Err(_) => (String::new(), Vec::new(), None),
        };
        let t = started.map_or(0.0, |s| s.elapsed().as_secs_f32());

        // The board, side view, rolling left to right and coming round again.
        const BOARD_W: i32 = 180;
        const SPEED: f32 = 170.0;
        let travelled = t * SPEED;
        let x = (travelled % (w + BOARD_W) as f32) as i32 - BOARD_W;
        let y = 64;
        let no_pen = CreatePen(5, 0, 0);
        let old_pen = SelectObject(dc, no_pen);
        let deck = CreateSolidBrush(DECK);
        let old_brush = SelectObject(dc, deck);
        RoundRect(dc, x, y, x + BOARD_W, y + 10, 10, 10);
        let truck = CreateSolidBrush(TRUCK);
        SelectObject(dc, truck);
        let wheel = CreateSolidBrush(WHEEL);
        const AXLES: [i32; 2] = [32, BOARD_W - 32];
        const WHEEL_D: i32 = 22;
        let turn = travelled / (WHEEL_D as f32 / 2.0);
        for ax in AXLES {
            SelectObject(dc, truck);
            RoundRect(dc, x + ax - 14, y + 9, x + ax + 14, y + 16, 3, 3);
            SelectObject(dc, wheel);
            let (cx, cy) = (x + ax, y + 16 + WHEEL_D / 2 - 2);
            Ellipse(dc, cx - WHEEL_D / 2, cy - WHEEL_D / 2, cx + WHEEL_D / 2, cy + WHEEL_D / 2);
            let mark = CreatePen(0, 3, WHEEL_MARK);
            let prev = SelectObject(dc, mark);
            MoveToEx(dc, cx, cy, std::ptr::null_mut());
            LineTo(dc, cx + (turn.sin() * 8.0) as i32, cy - (turn.cos() * 8.0) as i32);
            SelectObject(dc, prev);
            DeleteObject(mark);
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        for o in [deck, truck, wheel, no_pen] {
            DeleteObject(o);
        }

        SetBkMode(dc, 1);
        let face = wide("Segoe UI");
        let font = |size: i32, weight: i32| CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
        let draw = |s: &str, size: i32, weight: i32, color: u32, rect: Rect, format: u32| {
            let f = font(size, weight);
            let old = SelectObject(dc, f);
            SetTextColor(dc, color);
            let mut r = rect;
            let ws: Vec<u16> = s.encode_utf16().collect();
            DrawTextW(dc, ws.as_ptr(), ws.len() as i32, &mut r, format);
            SelectObject(dc, old);
            DeleteObject(f);
        };
        // DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS (0x8000) | DT_NOPREFIX (0x800)
        let one_line = 0x1 | 0x20 | 0x8000 | 0x800;
        draw("SKATE 3 RUST ENGINE", 13, 600, TITLE, Rect { left: 0, top: 16, right: w, bottom: 36 }, one_line);
        draw(&text, 16, 400, TEXT_BRIGHT, Rect { left: 16, top: 122, right: w - 16, bottom: 146 }, one_line);
        for (i, h_line) in history.iter().rev().enumerate() {
            let top = 152 + i as i32 * 19;
            draw(h_line, 13, 400, TEXT_DIM, Rect { left: 16, top, right: w - 16, bottom: top + 18 }, one_line);
        }

        BitBlt(screen, 0, 0, w, h, dc, 0, 0, 0x00CC_0020);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        EndPaint(hwnd, &ps);
        let _ = h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_items_become_finished_lines() {
        let mut t = Text { stage: String::new(), detail: String::new(), history: Vec::new(), started: None };
        push_history(&mut t, String::new());
        assert!(t.history.is_empty());
        for i in 0..6 {
            push_history(&mut t, format!("line {i}"));
        }
        assert_eq!(t.history, ["line 2", "line 3", "line 4", "line 5"]);
        t.stage = "Loading sound".into();
        t.detail = "sound bank grind".into();
        assert!(line(&t).starts_with("Loading sound: sound bank grind  ("));
    }
}
