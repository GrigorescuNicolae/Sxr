//! The Wayland half of the region selector: plumbing and input translation.
//!
//! All the state and all the drawing live in [`crate::selector`]; this file
//! only carries pixels and events between it and the compositor. It puts one
//! `zwlr_layer_shell_v1` surface per `wl_output` over the live screen, hands
//! [`Sel`] every pointer and key event translated into its own vocabulary, and
//! copies what [`Painter`] draws into a `wl_shm` buffer. Because the pixels
//! come from the frozen screenshot (see `capture::grab_screen`) and not from
//! the compositor, the overlay never shows itself.
//!
//! The three jobs that are genuinely ours, and that `selector` cannot do:
//!
//!   * **translation** — `wl_pointer` counts buttons in evdev codes, scrolls in
//!     three different units and does not know what a double click is. Turning
//!     that into [`Input`] is pure, and `--input-test` checks it offline;
//!   * **timing** — [`Sel`] wants a monotonic clock since the overlay came up,
//!     and it wants a frame whenever it says it is animating. The other half of
//!     the job is holding it back: a pointer reports motion many times faster
//!     than a screen can show it, so redraws are paced by the compositor's own
//!     frame callback;
//!   * **damage** — [`Painter`] says which rectangles it touched, and only
//!     those are copied into the buffer and reported to the compositor.
//!
//! Coordinates: everything that crosses between the two halves is in *image
//! pixels*, that is pixels of the frozen screenshot, with the origin at the
//! top-left corner of the whole desktop. Each output owns the sub-rectangle of
//! that image its `wl_output.geometry` and `wl_output.mode` describe.

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use tiny_skia::Pixmap;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_keyboard::{self, WlKeyboard},
    wl_output::{self, WlOutput},
    wl_pointer::{self, WlPointer},
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

use crate::selector::{Box2, Btn, Input, Outcome, Painter, Pos, Sel};

// ------------------------------------------------------- input translation

/// The button codes `wl_pointer.button` reports, straight out of
/// `linux/input-event-codes.h` — the protocol says so in as many words.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

/// How close two presses have to be, in time and in pixels, to be one double
/// click. 400ms is what KDE ships as its double-click interval; 4px is the
/// slack a hand needs to hold still, and small enough that a deliberate drag
/// between the two presses is never mistaken for one.
const DOUBLE_TIME: Duration = Duration::from_millis(400);
const DOUBLE_DIST: i32 = 4;

/// One notch of a plain mouse wheel, in the surface units `wl_pointer.axis`
/// reports. `axis_value120` counts the same notch in 120ths, which is the unit
/// everything below is accumulated in.
/// How long a `wl_surface.frame` callback may stay unanswered before we draw
/// without it. Long enough that it never fires on a compositor that is merely
/// busy, short enough that one which has stopped answering altogether leaves a
/// sluggish overlay rather than a dead one.
const FRAME_WATCHDOG: Duration = Duration::from_millis(100);

const AXIS_NOTCH: f64 = 10.0;
const V120: i32 = 120;

/// The `wl_pointer` button code as the selector's own button, or `None` for one
/// it has no use for.
fn map_button(code: u32) -> Option<Btn> {
    match code {
        BTN_LEFT => Some(Btn::Left),
        BTN_RIGHT => Some(Btn::Right),
        BTN_MIDDLE => Some(Btn::Middle),
        BTN_SIDE => Some(Btn::Side),
        BTN_EXTRA => Some(Btn::Extra),
        _ => None,
    }
}

/// A pointer position reported on an output, in image pixels. Surface
/// coordinates are logical and relative to the surface; the frozen screen is in
/// physical pixels and spans the whole desktop, so the position is scaled up
/// and then moved to where the output sits in the screenshot.
fn to_image(out: Box2, scale: i32, sx: f64, sy: f64) -> Pos {
    let s = scale.max(1) as f64;
    Pos {
        x: out.x + (sx * s).round() as i32,
        y: out.y + (sy * s).round() as i32,
    }
}

/// The double-click detector.
///
/// Wayland has no double-click event: `wl_pointer` reports presses and releases
/// and that is all, so the pairing is ours to do. Two presses close enough in
/// time and in space make one — and the pair is then forgotten, because
/// otherwise the third press of a triple click would pair with the second and
/// fire a second double click.
#[derive(Default)]
struct Clicks {
    /// The last press that has not been paired off yet.
    last: Option<(Duration, Pos)>,
}

impl Clicks {
    /// Whether this press completes a double click.
    fn press(&mut self, at: Duration, p: Pos) -> bool {
        let double = self.last.is_some_and(|(t, q)| {
            at.saturating_sub(t) <= DOUBLE_TIME
                && (p.x - q.x).pow(2) + (p.y - q.y).pow(2) <= DOUBLE_DIST.pow(2)
        });
        self.last = if double { None } else { Some((at, p)) };
        double
    }
}

