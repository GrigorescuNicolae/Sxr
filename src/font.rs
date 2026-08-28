use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontRef, FontVec, GlyphId, PxScale, ScaleFont as _, point};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Vec2};

use crate::i18n::{self, Msg, t};

/// The same font file is used by both egui (preview) and our own CPU
/// rasterizer (export), so the text comes out identical in both.
pub const REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

/// The default family, and the fallback for any family that cannot be loaded.
pub const DEFAULT_FAMILY: &str = "DejaVu Sans";

const FAM: &str = "sxr";
const FAM_BOLD: &str = "sxr-bold";

// ------------------------------------------------------------- text options

/// Text alignment inside the box (ShareX: `StringAlignment`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Near,
    Center,
    Far,
}

impl Align {
    /// The three values, in the order they appear in ShareX's menus.
    pub const ALL: [Align; 3] = [Align::Near, Align::Center, Align::Far];

    pub fn horiz_name(self) -> &'static str {
        t(match self {
            Align::Near => Msg::AlignLeft,
            Align::Center => Msg::AlignCenter,
            Align::Far => Msg::AlignRight,
        })
    }

    pub fn vert_name(self) -> &'static str {
        t(match self {
            Align::Near => Msg::AlignTop,
            Align::Center => Msg::AlignMiddle,
            Align::Far => Msg::AlignBottom,
        })
    }
}

/// Our copy of `TextDrawingOptions.cs`: everything that can be changed from
/// the text input window. No gradient — sxr does not draw it.
#[derive(Clone, PartialEq)]
pub struct TextOpts {
    pub family: String,
    pub size: f32,
    pub color: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub halign: Align,
    pub valign: Align,
    /// `TextDrawingOptions.EnterKeyNewLine`: when on, Enter starts a new line
    /// in the text window and Ctrl+Enter confirms; when off, it is the other way
    /// around. The button in the window's bottom-left corner toggles it.
    pub enter_new_line: bool,
}

impl Default for TextOpts {
    fn default() -> Self {
        Self {
            family: DEFAULT_FAMILY.to_owned(),
            size: 18.0,
            color: Color32::WHITE,
            bold: false,
            italic: false,
            underline: false,
            halign: Align::Center,
            valign: Align::Center,
            enter_new_line: false,
        }
    }
}

// --------------------------------------------------------------- system font list

struct Entry {
    family: String,
    style: String,
    file: PathBuf,
}

struct Db {
    families: Vec<String>,
    entries: Vec<Entry>,
    /// Why the list ended up empty, if that is the case.
    note: Option<String>,
}

/// The enumeration runs once per session, on the first request (opening the
/// text window), not on every frame.
fn db() -> &'static Db {
    static D: OnceLock<Db> = OnceLock::new();
    D.get_or_init(scan)
}

/// The extensions `ab_glyph` can open. The rest (pcf, bdf, pfb...) are bitmap
/// or Type1 fonts and have no business being in the list.
fn usable_file(p: &str) -> bool {
    let p = p.to_ascii_lowercase();
    p.ends_with(".ttf") || p.ends_with(".otf") || p.ends_with(".ttc") || p.ends_with(".otc")
}

fn scan() -> Db {
    let mut db = Db { families: Vec::new(), entries: Vec::new(), note: None };
    let out = match std::process::Command::new("fc-list")
        .args([":", "family", "file", "style"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            db.note = Some(i18n::fc_list_failed(&o.status.to_string()));
            return db;
        }
        Err(e) => {
            db.note = Some(i18n::fc_list_missing(&e.to_string()));
            return db;
        }
    };

    let text = String::from_utf8_lossy(&out);
    let mut fams = BTreeSet::new();
    for line in text.lines() {
        // format: "/path/file.ttf: Family1,Family2:style=Style1,Style2"
        let mut it = line.splitn(3, ':');
        let Some(file) = it.next().map(str::trim) else { continue };
        if file.is_empty() || !usable_file(file) {
            continue;
        }
        let names = it.next().unwrap_or("").trim();
        let styles = it
            .next()
            .unwrap_or("")
            .trim()
            .strip_prefix("style=")
            .unwrap_or("");
        let sl: Vec<&str> = styles.split(',').map(str::trim).collect();
        for (i, name) in names.split(',').map(str::trim).enumerate() {
            // fontconfig escapes some characters with a backslash; no family
            // name contains one, so we simply strip them
            let fam = name.replace('\\', "");
            if fam.is_empty() {
                continue;
            }
            let style = sl.get(i).or_else(|| sl.first()).copied().unwrap_or("Regular");
            fams.insert(fam.clone());
            db.entries.push(Entry {
                family: fam,
                style: style.to_owned(),
                file: PathBuf::from(file),
            });
        }
    }
    db.families = fams.into_iter().collect();
    db
}

