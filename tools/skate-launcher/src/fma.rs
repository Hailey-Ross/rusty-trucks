//! Free Music Archive sign-in for stream-safe playlists that need it (`"login": "fma"`): FMA lets only members
//! download, and membership is free. When such a playlist's songs are missing, the launcher opens its own small window
//! (`fma-login`): sign in with an FMA account, or create one in the browser first, or skip the playlist. The window
//! signs in itself (wrong details show there and can be retried) and leaves only the session cookie in a temporary
//! file, deleted when the downloads are done. The password goes to FMA once over https through curl's stdin (never a
//! command line or a file) and is not saved.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;

use eframe::egui::{self, Color32, FontFamily, FontId, RichText};

const LOGIN: &str = "https://freemusicarchive.org/login";
const REGISTER: &str = "https://freemusicarchive.org/register";
const RESET: &str = "https://freemusicarchive.org/password/reset";

pub struct Session {
    jar: PathBuf,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.jar);
    }
}

/// Opens the sign-in window for `playlist` and waits. Err: skipped, closed, or the window could not start.
pub fn sign_in(dir: &Path, playlist: &str) -> Result<Session, String> {
    let s = Session { jar: dir.join(format!(".fma-session-{}", std::process::id())) };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    println!("  Free Music Archive: sign in or create a free account in the window that opened.");
    let code = crate::proc::cmd(exe).arg("fma-login").arg(&s.jar).arg(playlist).stdin(Stdio::null()).status().map_err(|e| e.to_string())?.code();
    match code {
        Some(0) if s.jar.is_file() => Ok(s),
        Some(2) => Err("skipped (no Free Music Archive sign-in)".into()),
        _ => Err("the Free Music Archive sign-in window did not work".into()),
    }
}

/// Signs in with the cookie file `jar`. Err: what to tell the player.
fn login(jar: &Path, email: &str, password: &str) -> Result<(), String> {
    let page = curl(jar, &["-fsSL", LOGIN], None).ok_or("Free Music Archive did not answer. Check your internet connection and try again.")?;
    let token = token(&page).ok_or("Free Music Archive's sign-in page changed; the launcher needs an update.")?;
    let body = format!("_token={}&email={}&password={}&remember=on", enc(&token), enc(email.trim()), enc(password));
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let landed = curl(jar, &["-sSL", "-o", null, "-w", "%{url_effective}", "-e", LOGIN, "--data-binary", "@-", LOGIN], Some(&body))
        .ok_or("Free Music Archive did not answer. Check your internet connection and try again.")?;
    if landed.trim_end_matches('/').ends_with("/login") {
        return Err("That email and password did not work. Check them and try again.".into());
    }
    Ok(())
}

/// Downloads with the session; false when FMA sends a page (signed out, missing track) instead of a file.
pub fn download(s: &Session, url: &str, out: &Path) -> bool {
    let ok = crate::proc::cmd("curl").args(["-fsSL", "--retry", "2", "--max-time", "600", "-b"]).arg(&s.jar).arg("-c").arg(&s.jar)
        .arg("-o").arg(out).arg(url).status().is_ok_and(|st| st.success());
    let mut head = [0u8; 64];
    let n = std::fs::File::open(out).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    ok && n > 0 && !is_page(&head[..n])
}

fn is_page(head: &[u8]) -> bool {
    let t = String::from_utf8_lossy(head).trim_start().to_ascii_lowercase();
    t.starts_with("<!doctype") || t.starts_with("<html")
}

