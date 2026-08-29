//! The region selector, ShareX's classic `RegionCaptureForm`, without a window.
//!
//! Everything here is pure: a frozen screenshot goes in, pixels and decisions
//! come out. There is not a single Wayland call in this file and there must
//! never be one — the compositor half (`overlay.rs`) only pumps events into
//! [`Sel`] and copies what [`Painter`] draws into a `wl_shm` buffer. That split
//! is what makes the selector testable at all: a layer surface cannot be raised
//! from a test, but `--sel-shot` and `--sel-flow` exercise every rule in here
//! without a compositor in sight.
//!
//! The numbers are not guesses. They come from ShareX's own sources — mainly
//! `ShareX.ScreenCaptureLib/Forms/RegionCaptureForm.cs`,
//! `ShareX.ScreenCaptureLib/RegionCaptureOptions.cs`,
//! `ShareX.ScreenCaptureLib/Shapes/ShapeManager.cs` and
//! `ShareX.HelpersLib/ShareXTheme.cs` — and each constant below says where it
//! is from, so a later reader can check it instead of trusting it.
//!
//! Coordinates: everything the two halves exchange is in *image pixels*, that
//! is pixels of the frozen screenshot, with the origin at the top-left corner
//! of the whole desktop. Each output owns the sub-rectangle of that image its
//! `wl_output` geometry describes.

use std::time::Duration;

use anyhow::{Context, Result};
use image::RgbaImage;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::font;

// ---------------------------------------------------------------- ShareX values

/// `Options.BackgroundDimStrength = 20`, applied as
/// `alpha = round(255 * 20/100) = 51` of black over the frozen screen. The
/// factor is kept rather than the alpha so the backdrop is one multiply.
const VEIL_KEEP: f32 = 1.0 - 51.0 / 255.0; // exactly 0.8

/// `Options.MinimumSize` / `DefaultMinimumSize = 5`. A shape is valid only when
/// both sides reach it.
const MIN_SIZE: i32 = 5;

/// `Options.InputDelay = 500`. Keys arriving before this are dropped, which is
/// what stops the hotkey that launched us from closing us again.
const INPUT_DELAY: Duration = Duration::from_millis(500);

/// `borderDotPen.DashOffset = elapsed.TotalSeconds * -15`, and the pen's
/// `DashPattern = { 5, 5 }`. The offset is in pen widths, and the pen is 1px.
const DASH_ON: f32 = 5.0;
const DASH_PERIOD: f32 = 10.0;
const DASH_SPEED: f32 = -15.0;

/// `Options.SnapSizes` — 240p to 1080p — and `SnapDistance = 30`.
const SNAP_SIZES: [(i32, i32); 5] = [(426, 240), (640, 360), (854, 480), (1280, 720), (1920, 1080)];
const SNAP_DISTANCE: f32 = 30.0;

/// `MoveSpeedMinimum` / `MoveSpeedMaximum`, the arrow key step without and
/// with Shift.
const MOVE_SLOW: i32 = 1;
const MOVE_FAST: i32 = 10;

/// `RectangleAnimation { Duration = TimeSpan.FromMilliseconds(200) }`, the
/// tween between one hovered window rectangle and the next.
const HOVER_TWEEN: Duration = Duration::from_millis(200);

/// `MagnifierPixelCountMinimum/Maximum` and `MagnifierPixelSizeMinimum/Maximum`.
const MAG_COUNT_MIN: i32 = 3;
const MAG_COUNT_MAX: i32 = 35;
const MAG_SIZE_MIN: i32 = 3;
const MAG_SIZE_MAX: i32 = 30;

/// `DrawCursorGraphics`: `cursorOffsetX = 10, cursorOffsetY = 10, itemGap = 10`
/// and the info box's own `infoTextPadding = 3`.
const CURSOR_OFFSET: i32 = 10;
const ITEM_GAP: i32 = 10;
const INFO_PAD: i32 = 3;

/// `DrawAreaText`: `offset = 6, backgroundPadding = 3`.
const AREA_OFFSET: i32 = 6;
const AREA_PAD: i32 = 3;

/// `DrawCrosshair`: the gap left around the cursor, and the shortest segment
/// still worth drawing.
const GUIDE_GAP: i32 = 5;
const GUIDE_MIN: i32 = 10;

/// The default dark `ShareXTheme`, verbatim from `ShareXTheme.DarkTheme`.
const THEME_BG: [u8; 3] = [39, 39, 39];
const THEME_TEXT: [u8; 3] = [231, 233, 234];
const THEME_BORDER: [u8; 3] = [31, 31, 31];
const THEME_SEP_DARK: [u8; 3] = [31, 31, 31];
const THEME_SEP_LIGHT: [u8; 3] = [44, 44, 44];

/// The alphas `RegionCaptureForm` fixes in code around those theme colours:
/// `Color.FromArgb(200, ...)` for the box and its two borders. The text and
/// its shadow are opaque.
const TEXT_BOX_A: f32 = 200.0 / 255.0;

/// `markerPen = new Pen(Color.FromArgb(200, Color.Red))`, the snap previews.
const MARKER: [u8; 3] = [255, 0, 0];
const MARKER_A: f32 = 200.0 / 255.0;

/// The magnifier's pixel grid, `new Pen(Color.FromArgb(75, Color.Black))`.
const MAG_GRID_A: f32 = 75.0 / 255.0;
/// Its centre crosshair, `Color.FromArgb(125, Color.LightBlue)`; `LightBlue`
/// is `#ADD8E6`.
const MAG_CROSS: [u8; 3] = [0xAD, 0xD8, 0xE6];
const MAG_CROSS_A: f32 = 125.0 / 255.0;

/// `infoFont = new Font("Verdana", 9)`. GDI+ takes that as 9pt at 96dpi, so an
/// em of 9 * 96/72 = 12px. We have no Verdana, and DejaVu Sans is the closest
/// thing embedded here: both are humanist sans faces with a tall x-height and a
/// wide advance, so 12px keeps the readout the same physical size as ShareX's
/// even though the glyph shapes differ.
const INFO_SIZE: f32 = 12.0;

// ------------------------------------------------------------------- geometry

/// A point in image pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in image pixels. `w` and `h` are never negative.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Box2 {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Box2 {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Box2 { x, y, w: w.max(0), h: h.max(0) }
    }

    /// The rectangle spanned by two dragged corners.
    ///
    /// ShareX's `CaptureHelpers.CreateRectangle` counts *both* endpoints, so a
    /// drag from x=100 to x=105 is 6 pixels wide, not 5. That inclusive
    /// convention is kept here because `MinimumSize` and the snap sizes are
    /// written against it: `CalculateNewPosition` lands on
    /// `start + width - 1` precisely because the far pixel is included.
    pub fn from_corners(a: Pos, b: Pos) -> Self {
        Box2 {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x - b.x).abs() + 1,
            h: (a.y - b.y).abs() + 1,
        }
    }

    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(self, p: Pos) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    /// Whether `o` lies entirely inside `self` — `Rectangle.Contains(rect)`.
    pub fn holds(self, o: Box2) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// The common part of the two, or `None` when they do not touch.
    pub fn intersect(self, o: Box2) -> Option<Box2> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        (r > x && b > y).then(|| Box2::new(x, y, r - x, b - y))
    }

    /// Grown by `n` on every side. Used to make damage rectangles cover the
    /// antialiased fringe of the glyphs and the ellipse rings.
    fn grow(self, n: i32) -> Box2 {
        Box2::new(self.x - n, self.y - n, self.w + n * 2, self.h + n * 2)
    }

    fn shift(self, dx: i32, dy: i32) -> Box2 {
        Box2 { x: self.x + dx, y: self.y + dy, ..self }
    }
}

// -------------------------------------------------------------------- input

/// The mouse buttons the selector reacts to. `Side`/`Extra` are evdev's
/// `BTN_SIDE`/`BTN_EXTRA`, what ShareX calls X1 and X2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Left,
    Right,
    Middle,
    Side,
    Extra,
}

/// One thing the user did.
///
/// Keys arrive as raw evdev codes on purpose: there is no xkb here, and a
/// layout-independent scancode is both the smaller dependency and the more
/// predictable one — Esc is Esc on a Dvorak keyboard too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Motion(Pos),
    Button { btn: Btn, down: bool },
    DoubleClick,
    Wheel(i32),
    Key { code: u32, down: bool },
}

/// What the selector decided. `Continue` means keep going.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Continue,
    Region(Box2),
    Cancel,
}

/// evdev scancodes, `linux/input-event-codes.h`.
pub mod key {
    pub const ESC: u32 = 1;
    pub const N1: u32 = 2;
    pub const N0: u32 = 11;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const LEFTCTRL: u32 = 29;
    pub const C: u32 = 46;
    pub const LEFTSHIFT: u32 = 42;
    pub const RIGHTSHIFT: u32 = 54;
    pub const LEFTALT: u32 = 56;
    pub const SPACE: u32 = 57;
    pub const GRAVE: u32 = 41;
    pub const KPENTER: u32 = 96;
    pub const RIGHTCTRL: u32 = 97;
    pub const RIGHTALT: u32 = 100;
    pub const UP: u32 = 103;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const DOWN: u32 = 108;
    pub const INSERT: u32 = 110;
    pub const DELETE: u32 = 111;
}

// ------------------------------------------------------------------- the state

/// A hover rectangle sliding towards the next one.
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: Box2,
    to: Box2,
    start: Duration,
}

impl Tween {
    /// `MathHelpers.Lerp` on the four numbers independently, no easing, capped
    /// at 1. This is `RectangleAnimation.Update` line for line.
    fn at(&self, now: Duration) -> Box2 {
        let t = if HOVER_TWEEN.is_zero() {
            1.0
        } else {
            (now.saturating_sub(self.start).as_secs_f32() / HOVER_TWEEN.as_secs_f32()).min(1.0)
        };
        let l = |a: i32, b: i32| (a as f32 + (b - a) as f32 * t).round() as i32;
        Box2::new(
            l(self.from.x, self.to.x),
            l(self.from.y, self.to.y),
            l(self.from.w, self.to.w),
            l(self.from.h, self.to.h),
        )
    }

    fn done(&self, now: Duration) -> bool {
        now.saturating_sub(self.start) >= HOVER_TWEEN
    }
}

/// The whole region selector as a state machine: it knows the desktop, the
/// monitors, the windows worth snapping to, and what the mouse and the keyboard
/// have done so far. It draws nothing — [`Painter`] does that from what
/// [`Sel::items`] describes.
pub struct Sel {
    desktop: Box2,
    outs: Vec<Box2>,
    windows: Vec<Box2>,