thread_local! {
    /// Faces already read from disk, so we do not reread the file every frame.
    static CACHE: RefCell<BTreeMap<(String, bool, bool), Face>> = RefCell::new(BTreeMap::new());
    /// Fonts prepared for egui: family name -> file data.
    static REG: RefCell<BTreeMap<String, Arc<egui::FontData>>> = RefCell::new(BTreeMap::new());
    /// The names already handed to egui through `set_fonts`. Only these can be
    /// used in a `FontId`; otherwise egui cannot find the family.
    static LIVE: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// Families sent to `set_fonts` but not applied yet: egui rebuilds the
    /// atlas only at the start of the next frame, and a request for an unknown
    /// font would panic. They move into `LIVE` only then, through `sync`.
    static PEND: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// The families that turned out impossible to load; they drop off the list.
    static BAD: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// Messages for the status bar, collected by the editor after each load.
    static NOTES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

#[derive(Clone)]
struct Face {
    font: Rc<FontVec>,
    /// The family name the font is (or will be) registered under in egui.
    egui: String,
}

/// The system families, minus the ones that turned out impossible to load.
/// The list is empty (DejaVu only) if `fc-list` is missing.
pub fn families() -> Vec<&'static str> {
    if let Some(n) = db().note.as_deref() {
        note(n.to_owned());
    }
    BAD.with_borrow(|bad| {
        let mut v: Vec<&'static str> = db()
            .families
            .iter()
            .map(String::as_str)
            .filter(|f| !bad.contains(*f))
            .collect();
        if !v.iter().any(|f| *f == DEFAULT_FAMILY) {
            v.insert(0, DEFAULT_FAMILY);
        }
        v
    })
}

/// Drains the messages gathered since the last call (families that could not
/// be loaded, missing variants). The editor puts them in the status bar.
pub fn take_note() -> Option<String> {
    NOTES.with_borrow_mut(|n| {
        let m = n.first().cloned();
        n.clear();
        m
    })
}

fn note(msg: String) {
    NOTES.with_borrow_mut(|n| {
        if !n.contains(&msg) {
            n.push(msg);
        }
    });
}

/// How well a style from `fc-list` matches what was asked for.
/// Higher = better; styles with extra words ("Condensed", "Light") lose
/// points, so they do not steal the plain variant's place.
fn style_score(style: &str, bold: bool, italic: bool) -> i32 {
    let s = style.to_ascii_lowercase();
    let is_italic = s.contains("italic") || s.contains("oblique");
    let is_bold = s.contains("bold");
    let extra = s
        .split_whitespace()
        .filter(|w| {
            !matches!(
                *w,
                "regular" | "book" | "roman" | "normal" | "bold" | "italic" | "oblique"
            )
        })
        .count() as i32;
    let mut sc = -extra;
    if is_bold == bold {
        sc += 10;
    }
    if is_italic == italic {
        sc += 10;
    }
    sc
}

/// The best matching file in the family, plus whether it is the exact variant.
fn pick_file(family: &str, bold: bool, italic: bool) -> Option<(PathBuf, bool)> {
    let mut best: Option<(i32, &Entry)> = None;
    for e in db().entries.iter().filter(|e| e.family == family) {
        let sc = style_score(&e.style, bold, italic);
        if best.is_none_or(|(b, _)| sc > b) {
            best = Some((sc, e));
        }
    }
    best.map(|(sc, e)| (e.file.clone(), sc >= 20))
}

/// The fallback face: DejaVu Sans embedded in the binary.
fn fallback(bold: bool) -> Face {
    let data = if bold { BOLD } else { REGULAR };
    let font = FontVec::try_from_vec(data.to_vec()).expect("DejaVu încorporat invalid");
    Face {
        font: Rc::new(font),
        egui: if bold { FAM_BOLD } else { FAM }.to_owned(),
    }
}

fn build(family: &str, bold: bool, italic: bool) -> Face {
    // the default family without italic comes from the binary: it is always
    // available and `install` has already registered it in egui
    if family == DEFAULT_FAMILY && !italic {
        return fallback(bold);
    }
    let Some((file, exact)) = pick_file(family, bold, italic) else {
        if family != DEFAULT_FAMILY {
            note(i18n::family_missing(family, DEFAULT_FAMILY));
        }
        return fallback(bold);
    };
    let data = match std::fs::read(&file) {
        Ok(d) => d,
        Err(e) => {
            BAD.with_borrow_mut(|b| b.insert(family.to_owned()));
            note(i18n::cannot_read(&file.display().to_string(), &e.to_string()));
            return fallback(bold);
        }
    };
    let name = egui_name(family, bold, italic);
    // .ttc collections hold several faces; we take the first one
    match FontVec::try_from_vec_and_index(data.clone(), 0) {
        Ok(font) => {
            REG.with_borrow_mut(|r| {
                r.entry(name.clone())
                    .or_insert_with(|| Arc::new(egui::FontData::from_owned(data)));
            });
            if !exact {
                note(i18n::family_no_variant(family));
            }
            Face { font: Rc::new(font), egui: name }
        }
        Err(e) => {
            BAD.with_borrow_mut(|b| b.insert(family.to_owned()));
            note(i18n::family_load_failed(family, &e.to_string(), DEFAULT_FAMILY));
            fallback(bold)
        }
    }
}

