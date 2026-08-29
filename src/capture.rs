//! Getting the pixels off the screen.
//!
//! Two routes live here:
//!   * `grab_screen()` — the whole desktop in one go, the input our own overlay
//!     selector works on;
//!   * `select_region_spectacle()` — the native KDE selector, start to finish,
//!     kept as the fallback for when the overlay cannot come up.
//!
//! The screenshot portal (`org.freedesktop.portal.Screenshot`) would be the
//! cleaner source, and it is ~50ms faster, but it cannot be driven from a
//! short-lived helper: the answer does not come back from the call, it arrives
//! later as a `Response` signal, and the portal cancels the request the moment
//! the calling connection goes away — which is exactly what `gdbus call` does
//! when it exits. Talking to it means holding a session-bus connection open
//! ourselves, and that is a DBus client we have not written. Until then the
//! grab goes through spectacle, which is one process and already proven here.

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::selector::Box2;

/// How long the region selector waits for the window list before it gives up
/// and comes up without snapping. The lookup runs alongside the screen grab and
/// finishes in a couple of dozen milliseconds, so this is only ever reached
/// when KWin is not answering at all.
const WINDOWS_BUDGET: Duration = Duration::from_millis(250);

/// A temporary file of our own, removed as soon as it has been read.
fn temp_png(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sxr-{tag}-{}.png", std::process::id()))
}

/// Reads the PNG and takes the file with it, whether the decoding worked out
/// or not. The picture is the user's screen; it has no business staying around.
fn take_png(path: &PathBuf) -> Result<RgbaImage> {
    let loaded = image::open(path)
        .with_context(|| format!("could not decode the capture at {}", path.display()));
    let _ = std::fs::remove_file(path);
    Ok(loaded?.to_rgba8())
}

/// The whole desktop — every output, side by side, in one image. On a two
/// monitor setup this is as wide as both together, and the coordinates in it
/// are the global desktop coordinates the overlay works in.
pub fn grab_screen() -> Result<RgbaImage> {
    let tmp = temp_png("screen");
    let _ = std::fs::remove_file(&tmp);

    // -f the full workspace, -b no window of its own, -n no notification
    let status = Command::new("spectacle")
        .args(["-f", "-b", "-n", "-o"])
        .arg(&tmp)
        .status()
        .context("could not start spectacle to grab the screen")?;

    if !status.success() {
        bail!("spectacle exited with code {:?}", status.code());
    }
    if !tmp.exists() {
        bail!("the screen capture produced no file");
    }
    take_png(&tmp)
}

/// Region selection through the native KDE selector, kept as the fallback.
pub fn select_region_spectacle() -> Result<RgbaImage> {
    let tmp = temp_png("region");
    let _ = std::fs::remove_file(&tmp);

    let status = Command::new("spectacle")
        .args(["-r", "-b", "-n", "-o"])
        .arg(&tmp)
        .status()
        .context("could not start spectacle")?;

    if !status.success() {
        bail!("spectacle exited with code {:?}", status.code());
    }
    if !tmp.exists() {
        bail!("selection cancelled");
    }
    take_png(&tmp)
}

/// The region the user picked: the screen is frozen first, then our own overlay
/// goes over it and the rectangle is cut out of that same frozen image. Nothing
/// on screen can change between the picking and the cut, which is the whole
/// point of freezing it.
pub fn select_region() -> Result<RgbaImage> {
    // Asking KWin what is on screen is a round trip through the journal, so it
    // is started first and collected last: it runs while spectacle is grabbing
    // the screen and costs nothing of its own. `0, 0` because the size of the
    // screenshot is not known yet — the lookup clips to the outputs instead.
    let pending = crate::windows::spawn(0, 0);
    let screen = grab_screen()?;
    let wins: Vec<Box2> = pending
        .take(WINDOWS_BUDGET)
        .into_iter()
        .map(|w| Box2::new(w.x, w.y, w.w, w.h))
        .collect();

    // A compositor without `zwlr_layer_shell_v1` cannot show our overlay at
    // all. That is a reason to hand the job to the native selector, not to
    // fail: only a broken overlay falls back, a cancelled one is a decision.
    let picked = match crate::overlay::select(&screen, wins) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sxr: the overlay could not start ({e}), falling back to spectacle");
            return select_region_spectacle();
        }
    };

    let Some((x, y, w, h)) = picked else {
        bail!("selection cancelled");
    };
    if w == 0 || h == 0 {
        bail!("selection cancelled");
    }
    // the overlay works in the pixels of this very image, but a compositor that
    // reports its outputs oddly could still hand back something off the edge
    let x = x.min(screen.width().saturating_sub(1));
    let y = y.min(screen.height().saturating_sub(1));
    let w = w.min(screen.width() - x);
    let h = h.min(screen.height() - y);
    Ok(image::imageops::crop_imm(&screen, x, y, w, h).to_image())
}
