use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use image::RgbaImage;
use std::sync::Arc;

use crate::font;
pub use crate::font::{Align, TextOpts};
use crate::i18n::{Msg, t};

/// Culorile implicite din ShareX (AnnotationOptions.cs).
pub const PRIMARY: Color32 = Color32::from_rgb(242, 60, 60);
pub const SECONDARY: Color32 = Color32::WHITE;
pub const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 125);
pub const SHADOW_OFFSET: Vec2 = Vec2::new(0.0, 1.0);
/// Cât de tare se întunecă restul imaginii sub reflector
/// (ShareX: SpotlightDim = 30, adică 30% negru).
pub const SPOTLIGHT_DIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 77);

/// Uneltele din bara editorului clasic, în ordinea exactă din ShapeType (Enums.cs),
/// filtrate ca în modul editor: formele de regiune lipsesc, crop și cut out rămân.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
    Freehand,
    FreehandArrow,
    Line,
    Arrow,
    TextOutline,
    TextBackground,
    SpeechBalloon,
    Step,
    Magnify,
    ImageFile,
    ImageScreen,
    Sticker,
    Cursor,
    SmartEraser,
    Blur,
    Pixelate,
    Highlight,
    Spotlight,
    Crop,
    CutOut,
}

impl Tool {
    pub const ALL: [Tool; 23] = [
        Tool::Select,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Freehand,
        Tool::FreehandArrow,
        Tool::Line,
        Tool::Arrow,
        Tool::TextOutline,
        Tool::TextBackground,
        Tool::SpeechBalloon,
        Tool::Step,
        Tool::Magnify,
        Tool::ImageFile,
        Tool::ImageScreen,
        Tool::Sticker,
        Tool::Cursor,
        Tool::SmartEraser,
        Tool::Blur,
        Tool::Pixelate,
        Tool::Highlight,
        Tool::Spotlight,
        Tool::Crop,
        Tool::CutOut,
    ];

    /// Numele fișierului din assets/icons — aceleași iconițe Fugue pe care le
    /// folosește ShareX, varianta pentru temă întunecată acolo unde există.
    pub fn icon(self) -> &'static str {
        match self {
            Tool::Select => "cursor",
            Tool::Rect => "layer-shape",
            Tool::Ellipse => "layer-shape-ellipse",
            Tool::Freehand => "pencil",
            Tool::FreehandArrow => "pencil--arrow",
            Tool::Line => "layer-shape-line-white",
            Tool::Arrow => "layer-shape-arrow-white",
            Tool::TextOutline => "edit-outline-white",
            Tool::TextBackground => "edit-shade-white",
            Tool::SpeechBalloon => "balloon-box-left",
            Tool::Step => "counter-reset",
            Tool::Magnify => "magnifier-zoom",
            Tool::ImageFile => "folder-open-image",
            Tool::ImageScreen => "monitor-image",
            Tool::Sticker => "smiley-yell",
            Tool::Cursor => "stamp-cursor",
            Tool::SmartEraser => "eraser",
            Tool::Blur => "layer-shade-white",
            Tool::Pixelate => "grid-white",
            Tool::Highlight => "highlighter-text",
            Tool::Spotlight => "flashlight-shine",
            Tool::Crop => "image-crop",
            Tool::CutOut => "table-delete-column",
        }
    }

    /// Cheia de traducere a numelui uneltei.
    pub fn msg(self) -> Msg {
        match self {
            Tool::Select => Msg::ToolSelect,
            Tool::Rect => Msg::ToolRect,
            Tool::Ellipse => Msg::ToolEllipse,
            Tool::Freehand => Msg::ToolFreehand,
            Tool::FreehandArrow => Msg::ToolFreehandArrow,
            Tool::Line => Msg::ToolLine,
            Tool::Arrow => Msg::ToolArrow,
            Tool::TextOutline => Msg::ToolTextOutline,
            Tool::TextBackground => Msg::ToolTextBackground,
            Tool::SpeechBalloon => Msg::ToolSpeechBalloon,
            Tool::Step => Msg::ToolStep,
            Tool::Magnify => Msg::ToolMagnify,
            Tool::ImageFile => Msg::ToolImageFile,
            Tool::ImageScreen => Msg::ToolImageScreen,
            Tool::Sticker => Msg::ToolSticker,
            Tool::Cursor => Msg::ToolCursor,
            Tool::SmartEraser => Msg::ToolSmartEraser,
            Tool::Blur => Msg::ToolBlur,
            Tool::Pixelate => Msg::ToolPixelate,
            Tool::Highlight => Msg::ToolHighlight,
            Tool::Spotlight => Msg::ToolSpotlight,
            Tool::Crop => Msg::ToolCrop,
            Tool::CutOut => Msg::ToolCutOut,
        }
    }

    /// Numele uneltei în limba curentă, cum apare în bară.
    pub fn tooltip(self) -> &'static str {
        t(self.msg())
    }

    /// Scurtătura din ShapeManager.cs. Numpad-ul e legat separat în app.rs.
    pub fn key(self) -> Option<egui::Key> {
        use egui::Key as K;
        Some(match self {
            Tool::Select => K::M,
            Tool::Rect => K::R,
            Tool::Ellipse => K::E,
            Tool::Freehand => K::F,
            Tool::Line => K::L,
            Tool::Arrow => K::A,
            Tool::TextOutline => K::O,
            Tool::TextBackground => K::T,
            Tool::SpeechBalloon => K::S,
            Tool::Step => K::I,
            Tool::Blur => K::B,
            Tool::Pixelate => K::P,
            Tool::Highlight => K::H,
            Tool::Crop => K::C,
            Tool::CutOut => K::X,
            _ => return None,
        })
    }

    /// Ce e deja funcțional. Restul apar în bară (poziția contează) dar anunță în status.
    pub fn ready(self) -> bool {
        matches!(
            self,
            Tool::Select
                | Tool::Rect
                | Tool::Ellipse
                | Tool::Freehand
                | Tool::FreehandArrow
                | Tool::Line
                | Tool::Arrow
                | Tool::TextOutline
                | Tool::TextBackground
                | Tool::SpeechBalloon
                | Tool::Step
                | Tool::Magnify
                | Tool::ImageFile
                | Tool::ImageScreen
                | Tool::Sticker
                | Tool::Cursor
                | Tool::Pixelate
                | Tool::Highlight
                | Tool::Blur
                | Tool::Spotlight
                | Tool::SmartEraser
                | Tool::Crop
                | Tool::CutOut
        )
    }

    /// Unealta lasă în urmă o formă cu text, deci se plasează și dintr-un
    /// simplu clic și deschide fereastra de introducere a textului. Balonul
    /// intră aici pentru că în ShareX `SpeechBalloonDrawingShape` derivă din
    /// `TextDrawingShape`: se poartă în toate privințele ca o casetă de text.
    pub fn is_text(self) -> bool {
        matches!(self, Tool::TextOutline | Tool::TextBackground | Tool::SpeechBalloon)
    }
}