    cursor: Pos,
    /// The drag's anchor. `IsCornerMoving` moves it, so it is not simply the
    /// press position.
    start: Pos,
    creating: bool,
    /// The finished shape. With `quick_crop` on this is only ever reached
    /// through Insert, since a normal mouse-up ends the whole selection.
    shape: Option<Box2>,

    ctrl: bool,
    shift: bool,
    alt: bool,
    /// `IsCornerMoving`: Ctrl went down while a shape was being created.
    corner_moving: bool,

    hover: Option<Box2>,
    tween: Option<Tween>,

    keys_open: bool,
    now: Duration,

    /// `Options.QuickCrop`, default true: one valid drag ends the selection.
    pub quick_crop: bool,
    /// `Options.ShowCrosshair`, default **false** — the screen-wide guides are
    /// off in ShareX and they are off here.
    pub show_crosshair: bool,
    /// `Options.ShowInfo`, default true.
    pub show_info: bool,
    /// `Options.ShowMagnifier`, default true.
    pub show_magnifier: bool,
    /// `Options.UseSquareMagnifier`, default false — so it is round.
    pub square_magnifier: bool,
    /// `Options.MagnifierPixelCount`, default 15, always odd.
    pub mag_count: i32,
    /// `Options.MagnifierPixelSize`, default 10.
    pub mag_size: i32,
}

impl Sel {
    /// `outs` are the monitors in screenshot pixels, in the order the digit
    /// keys address them. `windows` are the snap rectangles, front-to-back.
    pub fn new(desktop: Box2, outs: Vec<Box2>, windows: Vec<Box2>) -> Self {
        Sel {
            desktop,
            outs,
            windows,
            cursor: Pos { x: desktop.x + desktop.w / 2, y: desktop.y + desktop.h / 2 },
            start: Pos::default(),
            creating: false,
            shape: None,
            ctrl: false,
            shift: false,
            alt: false,
            corner_moving: false,
            hover: None,
            tween: None,
            keys_open: false,
            now: Duration::ZERO,
            quick_crop: true,
            show_crosshair: false,
            show_info: true,
            show_magnifier: true,
            square_magnifier: false,
            mag_count: 15,
            mag_size: 10,
        }
    }

    // ------------------------------------------------------------ reading it

    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    /// The rectangle as it stands: the one being dragged, or the finished one.
    pub fn current(&self) -> Option<Box2> {
        if self.creating {
            Some(Box2::from_corners(self.start, self.drag_end()))
        } else {
            self.shape
        }
    }

    /// The window rectangle under the cursor, untweened. This is what the area
    /// label reports, exactly as ShareX labels `CurrentHoverShape.Rectangle`.
    pub fn hover(&self) -> Option<Box2> {
        self.hover
    }

    /// The hovered rectangle as it should be *drawn*: mid-tween while it is
    /// sliding from the previous one.
    ///
    /// The time comes from the caller, not from `self.now`: the tween has to
    /// keep running while the mouse is standing still, and `self.now` only
    /// moves when an event arrives. Reading the internal clock here would park
    /// the rectangle wherever the last motion left it — on the window the
    /// pointer has already left, if it stopped as soon as it got there.
    pub fn hover_drawn(&self, now: Duration) -> Option<Box2> {
        let h = self.hover?;
        match self.tween {
            Some(t) if !t.done(now) => {
                let c = t.at(now);
                // ShareX only uses the animated rectangle once it has some size
                Some(if c.w > 2 && c.h > 2 { c } else { h })
            }
            _ => Some(h),
        }
    }

    /// Whether the picture would differ from the last `render` at this time.
    ///
    /// The marching ants never stop, so anything with a dashed border keeps
    /// this true; with nothing selected and nothing hovered there is nothing
    /// moving on its own and the caller can sit still until the next event.
    pub fn animating(&self, now: Duration) -> bool {
        if let Some(t) = self.tween {
            if !t.done(now) {
                return true;
            }
        }
        self.current().is_some() || self.hover.is_some() || self.show_crosshair
    }

    fn valid(r: Box2) -> bool {
        r.w >= MIN_SIZE && r.h >= MIN_SIZE
    }

    /// The monitor the cursor is on — `CaptureHelpers.GetActiveScreenBounds()`.
    /// The whole desktop stands in when the cursor is between two outputs.
    fn active_monitor(&self) -> Box2 {
        self.outs.iter().copied().find(|o| o.contains(self.cursor)).unwrap_or(self.desktop)
    }

    // ------------------------------------------------------------ the drag

    /// Where the drag ends once the held modifiers have had their say.
    /// `BaseShape.OnUpdate` gives proportional resizing priority over snapping.
    fn drag_end(&self) -> Pos {
        let p = self.cursor;
        if self.shift {
            snap_to_degree(self.start, p)
        } else if self.alt {
            self.snap_position(self.start, p)
        } else {
            p
        }
    }

