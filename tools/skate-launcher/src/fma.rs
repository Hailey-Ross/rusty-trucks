//! Free Music Archive sign-in for stream-safe playlists that need it (`"login": "fma"`): FMA only lets signed-in
//! accounts download. The player's own email and password come from the system's sign-in dialog, go to FMA once over
//! https through curl's stdin (never a command line or a file), and are not saved; the session cookie lives in a
//! temporary file that is deleted when the downloads are done.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

const LOGIN: &str = "https://freemusicarchive.org/login";
const PROMPT: &str = "Sign in to Free Music Archive to download its stream-safe playlist. The launcher does not save your password.";

pub struct Session {
    jar: PathBuf,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.jar);
    }
}

/// Asks for the player's FMA login and signs in. Err: canceled, no dialog tool, or the sign-in did not work.
pub fn sign_in(dir: &Path) -> Result<Session, String> {
    let (email, password) = ask().ok_or("no Free Music Archive sign-in was given")?;
    let s = Session { jar: dir.join(format!(".fma-session-{}", std::process::id())) };
    let page = curl(&s, &["-fsSL", LOGIN], None).ok_or("Free Music Archive did not answer")?;
    let token = token(&page).ok_or("Free Music Archive's sign-in page changed (no form token)")?;
    let body = format!("_token={}&email={}&password={}&remember=on", enc(&token), enc(&email), enc(&password));
    let landed = curl(&s, &["-sSL", "-o", if cfg!(windows) { "NUL" } else { "/dev/null" }, "-w", "%{url_effective}", "-e", LOGIN, "--data-binary", "@-", LOGIN], Some(&body))
        .ok_or("Free Music Archive did not answer")?;
    if landed.trim_end_matches('/').ends_with("/login") {
        return Err("Free Music Archive did not accept that email and password".into());
    }
    Ok(s)
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

fn curl(s: &Session, args: &[&str], stdin: Option<&str>) -> Option<String> {
    let mut c = crate::proc::cmd("curl");
    c.arg("-b").arg(&s.jar).arg("-c").arg(&s.jar).args(args).stdout(Stdio::piped()).stderr(Stdio::null());
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

/// Email and password from the system's own dialog: Windows' credential prompt, macOS dialogs, zenity or kdialog.
fn ask() -> Option<(String, String)> {
    let lines = if cfg!(windows) {
        let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; $c = Get-Credential -Message '{PROMPT}'; if ($c) {{ [Console]::Out.WriteLine($c.UserName); [Console]::Out.WriteLine($c.GetNetworkCredential().Password) }}");
        run("powershell", &["-NoProfile", "-Command", &script])?
    } else if cfg!(target_os = "macos") {
        let q = |hidden: &str, label: &str| format!("text returned of (display dialog \"{PROMPT}\\n\\n{label}\" default answer \"\"{hidden} with title \"Free Music Archive\")");
        let email = run("osascript", &["-e", &q("", "Email")])?;
        let password = run("osascript", &["-e", &q(" with hidden answer", "Password")])?;
        format!("{}\n{}", email.trim_end_matches(['\r', '\n']), password.trim_end_matches(['\r', '\n']))
    } else if let Some(o) = run("zenity", &["--forms", "--title=Free Music Archive", &format!("--text={PROMPT}"), "--add-entry=Email", "--add-password=Password", "--separator=\n"]) {
        o
    } else {
        let email = run("kdialog", &["--title", "Free Music Archive", "--inputbox", &format!("{PROMPT}\n\nEmail")])?;
        let password = run("kdialog", &["--title", "Free Music Archive", "--password", "Password"])?;
        format!("{}\n{}", email.trim_end_matches(['\r', '\n']), password.trim_end_matches(['\r', '\n']))
    };
    let mut it = lines.split('\n').map(|l| l.trim_end_matches('\r'));
    let email = it.next()?.trim().trim_start_matches('\\').to_owned();
    let password = it.next()?.to_owned();
    (!email.is_empty() && !password.is_empty()).then_some((email, password))
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = crate::proc::cmd(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
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
