//! The rectangles of the windows on screen, the list the region selector snaps
//! to — ShareX's `EnumWindows` walk, front to back, done the way KDE allows.
//!
//! There is no Wayland protocol that hands a client the geometry of other
//! people's windows, and that is deliberate. KWin does expose it, but only to
//! code it runs itself: a script loaded over `org.kde.kwin.Scripting`. Such a
//! script has no return channel — whatever it prints goes to KWin's own stderr
//! and from there into the journal — so the lookup is a small round trip:
//! remember where the journal ends, load the script, let it print, read back
//! the new lines, unload. Measured at 8-25ms here, which is why `spawn()`
//! starts it on a thread of its own: it runs while the screen is being grabbed
//! and is long finished by the time anything asks for the answer.
//!
//! Every step of that can fail — no `gdbus`, no journal, a compositor that is
//! not KWin — and none of it is worth an error the caller has to deal with. A
//! failed lookup is an empty list, and an empty list only means the selector
//! does not snap.

use anyhow::{anyhow, bail, Context, Result};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// One on-screen window rectangle, in screenshot pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// How long the lookup itself keeps trying before it gives up. The round trip
/// is a couple of dozen milliseconds; anything past this is KWin not answering.
const POLL_BUDGET: Duration = Duration::from_millis(400);

/// The gap between two reads of the journal. Short enough not to add anything
/// noticeable to a 20ms round trip, long enough not to be a busy loop.
const POLL_STEP: Duration = Duration::from_millis(5);

/// The script KWin runs for us. `__TOKEN__` is replaced per run so two sxr
/// processes reading the same journal cannot pick up each other's lines.
///
/// `workspace.stackingOrder` comes back bottom-to-top, so the caller reverses
/// it to get ShareX's front-to-back order. The outputs come first because the
/// window rectangles are useless without them: KWin counts in logical
/// coordinates and the screenshot is in pixels.
///
/// Both halves sit in their own `try`, so a property this KWin does not have
/// reports itself instead of taking the whole run down with it.
const SCRIPT: &str = r#"var T = "__TOKEN__";
try {
    var os = workspace.screens;
    for (var i = 0; i < os.length; i++) {
        var o = os[i]; var g = o.geometry;
        print("SXR|" + T + "|OUT|" + g.x + "|" + g.y + "|" + g.width + "|" + g.height + "|" + o.devicePixelRatio);
    }
} catch (e) { print("SXR|" + T + "|OUT|ERR|" + e); }
try {
    var s = workspace.stackingOrder;
    for (var i = 0; i < s.length; i++) {
        var w = s[i]; var g = w.frameGeometry;
        print("SXR|" + T + "|WIN|" + g.x + "|" + g.y + "|" + g.width + "|" + g.height +
              "|" + w.minimized + "|" + w.dock + "|" + w.desktopWindow);
    }
} catch (e) { print("SXR|" + T + "|WIN|ERR|" + e); }
print("SXR|" + T + "|END");
"#;

/// A lookup already under way. Kicked off before the screen grab so the two
/// overlap, collected afterwards.
pub struct Handle {
    rx: mpsc::Receiver<Result<Vec<WinRect>>>,
}

/// Starts the lookup on a thread of its own and hands back a handle.
///
/// `desk_w` and `desk_h` are the size of the screenshot the rectangles will be
/// used against; everything is clipped to it. Either of them may be `0`, which
/// means "clip to the bounding box of the outputs the script itself reports" —
/// the lookup is started before the screen is grabbed, so the caller does not
/// always know yet how big the screenshot is going to be, and the script has to
/// ask KWin for the outputs anyway.
pub fn spawn(desk_w: u32, desk_h: u32) -> Handle {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // the receiver is gone when the caller's budget ran out first: the work
        // that mattered — unloading the script — is already done by then
        let _ = tx.send(collect(desk_w, desk_h));
    });
    Handle { rx }
}

impl Handle {
    /// The rectangles, front-to-back. Waits for the lookup to finish, but never
    /// longer than `budget`; an empty list simply means no snapping.
    pub fn take(self, budget: Duration) -> Vec<WinRect> {
        self.take_result(budget).unwrap_or_default()
    }

    /// The same wait with the reason for an empty list kept intact. Only the
    /// hidden test mode has any use for it — the selector cannot act on it.
    pub(crate) fn take_result(self, budget: Duration) -> Result<Vec<WinRect>> {
        self.rx
            .recv_timeout(budget)
            .map_err(|_| anyhow!("the window lookup did not answer within {}ms", budget.as_millis()))?
    }
}

