use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontRef, FontVec, GlyphId, PxScale, ScaleFont as _, point};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Vec2};

use crate::i18n::{self, Msg, t};

/// Același fișier de font e folosit și de egui (preview) și de rasterizatorul
/// nostru CPU (export), ca textul să iasă identic în ambele.
pub const REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

/// Familia implicită și rezerva pentru orice familie care nu se poate încărca.
pub const DEFAULT_FAMILY: &str = "DejaVu Sans";

const FAM: &str = "sxr";
const FAM_BOLD: &str = "sxr-bold";

// ------------------------------------------------------------- opțiuni text

/// Alinierea textului în casetă (ShareX: `StringAlignment`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Near,
    Center,
    Far,
}

impl Align {
    /// Cele trei valori, în ordinea din meniurile ShareX.
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

/// Copia noastră după `TextDrawingOptions.cs`: tot ce se poate schimba din
/// fereastra de introducere a textului. Fără degrade — sxr nu îl desenează.
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
    /// `TextDrawingOptions.EnterKeyNewLine`: cu ea pornită, în fereastra de
    /// text Enter face rând nou și Ctrl+Enter dă OK; oprită, e invers.
    /// Butonul din stânga-jos al ferestrei o comută.
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

// ---------------------------------------------------- lista fonturilor din sistem

struct Entry {
    family: String,
    style: String,
    file: PathBuf,
}

struct Db {
    families: Vec<String>,
    entries: Vec<Entry>,
    /// Motivul pentru care lista a rămas goală, dacă e cazul.
    note: Option<String>,
}

/// Enumerarea se face o singură dată pe sesiune, la prima cerere (deschiderea
/// ferestrei de text), nu la fiecare cadru.
fn db() -> &'static Db {
    static D: OnceLock<Db> = OnceLock::new();
    D.get_or_init(scan)
}

/// Extensiile pe care `ab_glyph` le poate deschide. Restul (pcf, bdf, pfb...)
/// sunt fonturi bitmap sau Type1 și nu au ce căuta în listă.
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
        // format: „/cale/fisier.ttf: Familie1,Familie2:style=Stil1,Stil2"
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
            // fontconfig scapă unele caractere cu backslash; niciun nume de
            // familie nu conține așa ceva, deci le scoatem pur și simplu
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
    /// Fețele deja citite de pe disc, ca să nu recitim fișierul la fiecare cadru.
    static CACHE: RefCell<BTreeMap<(String, bool, bool), Face>> = RefCell::new(BTreeMap::new());
    /// Fonturile pregătite pentru egui: nume de familie -> datele fișierului.
    static REG: RefCell<BTreeMap<String, Arc<egui::FontData>>> = RefCell::new(BTreeMap::new());
    /// Numele deja trimise lui egui prin `set_fonts`. Doar acestea pot fi
    /// folosite într-un `FontId`, altfel egui nu găsește familia.
    static LIVE: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// Familii trimise la `set_fonts`, dar încă neaplicate: egui reface atlasul
    /// abia la începutul cadrului următor, iar o cerere de font necunoscut
    /// ar arunca panică. Trec în `LIVE` de-abia atunci, prin `sync`.
    static PEND: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// Familiile care s-au dovedit imposibil de încărcat; ies din listă.
    static BAD: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    /// Mesaje pentru bara de stare, culese de editor după fiecare încărcare.
    static NOTES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

#[derive(Clone)]
struct Face {
    font: Rc<FontVec>,
    /// Numele familiei sub care e (sau va fi) înregistrat fontul în egui.
    egui: String,
}

/// Familiile din sistem, fără cele care s-au dovedit imposibil de încărcat.
/// Lista e goală (doar DejaVu) dacă `fc-list` lipsește.
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

/// Golește mesajele adunate de la ultima chemare (familii care n-au putut fi
/// încărcate, variante lipsă). Editorul le pune în bara de stare.
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

/// Cât de bine se potrivește un stil din `fc-list` cu ce s-a cerut.
/// Mai mare = mai bine; stilurile cu vorbe în plus („Condensed", „Light")
/// pierd puncte, ca să nu fure locul variantei obișnuite.
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

/// Fișierul cel mai potrivit din familie, plus dacă e chiar varianta cerută.
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