fn curl(jar: &Path, args: &[&str], stdin: Option<&str>) -> Option<String> {
    let mut c = crate::proc::cmd("curl");
    c.arg("-b").arg(jar).arg("-c").arg(jar).args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = c.spawn().ok()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn token(page: &str) -> Option<String> {
    let i = page.find("name=\"_token\"")?;
    let rest = &page[i..];
    let v = rest.find("value=\"")? + 7;
    let end = rest[v..].find('"')?;
    Some(rest[v..v + end].to_owned()).filter(|t| !t.is_empty())
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn open_url(url: &str) {
    let _ = if cfg!(windows) {
        crate::proc::cmd("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn()
    } else if cfg!(target_os = "macos") {
        crate::proc::cmd("open").arg(url).spawn()
    } else {
        crate::proc::cmd("xdg-open").arg(url).spawn()
    };
}

// The launcher panel's colors (camcorder.rs).
const BODY: Color32 = Color32::from_rgb(24, 25, 29);
const PANEL: Color32 = Color32::from_rgb(54, 56, 62);
const INK: Color32 = Color32::from_rgb(244, 246, 250);
const DIM: Color32 = Color32::from_rgb(168, 178, 196);
const AMBER: Color32 = Color32::from_rgb(255, 172, 44);
const RED: Color32 = Color32::from_rgb(255, 98, 86);

#[derive(PartialEq)]
enum Step {
    Choose,
    SignIn { new_account: bool },
}

struct Login {
    jar: PathBuf,
    playlist: String,
    step: Step,
    email: String,
    password: String,
    error: Option<String>,
    busy: Option<mpsc::Receiver<Result<(), String>>>,
    focus: bool,
}

/// `fma-login <cookie file> <playlist name>`: the window. Exit 0 signed in, 2 skipped or closed.
pub fn window(jar: &str, playlist: &str) -> i32 {
    if jar.is_empty() {
        eprintln!("usage: fma-login <cookie file> <playlist name>");
        return 1;
    }
    let result = std::sync::Arc::new(std::sync::Mutex::new(2));
    let out = result.clone();
    let viewport = egui::ViewportBuilder::default().with_title("Free Music Archive").with_inner_size([520.0, 440.0]).with_resizable(false)
        .with_maximize_button(false).with_minimize_button(false);
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    let app = Login { jar: PathBuf::from(jar), playlist: if playlist.is_empty() { "Free Music Archive".into() } else { playlist.into() }, step: Step::Choose,
        email: String::new(), password: String::new(), error: None, busy: None, focus: false };
    let run = eframe::run_native("Free Music Archive", options, Box::new(move |cc| {
        crate::camcorder::install_fonts(&cc.egui_ctx);
        style(&cc.egui_ctx);
        Ok(Box::new(Shell { app, out }))
    }));
    if run.is_err() {
        return 1;
    }
    result.lock().map_or(2, |r| *r)
}

struct Shell {
    app: Login,
    out: std::sync::Arc<std::sync::Mutex<i32>>,
}

impl eframe::App for Shell {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(code) = self.app.frame(ctx) {
            if let Ok(mut r) = self.out.lock() { *r = code; }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BODY;
    v.window_fill = BODY;
    v.extreme_bg_color = Color32::from_rgb(14, 15, 18);
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(78, 81, 90);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(100, 103, 112);
    v.selection.bg_fill = Color32::from_rgb(238, 124, 12);
    v.selection.stroke.color = AMBER;
    v.hyperlink_color = AMBER;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 10.0);
        s.spacing.button_padding = egui::vec2(16.0, 9.0);
    });
}

fn font(size: f32, bold: bool) -> FontId {
    FontId::new(size, FontFamily::Name(if bold { "segsb" } else { "seg" }.into()))
}

impl Login {
    /// One frame; Some(exit code) when the window should close.
    fn frame(&mut self, ctx: &egui::Context) -> Option<i32> {
        if let Some(rx) = &self.busy {
            match rx.try_recv() {
                Ok(Ok(())) => return Some(0),
                Ok(Err(e)) => { self.error = Some(e); self.busy = None; self.focus = true; }
                Err(mpsc::TryRecvError::Disconnected) => { self.error = Some("The sign-in stopped unexpectedly. Try again.".into()); self.busy = None; }
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            return Some(2);
        }
        let mut close = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BODY).inner_margin(28.0)).show(ctx, |ui| {
            ui.label(RichText::new("FREE MUSIC ARCHIVE").font(font(13.0, true)).color(AMBER));
            ui.label(RichText::new(format!("\u{201c}{}\u{201d}", self.playlist)).font(font(22.0, true)).color(INK));
            ui.add_space(4.0);
            match self.step {
                Step::Choose => close = self.choose(ui),
                Step::SignIn { new_account } => close = self.sign_in_form(ui, new_account),
            }
        });
        close
    }

    fn choose(&mut self, ui: &mut egui::Ui) -> Option<i32> {
        ui.label(RichText::new("This playlist comes from Free Music Archive. Its artists share their music for free, and FMA lets members download it. Membership is free.").font(font(15.0, false)).color(DIM));
        ui.add_space(14.0);
        let wide = |ui: &mut egui::Ui, text: &str, sub: &str| {
            let mut job = egui::text::LayoutJob::default();
            job.append(text, 0.0, egui::TextFormat::simple(font(17.0, true), INK));
            job.append(&format!("\n{sub}"), 0.0, egui::TextFormat::simple(font(13.0, false), DIM));
            ui.add(egui::Button::new(job).min_size(egui::vec2(ui.available_width(), 64.0))).clicked()
        };
        if wide(ui, "I have an account", "Sign in with your FMA email and password") {
            self.step = Step::SignIn { new_account: false };
            self.focus = true;
        }
        if wide(ui, "Create a free account", "Opens freemusicarchive.org in your browser, then sign in here") {
            open_url(REGISTER);
            self.step = Step::SignIn { new_account: true };
            self.focus = true;
        }
        ui.add_space(8.0);
        let skip = ui.add(egui::Button::new(RichText::new("Skip this playlist").font(font(14.0, false)).color(DIM)).frame(false));
        ui.label(RichText::new("The other stream-safe playlists download without an account.").font(font(13.0, false)).color(DIM));
        skip.clicked().then_some(2)
    }

    fn sign_in_form(&mut self, ui: &mut egui::Ui, new_account: bool) -> Option<i32> {
        let busy = self.busy.is_some();
        let note = if new_account {
            "Finish signing up in your browser (FMA may email you a link to confirm), then sign in here."
        } else {
            "Sign in with your Free Music Archive account."
        };
        ui.label(RichText::new(note).font(font(15.0, false)).color(DIM));
        ui.add_space(6.0);
        ui.label(RichText::new("Email").font(font(14.0, true)).color(INK));
        let email = ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.email).font(font(16.0, false)).desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)));
        if self.focus {
            email.request_focus();
            self.focus = false;
        }
        ui.label(RichText::new("Password").font(font(14.0, true)).color(INK));
        let pw = ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.password).password(true).font(font(16.0, false)).desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)));
        let enter = (pw.lost_focus() || email.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).font(font(14.0, false)).color(RED));
        }
        let mut close = None;
        ui.horizontal(|ui| {
            let ready = !busy && self.email.contains('@') && !self.password.is_empty();
            let label = if busy { "Signing in..." } else { "Sign in" };
            let go = ui.add_enabled(ready, egui::Button::new(RichText::new(label).font(font(16.0, true)).color(INK)).fill(Color32::from_rgb(238, 124, 12)));
            if busy {
                ui.spinner();
            }
            if (go.clicked() || (enter && ready)) && !busy {
                let (tx, rx) = mpsc::channel();
                let (jar, email, password) = (self.jar.clone(), self.email.clone(), std::mem::take(&mut self.password));
                std::thread::spawn(move || { let _ = tx.send(login(&jar, &email, &password)); });
                self.error = None;
                self.busy = Some(rx);
            }
            if ui.add_enabled(!busy, egui::Button::new(RichText::new("Back").font(font(15.0, false)).color(INK))).clicked() {
                self.step = Step::Choose;
                self.error = None;
            }
            if ui.add_enabled(!busy, egui::Button::new(RichText::new("Skip").font(font(15.0, false)).color(DIM)).frame(false)).clicked() {
                close = Some(2);
            }
        });
        ui.horizontal(|ui| {
            if ui.link(RichText::new("Forgot your password?").font(font(13.0, false))).clicked() { open_url(RESET); }
            if !new_account && ui.link(RichText::new("No account yet? Create one").font(font(13.0, false))).clicked() { open_url(REGISTER); }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Your password goes only to Free Music Archive and is not saved.").font(font(12.5, false)).color(DIM));
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_token_is_read_and_the_body_is_encoded() {
        let page = r#"<form method="POST"><input type="hidden" name="_token" value="abc123XYZ"><input name="email">"#;
        assert_eq!(token(page).as_deref(), Some("abc123XYZ"));
        assert_eq!(token("<form></form>"), None);
        assert_eq!(enc("a b&c=d@e.f+ü"), "a%20b%26c%3Dd%40e.f%2B%C3%BC");
    }

    #[test]
    fn a_page_is_not_a_song() {
        assert!(is_page(b"\n  <!DOCTYPE html><html>"));
        assert!(is_page(b"<html lang=en>"));
        assert!(!is_page(b"ID3\x04\x00"));
    }
}