fn egui_name(family: &str, bold: bool, italic: bool) -> String {
    format!(
        "{family}#{}{}",
        if bold { "b" } else { "" },
        if italic { "i" } else { "" }
    )
}

fn face_of(o: &TextOpts) -> Face {
    let key = (o.family.clone(), o.bold, o.italic);
    if let Some(f) = CACHE.with_borrow(|c| c.get(&key).cloned()) {
        return f;
    }
    let f = build(&o.family, o.bold, o.italic);
    CACHE.with_borrow_mut(|c| c.insert(key, f.clone()));
    f
}

// ------------------------------------------------------- registering with egui

fn apply(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    REG.with_borrow(|r| {
        for (name, data) in r.iter() {
            f.font_data.insert(name.clone(), data.clone());
            f.families
                .insert(FontFamily::Name(name.as_str().into()), vec![name.clone()]);
        }
    });
    ctx.set_fonts(f);
    PEND.with_borrow_mut(|p| REG.with_borrow(|r| p.extend(r.keys().cloned())));
}

pub fn install(ctx: &egui::Context) {
    REG.with_borrow_mut(|r| {
        r.insert(FAM.to_owned(), Arc::new(egui::FontData::from_static(REGULAR)));
        r.insert(FAM_BOLD.to_owned(), Arc::new(egui::FontData::from_static(BOLD)));
    });
    apply(ctx);
}

/// Makes sure the requested family is read from disk AND known to egui.
/// Called from the text window, not from drawing: `set_fonts` rebuilds the
/// atlas, so it has no business in the middle of a paint frame.
pub fn register(ctx: &egui::Context, o: &TextOpts) {
    let name = face_of(o).egui;
    if !LIVE.with_borrow(|l| l.contains(&name)) && !PEND.with_borrow(|p| p.contains(&name)) {
        apply(ctx);
        // the new font only shows up from the next frame, so we ask for one more
        ctx.request_repaint();
    }
}

/// To be called at the start of every frame: whatever was sent to `set_fonts`
/// in the previous frame is installed now and can be asked for without a panic.
pub fn sync() {
    PEND.with_borrow_mut(|p| {
        if !p.is_empty() {
            LIVE.with_borrow_mut(|l| l.append(p));
        }
    });
}

/// The `FontId` for the preview. If the family has not reached egui yet, we
/// fall back to DejaVu instead of asking for an unknown family (egui would panic).
pub fn opts_font_id(o: &TextOpts, size: f32) -> FontId {
    let f = face_of(o);
    let name = if LIVE.with_borrow(|l| l.contains(&f.egui)) {
        f.egui
    } else if o.bold {
        FAM_BOLD.to_owned()
    } else {
        FAM.to_owned()
    };
    FontId::new(size.max(1.0), FontFamily::Name(name.as_str().into()))
}

// --------------------------------------------------- DejaVu, the simple path

fn face(bold: bool) -> FontRef<'static> {
    FontRef::try_from_slice(if bold { BOLD } else { REGULAR }).expect("font invalid")
}

pub fn font_id(size: f32, bold: bool) -> FontId {
    FontId::new(
        size.max(1.0),
        FontFamily::Name(if bold { FAM_BOLD } else { FAM }.into()),
    )
}

/// Width and height of the text block in DejaVu. No wrapping — only `\n`
/// breaks a line, exactly like `layout_no_wrap` in the preview. Used by the
/// step counter, which always writes with the embedded font.
pub fn measure(text: &str, size: f32, bold: bool) -> (f32, f32) {
    let f = face(bold);
    let s = f.as_scaled(PxScale::from(size.max(1.0)));
    // same formula as egui: row_height = ascent - descent + line_gap
    let rh = s.ascent() - s.descent() + s.line_gap();
    let mut w = 0.0f32;
    let mut lines = 0;
    for line in text.split('\n') {
        let mut lw = 0.0;
        for c in line.chars() {
            lw += s.h_advance(f.glyph_id(c));
        }
        w = w.max(lw);
        lines += 1;
    }
    (w, rh * lines.max(1) as f32)
}