/// Fața de rezervă: DejaVu Sans încorporat în binar.
fn fallback(bold: bool) -> Face {
    let data = if bold { BOLD } else { REGULAR };
    let font = FontVec::try_from_vec(data.to_vec()).expect("DejaVu încorporat invalid");
    Face {
        font: Rc::new(font),
        egui: if bold { FAM_BOLD } else { FAM }.to_owned(),
    }
}

fn build(family: &str, bold: bool, italic: bool) -> Face {
    // familia implicită fără cursiv vine din binar: e mereu disponibilă și
    // e deja înregistrată în egui de `install`
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
    // colecțiile .ttc au mai multe fețe; o luăm pe prima
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

// ------------------------------------------------------- înregistrarea în egui

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

/// Se asigură că familia cerută e citită de pe disc ȘI cunoscută de egui.
/// Se cheamă din fereastra de text, nu din desenare: `set_fonts` reface
/// atlasul, deci n-are ce căuta în mijlocul unui cadru de pictat.
pub fn register(ctx: &egui::Context, o: &TextOpts) {
    let name = face_of(o).egui;
    if !LIVE.with_borrow(|l| l.contains(&name)) && !PEND.with_borrow(|p| p.contains(&name)) {
        apply(ctx);
        // fontul nou se vede din cadrul următor, deci mai cerem unul
        ctx.request_repaint();
    }
}

/// De chemat la începutul fiecărui cadru: ce s-a trimis la `set_fonts` în
/// cadrul precedent e acum instalat și poate fi cerut fără riscul unei panici.
pub fn sync() {
    PEND.with_borrow_mut(|p| {
        if !p.is_empty() {
            LIVE.with_borrow_mut(|l| l.append(p));
        }
    });
}

/// `FontId`-ul pentru preview. Dacă familia n-a apucat să ajungă la egui,
/// cădem pe DejaVu în loc să cerem o familie necunoscută (egui ar intra în panică).
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

// ------------------------------------------------------ DejaVu, calea simplă

fn face(bold: bool) -> FontRef<'static> {
    FontRef::try_from_slice(if bold { BOLD } else { REGULAR }).expect("font invalid")
}

pub fn font_id(size: f32, bold: bool) -> FontId {
    FontId::new(
        size.max(1.0),
        FontFamily::Name(if bold { FAM_BOLD } else { FAM }.into()),
    )
}

/// Lățimea și înălțimea blocului de text în DejaVu. Fără wrap — doar `\n`
/// rupe rândul, exact ca `layout_no_wrap` din preview. O folosește
/// numărătorul, care scrie mereu cu fontul încorporat.
pub fn measure(text: &str, size: f32, bold: bool) -> (f32, f32) {
    let f = face(bold);
    let s = f.as_scaled(PxScale::from(size.max(1.0)));
    // aceeași formulă ca egui: row_height = ascent - descent + line_gap
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

/// Rasterizează pe CPU cu fontul încorporat. `x`,`y` sunt colțul stânga-sus
/// al blocului; callback-ul primește pixelul și acoperirea (0..1).
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

// ----------------------------------------------------- așezarea textului în casetă

/// Un rând așezat: colțul lui din stânga-sus și lățimea măsurată.
pub struct Line {
    pub text: String,
    pub x: f32,
    pub top: f32,
    pub w: f32,
}

/// Rezultatul așezării. Aceeași structură hrănește și preview-ul, și exportul,
/// deci alinierea și sublinierea ies identice în amândouă.
pub struct Layout {
    pub lines: Vec<Line>,
    pub ascent: f32,
    /// Poziția liniei de subliniere față de vârful rândului, și grosimea ei.
    pub ul_top: f32,
    pub ul_h: f32,
}

/// Așază textul în `rect` după alinierile din `o`. `size` e mărimea deja
/// scalată (preview) sau chiar `o.size` (export).
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
    // ab_glyph nu dă metricile de subliniere din tabela `post`, deci punem
    // linia la jumătatea coborâtoarei — sub linia de bază, ca în GDI+
    Layout {
        lines,
        ascent: s.ascent(),
        ul_top: s.ascent() - s.descent() * 0.45,
        ul_h: (size / 14.0).max(1.0),
    }
}

/// Desenează pe CPU rândurile deja așezate, deplasate cu `off`
/// (folosit de contur, care repetă textul în jurul poziției de bază).
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

/// Dreptunghiurile de subliniere, câte unul pe rând. Goale dacă nu s-a cerut
/// subliniere sau dacă rândul e gol.
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