/// Ce citește fereastra de introducere a textului dintr-o formă existentă.
pub struct TextInfo<'a> {
    pub text: &'a str,
    pub opts: &'a TextOpts,
    /// Conturul (la textul cu contur) sau fundalul casetei (în rest).
    pub color2: Color32,
    /// Forma e text cu contur, nu text cu fundal.
    pub outline: bool,
}

/// Toate coordonatele sunt în spațiul imaginii (pixeli sursă), nu al ecranului.
/// Așa preview-ul și exportul rămân identice indiferent de zoom.
/// Fără `Debug`: `TextureHandle` nu îl implementează, iar noi nu îl foloseam.
#[derive(Clone)]
pub enum Shape {
    Rect { rect: Rect, border: Color32, fill: Color32, width: f32, radius: f32 },
    Ellipse { rect: Rect, border: Color32, fill: Color32, width: f32 },
    /// Linie cu noduri de curbură, ca `LineDrawingShape` din ShareX: pe lângă
    /// capete ține `mid`, punctele intermediare. Cât timp `curbat` e fals ele
    /// se așază singure pe segmentul dintre capete (`AutoPositionCenterPoints`)
    /// și linia se desenează dreaptă; după ce unul e tras cu mouse-ul rămân
    /// unde le-a pus utilizatorul, iar linia devine un spline cardinal.
    Line { from: Pos2, to: Pos2, mid: Vec<Pos2>, curbat: bool, color: Color32, width: f32 },
    Arrow { from: Pos2, to: Pos2, mid: Vec<Pos2>, curbat: bool, color: Color32, width: f32 },
    Free { pts: Vec<Pos2>, color: Color32, width: f32, arrow: bool },
    Text {
        rect: Rect,
        text: String,
        /// Fontul, mărimea, culoarea, B/I/U și alinierile — tot ce se schimbă
        /// din fereastra de introducere a textului.
        opts: TextOpts,
        fill: Color32,
        outline: Color32,
        outline_w: f32,
        radius: f32,
    },
    /// Balon de dialog: casetă cu text plus o coadă triunghiulară spre `tail`.
    Balloon {
        rect: Rect,
        tail: Pos2,
        text: String,
        opts: TextOpts,
        fill: Color32,
        border: Color32,
        width: f32,
        radius: f32,
    },
    Step { center: Pos2, n: u32, size: f32, fill: Color32, border: Color32, text: Color32, width: f32 },
    /// Lupa: conținutul imaginii de sub formă, mărit de `strength` la sută.
    Magnify { rect: Rect, strength: f32, border: Color32, width: f32 },
    Highlight { rect: Rect, color: Color32 },
    Pixelate { rect: Rect, block: f32 },
    Blur { rect: Rect, radius: f32 },
    Spotlight { rect: Rect },
    /// Guma inteligentă: dreptunghi plin cu culoarea calculată o singură dată
    /// la creare, din inelul de 1px din jurul lui.
    Erase { rect: Rect, color: Color32 },
    /// Imagine inserată (fișier, captură de ecran, sticker sau cursor).
    /// `img` e sursa pentru exportul pe CPU, `tex` aceeași imagine urcată o
    /// singură dată pentru preview. `Arc`, fiindcă formele se clonează la
    /// fiecare pas de undo și nu vrem să copiem pixelii de fiecare dată.
    Img { rect: Rect, img: Arc<RgbaImage>, tex: egui::TextureHandle },
}

/// Capul săgeții, ca `CustomLineCap`-ul din ShareX: un vârf PLIN cu spatele
/// concav, nu două linii. Dimensiunile sunt în unități de grosime a liniei
/// (arrowWidth=2, arrowHeight=6, arrowCurve=1), fiindcă GDI+ scalează capul
/// cu pana — la grosime 4 iese un cap de 24x16 px, exact ca în ShareX.
///
/// Întoarce conturul în ordine: vârf, colț dreapta, scobitura din spate,
/// colț stânga. ShareX folosește o curbă pentru spate; noi punem două
/// segmente drepte, care se abat sub un pixel la grosimile uzuale și au
/// avantajul că previzualizarea și exportul desenează exact aceeași formă.
pub fn arrow_head(from: Pos2, to: Pos2, width: f32) -> [Pos2; 4] {
    let w = width.max(1.0);
    let dir = to - from;
    let n = dir / dir.length().max(1.0);
    let perp = Vec2::new(-n.y, n.x);
    [
        to,
        to - n * (6.0 * w) + perp * (2.0 * w),
        to - n * (5.0 * w),
        to - n * (6.0 * w) - perp * (2.0 * w),
    ]
}

/// Capul plin în previzualizare: două triunghiuri, fiindcă forma e concavă
/// la scobitură, iar `convex_polygon` din egui presupune convexitate.
fn draw_arrow_head(
    p: &egui::Painter,
    [tip, c1, mid, c2]: [Pos2; 4],
    color: Color32,
    map: &impl Fn(Pos2) -> Pos2,
) {
    let (tip, c1, mid, c2) = (map(tip), map(c1), map(mid), map(c2));
    for tri in [[tip, c1, mid], [tip, mid, c2]] {
        p.add(egui::Shape::convex_polygon(tri.to_vec(), color, Stroke::NONE));
    }
}

/// Unde se oprește linia ca să nu răzbată prin capul plin — `BaseInset`-ul
/// din ShareX, adică arrowHeight - arrowCurve = 5 grosimi. La săgeți foarte
/// scurte se oprește la punctul de plecare, nu în spatele lui.
pub fn arrow_base(from: Pos2, to: Pos2, width: f32) -> Pos2 {
    let dir = to - from;
    let len = dir.length().max(1.0);
    to - (dir / len) * (5.0 * width.max(1.0)).min(len)
}

/// Câte puncte de curbură poate avea o linie (ShareX: `MaximumCenterPointCount`).
pub const MAX_MID: usize = 5;

/// Tensiunea implicită a lui `g.DrawCurve` din GDI+.
const CURVE_TENSION: f32 = 0.5;

/// Câte segmente drepte punem pe fiecare bucată de spline. Aceeași
/// discretizare se folosește și în previzualizare și la export, ca cele două
/// căi de desenare să nu se despartă.
const CURVE_STEPS: usize = 24;

/// `AutoPositionCenterPoints` din ShareX: cât timp niciun nod din mijloc n-a
/// fost tras, punctele intermediare se așază prin interpolare liniară între
/// capete, deci linia rămâne dreaptă și nodurile stau chiar pe ea.
pub fn auto_mid(from: Pos2, to: Pos2, mid: &mut [Pos2], curbat: bool) {
    if curbat {
        return;
    }
    let n = (mid.len() + 1) as f32;
    for (i, m) in mid.iter_mut().enumerate() {
        *m = from + (to - from) * ((i + 1) as f32 / n);
    }
}