/// The whole round trip, from journal cursor to a clipped list of rectangles.
fn collect(desk_w: u32, desk_h: u32) -> Result<Vec<WinRect>> {
    // taken before the script is loaded, so the only lines we ever read back
    // are the ones this run produced
    let cursor = journal_cursor()?;

    // the pid alone is not enough: several runs of the same process must not
    // collide on the plugin name, and a stale name refuses to load again
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let token = format!("{}-{stamp}", std::process::id());
    let plugin = format!("sxrwin-{token}");
    let path: PathBuf = std::env::temp_dir().join(format!("sxrwin-{token}.js"));

    std::fs::write(&path, SCRIPT.replace("__TOKEN__", &token))
        .with_context(|| format!("could not write the KWin script to {}", path.display()))?;

    let printed = run_script(&path, &plugin, &cursor, &token);

    // KWin must not be left holding our script whatever went wrong above, and
    // the script itself has no business staying in /tmp either
    let _ = gdbus("unloadScript", &[&plugin]);
    let _ = std::fs::remove_file(&path);

    Ok(parse(&printed?, &token, desk_w, desk_h))
}

/// Loads the script, starts it, and reads back everything it printed.
fn run_script(path: &PathBuf, plugin: &str, cursor: &str, token: &str) -> Result<String> {
    let script = path.to_str().context("the temp path is not valid UTF-8")?;
    gdbus("loadScript", &[script, plugin])?;
    gdbus("start", &[])?;

    let end = format!("SXR|{token}|END");
    let deadline = Instant::now() + POLL_BUDGET;
    loop {
        let seen = journal_since(cursor)?;
        if seen.contains(&end) {
            return Ok(seen);
        }
        if Instant::now() >= deadline {
            bail!("KWin printed nothing within {}ms", POLL_BUDGET.as_millis());
        }
        std::thread::sleep(POLL_STEP);
    }
}

/// One call on KWin's scripting interface. Anything but a clean exit is a
/// reason to give up on the whole lookup.
fn gdbus(method: &str, args: &[&str]) -> Result<()> {
    let out = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.KWin",
            "--object-path",
            "/Scripting",
            "--method",
        ])
        .arg(format!("org.kde.kwin.Scripting.{method}"))
        .args(args)
        .output()
        .with_context(|| format!("could not run gdbus for {method}"))?;
    if !out.status.success() {
        bail!("org.kde.kwin.Scripting.{method} failed");
    }
    Ok(())
}