/// The scroll accumulator.
///
/// `wl_pointer` describes the same wheel movement in up to three ways, and a
/// compositor sends more than one of them at once: `axis_value120` (120ths of a
/// notch, v8 and up), the older `axis_discrete` (whole notches), and the
/// continuous `axis`, whose unit is a surface pixel. The exact counts win where
/// they are given; `axis` on its own is accumulated until it is worth a whole
/// notch, so a touchpad's fine scrolling still gets somewhere instead of
/// rounding to nothing every frame. Everything is added up per `wl_pointer
/// .frame`, which is the boundary the protocol draws around one movement.
///
/// The sign flips on the way in: Wayland counts scrolling *down* as positive,
/// the selector counts up as positive because up is what grows the magnifier.
#[derive(Default)]
struct Wheel {
    /// 120ths of a notch left over from earlier frames.
    carry: i32,
    /// The exact count this frame carried, if it carried one at all.
    exact: Option<i32>,
    /// What this frame's continuous `axis` events add up to, in 120ths.
    smooth: i32,
}

impl Wheel {
    /// `wl_pointer.axis_discrete`, in whole notches.
    fn discrete(&mut self, notches: i32) {
        *self.exact.get_or_insert(0) -= notches * V120;
    }

    /// `wl_pointer.axis_value120`, in 120ths of a notch.
    fn value120(&mut self, v: i32) {
        *self.exact.get_or_insert(0) -= v;
    }

    /// `wl_pointer.axis`, in surface units.
    fn axis(&mut self, v: f64) {
        self.smooth -= (v / AXIS_NOTCH * V120 as f64).round() as i32;
    }

    /// `wl_pointer.frame`: how many whole notches this frame was worth. The
    /// remainder is kept for the next one.
    fn frame(&mut self) -> i32 {
        self.carry += self.exact.take().unwrap_or(self.smooth);
        self.smooth = 0;
        let n = self.carry / V120;
        self.carry -= n * V120;
        n
    }
}

// ------------------------------------------------------------- shm buffers

/// An `mmap`ed `memfd`, unmapped when it goes away.
struct Map {
    ptr: *mut u8,
    len: usize,
}

impl Map {
    fn as_slice(&mut self) -> &mut [u8] {
        // safe: the mapping is `len` bytes long, writable, and lives as long
        // as `self` does
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
        }
    }
}

/// A shared-memory file of `len` bytes, plus its mapping.
fn shm_file(len: usize) -> Result<(OwnedFd, Map)> {
    unsafe {
        let raw = libc::memfd_create(c"sxr-overlay".as_ptr(), libc::MFD_CLOEXEC);
        if raw < 0 {
            bail!("memfd_create failed: {}", std::io::Error::last_os_error());
        }
        let fd = OwnedFd::from_raw_fd(raw);
        if libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) < 0 {
            bail!("ftruncate failed: {}", std::io::Error::last_os_error());
        }
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        );
        if ptr == libc::MAP_FAILED {
            bail!("mmap failed: {}", std::io::Error::last_os_error());
        }
        Ok((fd, Map { ptr: ptr.cast(), len }))
    }
}

/// Copies one rectangle of a pixmap into a `wl_shm` buffer of the same size.
///
/// `tiny_skia` keeps premultiplied RGBA in memory order R,G,B,A; `wl_shm`'s
/// `Xrgb8888`/`Argb8888` are little-endian 0xAARRGGBB words, which is B,G,R,A
/// in memory order. So the copy is not a memcpy: red and blue swap places.
/// Getting this wrong is what turns the overlay blue.
///
/// Only `r` is copied — the same rectangle the compositor is told about, so the
/// two can never disagree about what changed.
fn blit(pm: &Pixmap, dst: &mut [u8], r: Box2) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let Some(r) = r.intersect(Box2::new(0, 0, w, h)) else { return };
    let src = pm.pixels();
    for y in r.y..r.bottom() {
        let row = (y * w) as usize;
        for x in r.x..r.right() {
            let s = src[row + x as usize];
            let d = (row + x as usize) * 4;
            dst[d] = s.blue();
            dst[d + 1] = s.green();
            dst[d + 2] = s.red();
            dst[d + 3] = 255;
        }
    }
}

// ------------------------------------------------------------- wayland state

/// One `wl_output` and the layer surface that covers it.
struct Out {
    wl: WlOutput,
    /// `wl_output.geometry`, in the compositor's global (logical) space.
    pos: (i32, i32),
    /// `wl_output.mode`, in physical pixels.
    mode: (i32, i32),
    scale: i32,
    /// Where this output sits in the frozen screenshot.
    rect: Box2,

    surface: Option<WlSurface>,
    layer: Option<ZwlrLayerSurfaceV1>,
    /// Set once the first `configure` has been acknowledged.
    configured: bool,

    /// Buffer size in pixels; `(0, 0)` until there is a pool.
    dim: (i32, i32),
    pool: Option<WlShmPool>,
    map: Option<Map>,
    fd: Option<OwnedFd>,
    /// Two slots, so a frame can be drawn while the compositor still reads the
    /// previous one.
    bufs: Vec<WlBuffer>,
    busy: [bool; 2],