/// Toate punctele liniei, în ordine: capătul de plecare, cele din mijloc, capătul final.
fn line_pts(from: Pos2, to: Pos2, mid: &[Pos2]) -> Vec<Pos2> {
    let mut v = Vec::with_capacity(mid.len() + 2);
    v.push(from);
    v.extend_from_slice(mid);
    v.push(to);
    v
}

/// Linia frântă care aproximează forma desenată: coarda dreaptă cât timp forma
/// nu e curbată, altfel splinele cardinal discretizat.
///
/// `g.DrawCurve` din GDI+ e un spline cardinal cu tensiunea 0.5 care trece prin
/// toate punctele. Îl convertim în Bezier cubice: pentru punctul `i` tangenta e
/// `T_i = tensiune * (P_{i+1} - P_{i-1})` (la capete se ia diferența față de
/// singurul vecin), iar bucata dintre `P_i` și `P_{i+1}` are punctele de
/// control `P_i + T_i/3` și `P_{i+1} - T_{i+1}/3`.
pub fn curve_poly(from: Pos2, to: Pos2, mid: &[Pos2], curbat: bool) -> Vec<Pos2> {
    if !curbat || mid.is_empty() {
        return vec![from, to];
    }
    let p = line_pts(from, to, mid);
    let n = p.len();
    let tan = |i: usize| (p[(i + 1).min(n - 1)] - p[i.saturating_sub(1)]) * CURVE_TENSION;
    let mut out = Vec::with_capacity((n - 1) * CURVE_STEPS + 1);
    out.push(p[0]);
    for i in 0..n - 1 {
        let (p0, p3) = (p[i], p[i + 1]);
        let c1 = p0 + tan(i) / 3.0;
        let c2 = p3 - tan(i + 1) / 3.0;
        for s in 1..=CURVE_STEPS {
            let t = s as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            let v = p0.to_vec2() * (u * u * u)
                + c1.to_vec2() * (3.0 * u * u * t)
                + c2.to_vec2() * (3.0 * u * t * t)
                + p3.to_vec2() * (t * t * t);
            out.push(v.to_pos2());
        }
    }
    out
}

/// Punctul de pe linia frântă aflat la distanța `d` de capătul final, măsurată
/// DE-A LUNGUL ei. Dacă linia e mai scurtă, întoarce punctul de plecare.
fn back_along(poly: &[Pos2], d: f32) -> Pos2 {
    let mut rest = d;
    for w in poly.windows(2).rev() {
        let len = w[0].distance(w[1]);
        if len >= rest {
            return w[1] + (w[0] - w[1]) / len.max(1e-6) * rest;
        }
        rest -= len;
    }
    poly[0]
}

/// Linia frântă scurtată cu `d` la capătul final, tot pe lungimea curbei.
fn poly_trim(poly: &[Pos2], d: f32) -> Vec<Pos2> {
    let total: f32 = poly.windows(2).map(|w| w[0].distance(w[1])).sum();
    let target = total - d;
    if target <= 0.0 {
        return vec![poly[0], poly[0]];
    }
    let mut out = vec![poly[0]];
    let mut acc = 0.0;
    for w in poly.windows(2) {
        let len = w[0].distance(w[1]);
        if acc + len >= target {
            out.push(w[0] + (w[1] - w[0]) / len.max(1e-6) * (target - acc));
            break;
        }
        acc += len;
        out.push(w[1]);
    }
    out
}

/// Geometria săgeții pe o linie frântă: coada (scurtată cu 5 grosimi ca linia
/// să nu răzbată prin capul plin, retragere măsurată de-a lungul curbei) și
/// conturul capului, orientat după tangenta din capătul final. Pe o linie
/// dreaptă trece prin exact aceleași `arrow_base`/`arrow_head` ca înainte.
pub fn arrow_geom(poly: &[Pos2], width: f32) -> (Vec<Pos2>, [Pos2; 4]) {
    let (from, to) = (poly[0], poly[poly.len() - 1]);
    if poly.len() == 2 {
        return (vec![from, arrow_base(from, to, width)], arrow_head(from, to, width));
    }
    let w = width.max(1.0);
    // tangenta la capăt: direcția dinspre un punct apropiat de pe curbă;
    // `arrow_head` normalizează cu `max(1.0)`, deci pasul nu poate fi sub 1 px
    let tan_from = back_along(poly, (0.75 * w).max(2.0));
    (poly_trim(poly, 5.0 * w), arrow_head(tan_from, to, w))
}

/// Desenează în previzualizare o linie frântă. Când are doar două puncte
/// folosim `line_segment`, drumul de dinainte, ca liniile drepte să iasă
/// neschimbate.
fn draw_poly(p: &Painter, poly: &[Pos2], st: Stroke, map: &impl Fn(Pos2) -> Pos2) {
    match poly.len() {
        0 | 1 => {}
        2 => {
            p.line_segment([map(poly[0]), map(poly[1])], st);
        }
        _ => {
            p.add(egui::Shape::line(poly.iter().map(|q| map(*q)).collect(), st));
        }
    }
}