/// The cursor of the last entry in the user journal, i.e. where our own output
/// is about to begin. Read as JSON because the export format may carry binary
/// fields, and the cursor itself is plain text with no escapes in it.
fn journal_cursor() -> Result<String> {
    let out = Command::new("journalctl")
        .args(["--user", "-b", "-n", "1", "--no-pager", "-o", "json"])
        .output()
        .context("could not run journalctl")?;
    if !out.status.success() {
        bail!("journalctl would not give us a cursor");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let key = "\"__CURSOR\":\"";
    let start = text.find(key).context("no cursor in the journal")? + key.len();
    let len = text[start..].find('"').context("the cursor is unterminated")?;
    Ok(text[start..start + len].to_string())
}

/// Everything the user journal gained since `cursor`. `-o cat` because the
/// message is all we want; the parser still looks for its own marker inside
/// the line, so a prefixed format would do just as well.
fn journal_since(cursor: &str) -> Result<String> {
    let out = Command::new("journalctl")
        .args(["--user", "--no-pager", "-o", "cat"])
        .arg(format!("--after-cursor={cursor}"))
        .output()
        .context("could not read the journal back")?;
    if !out.status.success() {
        bail!("journalctl refused to read from our cursor");
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Where an output sits, both in KWin's logical coordinates and in the pixels
/// of the stitched screenshot, so a window rectangle can be moved from one to
/// the other.
struct OutputMap {
    lx: f64,
    ly: f64,
    lw: f64,
    lh: f64,
    px: f64,
    py: f64,
    scale: f64,
}

/// Places every output in the screenshot. The screenshot starts at the
/// top-left of the logical arrangement, so an output's pixel origin is its
/// logical offset from that corner taken through its own scale.
///
/// That is exact whenever the outputs share a scale, which covers this machine
/// (two outputs, both scale 1, so the whole mapping is the identity) and very
/// nearly every setup. A desktop mixing scales stitches by a rule we have no
/// way to check here; the sizes stay right and only the placement could drift,
/// which costs a snap, not a capture.
fn map_outputs(outs: &[(f64, f64, f64, f64, f64)]) -> Vec<OutputMap> {
    let ox = outs.iter().map(|o| o.0).fold(f64::INFINITY, f64::min);
    let oy = outs.iter().map(|o| o.1).fold(f64::INFINITY, f64::min);
    outs.iter()
        .map(|&(lx, ly, lw, lh, scale)| OutputMap {
            lx,
            ly,
            lw,
            lh,
            px: (lx - ox) * scale,
            py: (ly - oy) * scale,
            scale,
        })
        .collect()
}

/// A logical rectangle in screenshot pixels, taken through the output it sits
/// on — the one it overlaps most, since a window may straddle two.
fn to_pixels(maps: &[OutputMap], x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let overlap = |m: &OutputMap| {
        let ow = (x + w).min(m.lx + m.lw) - x.max(m.lx);
        let oh = (y + h).min(m.ly + m.lh) - y.max(m.ly);
        ow.max(0.0) * oh.max(0.0)
    };
    let best = maps
        .iter()
        .max_by(|a, b| overlap(a).partial_cmp(&overlap(b)).unwrap_or(Ordering::Equal));
    // no outputs at all means we never learned the scale: logical is all we
    // have, and treating it as pixels is right on every unscaled desktop
    let Some(m) = best else {
        return (x, y, w, h);
    };
    (
        m.px + (x - m.lx) * m.scale,
        m.py + (y - m.ly) * m.scale,
        w * m.scale,
        h * m.scale,
    )
}

/// A pixel rectangle cut down to the desktop, or nothing if it fell outside.
fn clip(x: f64, y: f64, w: f64, h: f64, dw: f64, dh: f64) -> Option<WinRect> {
    // NaN collapses to 0 through these, which drops the rectangle a line later
    let l = x.round().max(0.0).min(dw);
    let t = y.round().max(0.0).min(dh);
    let r = (x + w).round().max(0.0).min(dw);
    let b = (y + h).round().max(0.0).min(dh);
    if r - l < 1.0 || b - t < 1.0 {
        return None;
    }
    Some(WinRect {
        x: l as i32,
        y: t as i32,
        w: (r - l) as i32,
        h: (b - t) as i32,
    })
}

/// What to clip against. A dimension the caller gave stands; a `0` is answered
/// with the far edge of the outputs themselves, which is the same number the
/// stitched screenshot ends up with. With no outputs reported there is nothing
/// to clip against either, so that dimension is left open.
fn desktop_size(maps: &[OutputMap], desk_w: u32, desk_h: u32) -> (f64, f64) {
    let (bw, bh) = maps.iter().fold((0.0f64, 0.0f64), |(w, h), m| {
        (w.max(m.px + m.lw * m.scale), h.max(m.py + m.lh * m.scale))
    });
    let pick = |given: u32, bound: f64| match given {
        0 if bound > 0.0 => bound,
        0 => f64::INFINITY,
        v => v as f64,
    };
    (pick(desk_w, bw), pick(desk_h, bh))
}

/// The printed lines turned into the list the selector wants: front to back,
/// clipped to the desktop, no duplicates.
fn parse(printed: &str, token: &str, desk_w: u32, desk_h: u32) -> Vec<WinRect> {
    let marker = format!("SXR|{token}|");
    let mut outs: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    let mut wins: Vec<(f64, f64, f64, f64)> = Vec::new();

    for line in printed.lines() {
        // the journal may or may not prefix the line depending on its format,
        // so the marker is searched for rather than expected at the front
        let Some(at) = line.find(&marker) else { continue };
        let f: Vec<&str> = line[at + marker.len()..].trim_end().split('|').collect();
        let num = |i: usize| f.get(i).and_then(|s| s.parse::<f64>().ok());
        match f.first() {
            Some(&"OUT") => {
                if let (Some(x), Some(y), Some(w), Some(h)) = (num(1), num(2), num(3), num(4)) {
                    // a KWin without devicePixelRatio still gives usable
                    // geometry, and scale 1 is the only sane guess
                    let s = num(5).filter(|s| *s > 0.0).unwrap_or(1.0);
                    outs.push((x, y, w, h, s));
                }
            }
            Some(&"WIN") => {
                // ShareX skips minimized windows; docks and the desktop stay,
                // the desktop sinking to the back of the order on its own where
                // it becomes the whole-monitor fallback rectangle
                if f.get(5) == Some(&"true") {
                    continue;
                }
                if let (Some(x), Some(y), Some(w), Some(h)) = (num(1), num(2), num(3), num(4)) {
                    if w > 0.0 && h > 0.0 {
                        wins.push((x, y, w, h));
                    }
                }
            }
            _ => {}
        }
    }

    let maps = map_outputs(&outs);
    let (dw, dh) = desktop_size(&maps, desk_w, desk_h);
    let mut out: Vec<WinRect> = Vec::with_capacity(wins.len());
    // stackingOrder is bottom-to-top; ShareX hit-tests front to back
    for &(x, y, w, h) in wins.iter().rev() {
        let (px, py, pw, ph) = to_pixels(&maps, x, y, w, h);
        if let Some(r) = clip(px, py, pw, ph, dw, dh) {
            // the frontmost of a set of identical rectangles is the one to keep
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}
