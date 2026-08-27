use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use wl_clipboard_rs::copy::{MimeType, Options, Source};

pub const SERVE_FLAG: &str = "--clipboard-server";

/// Pe Wayland conținutul din clipboard e servit live de procesul care l-a oferit.
/// `wl-clipboard-rs` pornește doar un *thread*, care moare odată cu noi — de aceea
/// delegăm unui subproces care rămâne în viață după ce editorul se închide.
fn pid_file() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("sxr-clipboard.pid")
}

/// Serverul anterior nu se stinge de fiecare dată singur când altcineva preia
/// clipboard-ul, așa că îl închidem noi ca să nu se adune procese.
fn kill_previous(pid: Option<i32>) {
    let Some(pid) = pid else { return };
    // ne asigurăm că e chiar serverul nostru înainte să trimitem semnalul
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let cmdline = String::from_utf8_lossy(&cmdline);
    if cmdline.contains("sxr") && cmdline.contains(SERVE_FLAG) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

pub fn copy_png(png: Vec<u8>) -> Result<()> {
    let previous = std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|t| t.trim().parse::<i32>().ok());
    let exe = std::env::current_exe().context("cannot locate own executable")?;
    let mut child = Command::new(exe)
        .arg(SERVE_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not start the clipboard server")?;
    child
        .stdin
        .take()
        .context("child process has no stdin")?
        .write_all(&png)
        .context("could not send the image to the clipboard server")?;

    // abia după ce noul server a primit datele închidem vechiul,
    // ca să nu rămână clipboard-ul gol între cele două
    kill_previous(previous);
    let _ = std::fs::write(pid_file(), child.id().to_string());
    Ok(())
}

/// Rulează în subprocesul dedicat: preia PNG-ul de pe stdin și îl servește
/// până când altcineva pune altceva în clipboard.
pub fn serve_from_stdin() -> Result<()> {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).context("reading stdin")?;
    if buf.is_empty() {
        bail!("nothing to serve");
    }
    let mut opts = Options::new();
    // blocant intenționat: ăsta e singurul rost al procesului
    opts.foreground(true);
    opts.copy(
        Source::Bytes(buf.into_boxed_slice()),
        MimeType::Specific("image/png".into()),
    )
    .context("offering to the clipboard failed")?;
    Ok(())
}

/// Tipurile MIME oferite acum de clipboard — folosit pentru verificare automată.
pub fn mime_types() -> Result<Vec<String>> {
    use wl_clipboard_rs::paste::{get_mime_types, ClipboardType, Seat};
    let set = get_mime_types(ClipboardType::Regular, Seat::Unspecified)
        .map_err(|e| anyhow::anyhow!("reading clipboard: {e}"))?;
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort();
    Ok(v)
}

/// Citește imaginea din clipboard într-un fișier — verificare automată a round-trip-ului.
pub fn paste_to_file(path: &str) -> Result<usize> {
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, Seat};
    let (mut pipe, _mime) = get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        wl_clipboard_rs::paste::MimeType::Specific("image/png"),
    )
    .map_err(|e| anyhow::anyhow!("reading clipboard: {e}"))?;
    let mut buf = Vec::new();
    pipe.read_to_end(&mut buf).context("reading clipboard pipe")?;
    std::fs::write(path, &buf).context("writing file")?;
    Ok(buf.len())
}
