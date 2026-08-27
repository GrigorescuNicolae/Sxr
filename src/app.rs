use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Vec2};
use image::RgbaImage;

use crate::clip;
use crate::font;
use crate::icons::Icons;
use crate::render;
use crate::shape::{self, Shape, Tool};

/// Lățimea barei de unelte în puncte — fereastra nu coboară sub ea,
/// altfel meniurile din dreapta ar ieși din cadru (Wayland ignoră
/// redimensionarea cerută de aplicație după creare).
const BAR_W: f32 = 1112.0;

/// Nodul de redimensionare, ca `Resources/CircleNode.png` din ShareX: un disc
/// alb plin de 18px, într-o casetă de 24px. Varianta desenată cu contur din
/// `ResizeNode.OnDraw` se folosește doar când `UseLightResizeNodes` e activat,
/// iar opțiunea aceea e implicit oprită, deci nu se vede niciodată în mod normal.
const NODE: f32 = 18.0;
/// Latura casetei nodului — și raza în care se prinde cu mausul.
const NODE_HIT: f32 = 24.0;

pub fn run(img: RgbaImage) -> Result<()> {
    let (w, h) = img.dimensions();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([
                (w as f32 + 24.0).clamp(BAR_W, 1750.0),
                (h as f32 + 108.0).min(980.0),
            ])
            .with_min_inner_size([BAR_W, 340.0])
            .with_title("sxr — editor"),
        ..Default::default()
    };
    eframe::run_native(
        "sxr",
        opts,
        Box::new(move |cc| Ok(Box::new(Editor::new(cc, img)) as Box<dyn eframe::App>)),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

/// Snapshot pentru undo. Imaginea e clonată doar la decupare,
/// ca să nu copiem câțiva MB la fiecare linie desenată.
struct Snap {
    shapes: Vec<Shape>,
    img: Option<RgbaImage>,
}

enum Drag {
    Move { last: Pos2 },
    Handle { idx: usize },
}

/// Acțiune amânată cu un cadru: comanda de minimizare trebuie să ajungă întâi
/// la compozitor, altfel fereastra noastră ar apărea în captură.
enum Pending {
    /// Captură de regiune, de ștampilat la punctul dat. `center` = cererea vine
    /// din meniu, deci forma se recentrează pe imagine după inserare.
    Screen { at: Pos2, center: bool },
}

/// Ce cere dialogul modal deschis peste pânză.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DlgKind {
    New,
    Size,
    Canvas,
    /// Fereastra de introducere a textului (ShareX: TextDrawingInputBox).
    Text,
}

/// Starea ferestrei de text. Câmpurile numerice de mai jos nu o privesc.
#[derive(Default)]
struct TextDlg {
    /// Forma editată.
    idx: usize,
    /// Forma tocmai a fost creată: dacă renunțăm, dispare cu totul.
    fresh: bool,
    /// E text cu contur (nu text cu fundal): decide unde se salvează implicitele.
    outline: bool,
    buf: String,
    opts: shape::TextOpts,
    /// Conturul, respectiv fundalul casetei — „culoarea secundară" din ShareX.
    color2: Color32,
    /// Primul cadru: mutăm focalizarea în zona de text.
    focus: bool,
}

/// Dialogul propriu: kdialog n-are nici câmpuri numerice, nici fereastră de text.
struct Dialog {
    kind: DlgKind,
    w: u32,
    h: u32,
    /// Fundalul imaginii noi, respectiv umplerea pânzei adăugate.
    color: Color32,
    /// „Păstrează proporțiile" — doar la Dimensiune imagine.
    keep: bool,
    /// Înălțime / lățime la deschidere: recalcularea pornește mereu de aici,
    /// ca raportul să nu derive după mai multe modificări.
    ratio: f32,
    /// Valorile de la cadrul anterior, ca să știm ce latură a schimbat omul.
    last_w: u32,
    last_h: u32,
    text: TextDlg,
}

impl Dialog {
    fn new(kind: DlgKind, w: u32, h: u32) -> Self {
        Self {
            kind,
            w,
            h,
            color: Color32::WHITE,
            keep: true,
            ratio: h as f32 / w.max(1) as f32,
            last_w: w,
            last_h: h,
            text: TextDlg::default(),
        }
    }
}

/// Comenzile barei și ale tastaturii. Le adunăm într-o listă și le executăm
/// după ce s-a terminat construirea barei, ca să nu ne batem pe împrumutul lui `self`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Apply,
    Close,
    Copy,
    Save,
    SaveAs,
    Upload,
    Print,
    Undo,
    Redo,
    Dup,
    Del,
    DelAll,
    Front,
    Forward,
    Backward,
    Back,
    CropTool,
    NewImg,
    OpenImg,
    InsertFile,
    InsertScreen,
    ImgSize,
    CanvasSize,
    AutoCrop,
    RotRight,
    RotLeft,
    DlgOk,
    DlgCancel,
}

/// Valorile implicite din AnnotationOptions.cs, câmp cu câmp.
struct Opts {
    border: Color32,
    fill: Color32,
    border_size: f32,
    corner_radius: f32,
    shadow: bool,
    /// Câte noduri de curbură primesc liniile și săgețile noi
    /// (ShareX: `LineCenterPointCount`, implicit 1).
    line_mid: usize,

    text_fill: Color32,
    text_border: Color32,
    text_border_w: f32,
    /// Implicitele casetei de text cu fundal (și ale balonului).
    text: shape::TextOpts,

    outline_border: Color32,
    outline_w: f32,
    /// Implicitele textului cu contur.
    outline: shape::TextOpts,

    step_fill: Color32,
    step_border: Color32,
    step_text: Color32,
    step_font: f32,
    step_border_w: f32,
    step_start: u32,

    pixelate: f32,
    blur: f32,
    magnify: f32,
    highlight: Color32,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            border: shape::PRIMARY,
            fill: Color32::TRANSPARENT,
            border_size: 4.0,
            corner_radius: 3.0,
            shadow: true,
            line_mid: 1,

            text_fill: shape::PRIMARY,
            text_border: shape::SECONDARY,
            text_border_w: 2.0,
            text: shape::TextOpts::default(),

            outline_border: shape::PRIMARY,
            outline_w: 5.0,
            // ShareX scrie textul cu contur mai mare și îngroșat
            outline: shape::TextOpts {
                size: 25.0,
                bold: true,
                ..shape::TextOpts::default()
            },

            step_fill: shape::PRIMARY,
            step_border: shape::SECONDARY,
            step_text: Color32::WHITE,
            // ShareX pornește de la 18; cifrele ieșeau prea mici, deci ~20% mai mare
            step_font: 22.0,
            step_border_w: 0.0,
            step_start: 1,

            pixelate: 15.0,
            blur: 35.0,
            magnify: 200.0,
            highlight: Color32::YELLOW,
        }
    }
}

struct Editor {
    img: RgbaImage,
    tex: TextureHandle,
    icons: Rc<Icons>,
    shapes: Vec<Shape>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    draft: Option<Shape>,
    sel: Option<usize>,
    drag: Option<Drag>,
    tool: Tool,
    opt: Opts,
    /// Numărul pe care îl va primi următorul contor plasat.
    step_next: u32,
    last_save: Option<std::path::PathBuf>,
    status: String,
    /// Lățimea reală a rândului de butoane, măsurată la cadrul anterior.
    /// O folosim ca să centrăm bara, ca în ShareX.
    bar_w: f32,
    /// Captura de ecran cerută la cadrul anterior, încă neexecutată.
    pending: Option<Pending>,
    /// Contor pentru numele texturilor: două imagini inserate nu au voie să
    /// primească același nume, altfel a doua o suprascrie pe prima.
    img_seq: u32,
    /// Săgeata încorporată, decodată la prima folosire.
    cursor: Option<Arc<RgbaImage>>,
    /// Dialogul modal deschis, dacă e vreunul.
    dialog: Option<Dialog>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// ~/Pictures dacă există, altfel directorul personal.
fn pictures_dir() -> PathBuf {
    let p = home().join("Pictures");
    if p.is_dir() { p } else { home() }
}

/// ~/.local/share/sxr/stickers, cu XDG_DATA_HOME respectat.
fn stickers_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/share"))
        .join("sxr")
        .join("stickers")
}