    /// The painter for this output, built once its size is known.
    painter: Option<Painter>,
    /// One pixmap per buffer slot, holding exactly what that slot holds. They
    /// are what makes partial damage possible: the painter repairs the parts of
    /// the *previous* frame that moved, so it has to be handed back the pixels
    /// it left there.
    pms: Vec<Pixmap>,
    /// Which render each slot was last drawn in, so the age can be worked out.
    drawn: [Option<u64>; 2],
    /// What the previous render had to repair. The compositor needs it too —
    /// see `draw_one`, where the two lists are reported together.
    last_dmg: Vec<Box2>,
    /// How many times this output has been rendered.
    renders: u64,
    /// Whether a `wl_surface.frame` callback is still outstanding.
    frame: bool,
    /// Set when a redraw was wanted but the frame callback had not come back
    /// yet. It is what tells the callback there is work waiting for it.
    pending: bool,
    /// When this output last committed, measured on the overlay's own clock.
    /// Only the watchdog below reads it.
    committed: Option<Duration>,
}

impl Out {
    fn new(wl: WlOutput) -> Self {
        Out {
            wl,
            pos: (0, 0),
            mode: (0, 0),
            scale: 1,
            rect: Box2::new(0, 0, 0, 0),
            surface: None,
            layer: None,
            configured: false,
            dim: (0, 0),
            pool: None,
            map: None,
            fd: None,
            bufs: Vec::new(),
            busy: [false; 2],
            painter: None,
            pms: Vec::new(),
            drawn: [None; 2],
            last_dmg: Vec::new(),
            renders: 0,
            frame: false,
            pending: false,
            committed: None,
        }
    }

    /// Whether the frame callback has been outstanding long enough that we
    /// should stop waiting for it. A compositor that never answers then costs
    /// us a slow overlay instead of a frozen one.
    fn overdue(&self, now: Duration) -> bool {
        self.committed.map_or(true, |t| now.saturating_sub(t) >= FRAME_WATCHDOG)
    }

    /// Drops the pool and everything that hangs off it. The slot pixmaps go
    /// with it: without their buffers there is nothing for them to mirror.
    fn drop_pool(&mut self) {
        for b in self.bufs.drain(..) {
            b.destroy();
        }
        if let Some(p) = self.pool.take() {
            p.destroy();
        }
        self.map = None;
        self.fd = None;
        self.dim = (0, 0);
        self.busy = [false; 2];
        self.pms = Vec::new();
        self.drawn = [None; 2];
        self.last_dmg = Vec::new();
        self.pending = false;
        self.committed = None;
    }
}

struct App {
    screen: Arc<RgbaImage>,
    shm: WlShm,
    compositor: WlCompositor,
    shell: ZwlrLayerShellV1,
    outs: Vec<Out>,

    pointer: Option<WlPointer>,
    keyboard: Option<WlKeyboard>,
    /// Which output the pointer is on, as an index into `outs`.
    focus: Option<usize>,
    clicks: Clicks,
    wheel: Wheel,

    /// The state machine. There is none until the outputs have said where they
    /// are, because that is what it is built from.
    sel: Option<Sel>,
    /// When the overlay was configured. `Sel` measures the input delay, the
    /// marching ants and the hover tween against this, so it has to be a real
    /// monotonic clock and not a frame counter.
    start: Option<Instant>,

    result: Option<Box2>,
    done: bool,
    dirty: bool,
    /// The first protocol-level failure; reported instead of a silent cancel.
    fatal: Option<String>,
}

impl App {
    /// How long the overlay has been up.
    fn now(&self) -> Duration {
        self.start.map(|t| t.elapsed()).unwrap_or_default()
    }

    /// Hands one event to the selector and acts on what it decided.
    fn feed(&mut self, ev: Input) {
        if self.done {
            return;
        }
        let now = self.now();
        let Some(sel) = self.sel.as_mut() else { return };
        match sel.feed(ev, now) {
            Outcome::Continue => {}
            Outcome::Region(r) => {
                self.result = Some(r);
                self.done = true;
            }
            Outcome::Cancel => {
                self.result = None;
                self.done = true;
            }
        }
        self.dirty = true;
    }

    /// Makes sure `outs[i]` has a pool big enough for two `w`x`h` frames.
    /// `Ok(true)` means the pool was just built, so the surface still needs to
    /// be told about the new size.
    fn ensure_pool(&mut self, i: usize, w: i32, h: i32, qh: &QueueHandle<Self>) -> Result<bool> {
        if self.outs[i].dim == (w, h) && self.outs[i].pool.is_some() {
            return Ok(false);
        }
        self.outs[i].drop_pool();
        let stride = w as usize * 4;
        let one = stride * h as usize;
        let (fd, map) = shm_file(one * 2).context("could not allocate the overlay buffer")?;
        let pool = self.shm.create_pool(fd.as_fd(), (one * 2) as i32, qh, ());
        let bufs = (0..2)
            .map(|s| {
                pool.create_buffer(
                    (one * s) as i32,
                    w,
                    h,
                    stride as i32,
                    wl_shm::Format::Xrgb8888,
                    qh,
                    (i, s),
                )
            })
            .collect();
        let o = &mut self.outs[i];
        o.fd = Some(fd);
        o.map = Some(map);
        o.pool = Some(pool);
        o.bufs = bufs;
        o.dim = (w, h);
        Ok(true)
    }