/// Rasterizes on the CPU with the embedded font. `x`,`y` are the block's
/// top-left corner; the callback receives the pixel and the coverage (0..1).
pub fn rasterize(text: &str, size: f32, bold: bool, x: f32, y: f32, mut px: impl FnMut(i32, i32, f32)) {
    let f = face(bold);
    let scale = PxScale::from(size.max(1.0));
    let s = f.as_scaled(scale);
    let rh = s.ascent() - s.descent() + s.line_gap();
    for (li, line) in text.split('\n').enumerate() {
        let base = y + rh * li as f32 + s.ascent();
        let mut cx = x;
        for c in line.chars() {
            let id: GlyphId = f.glyph_id(c);
            if let Some(og) = f.outline_glyph(id.with_scale_and_position(scale, point(cx, base))) {
                let b = og.px_bounds();
                let (ox, oy) = (b.min.x as i32, b.min.y as i32);
                og.draw(|gx, gy, cov| px(ox + gx as i32, oy + gy as i32, cov));
            }
            cx += s.h_advance(id);
        }
    }
}

// ------------------------------------------------------ laying out text in the box

/// One laid-out line: its top-left corner and its measured width.
pub struct Line {
    pub text: String,
    pub x: f32,
    pub top: f32,
    pub w: f32,
}

/// The layout result. The same structure feeds both the preview and the export,
/// so alignment and underlining come out identical in the two.
pub struct Layout {
    pub lines: Vec<Line>,
    pub ascent: f32,
    /// The underline's position relative to the top of the line, and its thickness.
    pub ul_top: f32,
    pub ul_h: f32,
}

/// Lays the text out in `rect` following the alignments in `o`. `size` is the
/// already scaled size (preview) or `o.size` itself (export).
pub fn layout(text: &str, o: &TextOpts, size: f32, rect: Rect) -> Layout {
    let f = face_of(o);
    let s = f.font.as_scaled(PxScale::from(size.max(1.0)));
    let row_h = s.ascent() - s.descent() + s.line_gap();
    let widths: Vec<f32> = text
        .split('\n')
        .map(|l| l.chars().map(|c| s.h_advance(f.font.glyph_id(c))).sum())
        .collect();
    let bh = row_h * widths.len().max(1) as f32;
    let r = Rect::from_two_pos(rect.min, rect.max);
    let top0 = match o.valign {
        Align::Near => r.min.y,
        Align::Center => r.center().y - bh / 2.0,
        Align::Far => r.max.y - bh,
    };
    let lines = text
        .split('\n')
        .zip(&widths)
        .enumerate()
        .map(|(i, (t, w))| Line {
            text: t.to_owned(),
            x: match o.halign {
                Align::Near => r.min.x,
                Align::Center => r.center().x - w / 2.0,
                Align::Far => r.max.x - w,
            },
            top: top0 + row_h * i as f32,
            w: *w,
        })
        .collect();
    // ab_glyph does not expose the underline metrics from the `post` table, so
    // we put the line at half the descender — below the baseline, as in GDI+
    Layout {
        lines,
        ascent: s.ascent(),
        ul_top: s.ascent() - s.descent() * 0.45,
        ul_h: (size / 14.0).max(1.0),
    }
}

/// Draws the already laid-out lines on the CPU, shifted by `off`
/// (used by the outline, which repeats the text around the base position).
pub fn rasterize_layout(
    l: &Layout,
    o: &TextOpts,
    size: f32,
    off: Vec2,
    mut px: impl FnMut(i32, i32, f32),
) {
    let f = face_of(o);
    let scale = PxScale::from(size.max(1.0));
    let s = f.font.as_scaled(scale);
    for line in &l.lines {
        let base = line.top + l.ascent + off.y;
        let mut cx = line.x + off.x;
        for c in line.text.chars() {
            let id: GlyphId = f.font.glyph_id(c);
            if let Some(og) = f
                .font
                .outline_glyph(id.with_scale_and_position(scale, point(cx, base)))
            {
                let b = og.px_bounds();
                let (ox, oy) = (b.min.x as i32, b.min.y as i32);
                og.draw(|gx, gy, cov| px(ox + gx as i32, oy + gy as i32, cov));
            }
            cx += s.h_advance(id);
        }
    }
}

/// The underline rectangles, one per line. Empty if no underline was asked
/// for, or if the line is empty.
pub fn underline_rects(l: &Layout, o: &TextOpts) -> Vec<Rect> {
    if !o.underline {
        return Vec::new();
    }
    l.lines
        .iter()
        .filter(|x| x.w > 0.5)
        .map(|x| {
            Rect::from_min_size(
                Pos2::new(x.x, x.top + l.ul_top),
                Vec2::new(x.w, l.ul_h.max(1.0)),
            )
        })
        .collect()
}