/// Culoarea medie a unui bloc din imaginea sursă (pentru pixelare).
pub fn block_avg(src: &RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64) -> Color32 {
    let (w, h) = (src.width() as i64, src.height() as i64);
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(w), y1.min(h));
    if x1 <= x0 || y1 <= y0 {
        return Color32::TRANSPARENT;
    }
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = src.get_pixel(x as u32, y as u32).0;
            r += p[0] as u64;
            g += p[1] as u64;
            b += p[2] as u64;
            n += 1;
        }
    }
    Color32::from_rgb((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

/// Regiunea estompată: pixeli RGBA gata calculați, plus colțul din care încep.
pub struct BlurBuf {
    pub x0: i64,
    pub y0: i64,
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

/// O trecere de box blur pe o singură direcție, cu fereastră glisantă (deci
/// costul nu depinde de rază). Elementul `j` al liniei `i` stă la
/// `i * lstep + j * step`; marginile se replică.
fn box_pass(s: &[[u8; 4]], d: &mut [[u8; 4]], lines: usize, len: usize, lstep: usize, step: usize, r: usize) {
    if len == 0 {
        return;
    }
    let n = (2 * r + 1) as u32;
    let last = len as i64 - 1;
    for i in 0..lines {
        let base = i * lstep;
        let idx = |j: i64| base + j.clamp(0, last) as usize * step;
        let mut sum = [0u32; 4];
        for j in -(r as i64)..=(r as i64) {
            let p = s[idx(j)];
            for c in 0..4 {
                sum[c] += p[c] as u32;
            }
        }
        for j in 0..len {
            let o = base + j * step;
            for c in 0..4 {
                d[o][c] = (sum[c] / n) as u8;
            }
            let out = s[idx(j as i64 - r as i64)];
            let inc = s[idx(j as i64 + r as i64 + 1)];
            for c in 0..4 {
                sum[c] = sum[c] + inc[c] as u32 - out[c] as u32;
            }
        }
    }
}

/// Estompează conținutul sursei din dreptunghi: trei treceri de box blur
/// separabil, care aproximează un gaussian dar rămân liniare ca timp.
/// Raza pe trecere e o treime din cea cerută, ca întinderea totală a nucleului
/// să fie aproximativ `radius`.
pub fn blur_region(src: &RgbaImage, rect: Rect, radius: f32) -> Option<BlurBuf> {
    let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(src.width() as f32, src.height() as f32));
    let r = Rect::from_two_pos(rect.min, rect.max).intersect(clip);
    let x0 = r.min.x.floor().max(0.0) as i64;
    let y0 = r.min.y.floor().max(0.0) as i64;
    let x1 = (r.max.x.ceil() as i64).min(src.width() as i64);
    let y1 = (r.max.y.ceil() as i64).min(src.height() as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut px = vec![[0u8; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            px[y * w + x] = src.get_pixel((x0 + x as i64) as u32, (y0 + y as i64) as u32).0;
        }
    }
    let rad = ((radius / 3.0).round() as i64).clamp(1, 200) as usize;
    let mut tmp = vec![[0u8; 4]; w * h];
    for _ in 0..3 {
        box_pass(&px, &mut tmp, h, w, w, 1, rad);
        box_pass(&tmp, &mut px, w, h, 1, w, rad);
    }
    Some(BlurBuf { x0, y0, w, h, px })
}

/// Media pixelilor de pe inelul de 1px din exteriorul dreptunghiului.
/// Numără doar partea validă a inelului; negru dacă nu prinde niciun pixel.
pub fn ring_avg(src: &RgbaImage, rect: Rect) -> Color32 {
    let r = Rect::from_two_pos(rect.min, rect.max);
    let (x0, y0) = (r.min.x.round() as i64, r.min.y.round() as i64);
    let (x1, y1) = (r.max.x.round() as i64, r.max.y.round() as i64);
    let (w, h) = (src.width() as i64, src.height() as i64);
    let (mut sr, mut sg, mut sb, mut n) = (0u64, 0u64, 0u64, 0u64);
    {
        let mut add = |x: i64, y: i64| {
            if x >= 0 && y >= 0 && x < w && y < h {
                let p = src.get_pixel(x as u32, y as u32).0;
                sr += p[0] as u64;
                sg += p[1] as u64;
                sb += p[2] as u64;
                n += 1;
            }
        };
        for x in (x0 - 1)..=x1 {
            add(x, y0 - 1);
            add(x, y1);
        }
        for y in y0..y1 {
            add(x0 - 1, y);
            add(x1, y);
        }
    }
    if n == 0 {
        Color32::BLACK
    } else {
        Color32::from_rgb((sr / n) as u8, (sg / n) as u8, (sb / n) as u8)
    }
}

/// Dreptunghiurile tuturor reflectoarelor din listă, normalizate.
pub fn spotlight_rects(shapes: &[Shape]) -> Vec<Rect> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Spotlight { rect } => Some(Rect::from_two_pos(rect.min, rect.max)),
            _ => None,
        })
        .collect()
}

/// Poziția primului reflector din listă: acolo se desenează stratul întunecat,
/// o singură dată, ca două reflectoare să nu se întunece unul pe altul.
pub fn spotlight_at(shapes: &[Shape]) -> Option<usize> {
    shapes.iter().position(|s| matches!(s, Shape::Spotlight { .. }))
}

/// Descompune „toată imaginea minus reuniunea găurilor" într-o listă de
/// dreptunghiuri disjuncte, prin baleiere pe benzi orizontale. Preview-ul și
/// exportul pornesc de la aceeași listă, deci dau același rezultat.
pub fn spotlight_cover(holes: &[Rect], w: f32, h: f32) -> Vec<Rect> {
    let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(w, h));
    let holes: Vec<Rect> = holes
        .iter()
        .map(|r| Rect::from_two_pos(r.min, r.max).intersect(full))
        .filter(|r| r.width() > 0.0 && r.height() > 0.0)
        .collect();
    let mut ys: Vec<f32> = vec![0.0, h];
    for r in &holes {
        ys.push(r.min.y);
        ys.push(r.max.y);
    }
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup();
    let mut out = Vec::new();
    for band in ys.windows(2) {
        let (t, b) = (band[0], band[1]);
        if b <= t {
            continue;
        }
        let mid = (t + b) / 2.0;
        let mut spans: Vec<(f32, f32)> = holes
            .iter()
            .filter(|r| r.min.y <= mid && mid < r.max.y)
            .map(|r| (r.min.x, r.max.x))
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut x = 0.0f32;
        for (s0, s1) in spans {
            if s0 > x {
                out.push(Rect::from_min_max(Pos2::new(x, t), Pos2::new(s0, b)));
            }
            x = x.max(s1);
        }
        if x < w {
            out.push(Rect::from_min_max(Pos2::new(x, t), Pos2::new(w, b)));
        }
    }
    out
}

/// Offseturile pe care le folosim ca să îngroșăm conturul textului.
/// Aceleași în preview și în export, deci rezultatul e identic prin construcție.
pub fn outline_offsets(w: f32) -> Vec<Vec2> {
    if w <= 0.0 {
        return Vec::new();
    }
    let steps = 16;
    (0..steps)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / steps as f32;
            Vec2::new(a.cos() * w, a.sin() * w)
        })
        .collect()
}

/// Cercul numărătorului: raza acoperă diagonala casetei de text, nu doar
/// latura mai lungă — altfel, la două sau trei cifre, colțurile cifrelor
/// ajung pe marginea cercului. Spațiul liber e proporțional cu fontul, ca
/// să arate la fel la orice mărime aleasă.
pub fn step_radius(n: u32, size: f32) -> f32 {
    let (w, _) = font::measure(&n.to_string(), size, true);
    // înălțimea vizuală a cifrelor, nu înălțimea rândului: aceasta din urmă
    // include loc pentru diacritice și coborâtoare, pe care cifrele nu le au
    let h = size * 0.72;
    w.hypot(h) / 2.0 + size * 0.21
}

/// Coada balonului: baza ei pe latura casetei plus vârful.
pub struct Tail {
    pub base: [Pos2; 2],
    pub tip: Pos2,
    /// Versorul dinspre latura pe care stă baza spre interiorul casetei.
    inward: Vec2,
}

impl Tail {
    /// Triunghiul de umplut. Baza intră în casetă cu grosimea conturului, ca
    /// umplerea să acopere linia de contur care ar tăia altfel gâtul cozii.
    pub fn fill_pts(&self, width: f32) -> [Pos2; 3] {
        let e = self.inward * width.max(0.0);
        [self.base[0] + e, self.base[1] + e, self.tip]
    }
}