    /// `ShapeManager.SnapPosition`: the snap size closest in (width, height)
    /// space wins, but only if it is strictly closer than `SnapDistance` and
    /// not a perfect match already, and only if the rectangle it produces still
    /// fits on the desktop.
    fn snap_position(&self, anchor: Pos, cur: Pos) -> Pos {
        let now = Box2::from_corners(anchor, cur);
        let mut best: Option<(f32, (i32, i32))> = None;
        for s in SNAP_SIZES {
            let d = (((now.w - s.0) as f32).powi(2) + ((now.h - s.1) as f32).powi(2)).sqrt();
            if d > 0.0 && d < SNAP_DISTANCE && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, s));
            }
        }
        if let Some((_, s)) = best {
            let p = calc_snap_pos(anchor, cur, s);
            if self.desktop.holds(Box2::from_corners(anchor, p)) {
                return p;
            }
        }
        cur
    }

    /// All snap sizes anchored at the drag's start corner, previewed while Alt
    /// is held. ShareX draws every one of them, in range or not.
    fn snap_previews(&self) -> Vec<Box2> {
        if !(self.creating && self.alt) {
            return Vec::new();
        }
        SNAP_SIZES
            .iter()
            .map(|&s| Box2::from_corners(self.start, calc_snap_pos(self.start, self.cursor, s)))
            .collect()
    }

    // ------------------------------------------------------------ feeding it

    /// Feeds one event. `now` is the time since the selector came up — the
    /// input delay, the dash animation and the hover tween all read it.
    pub fn feed(&mut self, ev: Input, now: Duration) -> Outcome {
        self.now = now;
        match ev {
            Input::Motion(p) => {
                let d = Pos { x: p.x - self.cursor.x, y: p.y - self.cursor.y };
                self.cursor = p;
                // `IsCornerMoving`: the anchor rides along with the pointer, so
                // the rectangle keeps its size and travels instead of growing.
                if self.creating && self.corner_moving {
                    self.start.x += d.x;
                    self.start.y += d.y;
                }
                self.refresh_hover();
                Outcome::Continue
            }
            Input::Button { btn, down } => self.button(btn, down),
            Input::DoubleClick => match self.current() {
                Some(r) if Self::valid(r) => Outcome::Region(r),
                _ => Outcome::Continue,
            },
            Input::Wheel(d) => {
                self.wheel(d);
                Outcome::Continue
            }
            Input::Key { code, down } => self.key(code, down),
        }
    }

    fn button(&mut self, btn: Btn, down: bool) -> Outcome {
        match (btn, down) {
            (Btn::Left, true) => {
                self.begin();
                Outcome::Continue
            }
            (Btn::Left, false) => self.end(),
            // `RegionCaptureActionRightClick = RemoveShapeCancelCapture`: the
            // first press throws the drag away, the next one gives up.
            (Btn::Right, false) => {
                if self.creating {
                    self.creating = false;
                    self.corner_moving = false;
                    self.shape = None;
                    self.refresh_hover();
                    Outcome::Continue
                } else {
                    Outcome::Cancel
                }
            }
            // `SwapToolType`. sxr has one region tool, so there is nothing to
            // swap to and this is deliberately a no-op.
            (Btn::Middle, false) => Outcome::Continue,
            (Btn::Side, false) => Outcome::Region(self.desktop),
            (Btn::Extra, false) => Outcome::Region(self.active_monitor()),
            _ => Outcome::Continue,
        }
    }

    /// `ShapeManager.StartRegionSelection`.
    fn begin(&mut self) {
        self.start = self.cursor;
        self.creating = true;
        self.corner_moving = self.ctrl;
        self.shape = None;
        self.refresh_hover();
    }

    /// `ShapeManager.EndRegionSelection`: an invalid shape falls back to the
    /// hovered window, and if there is not one either the click produced
    /// nothing at all.
    fn end(&mut self) -> Outcome {
        if !self.creating {
            return Outcome::Continue;
        }
        let r = Box2::from_corners(self.start, self.drag_end());
        self.creating = false;
        self.corner_moving = false;
        self.refresh_hover();

        let r = if Self::valid(r) {
            r
        } else {
            match self.hover.filter(|h| Self::valid(*h)) {
                Some(h) => h,
                None => {
                    self.shape = None;
                    return Outcome::Continue;
                }
            }
        };
        self.shape = Some(r);
        if self.quick_crop {
            Outcome::Region(r)
        } else {
            Outcome::Continue
        }
    }

    /// `ShapeManager.form_MouseWheel`: two pixels per notch, and the magnifier
    /// switches off when it would go below the minimum rather than clamping.
    fn wheel(&mut self, delta: i32) {
        if delta > 0 {
            if self.show_magnifier {
                self.mag_count = (self.mag_count + 2).min(MAG_COUNT_MAX);
            } else {
                self.show_magnifier = true;
            }
        } else if delta < 0 {
            let mut n = self.mag_count - 2;
            if n < MAG_COUNT_MIN {
                n = MAG_COUNT_MIN;
                self.show_magnifier = false;
            }
            self.mag_count = n;
        }
    }

    fn key(&mut self, code: u32, down: bool) -> Outcome {
        // The modifiers are tracked whether or not the delay has passed: a
        // Shift held down since before we came up still has to count.
        match code {
            key::LEFTCTRL | key::RIGHTCTRL => {
                self.ctrl = down;
                // `IsCornerMoving` only latches while something is being drawn
                self.corner_moving = down && self.creating;
            }
            key::LEFTSHIFT | key::RIGHTSHIFT => self.shift = down,
            key::LEFTALT | key::RIGHTALT => self.alt = down,
            _ => {}
        }
        if !down {
            return Outcome::Continue;
        }
        // `isKeyAllowed`: once one key has come through, the gate stays open.
        if !self.keys_open {
            if self.now < INPUT_DELAY {
                return Outcome::Continue;
            }
            self.keys_open = true;
        }

        match code {
            key::ESC => Outcome::Cancel,
            key::ENTER | key::KPENTER => match self.current().or(self.hover) {
                Some(r) if Self::valid(r) => Outcome::Region(r),
                // ShareX closes with `RegionResult.Region` regardless and ends
                // up with an empty rectangle; an empty region is a cancel here.
                _ => Outcome::Cancel,
            },
            key::SPACE => Outcome::Region(self.desktop),
            key::GRAVE => Outcome::Region(self.active_monitor()),
            key::N1..=key::N0 => {
                // `MonitorKey`: 1..9 address monitors 1..9, 0 addresses the tenth
                let n = if code == key::N0 { 10 } else { (code - key::N1 + 1) as usize };
                match self.outs.get(n - 1) {
                    Some(&o) => Outcome::Region(o),
                    None => Outcome::Continue,
                }
            }
            key::DELETE => {
                self.shape = None;
                self.creating = false;
                self.corner_moving = false;
                self.refresh_hover();
                Outcome::Continue
            }
            key::INSERT => {
                // `Keys.Insert`: the keyboard's version of press and release
                if self.creating {
                    self.end()
                } else {
                    self.begin();
                    Outcome::Continue
                }
            }
            // `SwapShapeType` — one region tool here, so nothing to swap.
            key::TAB => Outcome::Continue,
            // Ctrl+C copies the area text in ShareX; the clipboard belongs to
            // the caller, not to a module that must stay pure.
            key::C => Outcome::Continue,
            key::LEFT | key::RIGHT | key::UP | key::DOWN => {
                self.arrow(code);
                Outcome::Continue
            }
            _ => Outcome::Continue,
        }
    }

    /// `ShapeManager.form_KeyDown`'s tail: with nothing to move the arrows push
    /// the drawn cursor, otherwise they move or resize the rectangle.
    fn arrow(&mut self, code: u32) {
        let s = if self.shift { MOVE_FAST } else { MOVE_SLOW };
        let (dx, dy) = match code {
            key::LEFT => (-s, 0),
            key::RIGHT => (s, 0),
            key::UP => (0, -s),
            _ => (0, s),
        };
        match self.shape {
            None => {
                self.cursor.x += dx;
                self.cursor.y += dy;
                self.refresh_hover();
            }
            Some(_) if self.creating => {
                self.cursor.x += dx;
                self.cursor.y += dy;
            }
            Some(r) => {
                self.shape = Some(if self.ctrl {
                    // `Resize(x, y, fromBottomRight: true)`
                    Box2::new(r.x, r.y, r.w + dx, r.h + dy)
                } else if self.alt {
                    // `Resize(x, y, fromBottomRight: false)`
                    Box2::new(r.x + dx, r.y + dy, r.w - dx, r.h - dy)
                } else {
                    r.shift(dx, dy)
                });
            }
        }
    }

    /// `ShapeManager.CheckHover` + `FindSelectedWindow`: the first rectangle in
    /// the front-to-back list that contains the cursor, and nothing at all
    /// while a shape is being drawn. Plain traversal order — no biggest-window
    /// or smallest-window heuristic, because ShareX has none.
    fn refresh_hover(&mut self) {
        let target = if self.creating {
            None
        } else {
            self.windows
                .iter()
                .copied()
                .find(|w| w.contains(self.cursor))
                .and_then(|w| w.intersect(self.desktop))
                .filter(|w| Self::valid(*w))
        };
        if target == self.hover {
            return;
        }
        // ShareX only tweens between two real rectangles: with an empty
        // `PreviousHoverRectangle` the new one simply appears.
        self.tween = match (self.hover, target) {
            (Some(prev), Some(to)) => {
                let from = match self.tween {
                    Some(t) if !t.done(self.now) => {
                        let c = t.at(self.now);
                        if c.w > 2 && c.h > 2 { c } else { prev }
                    }
                    _ => prev,
                };
                Some(Tween { from, to, start: self.now })
            }
            _ => None,
        };
        self.hover = target;
    }

    // ------------------------------------------------------------ what to draw

    /// Everything that moves, in image pixels and in ShareX's paint order.
    /// The painter never has to work out geometry of its own, which is what
    /// lets it compute the damage before it draws a single pixel.
    pub fn items(&self, now: Duration) -> Vec<Item> {
        let mut v = Vec::new();

        // The hovered window: same animated border as a live drag, and it is
        // punched back through the veil too.
        let hover_drawn = if self.creating { None } else { self.hover_drawn(now) };
        if let Some(r) = hover_drawn {
            v.push(Item::Region { r, animated: true });
        }
        if let Some(r) = self.current().filter(|r| Self::valid(*r)) {
            v.push(Item::Region { r, animated: true });
        }

        // After the regions, never before: a region is punched back through the
        // veil at full brightness, so a preview smaller than the current
        // selection would be painted over — and those are exactly the ones the
        // user is looking at while shrinking towards a snap size. ShareX draws
        // them late in `DrawShapes` for the same reason.
        for r in self.snap_previews() {
            v.push(Item::Marker(r));
        }

        if self.show_info {
            // ShareX labels the true hovered rectangle, not the tweened one, so
            // the numbers do not flicker while the border slides.
            if let Some(r) = self.current().filter(|r| Self::valid(*r)) {
                v.push(self.area_label(r));
            }
            if let Some(r) = self.hover.filter(|_| !self.creating) {
                if Some(r) != self.current() {
                    v.push(self.area_label(r));
                }
            }
        }

        self.cursor_block(&mut v, now);

        if self.show_crosshair {
            v.push(Item::Guides(self.cursor));
        }
        v.push(Item::Cursor(self.cursor));
        v
    }

    /// `DrawAreaText`: above the region, flipped to inside-below when it would
    /// clip past the top of the desktop, clamped against the right edge.
    fn area_label(&self, area: Box2) -> Item {
        let text = format!("X: {} Y: {} W: {} H: {}", area.x, area.y, area.w, area.h);
        let (tw, th) = measure(&text);
        let (mut tx, ty);
        if area.y - AREA_OFFSET - th - AREA_PAD * 2 < self.desktop.y {
            tx = area.x + AREA_OFFSET + AREA_PAD;
            ty = area.y + AREA_OFFSET + AREA_PAD;
        } else {
            tx = area.x + AREA_PAD;
            ty = area.y - AREA_OFFSET - AREA_PAD - th;
        }
        if tx + tw + AREA_PAD >= self.desktop.right() {
            tx = self.desktop.right() - tw - AREA_PAD;
        }
        Item::Label {
            text,
            rect: Box2::new(tx - AREA_PAD, ty - AREA_PAD, tw + AREA_PAD * 2, th + AREA_PAD * 2),
        }
    }

    /// `DrawCursorGraphics`: the magnifier and the readout stacked below-right
    /// of the cursor, each axis flipped on its own when the block would leave
    /// the monitor the cursor is on — not the whole desktop, so the readout
    /// never straddles the seam between two screens.
    fn cursor_block(&self, v: &mut Vec<Item>, _now: Duration) {
        let mag = self.show_magnifier.then(|| mag_extent(self.mag_count, self.mag_size));
        let info = self.show_info.then(|| {
            let (tw, th) = measure(&self.info_text());
            (tw + INFO_PAD * 2, th + INFO_PAD * 2)
        });
        if mag.is_none() && info.is_none() {
            return;
        }

        let mut total = (0, 0);
        let mut items = 0;
        let mut mag_at = 0;
        if let Some(m) = mag {
            mag_at = total.1;
            total.0 = total.0.max(m);
            total.1 += m;
            items += 1;
        }
        let mut info_at = 0;
        if let Some((iw, ih)) = info {
            if items > 0 {
                total.1 += ITEM_GAP;
            }
            info_at = total.1;
            total.0 = total.0.max(iw);
            total.1 += ih;
        }

        let act = self.active_monitor();
        let mut x = self.cursor.x + CURSOR_OFFSET;
        if x + total.0 > act.right() {
            x = self.cursor.x - CURSOR_OFFSET - total.0;
        }
        let mut y = self.cursor.y + CURSOR_OFFSET;
        if y + total.1 > act.bottom() {
            y = self.cursor.y - CURSOR_OFFSET - total.1;
        }

        if let Some(m) = mag {
            v.push(Item::Magnifier {
                at: self.cursor,
                dest: Box2::new(x, y + mag_at, m, m),
                count: (self.mag_count | 1).clamp(MAG_COUNT_MIN, MAG_COUNT_MAX),
                size: self.mag_size.clamp(MAG_SIZE_MIN, MAG_SIZE_MAX),
                round: !self.square_magnifier,
            });
        }
        if let Some((iw, ih)) = info {
            v.push(Item::Label {
                text: self.info_text(),
                rect: Box2::new(x + total.0 / 2 - iw / 2, y + info_at, iw, ih),
            });
        }
    }

    /// `GetInfoText()` for a region capture: the cursor position and nothing
    /// else. The size lives in the area label next to the region itself.
    fn info_text(&self) -> String {
        format!("X: {} Y: {}", self.cursor.x, self.cursor.y)
    }
}

/// `CaptureHelpers.CalculateNewPosition`: the far corner that gives `size`,
/// on the side the drag is heading.
fn calc_snap_pos(anchor: Pos, cur: Pos, size: (i32, i32)) -> Pos {
    Pos {
        x: if cur.x > anchor.x { anchor.x + size.0 - 1 } else { anchor.x - size.0 + 1 },
        y: if cur.y > anchor.y { anchor.y + size.1 - 1 } else { anchor.y - size.1 + 1 },
    }
}

/// `CaptureHelpers.SnapPositionToDegree(start, pos, degree: 90, startDegree: 45)`
/// — the values region rectangles use, which snap a drag to the four diagonals
/// and so make it square.
fn snap_to_degree(a: Pos, b: Pos) -> Pos {
    let angle = ((b.y - a.y) as f32).atan2((b.x - a.x) as f32);
    let start = std::f32::consts::FRAC_PI_4;
    let snap = std::f32::consts::FRAC_PI_2;
    let new = ((angle + start) / snap).round() * snap - start;
    let d = (((b.x - a.x) as f32).powi(2) + ((b.y - a.y) as f32).powi(2)).sqrt();
    Pos {
        x: a.x + (new.cos() * d).round() as i32,
        y: a.y + (new.sin() * d).round() as i32,
    }
}

/// The text block's size, rounded out the way `Graphics.MeasureString(...)
/// .ToSize()` rounds it.
fn measure(text: &str) -> (i32, i32) {
    let (w, h) = font::measure(text, INFO_SIZE, false);
    (w.ceil() as i32, h.ceil() as i32)
}