/// Dialogul nativ KDE de deschidere. `None` dacă utilizatorul anulează sau
/// dacă `kdialog` lipsește — în ambele cazuri pur și simplu nu se întâmplă nimic.
fn pick_image(dir: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("kdialog")
        .arg("--getopenfilename")
        .arg(dir)
        .arg("image/png image/jpeg image/webp")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

fn to_color_image(img: &RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}

/// Iconița butonului de culoare, redesenată după ShareX
/// (`ImageHelpers.DrawColorPickerIcon`): pătrat plin cu chenar negru de 1px.
/// Dacă `hole > 0`, mijlocul rămâne gol și primește și el un chenar — așa
/// arată butonul „culoare contur", spre deosebire de cel de umplere.
/// Culoarea transparentă se vede ca tablă de șah, tot ca în ShareX.
/// Fundal de contrast pentru zona de scris, ca în `ColorHelpers.VisibleColor`:
/// alb sub un text închis, gri închis sub unul deschis.
fn visible_bg(c: Color32) -> Color32 {
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if l > 128.0 {
        Color32::from_gray(50)
    } else {
        Color32::WHITE
    }
}

fn draw_color_icon(p: &egui::Painter, rect: Rect, c: Color32, hole: f32) {
    let z = egui::CornerRadius::ZERO;
    let black = egui::Stroke::new(1.0, Color32::BLACK);
    let h = Rect::from_center_size(rect.center(), Vec2::splat(hole));
    // partea plină: tot pătratul, mai puțin gaura din mijloc
    let parts = if hole > 0.0 {
        vec![
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, h.min.y)),
            Rect::from_min_max(Pos2::new(rect.min.x, h.max.y), rect.max),
            Rect::from_min_max(Pos2::new(rect.min.x, h.min.y), Pos2::new(h.min.x, h.max.y)),
            Rect::from_min_max(Pos2::new(h.max.x, h.min.y), Pos2::new(rect.max.x, h.max.y)),
        ]
    } else {
        vec![rect]
    };
    let q = rect.size() / 2.0;
    for r in parts {
        if !r.is_positive() {
            continue;
        }
        if c.a() < 255 {
            let pc = p.with_clip_rect(r);
            pc.rect_filled(rect, z, Color32::from_gray(255));
            pc.rect_filled(Rect::from_min_size(rect.min, q), z, Color32::from_gray(204));
            pc.rect_filled(Rect::from_min_size(rect.center(), q), z, Color32::from_gray(204));
        }
        if c.a() > 0 {
            p.rect_filled(r, z, c);
        }
    }
    p.rect_stroke(rect, z, black, egui::StrokeKind::Inside);
    if hole > 0.0 {
        p.rect_stroke(h, z, black, egui::StrokeKind::Outside);
    }
}

/// Buton de culoare cu iconița de mai sus, care deschide același selector de
/// culoare al egui ca `color_edit_button_srgba` — doar desenul butonului diferă.
fn color_button(ui: &mut egui::Ui, id_salt: &str, color: &mut Color32, hole: f32, tip: &str) {
    let side = 16.0;
    let size = Vec2::splat(side) + ui.spacing().button_padding * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let popup_id = ui.auto_id_with(id_salt);
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if ui.is_rect_visible(rect) {
        let v = if open {
            ui.visuals().widgets.open
        } else {
            *ui.style().interact(&resp)
        };
        let bg = rect.expand(v.expansion);
        ui.painter()
            .rect(bg, v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        draw_color_icon(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(side)),
            *color,
            hole,
        );
    }
    let resp = resp.on_hover_text(tip);
    egui::Popup::menu(&resp)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 275.0;
            egui::widgets::color_picker::color_picker_color32(
                ui,
                color,
                egui::widgets::color_picker::Alpha::BlendOrAdditive,
            );
        });
}

impl Editor {
    fn new(cc: &eframe::CreationContext<'_>, img: RgbaImage) -> Self {
        let ctx = &cc.egui_ctx;
        font::install(ctx);
        ctx.set_theme(egui::Theme::Dark);
        let icons = Rc::new(Icons::load(ctx));
        let tex = ctx.load_texture("shot", to_color_image(&img), egui::TextureOptions::LINEAR);
        let opt = Opts::default();
        Self {
            img,
            tex,
            icons,
            shapes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            draft: None,
            sel: None,
            drag: None,
            tool: Tool::Rect,
            step_next: opt.step_start,
            opt,
            last_save: None,
            status: String::new(),
            bar_w: 0.0,
            pending: None,
            img_seq: 0,
            cursor: None,
            dialog: None,
        }
    }

    // ---------------------------------------------------------------- undo

    fn snap(&self, with_img: bool) -> Snap {
        Snap {
            shapes: self.shapes.clone(),
            img: with_img.then(|| self.img.clone()),
        }
    }

    fn push_undo(&mut self, with_img: bool) {
        self.undo.push(self.snap(with_img));
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, s: Snap) -> Snap {
        let back = Snap {
            shapes: std::mem::replace(&mut self.shapes, s.shapes),
            img: s.img.as_ref().map(|_| self.img.clone()),
        };
        if let Some(i) = s.img {
            self.img = i;
            self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
        }
        self.sel = None;
        back
    }