/// Baza cozii stă pe latura casetei cea mai apropiată de vârf, are lățimea
/// `min(24, latura/3)` și e centrată pe proiecția vârfului pe acea latură,
/// limitată ca să nu iasă din ea. Aceeași funcție în preview și în export.
pub fn balloon_tail(rect: Rect, tail: Pos2) -> Tail {
    let r = Rect::from_two_pos(rect.min, rect.max);
    let sides = [
        (r.left_top(), r.right_top(), Vec2::new(0.0, 1.0)),
        (r.right_top(), r.right_bottom(), Vec2::new(-1.0, 0.0)),
        (r.right_bottom(), r.left_bottom(), Vec2::new(0.0, -1.0)),
        (r.left_bottom(), r.left_top(), Vec2::new(1.0, 0.0)),
    ];
    let mut best = 0;
    let mut bd = f32::INFINITY;
    for (i, (a, b, _)) in sides.iter().enumerate() {
        let d = dist_seg(tail, *a, *b);
        if d < bd {
            bd = d;
            best = i;
        }
    }
    let (a, b, inward) = sides[best];
    let ab = b - a;
    let len = ab.length().max(1.0);
    let dir = ab / len;
    let half = (24.0f32.min(len / 3.0) / 2.0).min(len / 2.0);
    let t = (tail - a).dot(dir).clamp(half, len - half);
    let c = a + dir * t;
    Tail { base: [c - dir * half, c + dir * half], tip: tail, inward }
}

/// Caseta balonului (și chenarul lupei) ca dreptunghi obișnuit: preview-ul și
/// exportul refolosesc astfel codul deja verificat, deci ies identic.
pub fn box_shape(rect: Rect, fill: Color32, border: Color32, width: f32, radius: f32) -> Shape {
    Shape::Rect { rect, border, fill, width, radius }
}

/// Textul balonului ca formă de text fără fundal: logica de așezare rămâne una singură.
pub fn text_shape(rect: Rect, text: &str, opts: &TextOpts) -> Shape {
    Shape::Text {
        rect,
        text: text.to_owned(),
        opts: opts.clone(),
        fill: Color32::TRANSPARENT,
        outline: Color32::TRANSPARENT,
        outline_w: 0.0,
        radius: 0.0,
    }
}

/// Blocurile lupei: pentru fiecare pixel al regiunii sursă, dreptunghiul pe
/// care îl acoperă mărit (în spațiul imaginii) și culoarea lui. Regiunea sursă
/// are dimensiunea `rect / (strength/100)`, e centrată pe centrul lui `rect` și
/// e trunchiată la marginile imaginii.
pub fn magnify_blocks(src: &RgbaImage, rect: Rect, strength: f32) -> Vec<(Rect, Color32)> {
    let k = (strength / 100.0).max(1.0);
    let r = Rect::from_two_pos(rect.min, rect.max);
    if !r.is_positive() {
        return Vec::new();
    }
    let s = Rect::from_center_size(r.center(), r.size() / k);
    let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(src.width() as f32, src.height() as f32));
    let c = s.intersect(clip);
    if !c.is_positive() {
        return Vec::new();
    }
    let (x0, y0) = (c.min.x.floor() as i64, c.min.y.floor() as i64);
    let (x1, y1) = (c.max.x.ceil() as i64, c.max.y.ceil() as i64);
    let (mw, mh) = (src.width() as i64 - 1, src.height() as i64 - 1);
    let mut out = Vec::new();
    for iy in y0..y1 {
        let t = (r.min.y + (iy as f32 - s.min.y) * k).clamp(r.min.y, r.max.y);
        let b = (r.min.y + (iy as f32 + 1.0 - s.min.y) * k).clamp(r.min.y, r.max.y);
        if b <= t {
            continue;
        }
        for ix in x0..x1 {
            let l = (r.min.x + (ix as f32 - s.min.x) * k).clamp(r.min.x, r.max.x);
            let rr = (r.min.x + (ix as f32 + 1.0 - s.min.x) * k).clamp(r.min.x, r.max.x);
            if rr <= l {
                continue;
            }
            let px = src
                .get_pixel(ix.clamp(0, mw) as u32, iy.clamp(0, mh) as u32)
                .0;
            out.push((
                Rect::from_min_max(Pos2::new(l, t), Pos2::new(rr, b)),
                Color32::from_rgba_unmultiplied(px[0], px[1], px[2], px[3]),
            ));
        }
    }
    out
}

impl Shape {
    /// Copia desenată dedesubt ca umbră (ShareX: shadow on, offset 0/1, negru 125).
    pub fn shadow_copy(&self) -> Option<Shape> {
        let op = |c: Color32| if c.a() == 255 { SHADOW } else { Color32::TRANSPARENT };
        let mut s = match self {
            Shape::Highlight { .. }
            | Shape::Pixelate { .. }
            | Shape::Blur { .. }
            | Shape::Spotlight { .. }
            | Shape::Erase { .. }
            // o umbră dreptunghiulară ar apărea și în jurul zonelor
            // transparente ale imaginii, deci nu desenăm niciuna
            | Shape::Img { .. } => return None,
            Shape::Rect { rect, fill, width, radius, .. } => Shape::Rect {
                rect: *rect,
                border: SHADOW,
                fill: op(*fill),
                width: *width,
                radius: *radius,
            },
            Shape::Ellipse { rect, fill, width, .. } => Shape::Ellipse {
                rect: *rect,
                border: SHADOW,
                fill: op(*fill),
                width: *width,
            },
            Shape::Line { from, to, mid, curbat, width, .. } => Shape::Line {
                from: *from,
                to: *to,
                mid: mid.clone(),
                curbat: *curbat,
                color: SHADOW,
                width: *width,
            },
            Shape::Arrow { from, to, mid, curbat, width, .. } => Shape::Arrow {
                from: *from,
                to: *to,
                mid: mid.clone(),
                curbat: *curbat,
                color: SHADOW,
                width: *width,
            },
            Shape::Free { pts, width, arrow, .. } => Shape::Free {
                pts: pts.clone(),
                color: SHADOW,
                width: *width,
                arrow: *arrow,
            },
            Shape::Text { rect, text, opts, fill, outline_w, radius, .. } => Shape::Text {
                rect: *rect,
                text: text.clone(),
                opts: TextOpts { color: SHADOW, ..opts.clone() },
                fill: op(*fill),
                outline: if *outline_w > 0.0 { SHADOW } else { Color32::TRANSPARENT },
                outline_w: *outline_w,
                radius: *radius,
            },
            // balonul: casetă + coadă + text, toate în culoarea umbrei
            Shape::Balloon { rect, tail, text, opts, fill, width, radius, .. } => Shape::Balloon {
                rect: *rect,
                tail: *tail,
                text: text.clone(),
                opts: TextOpts { color: SHADOW, ..opts.clone() },
                fill: op(*fill),
                border: SHADOW,
                width: *width,
                radius: *radius,
            },
            // lupa nu se remărește în umbră: rămâne doar conturul ei
            Shape::Magnify { rect, border: _, width, .. } => Shape::Rect {
                rect: *rect,
                border: SHADOW,
                fill: Color32::TRANSPARENT,
                width: *width,
                radius: 0.0,
            },
            // umbra numărătorului e doar cercul, nu și cifra
            Shape::Step { center, n, size, fill, width, .. } => Shape::Step {
                center: *center,
                n: *n,
                size: *size,
                fill: op(*fill),
                border: SHADOW,
                text: Color32::TRANSPARENT,
                width: *width,
            },
        };
        s.translate(SHADOW_OFFSET);
        Some(s)
    }