/// The magnifier bitmap's side. ShareX allocates `count * size - 1` pixels and
/// draws a `count * size` image into it, so the last row and column fall off;
/// keeping the quirk keeps the round frame's radii right.
fn mag_extent(count: i32, size: i32) -> i32 {
    let c = (count | 1).clamp(MAG_COUNT_MIN, MAG_COUNT_MAX);
    let s = size.clamp(MAG_SIZE_MIN, MAG_SIZE_MAX);
    c * s - 1
}

// -------------------------------------------------------------------- items

/// One thing to draw, already placed. The painter turns each of these into
/// pixels and, before that, into a damage rectangle.
#[derive(Clone, Debug)]
pub enum Item {
    /// Punched back through the veil, then framed black-then-dashed-white.
    Region { r: Box2, animated: bool },
    /// A snap size preview, plain red, no dashes.
    Marker(Box2),
    /// A readout box: background, two borders, shadowed text.
    Label { text: String, rect: Box2 },
    /// The screen-wide crosshair guides through the cursor.
    Guides(Pos),
    Magnifier { at: Pos, dest: Box2, count: i32, size: i32, round: bool },
    /// The pointer itself: the compositor's is hidden, so we draw ShareX's.
    Cursor(Pos),
}

impl Item {
    /// The rectangle this item shows at full brightness, if it shows one.
    ///
    /// A region is the only item that unveils the frozen screen, and it is the
    /// one place where the damage is worth thinking about twice: the interior
    /// is enormous and, frame to frame, almost always identical. The painter
    /// therefore tracks these rectangles apart from everything else and repairs
    /// only where the bright area has actually moved.
    fn bright(&self) -> Option<Box2> {
        match *self {
            Item::Region { r, .. } => Some(r),
            _ => None,
        }
    }

    /// Every rectangle this item can touch *except* the bright interior that
    /// [`Item::bright`] reports, in image pixels. Conservative on purpose — a
    /// damage rectangle that is one pixel too big costs a memcpy, one that is a
    /// pixel too small leaves a smear on screen.
    ///
    /// A region contributes its dashed border, which is where the marching ants
    /// live and so the only part of it that changes while the rectangle stands
    /// still; a marker contributes its outline, because an outline is all
    /// `draw_item` ever paints for one.
    fn bounds(&self, desktop: Box2) -> Vec<Box2> {
        match *self {
            Item::Region { r, .. } => ring_bands(r),
            Item::Marker(r) => ring_bands(r),
            Item::Label { ref rect, .. } => vec![rect.grow(2)],
            Item::Guides(c) => vec![
                Box2::new(desktop.x, c.y, desktop.w, 1),
                Box2::new(c.x, desktop.y, 1, desktop.h),
            ],
            // the white ring sits one pixel outside the image
            Item::Magnifier { dest, .. } => vec![dest.grow(2)],
            Item::Cursor(c) => {
                vec![Box2::new(c.x - CURSOR_HOT.0, c.y - CURSOR_HOT.1, CURSOR_W, CURSOR_W)]
            }
        }
    }
}

// ------------------------------------------------------------- rectangle sets

/// How far outside and inside its own edge a one pixel outline is allowed to
/// stray before the damage misses it. Two pixels is pure slack: nothing here
/// draws thicker than one, but a rounding error in a future edge should not
/// cost a smear.
const RING_SLACK: i32 = 2;

/// The four edge bands of a rectangle, each `RING_SLACK` pixels to either side
/// of the edge it follows. Their union always covers the whole outline, and for
/// a rectangle thinner than the slack it simply covers the rectangle entire.
///
/// The bands do overlap at the corners. That is deliberate: an overlap costs a
/// few pixels copied twice, a gap costs a visible smear.
fn ring_bands(r: Box2) -> Vec<Box2> {
    if r.is_empty() {
        return Vec::new();
    }
    let n = RING_SLACK;
    let (w, h) = (r.w + n * 2, r.h - n * 2);
    vec![
        Box2::new(r.x - n, r.y - n, w, n * 2),
        Box2::new(r.x - n, r.bottom() - n, w, n * 2),
        Box2::new(r.x - n, r.y + n, n * 2, h),
        Box2::new(r.right() - n, r.y + n, n * 2, h),
    ]
    .into_iter()
    .filter(|b| !b.is_empty())
    .collect()
}

/// `a` with `b` cut out of it, as up to four disjoint rectangles: the strip
/// above the hole, the strip below it, then what is left to its left and right.
/// Nothing fancy — the painter only ever subtracts a couple of rectangles from
/// a couple of others, and being obviously right matters more here than being
/// clever.
fn sub(a: Box2, b: Box2) -> Vec<Box2> {
    let Some(i) = a.intersect(b) else {
        return if a.is_empty() { Vec::new() } else { vec![a] };
    };
    let mut v = Vec::new();
    if i.y > a.y {
        v.push(Box2::new(a.x, a.y, a.w, i.y - a.y));
    }
    if i.bottom() < a.bottom() {
        v.push(Box2::new(a.x, i.bottom(), a.w, a.bottom() - i.bottom()));
    }
    if i.x > a.x {
        v.push(Box2::new(a.x, i.y, i.x - a.x, i.h));
    }
    if i.right() < a.right() {
        v.push(Box2::new(i.right(), i.y, a.right() - i.right(), i.h));
    }
    v
}

/// Everything covered by `a` and by none of `b`. Subtracting one rectangle at a
/// time is exact — `(a \ b1) \ b2` really is `a \ (b1 ∪ b2)` — so the result
/// covers the set difference precisely, in disjoint pieces.
fn sub_all(a: &[Box2], b: &[Box2]) -> Vec<Box2> {
    let mut acc: Vec<Box2> = a.iter().copied().filter(|r| !r.is_empty()).collect();
    for cut in b {
        acc = acc.iter().flat_map(|r| sub(*r, *cut)).collect();
    }
    acc
}

// ------------------------------------------------------------------ rasterizing

/// A borrowed pixmap that takes image coordinates and knows where its output
/// sits, so nothing above it has to subtract origins by hand.
struct Canvas<'a> {
    px: &'a mut [PremultipliedColorU8],
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
}

impl Canvas<'_> {
    /// Source-over blend of a straight colour. The overlay has no transparent
    /// pixel anywhere — it starts as a copy of the screen — so destination
    /// alpha stays 255 and premultiplied equals straight.
    #[inline]
    fn blend(&mut self, x: i32, y: i32, c: [u8; 3], a: f32) {
        let (x, y) = (x - self.ox, y - self.oy);
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let a = a.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let i = (y * self.w + x) as usize;
        let d = self.px[i];
        let mix = |s: u8, d: u8| (s as f32 * a + d as f32 * (1.0 - a) + 0.5) as u8;
        self.px[i] = PremultipliedColorU8::from_rgba(
            mix(c[0], d.red()),
            mix(c[1], d.green()),
            mix(c[2], d.blue()),
            255,
        )
        .expect("opaque colour is always a valid premultiplied pixel");
    }

    #[inline]
    fn put(&mut self, x: i32, y: i32, c: [u8; 3]) {
        let (x, y) = (x - self.ox, y - self.oy);
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.px[(y * self.w + x) as usize] =
            PremultipliedColorU8::from_rgba(c[0], c[1], c[2], 255).expect("opaque pixel");
    }

    fn fill(&mut self, r: Box2, c: [u8; 3], a: f32) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.blend(x, y, c, a);
            }
        }
    }

    /// A one pixel ring on the rectangle's own outermost pixels — what
    /// `DrawRectangleProper` does with a 1px pen.
    fn ring(&mut self, r: Box2, c: [u8; 3], a: f32) {
        if r.is_empty() {
            return;
        }
        for p in perimeter(r) {
            self.blend(p.x, p.y, c, a);
        }
    }
}

/// The rectangle's outline walked as one closed path, clockwise from the
/// top-left. Walking it in one piece is what keeps a dash pattern continuous
/// around the corners, the way a GDI+ `GraphicsPath` does it.
fn perimeter(r: Box2) -> Vec<Pos> {
    let mut v = Vec::new();
    if r.is_empty() {
        return v;
    }
    let (l, t) = (r.x, r.y);
    let (rr, b) = (r.right() - 1, r.bottom() - 1);
    if r.w == 1 || r.h == 1 {
        for y in t..=b {
            for x in l..=rr {
                v.push(Pos { x, y });
            }
        }
        return v;
    }
    for x in l..=rr {
        v.push(Pos { x, y: t });
    }
    for y in t + 1..=b {
        v.push(Pos { x: rr, y });
    }
    for x in (l..rr).rev() {
        v.push(Pos { x, y: b });
    }
    for y in (t + 1..b).rev() {
        v.push(Pos { x: l, y });
    }
    v
}

/// The dash pattern `{5, 5}` at a given offset. ShareX advances the offset by
/// `-15` per second off a stopwatch, so the ants march at the same speed no
/// matter what frame rate we manage.
fn dash_phase(now: Duration, animated: bool) -> f32 {
    if animated { DASH_SPEED * now.as_secs_f32() } else { 0.0 }
}

fn dash_on(i: usize, phase: f32) -> bool {
    (i as f32 + phase).rem_euclid(DASH_PERIOD) < DASH_ON
}

/// One pixel of the frozen screen, or `None` off its edge. The screenshot does
/// not always cover the whole desktop, so a lookup outside it has to be
/// harmless.
#[inline]
fn screen_px(screen: &RgbaImage, x: i32, y: i32) -> Option<[u8; 3]> {
    if x < 0 || y < 0 || x as u32 >= screen.width() || y as u32 >= screen.height() {
        return None;
    }
    let i = ((y as u32 * screen.width() + x as u32) * 4) as usize;
    let b = screen.as_raw();
    Some([b[i], b[i + 1], b[i + 2]])
}

// ---------------------------------------------------------------- the cursor

/// `ShareX.ScreenCaptureLib/Resources/Crosshair.cur`, decoded from the file
/// itself: a 32x32 1bpp cursor with its hotspot at (15, 15). `W` is the white
/// fill, `B` the black outline that keeps it readable on any background, `.`
/// is transparent. Reproducing the bitmap rather than approximating it means
/// the pointer we draw ourselves — the compositor's is hidden under the
/// overlay — is the one ShareX users know.
const CURSOR: [&str; 32] = [
    "................................",
    "................................",
    "................................",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    ".............BW.WB..............",
    "...BBBBBBBBBBW...WBBBBBBBBBB....",
    "...WWWWWWWWWW.....WWWWWWWWWW....",
    "...BBBBBBBBBBW...WBBBBBBBBBB....",
    ".............BW.WB..............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "..............BWB...............",
    "................................",
    "................................",
    "................................",
    "................................",
];
const CURSOR_W: i32 = 32;
const CURSOR_HOT: (i32, i32) = (15, 15);

// ------------------------------------------------------------------- painting

/// How many frames of damage history to keep. Double buffering asks for two;
/// a third costs nothing and covers a triple-buffered compositor.
const HISTORY: usize = 3;