    fn do_undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let back = self.restore(s);
            self.redo.push(back);
        } else {
            self.status = "nimic de anulat".into();
        }
    }

    fn do_redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let back = self.restore(s);
            self.undo.push(back);
        } else {
            self.status = "nimic de refăcut".into();
        }
    }

    // ------------------------------------------------------------- schițe

    /// Forma de pornire pentru unealta curentă. `None` = unealta nu desenează nimic.
    fn new_draft(&self, at: Pos2) -> Option<Shape> {
        let o = &self.opt;
        let r = Rect::from_min_max(at, at);
        Some(match self.tool {
            Tool::Rect => Shape::Rect {
                rect: r,
                border: o.border,
                fill: o.fill,
                width: o.border_size,
                radius: o.corner_radius,
            },
            Tool::Ellipse => Shape::Ellipse {
                rect: r,
                border: o.border,
                fill: o.fill,
                width: o.border_size,
            },
            // nodurile de mijloc pornesc toate din punctul de plecare;
            // `update_draft` le împrăștie pe linie pe măsură ce tragi
            Tool::Line => Shape::Line {
                from: at,
                to: at,
                mid: vec![at; o.line_mid],
                curbat: false,
                color: o.border,
                width: o.border_size,
            },
            Tool::Arrow => Shape::Arrow {
                from: at,
                to: at,
                mid: vec![at; o.line_mid],
                curbat: false,
                color: o.border,
                width: o.border_size,
            },
            Tool::Freehand => Shape::Free {
                pts: vec![at],
                color: o.border,
                width: o.border_size,
                arrow: false,
            },
            Tool::FreehandArrow => Shape::Free {
                pts: vec![at],
                color: o.border,
                width: o.border_size,
                arrow: true,
            },
            Tool::Highlight => Shape::Highlight { rect: r, color: o.highlight },
            Tool::Pixelate => Shape::Pixelate { rect: r, block: o.pixelate },
            Tool::Blur => Shape::Blur { rect: r, radius: o.blur },
            Tool::Spotlight => Shape::Spotlight { rect: r },
            // culoarea gumei se calculează din inelul din jur; la schiță e încă
            // goală, update_draft o reface pe măsură ce tragi cu mouse-ul
            Tool::SmartEraser => Shape::Erase { rect: r, color: Color32::BLACK },
            Tool::TextOutline => Shape::Text {
                rect: r,
                text: String::new(),
                opts: o.outline.clone(),
                fill: Color32::TRANSPARENT,
                outline: o.outline_border,
                outline_w: o.outline_w,
                radius: 0.0,
            },
            Tool::TextBackground => Shape::Text {
                rect: r,
                text: String::new(),
                opts: o.text.clone(),
                fill: o.text_fill,
                outline: Color32::TRANSPARENT,
                outline_w: 0.0,
                radius: o.corner_radius,
            },
            // coada balonului se așază abia la commit, când caseta e finală
            Tool::SpeechBalloon => Shape::Balloon {
                rect: r,
                tail: at,
                text: String::new(),
                opts: o.text.clone(),
                fill: o.text_fill,
                border: o.text_border,
                width: o.text_border_w,
                radius: o.corner_radius,
            },
            Tool::Magnify => Shape::Magnify {
                rect: r,
                strength: o.magnify,
                border: o.border,
                width: o.border_size,
            },
            Tool::Step => Shape::Step {
                center: at,
                n: self.step_next,
                size: o.step_font,
                fill: o.step_fill,
                border: o.step_border,
                text: o.step_text,
                width: o.step_border_w,
            },
            // decuparea și tăierea de fâșie folosesc un chenar simplu,
            // nu culorile de adnotare
            Tool::Crop | Tool::CutOut => Shape::Rect {
                rect: r,
                border: Color32::WHITE,
                fill: Color32::TRANSPARENT,
                width: 1.0,
                radius: 0.0,
            },
            _ => return None,
        })
    }

    fn update_draft(&mut self, at: Pos2) {
        match self.draft.as_mut() {
            Some(
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
                | Shape::Text { rect, .. },
            ) => rect.max = at,
            Some(
                Shape::Arrow { from, to, mid, curbat, .. } | Shape::Line { from, to, mid, curbat, .. },
            ) => {
                *to = at;
                shape::auto_mid(*from, *to, mid, *curbat);
            }
            Some(Shape::Free { pts, .. }) => {
                if pts.last().is_none_or(|l| l.distance(at) > 1.0) {
                    pts.push(at);
                }
            }
            Some(Shape::Step { center, .. }) => *center = at,
            None => {}
        }
        // guma inteligentă își ia culoarea din inelul din jurul dreptunghiului;
        // o recalculăm cât timp o tragi, apoi rămâne fixă chiar dacă forma e mutată
        if let Some(Shape::Erase { rect, .. }) = &self.draft {
            let c = shape::ring_avg(&self.img, *rect);
            if let Some(Shape::Erase { color, .. }) = self.draft.as_mut() {
                *color = c;
            }
        }
    }

    /// Schița e destul de mare cât să merite păstrată?
    fn big_enough(s: &Shape) -> bool {
        match s {
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
            | Shape::Text { rect, .. } => rect.width().abs() > 2.0 && rect.height().abs() > 2.0,
            Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } => from.distance(*to) > 3.0,
            Shape::Free { pts, .. } => pts.len() > 1,
            // contorul se plasează dintr-un simplu clic
            Shape::Step { .. } => true,
        }
    }

    fn commit_draft(&mut self) {
        let Some(mut s) = self.draft.take() else { return };
        // tragerea de la dreapta la stânga (sau de jos în sus) lasă `max < min`;
        // `update_draft` nu are voie să normalizeze, altfel ancora ar migra odată
        // cu cursorul, deci îndreptăm dreptunghiul abia aici, la commit
        s.normalize();

        if matches!(self.tool, Tool::Crop | Tool::CutOut) {
            if let Shape::Rect { rect, .. } = s {
                if self.tool == Tool::Crop {
                    self.apply_crop(rect);
                } else {
                    self.apply_cutout(rect);
                }
            }
            return;
        }

        // o casetă de text (sau un balon) trasă prea scurt primește o dimensiune
        // implicită, altfel un clic nesigur ar pierde forma
        if let Shape::Text { rect, opts, .. } | Shape::Balloon { rect, opts, .. } = &mut s {
            let n = Rect::from_two_pos(rect.min, rect.max);
            *rect = if n.width() < 12.0 || n.height() < 12.0 {
                Rect::from_min_size(n.min, Vec2::new(220.0, opts.size * 1.8))
            } else {
                n
            };
        }
        // coada pornește cu 30px sub colțul din stânga-jos al casetei
        if let Shape::Balloon { rect, tail, .. } = &mut s {
            *tail = rect.left_bottom() + Vec2::new(0.0, 30.0);
        }

        if !Self::big_enough(&s) {
            return;
        }
        let is_text = s.has_text();
        let is_step = matches!(s, Shape::Step { .. });
        self.push_undo(false);
        self.shapes.push(s);
        let i = self.shapes.len() - 1;
        // ca în ShareX: forma abia trasă rămâne activă, cu nodurile la vedere,
        // ca să o poți ajusta imediat, fără să treci pe unealta de selecție
        self.sel = Some(i);
        if is_text {
            // ca în ShareX: forma abia trasă deschide fereastra de text
            self.open_text(i, true);
        }
        if is_step {
            self.step_next += 1;
        }
    }

    // ----------------------------------------------------- imagini inserate

    /// Inserează o imagine ca formă nouă: la dimensiunea nativă, cu colțul din
    /// stânga-sus în punctul de clic, micșorată proporțional dacă n-ar încăpea
    /// în imaginea de fundal.
    fn stamp(&mut self, ctx: &egui::Context, at: Pos2, img: Arc<RgbaImage>) {
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        if iw < 1.0 || ih < 1.0 {
            self.status = "imaginea e goală".into();
            return;
        }
        let k = (self.img.width() as f32 / iw)
            .min(self.img.height() as f32 / ih)
            .min(1.0);
        let rect = Rect::from_min_size(at, Vec2::new(iw * k, ih * k));
        self.img_seq += 1;
        let tex = ctx.load_texture(
            format!("sxr-img-{}", self.img_seq),
            to_color_image(&img),
            egui::TextureOptions::LINEAR,
        );
        self.push_undo(false);
        self.shapes.push(Shape::Img { rect, img, tex });
        // trecem pe unealta de selecție: altfel `sel` s-ar șterge la cadrul
        // următor (vezi `ui`) și forma abia pusă n-ar mai putea fi trasă
        self.tool = Tool::Select;
        self.sel = Some(self.shapes.len() - 1);
        self.status = format!("imagine inserată ({}x{})", iw as u32, ih as u32);
    }

    /// Încarcă un fișier de pe disc și îl ștampilează. La eroare doar anunță.
    fn stamp_file(&mut self, ctx: &egui::Context, at: Pos2, path: &Path) {
        match image::open(path) {
            Ok(i) => self.stamp(ctx, at, Arc::new(i.to_rgba8())),
            Err(e) => self.status = format!("nu pot deschide {}: {e}", path.display()),
        }
    }

    /// Săgeata clasică, încorporată în binar și decodată o singură dată.
    fn cursor_img(&mut self) -> Option<Arc<RgbaImage>> {
        if self.cursor.is_none() {
            const PNG: &[u8] = include_bytes!("../assets/cursor.png");
            match image::load_from_memory(PNG) {
                Ok(i) => self.cursor = Some(Arc::new(i.to_rgba8())),
                Err(e) => self.status = format!("cursor.png nu se poate decoda: {e}"),
            }
        }
        self.cursor.clone()
    }

    /// Uneltele care inserează o imagine: se plasează dintr-un singur clic.
    /// Dialogurile și captura blochează firul de interfață cât rulează.
    fn stamp_tool(&mut self, ctx: &egui::Context, at: Pos2) {
        match self.tool {
            Tool::ImageFile => {
                if let Some(p) = pick_image(&pictures_dir()) {
                    self.stamp_file(ctx, at, &p);
                }
            }
            Tool::Sticker => {
                let dir = stickers_dir();
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    self.status = format!("nu pot crea {}: {e}", dir.display());
                    return;
                }
                // dialogul pe un director gol n-ar avea ce arăta
                let empty = std::fs::read_dir(&dir)
                    .map(|it| !it.flatten().any(|e| e.path().is_file()))
                    .unwrap_or(true);
                if empty {
                    self.status = format!("pune fișiere PNG în {}", dir.display());
                    return;
                }
                if let Some(p) = pick_image(&dir) {
                    self.stamp_file(ctx, at, &p);
                }
            }
            Tool::Cursor => {
                // vârful săgeții e chiar pixelul din stânga-sus al imaginii,
                // deci colțul formei cade exact în punctul de clic
                if let Some(c) = self.cursor_img() {
                    self.stamp(ctx, at, c);
                }
            }
            Tool::ImageScreen => {
                // fereastra trebuie să apuce să dispară înainte de spectacle:
                // trimitem comanda acum, captura vine la cadrul următor
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                ctx.request_repaint();
                self.pending = Some(Pending::Screen { at, center: false });
                self.status = "selectează regiunea de pe ecran".into();
            }
            _ => {}
        }
    }

    /// Inserarea cerută din meniul „Imagine": aceleași unelte, doar că
    /// pornite fără clic, iar forma se așază în centrul imaginii.
    fn stamp_menu(&mut self, ctx: &egui::Context, t: Tool) {
        let prev = self.tool;
        self.tool = t;
        let at = Pos2::new(self.img.width() as f32 / 2.0, self.img.height() as f32 / 2.0);
        if t == Tool::ImageScreen {
            self.stamp_tool(ctx, at);
            // captura vine abia la cadrul următor; marcăm centrarea acolo
            if let Some(Pending::Screen { center, .. }) = self.pending.as_mut() {
                *center = true;
            }
        } else {
            let n = self.shapes.len();
            self.stamp_tool(ctx, at);
            if self.shapes.len() > n {
                self.center_last();
            }
        }
        // `stamp` trece pe unealta de selecție când reușește; altfel ne întoarcem
        if self.tool == t {
            self.tool = prev;
        }
    }

    /// Mută ultima formă inserată în centrul imaginii.
    fn center_last(&mut self) {
        let c = Pos2::new(self.img.width() as f32 / 2.0, self.img.height() as f32 / 2.0);
        if let Some(s) = self.shapes.last_mut() {
            let d = c - s.bounds().center();
            s.translate(d);
        }
    }

    /// Acțiunea amânată un cadru de `stamp_tool`.
    fn run_pending(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending.take() else { return };
        match p {
            Pending::Screen { at, center } => {
                let shot = crate::capture::select_region();
                // fereastra se întoarce orice ar fi ieșit din captură
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.request_repaint();
                match shot {
                    Ok(i) => {
                        self.stamp(ctx, at, Arc::new(i));
                        if center {
                            self.center_last();
                        }
                    }
                    Err(e) => self.status = format!("captură anulată: {e:#}"),
                }
            }
        }
    }

    // ------------------------------------------------- fereastra de text

    /// Deschide fereastra de introducere a textului pentru forma `i`.
    /// `fresh` = forma tocmai a fost creată (pasul de undo e deja pus).
    fn open_text(&mut self, i: usize, fresh: bool) {
        let Some(t) = self.shapes.get(i).and_then(Shape::text_of) else { return };
        let (w, h) = self.img.dimensions();
        let mut d = Dialog::new(DlgKind::Text, w, h);
        d.text = TextDlg {
            idx: i,
            fresh,
            outline: t.outline,
            buf: t.text.to_owned(),
            opts: t.opts.clone(),
            color2: t.color2,
            focus: true,
        };
        self.dialog = Some(d);
        self.sel = None;
    }

    /// Închiderea ferestrei. `ok` = s-a apăsat OK (sau Enter).
    fn text_done(&mut self, t: TextDlg, ok: bool) {
        // ShareX cheamă OnConfigSave după dialog, indiferent de buton:
        // opțiunile alese rămân implicite pentru textele următoare
        if t.outline {
            self.opt.outline = t.opts.clone();
            self.opt.outline_border = t.color2;
        } else {
            self.opt.text = t.opts.clone();
            self.opt.text_fill = t.color2;
        }
        let drop = !ok || t.buf.trim().is_empty();
        if drop {
            // renunțare pe o formă existentă: rămâne neatinsă
            if !ok && !t.fresh {
                return;
            }
            if t.fresh {
                // n-a existat niciodată cu conținut: scoatem și pasul de undo
                self.undo.pop();
            } else {
                self.push_undo(false);
            }
            if t.idx < self.shapes.len() {
                self.shapes.remove(t.idx);
            }
            self.sel = None;
            return;
        }
        if !t.fresh {
            self.push_undo(false);
        }
        if let Some(s) = self.shapes.get_mut(t.idx) {
            s.set_text(t.buf, t.opts, t.color2);
        }
    }

    // ------------------------------------------------------------- editare

    fn delete_sel(&mut self) {
        let Some(i) = self.sel.take() else {
            self.status = "nicio formă selectată".into();
            return;
        };
        if i < self.shapes.len() {
            self.push_undo(false);
            self.shapes.remove(i);
        }
    }

    fn delete_all(&mut self) {
        if self.shapes.is_empty() {
            return;
        }
        self.push_undo(false);
        self.shapes.clear();
        self.sel = None;
        self.step_next = self.opt.step_start;
    }

    fn duplicate(&mut self) {
        let Some(i) = self.sel else {
            self.status = "nicio formă selectată".into();
            return;
        };
        let Some(mut c) = self.shapes.get(i).cloned() else { return };
        self.push_undo(false);
        c.translate(Vec2::new(10.0, 10.0));
        self.shapes.push(c);
        self.sel = Some(self.shapes.len() - 1);
    }

    fn reorder(&mut self, a: Act) {
        let Some(i) = self.sel else {
            self.status = "nicio formă selectată".into();
            return;
        };
        if i >= self.shapes.len() {
            return;
        }
        self.push_undo(false);
        let s = self.shapes.remove(i);
        let j = match a {
            Act::Front => self.shapes.len(),
            Act::Forward => (i + 1).min(self.shapes.len()),
            Act::Backward => i.saturating_sub(1),
            _ => 0,
        };
        self.shapes.insert(j, s);
        self.sel = Some(j);
    }

    fn nudge(&mut self, d: Vec2) {
        let Some(i) = self.sel else { return };
        if let Some(s) = self.shapes.get_mut(i) {
            s.translate(d);
        }
    }

    fn apply_crop(&mut self, r: Rect) {
        let clip = Rect::from_min_max(
            Pos2::ZERO,
            Pos2::new(self.img.width() as f32, self.img.height() as f32),
        );
        let r = Rect::from_two_pos(r.min, r.max).intersect(clip);
        if r.width() < 2.0 || r.height() < 2.0 {
            return;
        }
        self.push_undo(true);
        self.img = image::imageops::crop_imm(
            &self.img,
            r.min.x as u32,
            r.min.y as u32,
            r.width() as u32,
            r.height() as u32,
        )
        .to_image();
        let d = -r.min.to_vec2();
        for s in self.shapes.iter_mut() {
            s.translate(d);
        }
        self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
        self.sel = None;
        self.status = format!("decupat la {}x{}", self.img.width(), self.img.height());
    }

    /// Taie o fâșie din imagine și lipește cele două jumătăți. Aceeași
    /// mecanică de undo ca decuparea: pasul salvează și imaginea.
    fn apply_cutout(&mut self, r: Rect) {
        let Some((img, horiz, end, band)) = cut_out(&self.img, r) else { return };
        self.push_undo(true);
        self.img = img;
        // formele de dincolo de fâșie se apropie cu grosimea ei
        let d = if horiz { Vec2::new(0.0, -band) } else { Vec2::new(-band, 0.0) };
        for s in self.shapes.iter_mut() {
            let b = s.bounds();
            if if horiz { b.min.y >= end } else { b.min.x >= end } {
                s.translate(d);
            }
        }
        self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
        self.sel = None;
        self.status = format!("tăiat la {}x{}", self.img.width(), self.img.height());
    }


    // ------------------------------------------------- operații pe imagine

    fn set_img(&mut self, img: RgbaImage) {
        self.img = img;
        self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
    }

    /// Imaginea cu formele desenate în ea. `None` dacă nu sunt forme (sau dacă
    /// randarea eșuează, caz în care rămâne un mesaj în bara de stare).
    fn flat_img(&mut self) -> Option<RgbaImage> {
        if self.shapes.is_empty() {
            return None;
        }
        match render::compose_opts(&self.img, &self.shapes, self.opt.shadow)
            .and_then(|png| Ok(image::load_from_memory(&png)?.to_rgba8()))
        {
            Ok(i) => Some(i),
            Err(e) => {
                self.status = format!("nu pot aplica formele: {e:#}");
                None
            }
        }
    }

    /// Desenează formele în imagine și golește lista. Rotirea unei casete de
    /// text ar cere text rotit, pe care nu-l desenăm; aplatizarea păstrează
    /// exact ce vede utilizatorul. Se cheamă DUPĂ `push_undo(true)`.
    fn flatten(&mut self) -> bool {
        let Some(flat) = self.flat_img() else { return false };
        self.set_img(flat);
        self.drop_shapes();
        true
    }

    fn drop_shapes(&mut self) {
        self.shapes.clear();
        self.sel = None;
        self.step_next = self.opt.step_start;
    }

    /// Mesajul de stare, cu mențiunea aplatizării dacă a avut loc.
    fn note(&mut self, msg: String, flat: bool) {
        self.status = if flat {
            format!("{msg} · formele au fost aplicate pe imagine")
        } else {
            msg
        };
    }

    fn img_new(&mut self, w: u32, h: u32, c: Color32) {
        self.push_undo(true);
        let flat = self.flatten();
        self.set_img(RgbaImage::from_pixel(w.max(1), h.max(1), rgba(c)));
        self.drop_shapes();
        self.note(format!("imagine nouă {}x{}", w.max(1), h.max(1)), flat);
    }

    fn img_open(&mut self) {
        let Some(p) = pick_image(&pictures_dir()) else { return };
        let img = match image::open(&p) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.status = format!("nu pot deschide {}: {e}", p.display());
                return;
            }
        };
        self.push_undo(true);
        let flat = self.flatten();
        let (w, h) = img.dimensions();
        self.set_img(img);
        self.drop_shapes();
        self.last_save = None;
        self.note(format!("deschis {} ({w}x{h})", p.display()), flat);
    }

    /// Redimensionare: singura operație care NU aplatizează — formele se
    /// scalează cu același factor și rămân editabile.
    fn img_resize(&mut self, w: u32, h: u32) {
        let (ow, oh) = self.img.dimensions();
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) == (ow, oh) {
            return;
        }
        self.push_undo(true);
        let out = resize_img(&self.img, w, h);
        self.set_img(out);
        let k = Vec2::new(w as f32 / ow as f32, h as f32 / oh as f32);
        for s in self.shapes.iter_mut() {
            s.scale(k);
        }
        self.note(format!("redimensionat la {w}x{h}"), false);
    }

    fn img_canvas(&mut self, w: u32, h: u32, c: Color32) {
        self.push_undo(true);
        let flat = self.flatten();
        let out = canvas_img(&self.img, w, h, c);
        let (w, h) = out.dimensions();
        self.set_img(out);
        self.note(format!("pânză {w}x{h}"), flat);
    }

    fn img_autocrop(&mut self) {
        // marginile se caută pe imaginea cu formele deja desenate: altfel o
        // formă lipită de margine ar rămâne pe dinafară după tăiere
        let flat = self.flat_img();
        let out = {
            let base = flat.as_ref().unwrap_or(&self.img);
            auto_crop_rect(base)
                .map(|(x, y, w, h)| image::imageops::crop_imm(base, x, y, w, h).to_image())
        };
        let Some(out) = out else {
            self.status = "nu există margini uniforme de tăiat".into();
            return;
        };
        self.push_undo(true);
        let flattened = flat.is_some();
        if flattened {
            self.drop_shapes();
        }
        let (w, h) = out.dimensions();
        self.set_img(out);
        self.note(format!("decupat automat la {w}x{h}"), flattened);
    }

    fn img_rotate(&mut self, right: bool) {
        self.push_undo(true);
        let flat = self.flatten();
        let out = if right {
            image::imageops::rotate90(&self.img)
        } else {
            image::imageops::rotate270(&self.img)
        };
        let (w, h) = out.dimensions();
        self.set_img(out);
        self.note(
            format!("rotit 90° {} la {w}x{h}", if right { "dreapta" } else { "stânga" }),
            flat,
        );
    }

    // ------------------------------------------------------------- dialog

    fn open_dialog(&mut self, kind: DlgKind) {
        let (w, h) = self.img.dimensions();
        self.dialog = Some(Dialog::new(kind, w, h));
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if self.dialog.as_ref().is_some_and(|d| d.kind == DlgKind::Text) {
            self.text_dialog_ui(ctx, acts);
            return;
        }
        let Some(d) = self.dialog.as_mut() else { return };
        let (title, ok_text) = match d.kind {
            DlgKind::New => ("Imagine nouă", "Creează"),
            DlgKind::Size => ("Dimensiune imagine", "OK"),
            DlgKind::Canvas => ("Dimensiune pânză", "OK"),
            DlgKind::Text => return,
        };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(240.0);
                egui::Grid::new("sxr-dlg").num_columns(2).show(ui, |ui| {
                    ui.label("Lățime");
                    ui.add(egui::DragValue::new(&mut d.w).range(1..=20000).suffix(" px"));
                    ui.end_row();
                    ui.label("Înălțime");
                    ui.add(egui::DragValue::new(&mut d.h).range(1..=20000).suffix(" px"));
                    ui.end_row();
                    match d.kind {
                        DlgKind::Size => {
                            ui.label("Proporții");
                            ui.checkbox(&mut d.keep, "Păstrează proporțiile");
                        }
                        DlgKind::New => {
                            ui.label("Fundal");
                            ui.color_edit_button_srgba(&mut d.color);
                        }
                        DlgKind::Canvas => {
                            ui.label("Umplere");
                            ui.color_edit_button_srgba(&mut d.color);
                        }
                        DlgKind::Text => {}
                    }
                    ui.end_row();
                });
                // latura schimbată o trage pe cealaltă după ea
                if d.kind == DlgKind::Size && d.keep {
                    if d.w != d.last_w {
                        d.h = ((d.w as f32 * d.ratio).round() as u32).max(1);
                    } else if d.h != d.last_h {
                        d.w = ((d.h as f32 / d.ratio.max(0.0001)).round() as u32).max(1);
                    }
                }
                d.last_w = d.w;
                d.last_h = d.h;
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.button(ok_text).clicked();
                    cancel = ui.button("Renunță").clicked();
                });
            });
        if ok {
            acts.push(Act::DlgOk);
        }
        if cancel {
            acts.push(Act::DlgCancel);
        }
    }


    /// Fereastra de introducere a textului, după `TextDrawingInputBox`:
    /// bara de unelte sus, zona de scris la mijloc, sfatul și butoanele jos.
    fn text_dialog_ui(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let Some(d) = self.dialog.as_mut() else { return };
        let t = &mut d.text;
        // familia aleasă se încarcă o dată și rămâne în egui pentru desen
        font::register(ctx, &t.opts);
        if let Some(n) = font::take_note() {
            self.status = n;
        }
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("sxr — introducere text")
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 320.0])
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                // ---- bara de unelte
                ui.horizontal_wrapped(|ui| {
                    ui.label("Font:");
                    egui::ComboBox::from_id_salt("sxr-font")
                        .width(190.0)
                        .height(360.0)
                        .selected_text(t.opts.family.clone())
                        .show_ui(ui, |ui| {
                            for f in font::families() {
                                let on = t.opts.family == f;
                                if ui.selectable_label(on, f).clicked() {
                                    t.opts.family = f.to_owned();
                                }
                            }
                        });
                    ui.label("Dimensiune:");
                    ui.add(egui::DragValue::new(&mut t.opts.size).range(6.0..=200.0).speed(0.5));
                    ui.color_edit_button_srgba(&mut t.opts.color)
                        .on_hover_text("Culoarea textului");
                    ui.color_edit_button_srgba(&mut t.color2).on_hover_text(if t.outline {
                        "Culoarea conturului"
                    } else {
                        "Culoarea fundalului"
                    });
                    ui.separator();
                    if ui.selectable_label(t.opts.bold, egui::RichText::new("B").strong()).clicked() {
                        t.opts.bold = !t.opts.bold;
                    }
                    if ui.selectable_label(t.opts.italic, egui::RichText::new("I").italics()).clicked() {
                        t.opts.italic = !t.opts.italic;
                    }
                    if ui.selectable_label(t.opts.underline, egui::RichText::new("U").underline()).clicked() {
                        t.opts.underline = !t.opts.underline;
                    }
                    ui.separator();
                    ui.menu_button(format!("⬌ {}", t.opts.halign.horiz_name()), |ui| {
                        for a in shape::Align::ALL {
                            if ui.button(a.horiz_name()).clicked() {
                                t.opts.halign = a;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Aliniere pe orizontală");
                    ui.menu_button(format!("⬍ {}", t.opts.valign.vert_name()), |ui| {
                        for a in shape::Align::ALL {
                            if ui.button(a.vert_name()).clicked() {
                                t.opts.valign = a;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Aliniere pe verticală");
                });
                ui.separator();
                // ---- zona de scris, cu fontul și culoarea alese
                let tip_h = ui.text_style_height(&egui::TextStyle::Body) + 28.0;
                let h = (ui.available_height() - tip_h).max(60.0);
                let bg = visible_bg(t.opts.color);
                {
                    let te = egui::TextEdit::multiline(&mut t.buf)
                        .font(font::opts_font_id(&t.opts, t.opts.size))
                        .text_color(t.opts.color)
                        .horizontal_align(match t.opts.halign {
                            shape::Align::Near => egui::Align::LEFT,
                            shape::Align::Center => egui::Align::Center,
                            shape::Align::Far => egui::Align::RIGHT,
                        })
                        .background_color(bg)
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY);
                    let r = ui.add_sized([ui.available_width(), h], te);
                    // prima deschidere: cursorul e direct în text
                    if t.focus {
                        t.focus = false;
                        r.request_focus();
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.small("Rând nou: Ctrl + Enter, OK: Enter");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel = ui.button("Renunță").clicked();
                        ok = ui.button("OK").clicked();
                    });
                });
            });
        if ok {
            acts.push(Act::DlgOk);
        }
        if cancel {
            acts.push(Act::DlgCancel);
        }
    }

    // ------------------------------------------------------------- ieșire

    fn png(&self) -> Result<Vec<u8>> {
        render::compose_opts(&self.img, &self.shapes, self.opt.shadow)
    }

    fn copy(&mut self) {
        match self.png().and_then(clip::copy_png) {
            Ok(()) => self.status = "✓ copiat în clipboard".into(),
            Err(e) => self.status = format!("✗ copiere eșuată: {e}"),
        }
    }

    fn save(&mut self, as_new: bool) {
        let path = match (&self.last_save, as_new) {
            (Some(p), false) => p.clone(),
            _ => {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                pictures_dir().join(format!("sxr-{secs}.png"))
            }
        };
        match self
            .png()
            .and_then(|png| std::fs::write(&path, png).map_err(Into::into))
        {
            Ok(()) => {
                self.status = format!("✓ salvat: {}", path.display());
                self.last_save = Some(path);
            }
            Err(e) => self.status = format!("✗ salvare eșuată: {e}"),
        }
    }

    // ------------------------------------------------------------- culori

    /// ShareX scrie culoarea de contur în câmpul uneltei curente, nu într-unul singur.
    fn border_mut(&mut self) -> &mut Color32 {
        match self.tool {
            Tool::TextBackground | Tool::SpeechBalloon => &mut self.opt.text_border,
            Tool::TextOutline => &mut self.opt.outline_border,
            Tool::Step => &mut self.opt.step_border,
            _ => &mut self.opt.border,
        }
    }

    fn fill_mut(&mut self) -> &mut Color32 {
        match self.tool {
            Tool::TextBackground | Tool::SpeechBalloon => &mut self.opt.text_fill,
            Tool::Step => &mut self.opt.step_fill,
            _ => &mut self.opt.fill,
        }
    }

    // ------------------------------------------------------------ tastatură

    fn keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::Key as K;
        // dialogul e modal: cât e deschis, nicio scurtătură a editorului nu
        // se declanșează, doar Enter (OK) și Esc (Renunță)
        if let Some(d) = &self.dialog {
            if d.kind == DlgKind::Text {
                self.text_keys(ctx, acts);
                return;
            }
            let typing = ctx.memory(|m| m.focused().is_some());
            ctx.input(|i| {
                if i.key_pressed(K::Escape) {
                    acts.push(Act::DlgCancel);
                }
                // dacă tocmai se scrie într-un câmp, Enter e al câmpului
                if !typing && i.key_pressed(K::Enter) {
                    acts.push(Act::DlgOk);
                }
            });
            return;
        }
        let mut tool: Option<Tool> = None;
        let mut nudge = Vec2::ZERO;

        ctx.input(|i| {
            let m = i.modifiers;
            if i.key_pressed(K::Escape) {
                acts.push(Act::Close);
            }
            if i.key_pressed(K::Enter) {
                acts.push(Act::Apply);
            }
            if m.ctrl {
                if i.key_pressed(K::S) {
                    acts.push(if m.shift { Act::SaveAs } else { Act::Save });
                }
                if i.key_pressed(K::C) {
                    acts.push(Act::Copy);
                }
                if i.key_pressed(K::U) {
                    acts.push(Act::Upload);
                }
                if i.key_pressed(K::P) {
                    acts.push(Act::Print);
                }
                if i.key_pressed(K::D) {
                    acts.push(Act::Dup);
                }
                if !m.shift && i.key_pressed(K::Z) {
                    acts.push(Act::Undo);
                }
                if (m.shift && i.key_pressed(K::Z)) || i.key_pressed(K::Y) {
                    acts.push(Act::Redo);
                }
            }
            if i.key_pressed(K::Delete) {
                acts.push(if m.shift { Act::DelAll } else { Act::Del });
            }
            if i.key_pressed(K::Home) {
                acts.push(Act::Front);
            }
            if i.key_pressed(K::End) {
                acts.push(Act::Back);
            }
            if i.key_pressed(K::PageUp) {
                acts.push(Act::Forward);
            }
            if i.key_pressed(K::PageDown) {
                acts.push(Act::Backward);
            }

            if m.ctrl || m.alt {
                return;
            }
            for t in Tool::ALL {
                if t.key().is_some_and(|k| i.key_pressed(k)) {
                    tool = Some(t);
                }
            }
            for (k, t) in [
                (K::Num1, Tool::Rect),
                (K::Num2, Tool::Ellipse),
                (K::Num3, Tool::Freehand),
                (K::Num4, Tool::Line),
                (K::Num5, Tool::Arrow),
                (K::Num6, Tool::TextOutline),
                (K::Num7, Tool::Step),
                (K::Num8, Tool::Blur),
                (K::Num9, Tool::Pixelate),
            ] {
                if i.key_pressed(k) {
                    tool = Some(t);
                }
            }
            let d = if m.shift { 10.0 } else { 1.0 };
            if i.key_pressed(K::ArrowLeft) {
                nudge.x -= d;
            }
            if i.key_pressed(K::ArrowRight) {
                nudge.x += d;
            }
            if i.key_pressed(K::ArrowUp) {
                nudge.y -= d;
            }
            if i.key_pressed(K::ArrowDown) {
                nudge.y += d;
            }
        });

        if let Some(t) = tool {
            self.pick(t);
        }
        if nudge != Vec2::ZERO {
            self.nudge(nudge);
        }
    }

    /// Tastatura ferestrei de text, exact ca în `TextDrawingInputBox`:
    /// Enter = OK, Ctrl+Enter = rând nou, Esc = renunță. Rulează ÎNAINTEA
    /// construirii interfeței, deci scoate evenimentele din coadă înainte
    /// să ajungă la `TextEdit`.
    fn text_keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::{Event, Key as K};
        let (mut ok, mut cancel) = (false, false);
        ctx.input_mut(|i| {
            i.events.retain_mut(|e| match e {
                Event::Key { key: K::Enter, pressed: true, modifiers, .. } => {
                    if modifiers.ctrl || modifiers.command {
                        // Ctrl+Enter: îl trecem drept Enter simplu, ca TextEdit
                        // să insereze rândul nou chiar la poziția cursorului
                        *modifiers = egui::Modifiers::NONE;
                        true
                    } else {
                        ok = true;
                        false
                    }
                }
                Event::Key { key: K::Escape, pressed: true, .. } => {
                    cancel = true;
                    false
                }
                _ => true,
            });
        });
        if ok {
            acts.push(Act::DlgOk);
        }
        if cancel {
            acts.push(Act::DlgCancel);
        }
    }

    fn pick(&mut self, t: Tool) {
        // schimbarea uneltei renunță la forma activă; cât stai pe aceeași
        // unealtă, forma abia trasă rămâne selectată, cu nodurile la vedere
        if t != self.tool {
            self.sel = None;
        }
        self.tool = t;
        if !t.ready() {
            self.status = format!("unealta {} nu e implementată încă", t.tooltip());
        }
    }

    fn run_acts(&mut self, ctx: &egui::Context, acts: &[Act]) {
        for a in acts {
            match a {
                Act::Apply => {
                                self.copy();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Act::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Act::Copy => self.copy(),
                Act::Save => self.save(false),
                Act::SaveAs => self.save(true),
                Act::Upload => self.status = "încărcarea nu e implementată".into(),
                // Butonul rămâne pentru fidelitate față de bara din ShareX,
                // dar tipărirea e în afara scopului lui sxr.
                Act::Print => self.status = "tipărirea nu e inclusă în sxr".into(),
                Act::Undo => self.do_undo(),
                Act::Redo => self.do_redo(),
                Act::Dup => self.duplicate(),
                Act::Del => self.delete_sel(),
                Act::DelAll => self.delete_all(),
                Act::Front | Act::Forward | Act::Backward | Act::Back => self.reorder(*a),
                Act::CropTool => {
                    self.pick(Tool::Crop);
                    self.status = "trage un dreptunghi ca să decupezi".into();
                }
                Act::NewImg => self.open_dialog(DlgKind::New),
                Act::OpenImg => self.img_open(),
                Act::InsertFile => self.stamp_menu(ctx, Tool::ImageFile),
                Act::InsertScreen => self.stamp_menu(ctx, Tool::ImageScreen),
                Act::ImgSize => self.open_dialog(DlgKind::Size),
                Act::CanvasSize => self.open_dialog(DlgKind::Canvas),
                Act::AutoCrop => self.img_autocrop(),
                Act::RotRight => self.img_rotate(true),
                Act::RotLeft => self.img_rotate(false),
                Act::DlgOk => {
                    if let Some(d) = self.dialog.take() {
                        match d.kind {
                            DlgKind::New => self.img_new(d.w, d.h, d.color),
                            DlgKind::Size => self.img_resize(d.w, d.h),
                            DlgKind::Canvas => self.img_canvas(d.w, d.h, d.color),
                            DlgKind::Text => self.text_done(d.text, true),
                        }
                    }
                }
                Act::DlgCancel => {
                    if let Some(d) = self.dialog.take() {
                        if d.kind == DlgKind::Text {
                            self.text_done(d.text, false);
                        }
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------- bară

    fn toolbar(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            // fără bara de derulare: pe fereastră îngustă apărea ca o dungă albă
            // chiar sub unelte, de fiecare dată când treceai cu mausul pe acolo.
            // Bara se poate în continuare derula cu rotița mausului.
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding = Vec2::new(5.0, 4.0);

                    // Bara stă centrată pe lățimea ferestrei, ca în ShareX.
                    let pad = ((ui.available_width() - self.bar_w) * 0.5).max(0.0);
                    if pad > 0.5 {
                        ui.add_space(pad);
                    }
                    let x0 = ui.cursor().min.x;

                    for (name, tip, act) in [
                        ("tick", "Aplică și închide (Enter)", Act::Apply),
                        ("disk-black", "Salvează (Ctrl+S)", Act::Save),
                        ("disks-black", "Salvează ca... (Ctrl+Shift+S)", Act::SaveAs),
                        ("clipboard", "Copiază în clipboard (Ctrl+C)", Act::Copy),
                        ("drive-globe", "Încarcă (Ctrl+U)", Act::Upload),
                        ("printer", "Tipărește (Ctrl+P)", Act::Print),
                    ] {
                        if ui
                            .add(egui::Button::image(icons.img(name)))
                            .on_hover_text(tip)
                            .clicked()
                        {
                            acts.push(act);
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    for t in Tool::ALL {
                        if ui
                            .add(egui::Button::image(icons.img(t.icon())).selected(self.tool == t))
                            .on_hover_text(t.tooltip())
                            .clicked()
                        {
                            self.pick(t);
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // conturul are gaură în mijloc, umplerea și evidențierea nu —
                    // exact cele trei iconițe din ShareX
                    color_button(ui, "contur", self.border_mut(), 8.0, "Culoare contur");
                    color_button(ui, "umplere", self.fill_mut(), 0.0, "Culoare umplere");
                    color_button(
                        ui,
                        "evidentiere",
                        &mut self.opt.highlight,
                        0.0,
                        "Culoare evidențiere",
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    self.menu_opts(ui, icons);
                    self.menu_edit(ui, icons, acts);
                    self.menu_image(ui, icons, acts);

                    self.bar_w = ui.cursor().min.x - x0;
                });
            });
        // bara de stare apare doar când are ceva de spus: un rând gol
        // ar lăsa altfel un spațiu inutil sub imagine
        if !self.status.is_empty() {
            ui.vertical_centered(|ui| ui.small(self.status.clone()));
        }
    }

    fn menu_opts(&mut self, ui: &mut egui::Ui, icons: &Icons) {
        let o = &mut self.opt;
        let mut start = o.step_start;
        ui.menu_image_button(icons.img("layer--pencil"), |ui| {
            ui.set_min_width(240.0);
            ui.add(egui::Slider::new(&mut o.border_size, 1.0..=32.0).text("Grosime contur"));
            ui.add(egui::Slider::new(&mut o.corner_radius, 0.0..=32.0).text("Rază colț"));
            ui.add(egui::Slider::new(&mut o.line_mid, 0..=shape::MAX_MID).text("Puncte de curbură linie"));
            ui.add(egui::Slider::new(&mut o.pixelate, 2.0..=64.0).text("Dimensiune pixelare"));
            ui.add(egui::Slider::new(&mut o.blur, 1.0..=100.0).text("Rază blur"));
            ui.add(egui::Slider::new(&mut o.magnify, 110.0..=800.0).text("Putere lupă"));
            ui.add(egui::Slider::new(&mut o.text.size, 8.0..=72.0).text("Dimensiune font text"));
            ui.add(egui::Slider::new(&mut o.step_font, 8.0..=72.0).text("Dimensiune font numărător"));
            ui.add(egui::Slider::new(&mut start, 1..=100).text("Valoare de pornire numărător"));
            ui.checkbox(&mut o.shadow, "Umbră");
        })
        .response
        .on_hover_text("Opțiuni unealtă");
        if start != self.opt.step_start {
            self.opt.step_start = start;
            self.step_next = start;
        }
    }

    fn menu_edit(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        ui.menu_image_button(icons.img("wrench-screwdriver"), |ui| {
            let mut item = |ui: &mut egui::Ui, icon: &str, text: &str, a: Act| {
                if ui
                    .add(egui::Button::image_and_text(icons.img(icon), text))
                    .clicked()
                {
                    acts.push(a);
                    ui.close();
                }
            };
            item(ui, "arrow-circle-225-left", "Anulează (Ctrl+Z)", Act::Undo);
            item(ui, "arrow-circle-315", "Refă (Ctrl+Y)", Act::Redo);
            item(ui, "document-copy", "Duplică (Ctrl+D)", Act::Dup);
            ui.separator();
            item(ui, "layer--minus", "Șterge (Delete)", Act::Del);
            item(ui, "eraser", "Șterge tot (Shift+Delete)", Act::DelAll);
            ui.separator();
            item(ui, "layers-stack-arrange", "Adu în față (Home)", Act::Front);
            item(ui, "layers-arrange", "Adu mai în față (PageUp)", Act::Forward);
            item(ui, "layers-arrange-back", "Trimite mai în spate (PageDown)", Act::Backward);
            item(ui, "layers-stack-arrange-back", "Trimite în spate (End)", Act::Back);
        })
        .response
        .on_hover_text("Editare");
    }

    fn menu_image(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        ui.menu_image_button(icons.img("image--pencil"), |ui| {
            ui.set_min_width(230.0);
            let mut item = |ui: &mut egui::Ui, icon: &str, text: &str, a: Act| {
                if ui
                    .add(egui::Button::image_and_text(icons.img(icon), text))
                    .clicked()
                {
                    acts.push(a);
                    ui.close();
                }
            };
            item(ui, "image-empty", "Imagine nouă", Act::NewImg);
            item(ui, "folder-open-image", "Deschide fișier imagine", Act::OpenImg);
            item(ui, "image--plus", "Inserează fișier imagine", Act::InsertFile);
            item(ui, "camera", "Inserează imagine din ecran", Act::InsertScreen);
            ui.separator();
            item(ui, "image-select", "Dimensiune imagine", Act::ImgSize);
            item(ui, "image-resize", "Dimensiune pânză", Act::CanvasSize);
            item(ui, "image-crop", "Decupează imaginea", Act::CropTool);
            item(ui, "image-resize-actual", "Decupare automată", Act::AutoCrop);
            ui.separator();
            item(ui, "arrow-circle", "Rotește 90° dreapta", Act::RotRight);
            item(ui, "arrow-circle-135-left", "Rotește 90° stânga", Act::RotLeft);
        })
        .response
        .on_hover_text("Imagine");
    }

    // ---------------------------------------------------------------- pânză

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (iw, ih) = (self.img.width() as f32, self.img.height() as f32);
        let avail = ui.available_size();
        let zoom = (avail.x / iw).min(avail.y / ih).min(1.0).max(0.05);
        let size = Vec2::new(iw * zoom, ih * zoom);
        // Alocăm tot spațiul rămas și centrăm imaginea în el: la redimensionarea
        // ferestrei rămâne în mijloc, nu lipită de colțul stânga-sus.
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let img_rect = Rect::from_center_size(resp.rect.center(), size);
        let origin = img_rect.min;
        let to_screen = move |p: Pos2| origin + p.to_vec2() * zoom;
        // Cursorul e limitat la imagine, ca formele să nu ajungă în afara ei.
        let to_img = move |p: Pos2| ((img_rect.clamp(p) - origin) / zoom).to_pos2();
        let tol = 6.0 / zoom;
        // nodul se prinde pe toată caseta lui, ca în ShareX
        let htol = NODE_HIT / 2.0 / zoom;

        // Forma de sub cursor: ea primește chenarul animat, ca `CurrentHoverShape`.
        let hover = resp.hover_pos().and_then(|sp| {
            let p = to_img(sp);
            self.shapes
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| s.hit(p, tol))
                .map(|(i, _)| i)
        });
        // Peste un nod, cursorul devine mânuță deschisă; cât tragi de el, închisă
        // (`SetHandCursor` din ShareX).
        let on_node = resp
            .hover_pos()
            .is_some_and(|sp| self.handle_at(to_img(sp), htol).is_some());
        if self.drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if on_node {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        painter.image(
            self.tex.id(),
            img_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        if self.dialog.is_some() {
            // dialogul modal ține pânza inactivă
        } else if self.tool == Tool::Select {
            self.select_input(&resp, to_img, tol, htol);
        } else {
            if resp.clicked() {
                // contorul, casetele de text și imaginile inserate se plasează
                // dintr-un simplu clic, nu prin tragere
                if let Some(p) = resp.interact_pointer_pos() {
                    let at = to_img(p);
                    if matches!(self.tool, Tool::Step) || self.tool.is_text() {
                        self.draft = self.new_draft(at);
                        self.commit_draft();
                    } else if matches!(
                        self.tool,
                        Tool::ImageFile | Tool::ImageScreen | Tool::Sticker | Tool::Cursor
                    ) {
                        let ctx = ui.ctx().clone();
                        self.stamp_tool(&ctx, at);
                    }
                }
            }
            if resp.drag_started() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let at = to_img(p);
                    // nodurile formei active se pot trage și fără a schimba
                    // unealta, ca în ShareX; abia dacă nu e niciun nod sub
                    // cursor începe o formă nouă
                    if let Some(idx) = self.handle_at(at, htol) {
                        self.push_undo(false);
                        self.drag = Some(Drag::Handle { idx });
                    } else {
                        self.draft = self.new_draft(at);
                    }
                }
            }
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let at = to_img(p);
                    if let Some(Drag::Handle { idx }) = self.drag {
                        if let Some(s) = self.sel.and_then(|i| self.shapes.get_mut(i)) {
                            s.move_handle(idx, at);
                        }
                    } else {
                        self.update_draft(at);
                    }
                }
            }
            if resp.drag_stopped() {
                if self.drag.take().is_none() {
                    self.commit_draft();
                }
            }
        }

        // reflectorul: un singur strat întunecat cu găuri pentru toate
        // dreptunghiurile, la poziția primului reflector din listă (schița
        // în curs de tragere intră și ea în reuniune)
        let mut spots = shape::spotlight_rects(&self.shapes);
        if let Some(Shape::Spotlight { rect }) = &self.draft {
            spots.push(Rect::from_two_pos(rect.min, rect.max));
        }
        let spot_at = shape::spotlight_at(&self.shapes).unwrap_or(self.shapes.len());
        let dim = |at: usize| {
            if at != spot_at || spots.is_empty() {
                return;
            }
            for r in shape::spotlight_cover(&spots, iw, ih) {
                painter.rect_filled(
                    Rect::from_two_pos(to_screen(r.min), to_screen(r.max)),
                    egui::CornerRadius::ZERO,
                    shape::SPOTLIGHT_DIM,
                );
            }
        };

        for (i, s) in self.shapes.iter().enumerate() {
            dim(i);
            if self.opt.shadow {
                if let Some(sh) = s.shadow_copy() {
                    sh.draw(&painter, to_screen, zoom, &self.img);
                }
            }
            s.draw(&painter, to_screen, zoom, &self.img);
        }
        dim(self.shapes.len());
        if let Some(d) = &self.draft {
            if self.opt.shadow && !matches!(self.tool, Tool::Crop | Tool::CutOut) {
                if let Some(sh) = d.shadow_copy() {
                    sh.draw(&painter, to_screen, zoom, &self.img);
                }
            }
            d.draw(&painter, to_screen, zoom, &self.img);
            if matches!(self.tool, Tool::Crop | Tool::CutOut) {
                let b = d.bounds();
                painter.rect_stroke(
                    Rect::from_two_pos(to_screen(b.min), to_screen(b.max)),
                    egui::CornerRadius::ZERO,
                    egui::Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
            }
        }

        // Chenarul „furnicilor" din ShareX: o linie neagră continuă, peste ea
        // liniuțe albe de 5 pe 5, cu decalajul mișcat cu 15 px/s
        // (`borderDotPen.DashOffset = elapsed * -15`). Se vede DOAR cât ții
        // mausul pe formă, nu tot timpul.
        if let Some(s) = hover.and_then(|i| self.shapes.get(i)) {
            let b = s.bounds();
            let r = Rect::from_two_pos(to_screen(b.min), to_screen(b.max)).expand(1.0);
            let pts = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            // decalajul se ține într-o singură perioadă (liniuță + spațiu = 10):
            // dat brut, ar porni desenarea cu mult înaintea colțului și ar lăsa
            // o dungă albă tot mai lungă spre stânga
            let t = (ui.ctx().input(|i| i.time) as f32 * -15.0).rem_euclid(10.0);
            painter.add(egui::Shape::line(pts.to_vec(), egui::Stroke::new(1.0, Color32::BLACK)));
            painter.extend(egui::Shape::dashed_line_with_offset(
                &pts,
                egui::Stroke::new(1.0, Color32::WHITE),
                &[5.0],
                &[5.0],
                t,
            ));
            // liniuțele se mișcă, deci cerem cadre cât timp chenarul e vizibil
            ui.ctx().request_repaint();
        }

        if let Some(i) = self.sel {
            if let Some(s) = self.shapes.get(i) {
                // Toate formele au aceleași noduri: bulinele albe pline din
                // `CircleNode.png`, nu pătrate — ShareX lipește aceeași imagine
                // în toate colțurile, indiferent de unealtă.
                for hp in s.handles() {
                    painter.circle_filled(to_screen(hp), NODE / 2.0, Color32::WHITE);
                }
            }
        }
    }

    /// Indicele nodului formei selectate aflat sub punctul dat, dacă există.
    fn handle_at(&self, p: Pos2, tol: f32) -> Option<usize> {
        let s = self.shapes.get(self.sel?)?;
        // ShareX verifică `Rectangle.Contains`, adică toată caseta pătrată a
        // nodului, nu un cerc — pe diagonală iartă cu vreo 40% mai mult
        s.handles()
            .iter()
            .position(|hp| Rect::from_center_size(*hp, Vec2::splat(tol * 2.0)).contains(p))
    }

    fn select_input(
        &mut self,
        resp: &egui::Response,
        to_img: impl Fn(Pos2) -> Pos2,
        tol: f32,
        htol: f32,
    ) {
        // dublu-clic pe o casetă de text o redeschide în editare
        if resp.double_clicked() {
            if let Some(sp) = resp.interact_pointer_pos() {
                let p = to_img(sp);
                let hit = self
                    .shapes
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, s)| s.has_text() && s.hit(p, tol))
                    .map(|(i, _)| i);
                if let Some(i) = hit {
                    self.open_text(i, false);
                    return;
                }
            }
        }
        if resp.drag_started() {
            if let Some(sp) = resp.interact_pointer_pos() {
                let p = to_img(sp);
                let mut started = None;
                // întâi handle-urile formei deja selectate, ca redimensionarea
                // să aibă prioritate față de selectarea altei forme dedesubt
                if let Some(i) = self.sel {
                    if let Some(s) = self.shapes.get(i) {
                        for (hi, hp) in s.handles().iter().enumerate() {
                            if Rect::from_center_size(*hp, Vec2::splat(htol * 2.0)).contains(p) {
                                started = Some(Drag::Handle { idx: hi });
                                break;
                            }
                        }
                    }
                }
                if started.is_none() {
                    let hit = self
                        .shapes
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, s)| s.hit(p, tol))
                        .map(|(i, _)| i);
                    self.sel = hit;
                    if hit.is_some() {
                        started = Some(Drag::Move { last: p });
                    }
                }
                if started.is_some() {
                    self.push_undo(false);
                }
                self.drag = started;
            }
        }
        if resp.dragged() {
            if let Some(sp) = resp.interact_pointer_pos() {
                let p = to_img(sp);
                if let (Some(i), Some(d)) = (self.sel, self.drag.as_mut()) {
                    if let Some(s) = self.shapes.get_mut(i) {
                        match d {
                            Drag::Move { last } => {
                                s.translate(p - *last);
                                *last = p;
                            }
                            Drag::Handle { idx } => s.move_handle(*idx, p),
                        }
                    }
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        // un clic simplu pe gol deselectează
        if resp.clicked() {
            if let Some(sp) = resp.interact_pointer_pos() {
                let p = to_img(sp);
                self.sel = self
                    .shapes
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, s)| s.hit(p, tol))
                    .map(|(i, _)| i);
            }
        }
    }
}