    /// Desenează în preview. `map` duce din spațiul imaginii în cel al ecranului.
    pub fn draw(&self, p: &Painter, map: impl Fn(Pos2) -> Pos2 + Copy, zoom: f32, src: &RgbaImage) {
        let sw = |w: f32| (w * zoom).max(1.0);
        match self {
            Shape::Rect { rect, border, fill, width, radius } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                let cr = egui::CornerRadius::same((radius * zoom).round().clamp(0.0, 255.0) as u8);
                if fill.a() > 0 {
                    p.rect_filled(r, cr, *fill);
                }
                if *width > 0.0 && border.a() > 0 {
                    p.rect_stroke(r, cr, Stroke::new(sw(*width), *border), StrokeKind::Middle);
                }
            }
            Shape::Ellipse { rect, border, fill, width } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                if fill.a() > 0 {
                    p.add(egui::Shape::ellipse_filled(r.center(), r.size() / 2.0, *fill));
                }
                if *width > 0.0 && border.a() > 0 {
                    p.add(egui::Shape::ellipse_stroke(
                        r.center(),
                        r.size() / 2.0,
                        Stroke::new(sw(*width), *border),
                    ));
                }
            }
            Shape::Line { from, to, mid, curbat, color, width } => {
                let poly = curve_poly(*from, *to, mid, *curbat);
                draw_poly(p, &poly, Stroke::new(sw(*width), *color), &map);
            }
            Shape::Arrow { from, to, mid, curbat, color, width } => {
                let poly = curve_poly(*from, *to, mid, *curbat);
                let (tail, head) = arrow_geom(&poly, *width);
                draw_poly(p, &tail, Stroke::new(sw(*width), *color), &map);
                draw_arrow_head(p, head, *color, &map);
            }
            Shape::Free { pts, color, width, arrow } => {
                if pts.len() >= 2 {
                    let path: Vec<Pos2> = pts.iter().map(|q| map(*q)).collect();
                    let st = Stroke::new(sw(*width), *color);
                    p.add(egui::Shape::line(path, st));
                    if *arrow {
                        let (a, b) = (pts[pts.len().saturating_sub(4)], pts[pts.len() - 1]);
                        draw_arrow_head(p, arrow_head(a, b, *width), *color, &map);
                    }
                }
            }
            Shape::Text { rect, text, opts, fill, outline, outline_w, radius } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                if fill.a() > 0 {
                    p.rect_filled(
                        r,
                        egui::CornerRadius::same((radius * zoom).round().clamp(0.0, 255.0) as u8),
                        *fill,
                    );
                }
                if text.is_empty() {
                    return;
                }
                // aceeași așezare ca la export: rând cu rând, la pozițiile date
                // de aliniere, ca preview-ul și PNG-ul să nu se despartă
                let size = opts.size * zoom;
                let lay = font::layout(text, opts, size, r);
                let fid = font::opts_font_id(opts, size);
                let uls = font::underline_rects(&lay, opts);
                let row = |off: Vec2, c: Color32| {
                    for line in &lay.lines {
                        if line.text.is_empty() {
                            continue;
                        }
                        let g = p.layout_no_wrap(line.text.clone(), fid.clone(), c);
                        p.add(egui::Shape::galley(Pos2::new(line.x, line.top) + off, g, c));
                    }
                    for u in &uls {
                        p.rect_filled(u.translate(off), egui::CornerRadius::ZERO, c);
                    }
                };
                for off in outline_offsets(*outline_w * zoom) {
                    row(off, *outline);
                }
                row(Vec2::ZERO, opts.color);
            }
            Shape::Balloon { rect, tail, text, opts, fill, border, width, radius } => {
                // schița nefinalizată poate avea `max < min` (tragere înapoi)
                let rect = &Rect::from_two_pos(rect.min, rect.max);
                // caseta: exact aceleași reguli ca dreptunghiul cu colțuri rotunjite
                box_shape(*rect, *fill, *border, *width, *radius).draw(p, map, zoom, src);
                let t = balloon_tail(*rect, *tail);
                if fill.a() > 0 {
                    let pts: Vec<Pos2> = t.fill_pts(*width).iter().map(|q| map(*q)).collect();
                    p.add(egui::Shape::convex_polygon(pts, *fill, Stroke::NONE));
                }
                if *width > 0.0 && border.a() > 0 {
                    // doar laturile oblice: baza ar trage o linie prin balon
                    let st = Stroke::new(sw(*width), *border);
                    for b in t.base {
                        p.line_segment([map(b), map(t.tip)], st);
                    }
                }
                // textul trece prin aceeași cale ca la caseta de text
                text_shape(*rect, text, opts).draw(p, map, zoom, src);
            }
            Shape::Magnify { rect, strength, border, width } => {
                let rect = &Rect::from_two_pos(rect.min, rect.max);
                let blocks = magnify_blocks(src, *rect, *strength);
                if !blocks.is_empty() {
                    let mut mesh = egui::Mesh::default();
                    for (d, c) in blocks {
                        mesh.add_colored_rect(Rect::from_two_pos(map(d.min), map(d.max)), c);
                    }
                    p.add(egui::Shape::mesh(mesh));
                }
                box_shape(*rect, Color32::TRANSPARENT, *border, *width, 0.0).draw(p, map, zoom, src);
            }
            Shape::Step { center, n, size, fill, border, text, width } => {
                let rad = step_radius(*n, *size) * zoom;
                let c = map(*center);
                if fill.a() > 0 {
                    p.circle_filled(c, rad, *fill);
                }
                if *width > 0.0 && border.a() > 0 {
                    p.circle_stroke(c, rad, Stroke::new(sw(*width), *border));
                }
                if text.a() > 0 {
                    let g = p.layout_no_wrap(n.to_string(), font::font_id(*size * zoom, true), *text);
                    p.add(egui::Shape::galley(c - g.size() / 2.0, g, *text));
                }
            }
            Shape::Highlight { rect, color } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                p.rect_filled(r, egui::CornerRadius::ZERO, highlight_paint(*color));
            }
            Shape::Pixelate { rect, block } => {
                let b = block.max(2.0);
                let r = Rect::from_two_pos(rect.min, rect.max).intersect(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(src.width() as f32, src.height() as f32),
                ));
                if !r.is_positive() {
                    return;
                }
                let mut y = r.min.y;
                while y < r.max.y {
                    let mut x = r.min.x;
                    while x < r.max.x {
                        let (x2, y2) = ((x + b).min(r.max.x), (y + b).min(r.max.y));
                        let c = block_avg(src, x as i64, y as i64, x2 as i64, y2 as i64);
                        p.rect_filled(
                            Rect::from_two_pos(map(Pos2::new(x, y)), map(Pos2::new(x2, y2))),
                            egui::CornerRadius::ZERO,
                            c,
                        );
                        x += b;
                    }
                    y += b;
                }
            }
            Shape::Blur { rect, radius } => {
                let rect = &Rect::from_two_pos(rect.min, rect.max);
                let Some(b) = blur_region(src, *rect, *radius) else { return };
                // un singur mesh cu câte un dreptunghi per pixel (rulările de
                // aceeași culoare se unesc), ca să nu inundăm painter-ul
                let mut mesh = egui::Mesh::default();
                for y in 0..b.h {
                    let (yt, yb) = ((b.y0 + y as i64) as f32, (b.y0 + y as i64 + 1) as f32);
                    let mut x = 0usize;
                    while x < b.w {
                        let c = b.px[y * b.w + x];
                        let mut x2 = x + 1;
                        while x2 < b.w && b.px[y * b.w + x2] == c {
                            x2 += 1;
                        }
                        let a = map(Pos2::new((b.x0 + x as i64) as f32, yt));
                        let d = map(Pos2::new((b.x0 + x2 as i64) as f32, yb));
                        mesh.add_colored_rect(
                            Rect::from_two_pos(a, d),
                            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                        );
                        x = x2;
                    }
                }
                p.add(egui::Shape::mesh(mesh));
            }
            // Reflectorul nu desenează nimic pe cont propriu: stratul întunecat
            // se pune o singură dată, cu găuri pentru toate reflectoarele
            // (vezi spotlight_cover, apelat din app.rs și render.rs).
            Shape::Spotlight { .. } => {}
            Shape::Erase { rect, color } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                p.rect_filled(r, egui::CornerRadius::ZERO, *color);
            }
            // preview-ul lasă scalarea pe seama plăcii video; exportul pe CPU
            // reeșantionează `img` biliniar, în render.rs
            Shape::Img { rect, tex, .. } => {
                let r = Rect::from_two_pos(map(rect.min), map(rect.max));
                p.image(
                    tex.id(),
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }
}