    /// Repaints every configured output that has a free buffer slot. Leaves
    /// `dirty` set when a slot was missing, so the `wl_buffer.release` that
    /// frees it wakes the loop up for another try.
    fn redraw(&mut self, qh: &QueueHandle<Self>) {
        let mut all = true;
        for i in 0..self.outs.len() {
            match self.draw_one(i, qh) {
                Ok(true) => {}
                Ok(false) => all = false,
                Err(e) => {
                    self.fatal.get_or_insert_with(|| e.to_string());
                    self.done = true;
                    return;
                }
            }
        }
        if all {
            self.dirty = false;
        }
    }

    /// Draws one output. `Ok(false)` means "no free buffer, try again later".
    fn draw_one(&mut self, i: usize, qh: &QueueHandle<Self>) -> Result<bool> {
        if !self.outs[i].configured || self.outs[i].rect.is_empty() || self.sel.is_none() {
            return Ok(true);
        }
        // One frame per compositor frame and no more. A pointer reports motion
        // far faster than a screen can show it — a thousand times a second is
        // ordinary for a gaming mouse — and drawing every report buys nothing:
        // it burns the CPU and floods the compositor with commits it will never
        // put on screen. While a callback is outstanding the redraw is
        // deferred, and the callback is what wakes us for it.
        if self.outs[i].frame && !self.outs[i].overdue(self.now()) {
            self.outs[i].pending = true;
            return Ok(false);
        }
        let scale = self.outs[i].scale.max(1);
        let (w, h) = (self.outs[i].rect.w, self.outs[i].rect.h);
        if self.ensure_pool(i, w, h, qh)? {
            // The overlay has no transparent pixel anywhere, so saying so lets
            // the compositor skip blending whatever is underneath. The region
            // is in surface-local (logical) coordinates, hence the divide.
            let r = self.compositor.create_region(qh, ());
            r.add(0, 0, w / scale, h / scale);
            if let Some(s) = self.outs[i].surface.as_ref() {
                s.set_opaque_region(Some(&r));
            }
            r.destroy();
        }
        let Some(slot) = (0..2).find(|s| !self.outs[i].busy[*s]) else {
            return Ok(false);
        };
        let now = self.now();

        // `sel` and `outs` are separate fields, so both can be borrowed at
        // once; a `&mut self` method here would not compile.
        let App { screen, sel, outs, .. } = self;
        let sel = sel.as_ref().expect("checked at the top");
        let o = &mut outs[i];
        let rect = o.rect;

        // A resized output invalidates the dimmed backdrop, the slot pixmaps
        // and everything we believed the slots held.
        if o.painter.as_ref().map(Painter::out) != Some(rect) || o.pms.len() != 2 {
            o.painter = Some(Painter::new(screen, rect));
            o.pms = (0..2)
                .map(|_| Pixmap::new(w as u32, h as u32).expect("an output is never 0x0"))
                .collect();
            o.drawn = [None; 2];
        }

        // How many renders ago this slot was last drawn into. A slot that has
        // never been drawn, or that is older than the painter remembers, gets
        // `None` and a full repaint — the alternative is a trail of old dashes
        // left behind wherever the last frame is not what we think it is.
        let age = o.drawn[slot].map(|n| (o.renders - n).min(u32::MAX as u64) as u32);
        let painter = o.painter.as_mut().expect("just filled in");
        let dmg = painter.render(&mut o.pms[slot], screen, sel, now, age);
        o.drawn[slot] = Some(o.renders);
        o.renders += 1;

        let one = w as usize * 4 * h as usize;
        let map = o.map.as_mut().expect("ensure_pool left a mapping");
        let dst = &mut map.as_slice()[slot * one..(slot + 1) * one];
        for r in &dmg {
            blit(&o.pms[slot], dst, *r);
        }

        let surface = o.surface.as_ref().expect("configured means there is a surface");
        surface.set_buffer_scale(scale);
        surface.attach(Some(&o.bufs[slot]), 0, 0);
        // `damage_buffer` says what changed against what is *on screen*, and
        // that is the other slot's frame — one render old, not two. `dmg` only
        // covers the difference from what this slot itself held, so the
        // rectangles the render in between moved have to be added or their old
        // dashes stay on screen: with the pointer moving faster than we redraw,
        // the three frames barely overlap and the middle one leaves a trail.
        // The buffer is a correct frame everywhere, so the extra rectangles
        // never show anything wrong — they only cost the compositor a copy.
        for r in dmg.iter().chain(o.last_dmg.iter()) {
            surface.damage_buffer(r.x, r.y, r.w, r.h);
        }
        // A callback after every frame, not only the animated ones: it is what
        // paces the next one. The marching ants and the magnifier move on their
        // own, so when something is animating the callback also asks for the
        // redraw; when nothing is, it only lifts the throttle and we go back to
        // sleep until the next event arrives.
        if !o.frame {
            let _ = surface.frame(qh, i);
            o.frame = true;
        }
        surface.commit();
        o.busy[slot] = true;
        o.last_dmg = dmg;
        o.pending = false;
        o.committed = Some(now);
        Ok(true)
    }