/// Draws one output. Holds the dimmed backdrop so it is built once, and
/// remembers what the moving parts covered so only that has to be repaired.
pub struct Painter {
    out: Box2,
    backdrop: Pixmap,
    /// What each of the last renders touched apart from the bright interiors,
    /// most recent first.
    history: Vec<Vec<Box2>>,
    /// The bright areas of those same renders, index for index with `history`.
    /// Kept apart because they are compared rather than repaired: only where
    /// one frame is bright and the other is not does anything have to change.
    regions: Vec<Vec<Box2>>,
}

impl Painter {
    pub fn new(screen: &RgbaImage, out: Box2) -> Self {
        Painter {
            out,
            backdrop: backdrop(screen, out),
            history: Vec::new(),
            regions: Vec::new(),
        }
    }

    /// The output this painter owns, in image pixels.
    pub fn out(&self) -> Box2 {
        self.out
    }

    /// Redraws into `dst` (exactly `out`'s size) and returns the rectangles it
    /// touched, in output-local pixels, for `wl_surface.damage_buffer`.
    /// `age` is how many renders ago `dst` was last drawn into: `None` — and
    /// `Some(0)`, which is what Wayland's `buffer_age` reports for a buffer it
    /// has never handed back — means its contents are unknown and everything
    /// must be redrawn.
    pub fn render(
        &mut self,
        dst: &mut Pixmap,
        screen: &RgbaImage,
        sel: &Sel,
        now: Duration,
        age: Option<u32>,
    ) -> Vec<Box2> {
        let (w, h) = (dst.width() as i32, dst.height() as i32);
        let items = sel.items(now);

        // What this frame covers outside the bright interiors, clipped to us,
        // and the bright rectangles themselves.
        let mine: Vec<Box2> = items
            .iter()
            .flat_map(|i| i.bounds(sel.desktop))
            .filter_map(|b| b.intersect(self.out))
            .collect();
        let bright_now: Vec<Box2> =
            items.iter().filter_map(Item::bright).filter_map(|b| b.intersect(self.out)).collect();

        // Whatever the frame currently sitting in `dst` covered, so the
        // backdrop shows through again where the moving parts have left.
        let stale = match age {
            Some(n) if n >= 1 => {
                self.history.get(n as usize - 1).zip(self.regions.get(n as usize - 1))
            }
            _ => None,
        };
        let dmg: Vec<Box2> = match stale {
            // Outside the union below, this frame and the one already in `dst`
            // agree pixel for pixel: no moving part and no border sits there in
            // either of them, and the bright areas either both cover it — same
            // frozen screen, same value — or neither does. So only the borders,
            // the other items and the places where the bright area has come or
            // gone need touching, which is nothing at all while a hover sits
            // still and the ants march.
            Some((old, bright_was)) => mine
                .iter()
                .chain(old)
                .copied()
                .chain(sub_all(&bright_now, bright_was))
                .chain(sub_all(bright_was, &bright_now))
                .filter(|r| !r.is_empty())
                .collect(),
            // unknown contents, or older than we remember: repair the lot
            None => vec![self.out],
        };

        // Repair from the backdrop, then draw this frame on top. The bright
        // interiors are clipped to the repaired area — every pixel of theirs
        // that the repair just dimmed is in it, and every pixel outside it is
        // already right.
        for r in &dmg {
            self.copy_backdrop(dst, *r);
        }
        {
            let mut c = Canvas { px: dst.pixels_mut(), w, h, ox: self.out.x, oy: self.out.y };
            for it in &items {
                draw_item(&mut c, screen, it, now, &dmg);
            }
        }

        self.history.insert(0, mine);
        self.history.truncate(HISTORY);
        self.regions.insert(0, bright_now);
        self.regions.truncate(HISTORY);

        dmg.into_iter().map(|r| r.shift(-self.out.x, -self.out.y)).collect()
    }

    fn copy_backdrop(&self, dst: &mut Pixmap, r: Box2) {
        let Some(r) = r.intersect(self.out) else { return };
        let w = self.backdrop.width() as i32;
        let dw = dst.width() as i32;
        let src = self.backdrop.pixels();
        let px = dst.pixels_mut();
        for y in r.y..r.bottom() {
            let sy = y - self.out.y;
            for x in r.x..r.right() {
                let sx = x - self.out.x;
                if sx < 0 || sy < 0 || sx >= w || sy >= self.backdrop.height() as i32 {
                    continue;
                }
                px[(sy * dw + sx) as usize] = src[(sy * w + sx) as usize];
            }
        }
    }
}

/// The frozen screen for one output, dimmed by the veil. This never changes
/// while the user drags, so it is built once and everything interactive is
/// drawn on a copy of it.
fn backdrop(screen: &RgbaImage, out: Box2) -> Pixmap {
    let mut pm = Pixmap::new(out.w.max(1) as u32, out.h.max(1) as u32)
        .expect("an output is never 0x0 nor absurdly large");
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let px = pm.pixels_mut();
    for y in 0..h {
        for x in 0..w {
            let c = screen_px(screen, out.x + x, out.y + y).unwrap_or([0, 0, 0]);
            let d = |v: u8| (v as f32 * VEIL_KEEP + 0.5) as u8;
            px[(y * w + x) as usize] =
                PremultipliedColorU8::from_rgba(d(c[0]), d(c[1]), d(c[2]), 255)
                    .expect("opaque pixel");
        }
    }
    pm
}

/// Paints one item. `clip` is the damage the painter has just repaired: the
/// bright interior of a region is drawn only inside it, since everywhere else
/// the buffer already holds the right pixels. Everything else is drawn whole —
/// the damage covers those items entirely anyway, and clipping them would only
/// buy arithmetic.
fn draw_item(c: &mut Canvas, screen: &RgbaImage, it: &Item, now: Duration, clip: &[Box2]) {
    match *it {
        Item::Region { r, animated } => {
            // Back to full brightness, straight from the frozen screen.
            for d in clip {
                if let Some(v) = r.intersect(*d) {
                    unveil(c, screen, v);
                }
            }
            dashed_ring(c, r, dash_phase(now, animated));
        }
        Item::Marker(r) => c.ring(r, MARKER, MARKER_A),
        Item::Label { ref text, rect } => label(c, text, rect),
        Item::Guides(p) => guides(c, p, now),
        Item::Magnifier { at, dest, count, size, round } => {
            magnifier(c, screen, at, dest, count, size, round)
        }
        Item::Cursor(p) => cursor(c, p),
    }
}

/// The frozen screen copied back over `v` at full brightness, a row at a time.
/// The area is clipped once, against the pixmap and against the screenshot, and
/// then walked as plain slices — the interior of a region is by far the largest
/// thing drawn here, and a bounds check per pixel is a price it should not pay.
fn unveil(c: &mut Canvas, screen: &RgbaImage, v: Box2) {
    let sw = screen.width() as i32;
    let Some(v) = v
        .intersect(Box2::new(c.ox, c.oy, c.w, c.h))
        .and_then(|v| v.intersect(Box2::new(0, 0, sw, screen.height() as i32)))
    else {
        return;
    };
    let raw = screen.as_raw();
    let n = v.w as usize;
    for y in v.y..v.bottom() {
        let si = ((y * sw + v.x) * 4) as usize;
        let di = ((y - c.oy) * c.w + (v.x - c.ox)) as usize;
        let (src, _) = raw[si..si + n * 4].as_chunks::<4>();
        let dst = &mut c.px[di..di + n];
        for (o, p) in dst.iter_mut().zip(src) {
            *o = PremultipliedColorU8::from_rgba(p[0], p[1], p[2], 255).expect("opaque pixel");
        }
    }
}

/// `borderPen` then `borderDotPen`: solid black all the way round, white
/// 5-on/5-off on top. Two passes, always — the black is what makes the white
/// readable over a bright screenshot.
fn dashed_ring(c: &mut Canvas, r: Box2, phase: f32) {
    let path = perimeter(r);
    for p in &path {
        c.put(p.x, p.y, [0, 0, 0]);
    }
    for (i, p) in path.iter().enumerate() {
        if dash_on(i, phase) {
            c.put(p.x, p.y, [255, 255, 255]);
        }
    }
}

/// `DrawCrosshair`: four segments from the cursor to the edges of the desktop,
/// each with a 5px gap around the cursor and only when longer than 10px.
fn guides(c: &mut Canvas, p: Pos, now: Duration) {
    let phase = dash_phase(now, true);
    let area = Box2::new(c.ox, c.oy, c.w, c.h);
    let seg = |c: &mut Canvas, from: Pos, to: Pos| {
        let n = ((to.x - from.x).abs()).max((to.y - from.y).abs());
        let (sx, sy) = ((to.x - from.x).signum(), (to.y - from.y).signum());
        for i in 0..=n {
            let (x, y) = (from.x + sx * i, from.y + sy * i);
            c.put(x, y, [0, 0, 0]);
            if dash_on(i as usize, phase) {
                c.put(x, y, [255, 255, 255]);
            }
        }
    };
    // the guides span the whole desktop, but each painter only owns a slice of
    // it; clipping happens in `put`, the length test does not
    let (l, r) = (area.x, area.right() - 1);
    let (t, b) = (area.y, area.bottom() - 1);
    if p.x - GUIDE_GAP - l > GUIDE_MIN {
        seg(c, Pos { x: p.x - GUIDE_GAP, y: p.y }, Pos { x: l, y: p.y });
    }
    if r - (p.x + GUIDE_GAP) > GUIDE_MIN {
        seg(c, Pos { x: p.x + GUIDE_GAP, y: p.y }, Pos { x: r, y: p.y });
    }
    if p.y - GUIDE_GAP - t > GUIDE_MIN {
        seg(c, Pos { x: p.x, y: p.y - GUIDE_GAP }, Pos { x: p.x, y: t });
    }
    if b - (p.y + GUIDE_GAP) > GUIDE_MIN {
        seg(c, Pos { x: p.x, y: p.y + GUIDE_GAP }, Pos { x: p.x, y: b });
    }
}

/// `DrawInfoText`: the background shrunk by two, the light inner border shrunk
/// by one, the dark outer border on the rectangle itself, then the text with a
/// one pixel shadow under it. The three rings are contiguous, which is what
/// gives the box its sunken double edge.
fn label(c: &mut Canvas, text: &str, rect: Box2) {
    c.fill(rect.grow(-2), THEME_BG, TEXT_BOX_A);
    c.ring(rect.grow(-1), THEME_SEP_LIGHT, TEXT_BOX_A);
    c.ring(rect, THEME_SEP_DARK, TEXT_BOX_A);
    let (tx, ty) = ((rect.x + INFO_PAD) as f32, (rect.y + INFO_PAD) as f32);
    font::rasterize(text, INFO_SIZE, false, tx + 1.0, ty + 1.0, |x, y, a| {
        c.blend(x, y, THEME_BORDER, a)
    });
    font::rasterize(text, INFO_SIZE, false, tx, ty, |x, y, a| c.blend(x, y, THEME_TEXT, a));
}