/// Evidențierea: culoare semi-transparentă, ca marker-ul. Aceeași formulă
/// în preview și în export, ca să nu apară diferențe la copiere.
pub fn highlight_paint(c: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 128)
}

/// Punctul e în triunghi? Semnele produselor vectoriale trebuie să coincidă.
fn in_tri(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let cr = |u: Pos2, v: Pos2| (v.x - u.x) * (p.y - u.y) - (v.y - u.y) * (p.x - u.x);
    let (d1, d2, d3) = (cr(a, b), cr(b, c), cr(c, a));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Colțul opus rămâne ancorat la redimensionare, exact ca în ShareX.
/// Cele opt noduri ale unui dreptunghi, în ordinea din `NodePosition`:
/// colț stânga-sus, mijloc sus, colț dreapta-sus, mijloc dreapta, colț
/// dreapta-jos, mijloc jos, colț stânga-jos, mijloc stânga.
pub fn rect_nodes(rect: Rect) -> Vec<Pos2> {
    let r = Rect::from_two_pos(rect.min, rect.max);
    let (cx, cy) = (r.center().x, r.center().y);
    vec![
        r.left_top(),
        Pos2::new(cx, r.min.y),
        r.right_top(),
        Pos2::new(r.max.x, cy),
        r.right_bottom(),
        Pos2::new(cx, r.max.y),
        r.left_bottom(),
        Pos2::new(r.min.x, cy),
    ]
}

/// Nodurile de colț mută ambele coordonate, cele de pe laturi doar una —
/// latura opusă rămâne pe loc, ca în ShareX.
fn resize(rect: Rect, idx: usize, p: Pos2) -> Rect {
    let r = Rect::from_two_pos(rect.min, rect.max);
    let (mut min, mut max) = (r.min, r.max);
    match idx {
        0 => min = p,
        1 => min.y = p.y,
        2 => {
            max.x = p.x;
            min.y = p.y;
        }
        3 => max.x = p.x,
        4 => max = p,
        5 => max.y = p.y,
        6 => {
            min.x = p.x;
            max.y = p.y;
        }
        _ => min.x = p.x,
    }
    Rect::from_two_pos(min, max)
}

fn dist_seg(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    if l2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

impl Shape {
    /// Ce trebuie să știe fereastra de introducere a textului despre forma
    /// pe care o editează.
    pub fn text_of(&self) -> Option<TextInfo<'_>> {
        match self {
            Shape::Text { text, opts, fill, outline, outline_w, .. } => Some(TextInfo {
                text,
                opts,
                // „culoarea secundară": conturul la textul cu contur,
                // fundalul casetei la textul cu fundal
                color2: if *outline_w > 0.0 { *outline } else { *fill },
                outline: *outline_w > 0.0,
            }),
            Shape::Balloon { text, opts, fill, .. } => Some(TextInfo {
                text,
                opts,
                color2: *fill,
                outline: false,
            }),
            _ => None,
        }
    }

    /// Forma poartă text editabil (casetă de text sau balon)?
    pub fn has_text(&self) -> bool {
        self.text_of().is_some()
    }

    /// Scrie înapoi rezultatul ferestrei de text.
    pub fn set_text(&mut self, s: String, o: TextOpts, c2: Color32) {
        match self {
            Shape::Text { text, opts, fill, outline, outline_w, .. } => {
                *text = s;
                *opts = o;
                if *outline_w > 0.0 {
                    *outline = c2;
                } else {
                    *fill = c2;
                }
            }
            Shape::Balloon { text, opts, fill, .. } => {
                *text = s;
                *opts = o;
                *fill = c2;
            }
            _ => {}
        }
    }

    /// Aduce dreptunghiul formei la forma normală (`min` sus-stânga).
    /// Tragerea de la dreapta la stânga sau de jos în sus lasă `max < min`, iar
    /// variantele care folosesc `rect` direct n-ar mai desena nimic
    /// (`is_positive()` e fals). O chemăm la commit, nu în `update_draft`:
    /// acolo `min` e ancora și trebuie să rămână fixă cât timp tragi.
    pub fn normalize(&mut self) {
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Balloon { rect, .. }
            | Shape::Text { rect, .. } => *rect = Rect::from_two_pos(rect.min, rect.max),
            Shape::Line { .. } | Shape::Arrow { .. } | Shape::Free { .. } | Shape::Step { .. } => {}
        }
    }

    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => Rect::from_two_pos(rect.min, rect.max),
            // balonul: caseta plus vârful cozii, ca selecția să le cuprindă pe amândouă
            Shape::Balloon { rect, tail, .. } => {
                let mut r = Rect::from_two_pos(rect.min, rect.max);
                r.extend_with(*tail);
                r
            }
            // curba poate ieși ușor din poligonul punctelor, deci luăm chiar
            // linia frântă desenată
            Shape::Arrow { from, to, mid, curbat, .. } | Shape::Line { from, to, mid, curbat, .. } => {
                let mut r = Rect::NOTHING;
                for q in curve_poly(*from, *to, mid, *curbat) {
                    r.extend_with(q);
                }
                r
            }
            Shape::Free { pts, .. } => {
                let mut r = Rect::NOTHING;
                for p in pts {
                    r.extend_with(*p);
                }
                r
            }
            Shape::Step { center, n, size, .. } => {
                Rect::from_center_size(*center, Vec2::splat(step_radius(*n, *size) * 2.0))
            }
        }
    }

    /// Punctele de control trase cu mouse-ul pentru redimensionare.
    pub fn handles(&self) -> Vec<Pos2> {
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => rect_nodes(*rect),
            // al nouălea mâner al balonului e vârful cozii, ca `NodePosition::Extra`
            Shape::Balloon { rect, tail, .. } => {
                let mut v = rect_nodes(*rect);
                v.push(*tail);
                v
            }
            // capătul de plecare, nodurile de curbură, capătul final
            Shape::Arrow { from, to, mid, .. } | Shape::Line { from, to, mid, .. } => {
                line_pts(*from, *to, mid)
            }
            Shape::Free { .. } | Shape::Step { .. } => Vec::new(),
        }
    }

    pub fn move_handle(&mut self, idx: usize, p: Pos2) {
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => *rect = resize(*rect, idx, p),
            // vârful cozii se mută independent de casetă
            Shape::Balloon { rect, tail, .. } => {
                if idx >= 8 {
                    *tail = p;
                } else {
                    *rect = resize(*rect, idx, p);
                }
            }
            // tragerea unui nod din mijloc curbează linia pe veci, ca în ShareX
            Shape::Arrow { from, to, mid, curbat, .. } | Shape::Line { from, to, mid, curbat, .. } => {
                if idx == 0 {
                    *from = p;
                } else if idx > mid.len() {
                    *to = p;
                } else {
                    mid[idx - 1] = p;
                    *curbat = true;
                }
                auto_mid(*from, *to, mid, *curbat);
            }
            Shape::Free { .. } | Shape::Step { .. } => {}
        }
    }

    pub fn translate(&mut self, d: Vec2) {
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => *rect = rect.translate(d),
            Shape::Balloon { rect, tail, .. } => {
                *rect = rect.translate(d);
                *tail += d;
            }
            Shape::Arrow { from, to, mid, .. } | Shape::Line { from, to, mid, .. } => {
                *from += d;
                *to += d;
                for m in mid.iter_mut() {
                    *m += d;
                }
            }
            Shape::Free { pts, .. } => {
                for p in pts.iter_mut() {
                    *p += d;
                }
            }
            Shape::Step { center, .. } => *center += d,
        }
    }

    /// Înmulțește toate coordonatele cu `k`. Folosită la redimensionarea
    /// imaginii, ca formele să rămână peste aceleași locuri din poză.
    pub fn scale(&mut self, k: Vec2) {
        let pt = |q: Pos2| Pos2::new(q.x * k.x, q.y * k.y);
        let rc = |b: Rect| Rect::from_min_max(pt(b.min), pt(b.max));
        match self {
            Shape::Rect { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => *rect = rc(*rect),
            Shape::Balloon { rect, tail, .. } => {
                *rect = rc(*rect);
                *tail = pt(*tail);
            }
            Shape::Arrow { from, to, mid, .. } | Shape::Line { from, to, mid, .. } => {
                *from = pt(*from);
                *to = pt(*to);
                for m in mid.iter_mut() {
                    *m = pt(*m);
                }
            }
            Shape::Free { pts, .. } => {
                for q in pts.iter_mut() {
                    *q = pt(*q);
                }
            }
            Shape::Step { center, .. } => *center = pt(*center),
        }
    }

    pub fn hit(&self, p: Pos2, tol: f32) -> bool {
        match self {
            Shape::Rect { rect, width, fill, .. } => {
                let r = Rect::from_two_pos(rect.min, rect.max);
                let t = tol.max(*width);
                if fill.a() > 0 {
                    return r.expand(t).contains(p);
                }
                r.expand(t).contains(p) && !r.shrink(t).contains(p)
            }
            Shape::Pixelate { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Spotlight { rect }
            | Shape::Erase { rect, .. }
            | Shape::Magnify { rect, .. }
            | Shape::Img { rect, .. }
            | Shape::Text { rect, .. } => Rect::from_two_pos(rect.min, rect.max).contains(p),
            // balonul: interiorul casetei sau al triunghiului cozii
            Shape::Balloon { rect, tail, .. } => {
                if Rect::from_two_pos(rect.min, rect.max).contains(p) {
                    return true;
                }
                let t = balloon_tail(*rect, *tail);
                in_tri(p, t.base[0], t.base[1], t.tip)
            }
            Shape::Ellipse { rect, width, fill, .. } => {
                let r = Rect::from_two_pos(rect.min, rect.max);
                let c = r.center();
                let (rx, ry) = (r.width().max(1.0) / 2.0, r.height().max(1.0) / 2.0);
                let n = (((p.x - c.x) / rx).powi(2) + ((p.y - c.y) / ry).powi(2)).sqrt();
                if fill.a() > 0 && n <= 1.0 {
                    return true;
                }
                (n - 1.0).abs() * rx.min(ry) <= tol.max(*width)
            }
            // pe o linie curbată nimerirea urmează curba, nu coarda
            Shape::Arrow { from, to, mid, curbat, width, .. }
            | Shape::Line { from, to, mid, curbat, width, .. } => curve_poly(*from, *to, mid, *curbat)
                .windows(2)
                .any(|w| dist_seg(p, w[0], w[1]) <= tol.max(*width)),
            Shape::Free { pts, width, .. } => pts
                .windows(2)
                .any(|w| dist_seg(p, w[0], w[1]) <= tol.max(*width)),
            Shape::Step { center, n, size, .. } => center.distance(p) <= step_radius(*n, *size) + tol,
        }
    }
}