    /// Tears the surfaces down. Without this the overlay stays on screen until
    /// the process exits, which is exactly the kind of thing that traps a user.
    fn teardown(&mut self) {
        for o in &mut self.outs {
            o.painter = None;
            o.drop_pool();
            if let Some(l) = o.layer.take() {
                l.destroy();
            }
            if let Some(s) = o.surface.take() {
                s.destroy();
            }
        }
        if let Some(p) = self.pointer.take() {
            if p.version() >= 3 {
                p.release();
            }
        }
        if let Some(k) = self.keyboard.take() {
            if k.version() >= 3 {
                k.release();
            }
        }
    }
}

// ------------------------------------------------------------- dispatching

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The overlay lives for a couple of seconds; an output plugged in
        // half-way through is not worth the bookkeeping.
    }
}

impl Dispatch<WlOutput, usize> for App {
    fn event(
        app: &mut Self,
        _: &WlOutput,
        ev: wl_output::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(o) = app.outs.get_mut(i) else { return };
        match ev {
            wl_output::Event::Geometry { x, y, .. } => o.pos = (x, y),
            wl_output::Event::Mode { flags, width, height, .. } => {
                // only the mode that is actually in use
                if matches!(flags, WEnum::Value(f) if f.contains(wl_output::Mode::Current)) {
                    o.mode = (width, height);
                }
            }
            wl_output::Event::Scale { factor } => o.scale = factor.max(1),
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for App {
    fn event(
        app: &mut Self,
        seat: &WlSeat,
        ev: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = ev {
            if c.contains(wl_seat::Capability::Pointer) && app.pointer.is_none() {
                app.pointer = Some(seat.get_pointer(qh, ()));
            }
            if c.contains(wl_seat::Capability::Keyboard) && app.keyboard.is_none() {
                app.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for App {
    fn event(
        app: &mut Self,
        ptr: &WlPointer,
        ev: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Only the vertical wheel means anything here; a sideways scroll has
        // nothing to change.
        let vertical =
            |a: WEnum<wl_pointer::Axis>| matches!(a, WEnum::Value(wl_pointer::Axis::VerticalScroll));

        match ev {
            wl_pointer::Event::Enter { serial, surface, surface_x, surface_y } => {
                // `selector` draws ShareX's own crosshair as `Item::Cursor`, so
                // the compositor's pointer has to go or there would be two. A
                // `None` surface is how one is hidden, and it has to be done on
                // every enter: the cursor belongs to the focus, not to us.
                ptr.set_cursor(serial, None, 0, 0);
                app.focus = app
                    .outs
                    .iter()
                    .position(|o| o.surface.as_ref().is_some_and(|s| s.id() == surface.id()));
                if let Some(i) = app.focus {
                    let o = &app.outs[i];
                    let p = to_image(o.rect, o.scale, surface_x, surface_y);
                    app.feed(Input::Motion(p));
                }
            }
            wl_pointer::Event::Leave { .. } => {
                // The overlay covers every output, so the pointer only ever
                // leaves one of our surfaces for another and the next `enter`
                // puts the focus right. During a drag neither event is sent at
                // all: the implicit grab keeps the motion coming to the surface
                // the press landed on, which is what we want.
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if let Some(i) = app.focus {
                    let o = &app.outs[i];
                    let p = to_image(o.rect, o.scale, surface_x, surface_y);
                    // Sub-pixel motion inside one pixel would redraw for
                    // nothing; the selector only ever sees whole pixels.
                    if app.sel.as_ref().map(Sel::cursor) != Some(p) {
                        app.feed(Input::Motion(p));
                    }
                }
            }
            wl_pointer::Event::Button { button, state, .. } => {
                let down = matches!(state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                let Some(btn) = map_button(button) else { return };
                app.feed(Input::Button { btn, down });
                // The double click goes *on top of* the presses, never instead
                // of them: `Sel` counts both, and ShareX's own handler does the
                // same thing.
                if btn == Btn::Left && down {
                    let (now, at) = (app.now(), app.sel.as_ref().map(Sel::cursor).unwrap_or_default());
                    if app.clicks.press(now, at) {
                        app.feed(Input::DoubleClick);
                    }
                }
            }
            wl_pointer::Event::Axis { axis, value, .. } if vertical(axis) => {
                app.wheel.axis(value);
                // `wl_pointer.frame` only exists from version 5; below that
                // every axis event stands on its own.
                if ptr.version() < 5 {
                    let n = app.wheel.frame();
                    if n != 0 {
                        app.feed(Input::Wheel(n));
                    }
                }
            }
            wl_pointer::Event::AxisDiscrete { axis, discrete } if vertical(axis) => {
                app.wheel.discrete(discrete);
            }
            wl_pointer::Event::AxisValue120 { axis, value120 } if vertical(axis) => {
                app.wheel.value120(value120);
            }
            wl_pointer::Event::Frame => {
                let n = app.wheel.frame();
                if n != 0 {
                    app.feed(Input::Wheel(n));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &WlKeyboard,
        ev: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { key, state, .. } = ev {
            // No xkb here, and no `+ 8` either: `key` is the raw evdev code and
            // that is exactly what `selector::key` is written against. The 8 is
            // what X11 adds to make an xkb keycode, and adding it would shift
            // every key the selector knows.
            let down = matches!(state, WEnum::Value(wl_keyboard::KeyState::Pressed));
            app.feed(Input::Key { code: key, down });
        }
    }
}

impl Dispatch<WlCallback, usize> for App {
    fn event(
        app: &mut Self,
        _: &WlCallback,
        ev: wl_callback::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(ev, wl_callback::Event::Done { .. }) {
            // Something animating wants the next frame unprompted; anything
            // else only wants one if a redraw was held back waiting for this.
            let now = app.now();
            let mut wake = app.sel.as_ref().is_some_and(|s| s.animating(now));
            if let Some(o) = app.outs.get_mut(i) {
                o.frame = false;
                wake |= o.pending;
            }
            if wake {
                app.dirty = true;
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for App {
    fn event(
        app: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        ev: zwlr_layer_surface_v1::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                // The clock the selector measures everything against starts
                // here, at the first moment there is something on screen to
                // react to — anything earlier would spend the 500ms input delay
                // on the Wayland handshake.
                app.start.get_or_insert_with(Instant::now);
                let Some(o) = app.outs.get_mut(i) else { return };
                // The size the compositor hands back is logical; the buffer is
                // in pixels. Anchored on all four edges it is the whole output,
                // so this agrees with `mode` — but the compositor has the last
                // word, so we follow it.
                let s = o.scale.max(1);
                let (w, h) = ((width as i32 * s).max(1), (height as i32 * s).max(1));
                if (w, h) != (o.rect.w, o.rect.h) {
                    o.rect = Box2::new(o.rect.x, o.rect.y, w, h);
                }
                o.configured = true;
                app.dirty = true;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                // The compositor took the surface away; there is nothing left
                // to select on.
                app.result = None;
                app.done = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, (usize, usize)> for App {
    fn event(
        app: &mut Self,
        _: &WlBuffer,
        ev: wl_buffer::Event,
        &(i, slot): &(usize, usize),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(ev, wl_buffer::Event::Release) {
            if let Some(o) = app.outs.get_mut(i) {
                if slot < o.busy.len() {
                    o.busy[slot] = false;
                }
            }
        }
    }
}

delegate_noop!(App: ignore WlCompositor);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlShmPool);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlRegion);
delegate_noop!(App: ignore ZwlrLayerShellV1);

// ------------------------------------------------------------- the loop

/// Shows the frozen screen and lets the user pick a region over it. `windows`
/// are the on-screen window rectangles to snap to, front-to-back, in the pixels
/// of `screen`; an empty list simply means no snapping. Returns the selection
/// in image pixels, or `None` if it was cancelled.
pub fn select(screen: &RgbaImage, windows: Vec<Box2>) -> Result<Option<(u32, u32, u32, u32)>> {
    // The event queue hands the state back as `&mut App` from a `'static`
    // context, so the image cannot be borrowed across it; one copy up front is
    // the price, and it is paid once per capture.
    let screen = Arc::new(screen.clone());

    let conn = Connection::connect_to_env().context("no Wayland connection")?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&conn).context("could not read the Wayland globals")?;
    let qh = queue.handle();

    let compositor = globals
        .bind::<WlCompositor, _, _>(&qh, 4..=6, ())
        .context("the compositor does not offer wl_compositor v4")?;
    let shm = globals
        .bind::<WlShm, _, _>(&qh, 1..=1, ())
        .context("the compositor does not offer wl_shm")?;
    let shell = globals
        .bind::<ZwlrLayerShellV1, _, _>(&qh, 1..=5, ())
        .context("the compositor does not offer zwlr_layer_shell_v1")?;
    // v9 so the wheel can arrive as `axis_value120`; older compositors fall
    // back to `axis_discrete` or to the continuous `axis` on their own.
    let _seat = globals
        .bind::<WlSeat, _, _>(&qh, 1..=9, ())
        .context("the compositor does not offer wl_seat")?;

    // One `wl_output` per screen; the index is the object's user data, so the
    // events find their way back without a lookup.
    let mut outs = Vec::new();
    for g in globals.contents().clone_list() {
        if g.interface == WlOutput::interface().name {
            let i = outs.len();
            let wl = globals
                .registry()
                .bind::<WlOutput, _, _>(g.name, g.version.min(4), &qh, i);
            outs.push(Out::new(wl));
        }
    }
    if outs.is_empty() {
        bail!("the compositor reported no outputs");
    }

    let mut app = App {
        screen,
        shm,
        compositor,
        shell,
        outs,
        pointer: None,
        keyboard: None,
        focus: None,
        clicks: Clicks::default(),
        wheel: Wheel::default(),
        sel: None,
        start: None,
        result: None,
        done: false,
        dirty: true,
        fatal: None,
    };

    // geometry, mode, scale and the seat capabilities
    queue.roundtrip(&mut app).context("the Wayland roundtrip failed")?;

    // Where each output sits in the screenshot. `geometry` is in logical
    // coordinates and `mode` in pixels, so the origin is scaled up while the
    // size is taken as it stands. That is exact for a uniform scale factor
    // (including the plain 1 this is normally used with); a mixed-DPI desktop
    // would need `zxdg_output_manager_v1` for the logical sizes.
    for o in &mut app.outs {
        let s = o.scale.max(1);
        let (w, h) = if o.mode == (0, 0) { (1, 1) } else { o.mode };
        o.rect = Box2::new(o.pos.0 * s, o.pos.1 * s, w, h);
    }

    // The monitors as the selector sees them, left to right and then top to
    // bottom. The digit keys 1-9 address them in this order, so it has to come
    // from where the screens actually are and not from whatever order the
    // registry happened to advertise them in.
    let mut rects: Vec<Box2> = app.outs.iter().map(|o| o.rect).collect();
    rects.sort_by_key(|r| (r.x, r.y));
    let desktop = {
        let l = rects.iter().map(|r| r.x).min().unwrap_or(0);
        let t = rects.iter().map(|r| r.y).min().unwrap_or(0);
        let r = rects.iter().map(|r| r.right()).max().unwrap_or(0);
        let b = rects.iter().map(|r| r.bottom()).max().unwrap_or(0);
        Box2::new(l, t, r - l, b - t)
    };
    app.sel = Some(Sel::new(desktop, rects, windows));

    // The surfaces themselves.
    for i in 0..app.outs.len() {
        let surface = app.compositor.create_surface(&qh, ());
        let layer = app.shell.get_layer_surface(
            &surface,
            Some(&app.outs[i].wl),
            zwlr_layer_shell_v1::Layer::Overlay,
            "sxr-region".to_owned(),
            &qh,
            i,
        );
        // anchored to all four edges, so the compositor sizes it to the whole
        // output and `set_size(0, 0)` is the right thing to ask for
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_size(0, 0);
        // -1: ignore every panel's exclusive zone and cover them too
        layer.set_exclusive_zone(-1);
        layer.set_margin(0, 0, 0, 0);
        // so Esc reaches us and not whatever had the focus
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
        // the opaque region needs the configured size, so it is set with the
        // first buffer instead (see `draw_one`)
        // no buffer yet: the first commit only asks for a configure
        surface.commit();
        app.outs[i].surface = Some(surface);
        app.outs[i].layer = Some(layer);
    }

    // the configure events, then the first frame
    queue
        .roundtrip(&mut app)
        .context("the compositor did not configure the overlay")?;
    app.redraw(&qh);

    while !app.done {
        // `blocking_dispatch` only flushes when it had nothing pending, and a
        // moving mouse means it almost always has; without this the frames we
        // just queued would sit in the send buffer instead of reaching the
        // screen.
        conn.flush().context("could not flush the Wayland queue")?;
        queue
            .blocking_dispatch(&mut app)
            .context("the Wayland connection broke")?;
        if app.dirty && !app.done {
            app.redraw(&qh);
        }
    }

    app.teardown();
    let _ = queue.roundtrip(&mut app);
    if let Some(e) = app.fatal.take() {
        bail!(e);
    }

    // Back to image pixels, clipped to what the screenshot actually holds.
    let full = Box2::new(0, 0, app.screen.width() as i32, app.screen.height() as i32);
    Ok(app
        .result
        .and_then(|r| r.intersect(full))
        .filter(|r| !r.is_empty())
        .map(|r| (r.x as u32, r.y as u32, r.w as u32, r.h as u32)))
}

// ------------------------------------------------------------- offline check

fn ck(fail: &mut usize, ok: bool, msg: String) {
    if !ok {
        *fail += 1;
    }
    println!("{} {msg}", if ok { "OK" } else { "FAILED" });
}

/// `--input-test`: the translation from `wl_pointer` and `wl_keyboard` numbers
/// into [`Input`], replayed without a compositor.
///
/// This is the only part of the overlay that can be checked offline. The rest
/// of the file needs a live session, and raising a layer surface from a test
/// would put a keyboard-grabbing window over whoever ran it, so the translation
/// is deliberately written as plain functions with no Wayland in them.
pub fn input_test() -> Result<()> {
    let mut fail = 0usize;
    let ms = Duration::from_millis;

    // 1. every button the selector knows, and nothing else
    for (code, want, name) in [
        (BTN_LEFT, Btn::Left, "BTN_LEFT"),
        (BTN_RIGHT, Btn::Right, "BTN_RIGHT"),
        (BTN_MIDDLE, Btn::Middle, "BTN_MIDDLE"),
        (BTN_SIDE, Btn::Side, "BTN_SIDE"),
        (BTN_EXTRA, Btn::Extra, "BTN_EXTRA"),
    ] {
        let got = map_button(code);
        ck(&mut fail, got == Some(want), format!("1 {name} ({code:#x}) -> {got:?}"));
    }
    ck(&mut fail, map_button(0x115).is_none(), "1 BTN_FORWARD is not one of ours".into());
    ck(&mut fail, map_button(0).is_none(), "1 button 0 is not one of ours".into());

    // 2. the keyboard hands over raw evdev codes; the xkb offset of 8 must
    //    never creep in, or Esc would arrive as Ctrl
    ck(&mut fail, crate::selector::key::ESC == 1, "2 Esc is evdev 1, not xkb 9".into());

    // 3. two presses close in time and place are a double click
    let mut c = Clicks::default();
    ck(&mut fail, !c.press(ms(0), Pos { x: 100, y: 100 }), "3 the first press is never a double".into());
    let d = c.press(ms(100), Pos { x: 102, y: 100 });
    ck(&mut fail, d, format!("3 100ms and 2px later: {d}"));

    // 4. too slow is not
    let mut c = Clicks::default();
    c.press(ms(0), Pos { x: 100, y: 100 });
    let d = c.press(ms(500), Pos { x: 100, y: 100 });
    ck(&mut fail, !d, format!("4 500ms later: {d}"));

    // 5. and neither is too far
    let mut c = Clicks::default();
    c.press(ms(0), Pos { x: 100, y: 100 });
    let d = c.press(ms(100), Pos { x: 110, y: 100 });
    ck(&mut fail, !d, format!("5 100ms but 10px later: {d}"));

    // 6. a triple click is one double click, not two
    let mut c = Clicks::default();
    let n = (0..3).filter(|k| c.press(ms(k * 100), Pos { x: 40, y: 40 })).count();
    ck(&mut fail, n == 1, format!("6 three presses give {n} double click(s)"));

    // 7. one exact notch, in either direction; up is positive for us and
    //    negative for Wayland
    let mut w = Wheel::default();
    w.value120(-120);
    let n = w.frame();
    ck(&mut fail, n == 1, format!("7 value120 -120 (up) -> {n}"));
    w.value120(120);
    let n = w.frame();
    ck(&mut fail, n == -1, format!("7 value120 +120 (down) -> {n}"));
    w.discrete(-2);
    let n = w.frame();
    ck(&mut fail, n == 2, format!("7 axis_discrete -2 -> {n}"));

    // 8. a fraction of a notch is kept until it is worth one
    let mut w = Wheel::default();
    w.value120(-60);
    let a = w.frame();
    w.value120(-60);
    let b = w.frame();
    ck(&mut fail, (a, b) == (0, 1), format!("8 two half notches -> {a}, then {b}"));

    // 9. the continuous axis alone, ten surface units to the notch
    let mut w = Wheel::default();
    w.axis(-10.0);
    let n = w.frame();
    ck(&mut fail, n == 1, format!("9 axis -10.0 -> {n}"));
    w.axis(-5.0);
    let a = w.frame();
    w.axis(-5.0);
    let b = w.frame();
    ck(&mut fail, (a, b) == (0, 1), format!("9 two half notches of axis -> {a}, then {b}"));

    // 10. an exact count and the `axis` that describes the same movement must
    //     count once, whichever order they arrive in
    let mut w = Wheel::default();
    w.value120(-120);
    w.axis(-10.0);
    let n = w.frame();
    ck(&mut fail, n == 1, format!("10 value120 then axis, one movement -> {n}"));
    w.axis(-10.0);
    w.value120(-120);
    let n = w.frame();
    ck(&mut fail, n == 1, format!("10 axis then value120, one movement -> {n}"));

    // 11. surface coordinates become desktop pixels: the output's own origin
    //     plus the position taken through its scale
    let right = Box2::new(1920, 0, 1920, 1080);
    let p = to_image(right, 1, 100.4, 200.6);
    ck(&mut fail, p == Pos { x: 2020, y: 201 }, format!("11 unscaled: {p:?}"));
    let hidpi = Box2::new(0, 1080, 2560, 1440);
    let p = to_image(hidpi, 2, 100.0, 50.0);
    ck(&mut fail, p == Pos { x: 200, y: 1180 }, format!("11 scale 2: {p:?}"));

    if fail > 0 {
        anyhow::bail!("{fail} checks failed");
    }
    println!("all good");
    Ok(())
}