/// `RegionCaptureForm.Magnifier` plus the frame `DrawCursorGraphics` puts
/// around it. Nearest-neighbour throughout: the whole point is to show the
/// pixel the cursor is on, so smoothing would defeat it.
#[allow(clippy::too_many_arguments)]
fn magnifier(
    c: &mut Canvas,
    screen: &RgbaImage,
    at: Pos,
    dest: Box2,
    count: i32,
    size: i32,
    round: bool,
) {
    let side = dest.w; // count * size - 1
    let full = count * size; // the image ShareX draws into that bitmap
    let sx0 = at.x - count / 2;
    let sy0 = at.y - count / 2;
    let half = side as f32 / 2.0;
    let cx = dest.x as f32 + half;
    let cy = dest.y as f32 + half;
    // FillEllipse over a `side` wide box: radius is half of it.
    let inside = |x: i32, y: i32| {
        if !round {
            return true;
        }
        let dx = x as f32 + 0.5 - cx;
        let dy = y as f32 + 0.5 - cy;
        dx * dx + dy * dy <= half * half
    };

    // The sampled screen, or the theme background where the source area runs
    // off the screenshot — `g.Clear(canvasBackgroundColor)` in ShareX.
    for j in 0..side {
        for i in 0..side {
            let (x, y) = (dest.x + i, dest.y + j);
            if !inside(x, y) {
                continue;
            }
            let p = screen_px(screen, sx0 + i / size, sy0 + j / size).unwrap_or(THEME_BG);
            c.put(x, y, p);
        }
    }

    // The centre crosshair: four bars framing the middle cell, always drawn.
    let bar = (full - size) / 2;
    let cross = [
        Box2::new(dest.x, dest.y + (full - size) / 2, bar, size),
        Box2::new(dest.x + (full + size) / 2, dest.y + (full - size) / 2, bar, size),
        Box2::new(dest.x + (full - size) / 2, dest.y, size, bar),
        Box2::new(dest.x + (full - size) / 2, dest.y + (full + size) / 2, size, bar),
    ];
    for r in cross {
        if let Some(r) = r.intersect(dest) {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    if inside(x, y) {
                        c.blend(x, y, MAG_CROSS, MAG_CROSS_A);
                    }
                }
            }
        }
    }

    // The pixel grid, one line short of every boundary.
    for k in 1..count {
        let gx = dest.x + k * size - 1;
        for y in dest.y..dest.y + side {
            if inside(gx, y) {
                c.blend(gx, y, [0, 0, 0], MAG_GRID_A);
            }
        }
        let gy = dest.y + k * size - 1;
        for x in dest.x..dest.x + side {
            if inside(x, gy) {
                c.blend(x, gy, [0, 0, 0], MAG_GRID_A);
            }
        }
    }

    // The centre pixel: black always, white inset only for a big enough cell.
    let b = Box2::new(dest.x + (full - size) / 2 - 1, dest.y + (full - size) / 2 - 1, size + 1, size + 1);
    for p in perimeter(b) {
        if inside(p.x, p.y) {
            c.put(p.x, p.y, [0, 0, 0]);
        }
    }
    if size >= 6 {
        let w = Box2::new(dest.x + (full - size) / 2, dest.y + (full - size) / 2, size - 1, size - 1);
        for p in perimeter(w) {
            if inside(p.x, p.y) {
                c.put(p.x, p.y, [255, 255, 255]);
            }
        }
    }

    // The frame: white one pixel outside, black on the edge of the image.
    if round {
        ring_ellipse(cx, cy, half + 1.0, |x, y| c.put(x, y, [255, 255, 255]));
        ring_ellipse(cx, cy, half, |x, y| c.put(x, y, [0, 0, 0]));
    } else {
        c.ring(dest.grow(1), [255, 255, 255], 1.0);
        c.ring(dest, [0, 0, 0], 1.0);
    }
}

/// A one pixel circle. Scanned along both axes so the near-horizontal and
/// near-vertical parts are both closed — a pure per-row scan leaves gaps at
/// the top and bottom.
fn ring_ellipse(cx: f32, cy: f32, r: f32, mut px: impl FnMut(i32, i32)) {
    if r <= 0.0 {
        return;
    }
    let y0 = (cy - r).floor() as i32;
    let y1 = (cy + r).ceil() as i32;
    for y in y0..=y1 {
        let dy = y as f32 + 0.5 - cy;
        if dy.abs() > r {
            continue;
        }
        let dx = (r * r - dy * dy).sqrt();
        px((cx - dx).round() as i32, y);
        px((cx + dx).round() as i32 - 1, y);
    }
    let x0 = (cx - r).floor() as i32;
    let x1 = (cx + r).ceil() as i32;
    for x in x0..=x1 {
        let dx = x as f32 + 0.5 - cx;
        if dx.abs() > r {
            continue;
        }
        let dy = (r * r - dx * dx).sqrt();
        px(x, (cy - dy).round() as i32);
        px(x, (cy + dy).round() as i32 - 1);
    }
}

fn cursor(c: &mut Canvas, p: Pos) {
    for (j, row) in CURSOR.iter().enumerate() {
        for (i, ch) in row.bytes().enumerate() {
            let col = match ch {
                b'W' => [255, 255, 255],
                b'B' => [0, 0, 0],
                _ => continue,
            };
            c.put(p.x - CURSOR_HOT.0 + i as i32, p.y - CURSOR_HOT.1 + j as i32, col);
        }
    }
}

// ------------------------------------------------------------- hidden modes

/// A screen to test against: two 1920x1080 halves with content that makes a
/// mistake visible. Gradients so the veil can be measured at many values,
/// blocks of known colour so it can be measured exactly, and one pixel wide
/// lines so the magnifier's nearest-neighbour sampling has something it cannot
/// fake. Generated, never captured — this must never look at the real desktop.
fn fake_screen() -> RgbaImage {
    let mut img = RgbaImage::new(3840, 1080);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = if x < 1920 {
            image::Rgba([20, 24 + (y / 24) as u8, 60 + (x / 24) as u8, 255])
        } else {
            image::Rgba([236, 232 - (y / 48) as u8, 214, 255])
        };
    }
    let blocks: [(u32, u32, [u8; 3]); 6] = [
        (200, 120, [255, 255, 255]),
        (600, 120, [0, 0, 0]),
        (1000, 120, [128, 128, 128]),
        (2120, 120, [255, 255, 255]),
        (2520, 120, [0, 0, 0]),
        (2920, 120, [128, 128, 128]),
    ];
    for (bx, by, c) in blocks {
        for y in by..by + 200 {
            for x in bx..bx + 300 {
                img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    // single pixel lines every 8px in a patch under each cursor position we
    // shoot: at 10x magnification these have to come out as clean stripes
    for (px, py) in [(940u32, 520u32), (2860, 520), (60, 60), (3780, 1020)] {
        for y in py.saturating_sub(40)..(py + 40).min(1080) {
            for x in px.saturating_sub(40)..(px + 40).min(3840) {
                if x % 8 == 0 || y % 8 == 0 {
                    img.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
                }
            }
        }
    }
    img
}

const OUTS: [Box2; 2] =
    [Box2 { x: 0, y: 0, w: 1920, h: 1080 }, Box2 { x: 1920, y: 0, w: 1920, h: 1080 }];
const DESKTOP: Box2 = Box2 { x: 0, y: 0, w: 3840, h: 1080 };

fn new_sel() -> Sel {
    Sel::new(DESKTOP, OUTS.to_vec(), Vec::new())
}

/// Paints every output and puts the results side by side, so the seam between
/// two layer surfaces can be looked at in one picture.
fn shoot(screen: &RgbaImage, sel: &Sel, now: Duration, dir: &str, name: &str) -> Result<()> {
    let mut out = RgbaImage::new(DESKTOP.w as u32, DESKTOP.h as u32);
    for o in OUTS {
        let mut p = Painter::new(screen, o);
        let mut pm = Pixmap::new(o.w as u32, o.h as u32).expect("output pixmap");
        p.render(&mut pm, screen, sel, now, None);
        for y in 0..o.h {
            for x in 0..o.w {
                let q = pm.pixels()[(y * o.w + x) as usize];
                out.put_pixel(
                    (o.x + x) as u32,
                    (o.y + y) as u32,
                    image::Rgba([q.red(), q.green(), q.blue(), 255]),
                );
            }
        }
    }
    let path = std::path::Path::new(dir).join(name);
    out.save(&path).with_context(|| format!("could not write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// `--sel-shot <dir>`: renders the selector over a synthetic screen in every
/// state worth looking at. Non-interactive, no compositor, no window.
pub fn sel_shot(dir: &str) -> Result<()> {
    let screen = fake_screen();
    std::fs::create_dir_all(dir).with_context(|| format!("could not create {dir}"))?;
    let t0 = Duration::from_millis(600);

    // 01 — idle: magnifier and readout follow the cursor, no guides, no region
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 940, y: 520 }), t0);
    shoot(&screen, &s, t0, dir, "01-idle.png")?;

    // 02 — a drag inside one output
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 300, y: 300 }), t0);
    s.feed(Input::Button { btn: Btn::Left, down: true }, t0);
    s.feed(Input::Motion(Pos { x: 900, y: 800 }), t0);
    shoot(&screen, &s, t0, dir, "02-drag-one-output.png")?;

    // 03 — a drag over the seam between the two outputs
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 1600, y: 300 }), t0);
    s.feed(Input::Button { btn: Btn::Left, down: true }, t0);
    s.feed(Input::Motion(Pos { x: 2400, y: 800 }), t0);
    shoot(&screen, &s, t0, dir, "03-drag-across-outputs.png")?;

    // 04 — a hovered window, un-dimmed and labelled without a click
    let wins = vec![Box2::new(2100, 200, 900, 600), Box2::new(200, 400, 700, 400)];
    let mut s = Sel::new(DESKTOP, OUTS.to_vec(), wins.clone());
    s.feed(Input::Motion(Pos { x: 2500, y: 500 }), t0);
    shoot(&screen, &s, t0, dir, "04-hover-window.png")?;

    // 05 — Alt during a drag: every snap size previewed in red
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 300, y: 200 }), t0);
    s.feed(Input::Button { btn: Btn::Left, down: true }, t0);
    s.feed(Input::Motion(Pos { x: 1000, y: 620 }), t0);
    s.feed(Input::Key { code: key::LEFTALT, down: true }, t0);
    shoot(&screen, &s, t0, dir, "05-alt-snap-previews.png")?;

    // 06 — the guides, which ShareX ships switched off
    let mut s = new_sel();
    s.show_crosshair = true;
    s.feed(Input::Motion(Pos { x: 1400, y: 600 }), t0);
    shoot(&screen, &s, t0, dir, "06-crosshair-guides.png")?;

    // 07 — the magnifier against the bottom-right corner: the block flips to
    // the other side of the cursor on both axes, and the sampled area runs off
    // the screenshot so the theme background fills in
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 3780, y: 1020 }), t0);
    shoot(&screen, &s, t0, dir, "07-magnifier-corner.png")?;

    // 08 — a region against the top edge: the area label cannot go above it,
    // so it flips to inside-below
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 500, y: 2 }), t0);
    s.feed(Input::Button { btn: Btn::Left, down: true }, t0);
    s.feed(Input::Motion(Pos { x: 1100, y: 500 }), t0);
    shoot(&screen, &s, t0, dir, "08-label-flipped.png")?;

    // 09/10 — the same drag 0.2s apart: only the dashes may differ
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 400, y: 300 }), t0);
    s.feed(Input::Button { btn: Btn::Left, down: true }, t0);
    s.feed(Input::Motion(Pos { x: 1000, y: 700 }), t0);
    shoot(&screen, &s, Duration::from_millis(1000), dir, "09-dash-t0.png")?;
    shoot(&screen, &s, Duration::from_millis(1200), dir, "10-dash-t200ms.png")?;

    // 11 — mid-tween between two hovered windows
    let mut s = Sel::new(DESKTOP, OUTS.to_vec(), wins);
    s.feed(Input::Motion(Pos { x: 2500, y: 500 }), t0);
    s.feed(Input::Motion(Pos { x: 500, y: 500 }), t0);
    shoot(&screen, &s, t0 + Duration::from_millis(100), dir, "11-hover-tween-mid.png")?;

    // 12 — the square magnifier, the other half of `UseSquareMagnifier`
    let mut s = new_sel();
    s.square_magnifier = true;
    s.feed(Input::Motion(Pos { x: 2860, y: 520 }), t0);
    shoot(&screen, &s, t0, dir, "12-square-magnifier.png")?;

    Ok(())
}