fn rgba(c: Color32) -> image::Rgba<u8> {
    image::Rgba([c.r(), c.g(), c.b(), c.a()])
}

/// Rescalare cu Lanczos3, ca în ShareX.
pub fn resize_img(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    image::imageops::resize(img, w.max(1), h.max(1), image::imageops::FilterType::Lanczos3)
}

/// Pânză nouă de `w`x`h`, umplută cu `fill`, cu imaginea veche așezată
/// centrat. Dacă pânza e mai mică, imaginea se taie simetric.
pub fn canvas_img(img: &RgbaImage, w: u32, h: u32, fill: Color32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let mut out = RgbaImage::from_pixel(w, h, rgba(fill));
    let dx = (w as i64 - img.width() as i64) / 2;
    let dy = (h as i64 - img.height() as i64) / 2;
    for y in 0..img.height() as i64 {
        let ty = y + dy;
        if ty < 0 || ty >= h as i64 {
            continue;
        }
        for x in 0..img.width() as i64 {
            let tx = x + dx;
            if tx < 0 || tx >= w as i64 {
                continue;
            }
            out.put_pixel(tx as u32, ty as u32, *img.get_pixel(x as u32, y as u32));
        }
    }
    out
}

/// Dreptunghiul rămas după tăierea marginilor uniforme, luând ca reper pixelul
/// din colțul stânga-sus și o toleranță de 10 pe canal. `None` dacă nu e nimic
/// de tăiat (sau dacă toată imaginea e uniformă).
pub fn auto_crop_rect(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    const TOL: i32 = 10;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let base = img.get_pixel(0, 0).0;
    let same = |p: &image::Rgba<u8>| {
        (0..4).all(|k| (p.0[k] as i32 - base[k] as i32).abs() <= TOL)
    };
    let row = |y: u32| (0..w).all(|x| same(img.get_pixel(x, y)));
    let col = |x: u32| (0..h).all(|y| same(img.get_pixel(x, y)));

    let top = (0..h).find(|&y| !row(y))?;
    let bottom = (0..h).rev().find(|&y| !row(y))?;
    let left = (0..w).find(|&x| !col(x))?;
    let right = (0..w).rev().find(|&x| !col(x))?;
    if top == 0 && left == 0 && bottom == h - 1 && right == w - 1 {
        return None;
    }
    Some((left, top, right - left + 1, bottom - top + 1))
}