fn ck(fail: &mut usize, ok: bool, msg: String) {
    if !ok {
        *fail += 1;
    }
    println!("{} {msg}", if ok { "OK" } else { "FAILED" });
}

/// `--sel-flow`: the state machine replayed step by step without a compositor.
/// Every rule that cannot be seen in a screenshot — the input delay, the
/// fallbacks, the snapping, the tween — is asserted here instead.
pub fn sel_flow() -> Result<()> {
    let mut fail = 0usize;
    let late = Duration::from_millis(600);

    // 1. the input delay swallows a key that arrives too early
    let mut s = new_sel();
    let early = s.feed(Input::Key { code: key::ESC, down: true }, Duration::from_millis(100));
    ck(&mut fail, early == Outcome::Continue, format!("1 Esc at 100ms: {early:?}"));
    let after = s.feed(Input::Key { code: key::ESC, down: true }, late);
    ck(&mut fail, after == Outcome::Cancel, format!("1 Esc at 600ms: {after:?}"));

    // 2. a drag under the minimum size falls back to the hovered window
    let win = Box2::new(200, 200, 800, 600);
    let mut s = Sel::new(DESKTOP, OUTS.to_vec(), vec![win]);
    s.feed(Input::Motion(Pos { x: 400, y: 400 }), late);
    ck(&mut fail, s.hover() == Some(win), format!("2 hover: {:?}", s.hover()));
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 402, y: 401 }), late);
    let r = s.feed(Input::Button { btn: Btn::Left, down: false }, late);
    ck(&mut fail, r == Outcome::Region(win), format!("2 tiny drag adopts the window: {r:?}"));

    // 3. the same click with nothing under it produces nothing
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 400, y: 400 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 402, y: 401 }), late);
    let r = s.feed(Input::Button { btn: Btn::Left, down: false }, late);
    ck(&mut fail, r == Outcome::Continue, format!("3 tiny drag with no hover: {r:?}"));
    ck(&mut fail, s.current().is_none(), format!("3 nothing left behind: {:?}", s.current()));

    // 4. QuickCrop: one valid drag ends it on mouse-up, inclusive of both ends
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 100, y: 100 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 399, y: 299 }), late);
    let r = s.feed(Input::Button { btn: Btn::Left, down: false }, late);
    ck(&mut fail, r == Outcome::Region(Box2::new(100, 100, 300, 200)), format!("4 quick crop: {r:?}"));

    // 5. Shift snaps the drag to 45 degrees, which makes it square
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 100, y: 100 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Key { code: key::LEFTSHIFT, down: true }, late);
    s.feed(Input::Motion(Pos { x: 400, y: 200 }), late);
    let c = s.current().expect("a shape while dragging");
    ck(&mut fail, c.w == c.h, format!("5 shift makes it square: {}x{}", c.w, c.h));

    // 6. Alt snaps to 720p when the drag is within 30 of it
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 100, y: 100 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Key { code: key::LEFTALT, down: true }, late);
    s.feed(Input::Motion(Pos { x: 100 + 1289, y: 100 + 719 }), late); // 1290 x 720
    let c = s.current().expect("a shape while dragging");
    ck(&mut fail, (c.w, c.h) == (1280, 720), format!("6 alt snaps to 720p: {}x{}", c.w, c.h));

    // 7. ... and leaves it alone when it is further away than SnapDistance
    s.feed(Input::Motion(Pos { x: 100 + 1329, y: 100 + 719 }), late); // 1330 x 720
    let c = s.current().expect("a shape while dragging");
    ck(&mut fail, (c.w, c.h) == (1330, 720), format!("7 no snap beyond 30: {}x{}", c.w, c.h));

    // 8. a snap that would not fit on the desktop is refused
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 3000, y: 100 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Key { code: key::LEFTALT, down: true }, late);
    s.feed(Input::Motion(Pos { x: 3000 + 1289, y: 100 + 719 }), late);
    let c = s.current().expect("a shape while dragging");
    ck(&mut fail, (c.w, c.h) == (1290, 720), format!("8 snap refused off screen: {}x{}", c.w, c.h));

    // 9. Ctrl during a drag is IsCornerMoving: the anchor travels with the
    // pointer, so the rectangle keeps its size and moves. (ShareX adds the
    // mouse velocity to StartPosition while EndPosition follows the mouse.)
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 200, y: 200 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 400, y: 300 }), late);
    let before = s.current().expect("a shape while dragging");
    s.feed(Input::Key { code: key::LEFTCTRL, down: true }, late);
    s.feed(Input::Motion(Pos { x: 450, y: 340 }), late);
    let after = s.current().expect("a shape while dragging");
    let moved = after == before.shift(50, 40);
    ck(&mut fail, moved, format!("9 ctrl moves the whole rect: {before:?} -> {after:?}"));

    // 10. the arrow keys move a finished shape by 1, or by 10 with Shift.
    // QuickCrop has to be off for a shape to outlive the mouse-up at all.
    let mut s = new_sel();
    s.quick_crop = false;
    s.feed(Input::Motion(Pos { x: 100, y: 100 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 299, y: 249 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: false }, late);
    let base = s.current().expect("a finished shape");
    s.feed(Input::Key { code: key::RIGHT, down: true }, late);
    ck(&mut fail, s.current() == Some(base.shift(1, 0)), format!("10 right by 1: {:?}", s.current()));
    s.feed(Input::Key { code: key::LEFTSHIFT, down: true }, late);
    s.feed(Input::Key { code: key::DOWN, down: true }, late);
    ck(&mut fail, s.current() == Some(base.shift(1, 10)), format!("10 down by 10: {:?}", s.current()));
    s.feed(Input::Key { code: key::LEFTSHIFT, down: false }, late);

    // 11. Ctrl+arrow resizes from the far corner, Alt+arrow from the near one
    let now = s.current().expect("a shape");
    s.feed(Input::Key { code: key::LEFTCTRL, down: true }, late);
    s.feed(Input::Key { code: key::RIGHT, down: true }, late);
    let grown = s.current().expect("a shape");
    ck(&mut fail, grown == Box2::new(now.x, now.y, now.w + 1, now.h), format!("11 ctrl+right grows: {grown:?}"));
    s.feed(Input::Key { code: key::LEFTCTRL, down: false }, late);
    s.feed(Input::Key { code: key::LEFTALT, down: true }, late);
    s.feed(Input::Key { code: key::RIGHT, down: true }, late);
    let pulled = s.current().expect("a shape");
    ck(
        &mut fail,
        pulled == Box2::new(grown.x + 1, grown.y, grown.w - 1, grown.h),
        format!("11 alt+right pulls the near edge: {pulled:?}"),
    );
    s.feed(Input::Key { code: key::LEFTALT, down: false }, late);

    // 12. Delete drops it; the mouse buttons cover the rest of the exits
    s.feed(Input::Key { code: key::DELETE, down: true }, late);
    ck(&mut fail, s.current().is_none(), format!("12 delete drops the shape: {:?}", s.current()));
    let mid = s.feed(Input::Button { btn: Btn::Middle, down: false }, late);
    ck(&mut fail, mid == Outcome::Continue, format!("12 middle click is a no-op: {mid:?}"));
    let side = s.feed(Input::Button { btn: Btn::Side, down: false }, late);
    ck(&mut fail, side == Outcome::Region(DESKTOP), format!("12 X1 grabs the desktop: {side:?}"));

    // 13. Space, the backtick and the digits pick whole screens
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 2500, y: 500 }), late);
    let sp = s.feed(Input::Key { code: key::SPACE, down: true }, late);
    ck(&mut fail, sp == Outcome::Region(DESKTOP), format!("13 space: {sp:?}"));
    let gr = s.feed(Input::Key { code: key::GRAVE, down: true }, late);
    ck(&mut fail, gr == Outcome::Region(OUTS[1]), format!("13 backtick takes the cursor's monitor: {gr:?}"));
    let d1 = s.feed(Input::Key { code: key::N1, down: true }, late);
    ck(&mut fail, d1 == Outcome::Region(OUTS[0]), format!("13 digit 1: {d1:?}"));
    let d2 = s.feed(Input::Key { code: key::N1 + 1, down: true }, late);
    ck(&mut fail, d2 == Outcome::Region(OUTS[1]), format!("13 digit 2: {d2:?}"));
    let d3 = s.feed(Input::Key { code: key::N1 + 2, down: true }, late);
    ck(&mut fail, d3 == Outcome::Continue, format!("13 digit 3 with two monitors: {d3:?}"));
    let d0 = s.feed(Input::Key { code: key::N0, down: true }, late);
    ck(&mut fail, d0 == Outcome::Continue, format!("13 digit 0 addresses the tenth: {d0:?}"));

    // 14. the hover tween: halfway at 100ms, arrived at 200ms
    let a = Box2::new(200, 200, 400, 400);
    let b = Box2::new(1000, 200, 800, 400);
    let mut s = Sel::new(DESKTOP, OUTS.to_vec(), vec![a, b]);
    s.feed(Input::Motion(Pos { x: 300, y: 300 }), late);
    ck(&mut fail, s.hover_drawn(late) == Some(a), format!("14 first hover: {:?}", s.hover_drawn(late)));
    // one motion event, then the pointer stops dead — the tween has to finish
    // on its own, which is the whole point of taking the time as an argument
    let jump = s.feed(Input::Motion(Pos { x: 1400, y: 300 }), late);
    ck(&mut fail, jump == Outcome::Continue, format!("14 motion keeps going: {jump:?}"));
    let half = s.hover_drawn(late + Duration::from_millis(100)).expect("a tweened hover");
    let want = Box2::new(600, 200, 600, 400);
    ck(&mut fail, half == want, format!("14 halfway at 100ms: {half:?} (wanted {want:?})"));
    ck(&mut fail, s.animating(late + Duration::from_millis(100)), "14 the tween counts as animating".into());
    let done = s.hover_drawn(late + Duration::from_millis(200));
    ck(&mut fail, done == Some(b), format!("14 arrived at 200ms with the mouse still: {done:?}"));
    // no check that it stops animating: the marching ants under the pointer
    // never stop, which is exactly what keeps the frames coming

    // 15. the wheel switches the magnifier off under the minimum, and back on
    let mut s = new_sel();
    for _ in 0..6 {
        s.feed(Input::Wheel(-1), late);
    }
    ck(&mut fail, s.mag_count == 3 && s.show_magnifier, format!("15 six notches down: {} on={}", s.mag_count, s.show_magnifier));
    s.feed(Input::Wheel(-1), late);
    ck(&mut fail, !s.show_magnifier, format!("15 one more switches it off: on={}", s.show_magnifier));
    s.feed(Input::Wheel(1), late);
    ck(&mut fail, s.show_magnifier && s.mag_count == 3, format!("15 scrolling up brings it back: {} on={}", s.mag_count, s.show_magnifier));
    s.feed(Input::Wheel(1), late);
    ck(&mut fail, s.mag_count == 5, format!("15 then it grows by two: {}", s.mag_count));

    // 16. the right button drops the drag first and gives up only after
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 300, y: 300 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 600, y: 600 }), late);
    let r1 = s.feed(Input::Button { btn: Btn::Right, down: false }, late);
    ck(&mut fail, r1 == Outcome::Continue, format!("16 right click drops the drag: {r1:?}"));
    ck(&mut fail, s.current().is_none(), format!("16 the drag is gone: {:?}", s.current()));
    let r2 = s.feed(Input::Button { btn: Btn::Right, down: false }, late);
    ck(&mut fail, r2 == Outcome::Cancel, format!("16 the next one cancels: {r2:?}"));

    // 17. Enter commits, a double click commits, X2 takes the monitor
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 500, y: 500 }), late);
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 799, y: 699 }), late);
    let want = Box2::new(500, 500, 300, 200);
    let en = s.feed(Input::Key { code: key::ENTER, down: true }, late);
    ck(&mut fail, en == Outcome::Region(want), format!("17 enter commits: {en:?}"));
    let dc = s.feed(Input::DoubleClick, late);
    ck(&mut fail, dc == Outcome::Region(want), format!("17 double click commits: {dc:?}"));
    let x2 = s.feed(Input::Button { btn: Btn::Extra, down: false }, late);
    ck(&mut fail, x2 == Outcome::Region(OUTS[0]), format!("17 X2 takes the monitor: {x2:?}"));

    // 18. nothing is moving when nothing is selected, so the caller may sleep
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 500, y: 500 }), late);
    ck(&mut fail, !s.animating(late), "18 idle is not animating".into());
    s.feed(Input::Button { btn: Btn::Left, down: true }, late);
    s.feed(Input::Motion(Pos { x: 900, y: 900 }), late);
    ck(&mut fail, s.animating(late), "18 a live drag is".into());

    // 19. Insert is the keyboard's mouse: it opens a drag and closes it
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 100, y: 100 }), late);
    s.feed(Input::Key { code: key::INSERT, down: true }, late);
    s.feed(Input::Motion(Pos { x: 399, y: 299 }), late);
    let ins = s.feed(Input::Key { code: key::INSERT, down: true }, late);
    ck(&mut fail, ins == Outcome::Region(Box2::new(100, 100, 300, 200)), format!("19 insert twice: {ins:?}"));
    let tab = s.feed(Input::Key { code: key::TAB, down: true }, late);
    ck(&mut fail, tab == Outcome::Continue, format!("19 tab has nothing to swap to: {tab:?}"));
    let cc = s.feed(Input::Key { code: key::C, down: true }, late);
    ck(&mut fail, cc == Outcome::Continue, format!("19 C is the caller's business: {cc:?}"));

    // 20. the damage shrinks once the painter knows what the last frame held
    let screen = fake_screen();
    let mut s = new_sel();
    s.feed(Input::Motion(Pos { x: 900, y: 500 }), late);
    let mut p = Painter::new(&screen, OUTS[0]);
    let mut pm = Pixmap::new(1920, 1080).expect("pixmap");
    let first = p.render(&mut pm, &screen, &s, late, None);
    let full = first.iter().map(|r| r.w as i64 * r.h as i64).sum::<i64>();
    ck(&mut fail, full == 1920 * 1080, format!("20 an unknown buffer costs a full repaint: {full}"));
    ck(&mut fail, p.out() == OUTS[0], format!("20 the painter knows its output: {:?}", p.out()));
    ck(&mut fail, s.cursor() == Pos { x: 900, y: 500 }, format!("20 the cursor is where it was put: {:?}", s.cursor()));
    s.feed(Input::Motion(Pos { x: 905, y: 505 }), late);
    let second = p.render(&mut pm, &screen, &s, late, Some(1));
    let part = second.iter().map(|r| r.w as i64 * r.h as i64).sum::<i64>();
    ck(&mut fail, part < 1920 * 1080 / 10, format!("20 a known one costs {part} px"));
    ck(&mut fail, !second.is_empty(), "20 and it is not empty".into());

    if fail > 0 {
        anyhow::bail!("{fail} checks failed");
    }
    println!("all good");
    Ok(())
}

// ------------------------------------------------------------- rect set tests

/// The rectangle arithmetic the damage tracking rests on. It is small enough to
/// read and important enough to be wrong silently, so it is pinned here rather
/// than left to the end-to-end checks in `--sel-flow`.
#[cfg(test)]
mod tests {
    use super::*;

    /// Every pixel of `r`, as a set, so a claim about rectangles can be checked
    /// against the pixels it is really about.
    fn pixels(v: &[Box2]) -> std::collections::HashSet<(i32, i32)> {
        let mut s = std::collections::HashSet::new();
        for r in v {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    s.insert((x, y));
                }
            }
        }
        s
    }

    #[test]
    fn subtracting_a_cover_leaves_nothing() {
        let a = Box2::new(10, 10, 20, 20);
        assert!(sub(a, a).is_empty());
        assert!(sub(a, Box2::new(0, 0, 100, 100)).is_empty());
    }

    #[test]
    fn subtracting_a_stranger_changes_nothing() {
        let a = Box2::new(10, 10, 20, 20);
        assert_eq!(sub(a, Box2::new(40, 40, 5, 5)), vec![a]);
        // touching along an edge is still disjoint: `right()` is exclusive
        assert_eq!(sub(a, Box2::new(30, 10, 5, 20)), vec![a]);
    }

    #[test]
    fn a_hole_in_the_middle_leaves_four_pieces() {
        let a = Box2::new(0, 0, 30, 30);
        let b = Box2::new(10, 10, 10, 10);
        let v = sub(a, b);
        assert_eq!(v.len(), 4);
        assert_eq!(pixels(&v), &pixels(&[a]) - &pixels(&[b]));
    }

    #[test]
    fn a_corner_bite_leaves_an_l() {
        let a = Box2::new(0, 0, 30, 30);
        let b = Box2::new(20, 20, 30, 30);
        let v = sub(a, b);
        assert_eq!(v.len(), 2);
        assert_eq!(pixels(&v), &pixels(&[a]) - &pixels(&[b]));
    }

    #[test]
    fn the_pieces_never_overlap() {
        let a = Box2::new(0, 0, 30, 30);
        for b in [Box2::new(10, 10, 10, 10), Box2::new(-5, 10, 40, 5), Box2::new(25, -5, 3, 40)] {
            let v = sub(a, b);
            let sum: usize = v.iter().map(|r| (r.w * r.h) as usize).sum();
            assert_eq!(sum, pixels(&v).len(), "pieces of {a:?} minus {b:?} overlap");
        }
    }

    #[test]
    fn subtracting_a_set_subtracts_their_union() {
        let a = [Box2::new(0, 0, 40, 40), Box2::new(30, 30, 40, 40)];
        let b = [Box2::new(10, 10, 15, 50), Box2::new(35, 0, 10, 45)];
        assert_eq!(pixels(&sub_all(&a, &b)), &pixels(&a) - &pixels(&b));
        assert!(sub_all(&a, &a).is_empty());
        assert_eq!(pixels(&sub_all(&a, &[])), pixels(&a));
    }

    #[test]
    fn the_bands_cover_the_whole_outline() {
        for r in [
            Box2::new(10, 10, 40, 30),
            Box2::new(0, 0, 1, 1),
            Box2::new(5, 5, 1, 20),
            Box2::new(5, 5, 20, 1),
            Box2::new(5, 5, 4, 4),
            Box2::new(5, 5, 5, 5),
        ] {
            let covered = pixels(&ring_bands(r));
            for p in perimeter(r) {
                assert!(covered.contains(&(p.x, p.y)), "{r:?} leaves {p:?} undamaged");
            }
        }
        assert!(ring_bands(Box2::new(0, 0, 0, 10)).is_empty());
    }

    #[test]
    fn the_bands_leave_a_big_interior_alone() {
        // the whole point of the split: a large region damages its border, not
        // its middle
        let r = Box2::new(0, 0, 1920, 1080);
        let cost: i32 = ring_bands(r).iter().map(|b| b.w * b.h).sum();
        assert!(cost < r.w * r.h / 50, "the bands of {r:?} cost {cost} px");
    }
}