/// Scoate o fâșie din imagine și lipește cele două jumătăți rămase. Fâșia e
/// orizontală (scade înălțimea) dacă dreptunghiul e mai lat decât înalt,
/// altfel verticală. Întoarce imaginea nouă, orientarea, unde se termina fâșia
/// în coordonatele vechi și grosimea ei; `None` dacă fâșia e prea subțire sau
/// ar înghiți toată imaginea.
pub fn cut_out(img: &RgbaImage, r: Rect) -> Option<(RgbaImage, bool, f32, f32)> {
    let (w, h) = img.dimensions();
    let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(w as f32, h as f32));
    let r = Rect::from_two_pos(r.min, r.max).intersect(clip);
    if r.width() < 2.0 || r.height() < 2.0 {
        return None;
    }
    let horiz = r.width() >= r.height();
    let (a, b, total) = if horiz {
        (r.min.y.round() as u32, r.max.y.round() as u32, h)
    } else {
        (r.min.x.round() as u32, r.max.x.round() as u32, w)
    };
    let b = b.min(total);
    let band = b.saturating_sub(a);
    if band == 0 || band >= total {
        return None;
    }
    let mut out = if horiz {
        RgbaImage::new(w, h - band)
    } else {
        RgbaImage::new(w - band, h)
    };
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = if horiz {
                if y >= a && y < b {
                    continue;
                }
                (x, if y >= b { y - band } else { y })
            } else {
                if x >= a && x < b {
                    continue;
                }
                (if x >= b { x - band } else { x }, y)
            };
            out.put_pixel(dx, dy, *img.get_pixel(x, y));
        }
    }
    Some((out, horiz, b as f32, band as f32))
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let icons = self.icons.clone();
        let mut acts: Vec<Act> = Vec::new();
        // fonturile trimise la cadrul trecut sunt acum instalate în egui
        font::sync();

        // captura cerută la cadrul anterior: acum minimizarea a ajuns deja
        // la compozitor, deci fereastra noastră nu mai intră în poză
        self.run_pending(&ctx);

        self.keys(&ctx, &mut acts);

        egui::Panel::top("bara").show(ui, |ui| {
            self.toolbar(ui, &icons, &mut acts);
        });

        self.run_acts(&ctx, &acts);
        acts.clear();

        egui::CentralPanel::default().show(ui, |ui| {
            self.canvas(ui);
        });

        self.dialog_ui(&ctx, &mut acts);

        self.run_acts(&ctx, &acts);
    }
}
