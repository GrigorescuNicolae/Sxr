//! ShareX's main window, reproduced as an inert shell.
//!
//! Every geometry number below comes from `ShareX/Forms/MainForm.resx` on the
//! `master` branch, where the designer's 264 `ApplyResources` calls leave the
//! sizes and the labels. The controls are drawn, not wired: the window is a
//! reference for the layout, so a control that would start an upload here does
//! nothing and says so by rendering greyed out.
//!
//! The labels are ShareX's own English control names, kept as literals rather
//! than going through `i18n`: they name controls that do nothing yet, and
//! `--i18n-check` asks every language to carry every key. They move into
//! `i18n` on the day a control becomes live.

use anyhow::Result;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{HOVER_BG, SEL_BG, SEL_BORDER, shot_ctx};
use crate::font;
use crate::icons::Icons;

// ------------------------------------------------------------------ palette
//
// `ShareXTheme.DarkTheme` in `ShareX.HelpersLib/ShareXTheme.cs`. Three of its
// entries are already in `app`, because the region editor's toolbar uses them,
// and the two windows have to look like one application.

/// `BackgroundColor` — the form and the toolbar.
const BG: Color32 = Color32::from_rgb(0x27, 0x27, 0x27);
/// `DarkBackgroundColor` — the sunken surfaces, list body and preview.
const DARK_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
/// `TextColor`.
const TEXT: Color32 = Color32::from_rgb(0xE7, 0xE9, 0xEA);
/// `BorderColor` — also `SeparatorDarkColor`.
const BORDER: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
/// `SeparatorLightColor` — the highlight under a separator line.
const SEP_LIGHT: Color32 = Color32::from_rgb(0x2C, 0x2C, 0x2C);

// ----------------------------------------------------------------- geometry

/// `$this.ClientSize`, plus the `BAR_EXTRA` the toolbar needs. Everything to
/// the right of the toolbar keeps ShareX's own width, so the form grows by
/// exactly what the toolbar took.
pub const FORM_W: f32 = 879.0 + BAR_EXTRA;
pub const FORM_H: f32 = 531.0;
/// `$this.MinimumSize`.
const MIN_W: f32 = 650.0 + BAR_EXTRA;
const MIN_H: f32 = 500.0;
/// How much wider than ShareX our toolbar has to be. Segoe UI 9pt fits
/// "Custom uploader settings..." inside a 171-point item; DejaVu, the face we
/// ship, is wider and needs 22 points more for the same label at the same size.
/// The alternative was to shrink the text, which reads as the wrong font size
/// long before anyone notices twenty-two points of panel.
const BAR_EXTRA: f32 = 22.0;
/// `pToolbars.Size` — the panel that holds `tsMain`, docked left.
const BAR_W: f32 = 188.0 + BAR_EXTRA;
/// `tsMain.Padding` = 8, 6, 8, 3.
const BAR_PAD_X: f32 = 8.0;
const BAR_PAD_TOP: f32 = 6.0;
/// `<toolbar item>.Size` — every item on `tsMain` is 171 x 20, widened by the
/// same `BAR_EXTRA` as the panel around it.
const ITEM_W: f32 = 171.0 + BAR_EXTRA;
const ITEM_H: f32 = 20.0;
/// A vertical `ToolStripSeparator` takes six points.
const SEP_H: f32 = 6.0;
/// `<menu item>.Size` — every `ToolStripMenuItem` is 22 high.
const MENU_H: f32 = 22.0;
/// Where a menu item's icon sits and where its text starts, inside the item.
const MENU_ICON_X: f32 = 4.0;
const MENU_TEXT_X: f32 = 28.0;
/// `scMain.SplitterDistance` and `scMain.SplitterWidth`. `FixedPanel` is
/// `Panel1`, so the task list keeps its width when the form is resized.
const SPLIT: f32 = 335.0;
const SPLIT_W: f32 = 6.0;
/// `AutoScaleDimensions` is 6, 13 — Segoe UI 9pt, which is 12 points at 96 dpi.
/// The dropdowns all fit that size inside the widths the resx gives them; only
/// the toolbar had to grow, which is what `BAR_EXTRA` is for.
const FONT: f32 = 12.0;
/// A `ListView` header row at 96 dpi.
const HEAD_H: f32 = 22.0;
/// How far a disabled icon is faded, next to `gray_out` on the label.
const ICON_A: f32 = 0.55;

// --------------------------------------------------------------- the strips

/// An icon: either one of the Fugue Icons files the editor already carries, or
/// a glyph drawn in code for a ShareX icon we have no counterpart for. The
/// drawn ones are approximations — the shape a reader recognises, not a copy.
#[derive(Clone, Copy)]
enum G {
    /// A file under `assets/icons`, by stem.
    File(&'static str),
    None,
    Up,
    Layers,
    Toolbox,
    ImgOut,
    CloudUp,
    Cloud,
    Keyboard,
    GlobePen,
    Doc,
    Picture,
    Cone,
    Heart,
    XLogo,
    Discord,
    Crown,
    Screen,
    Window,
    DashRect,
    Film,
    Scroll,
    Clock,
    Folder,
    Note,
    Tray,
    Link,
    Palette,
    Pipette,
    Ruler,
    Pin,
    Sunset,
    Wand,
    Robot,
    LetterA,
    Hash,
    Split,
}

/// A row of a dropdown. An empty label is a `ToolStripSeparator`; `arrow` is
/// set when the real item carries a submenu, whether or not we know its rows.
struct Menu {
    label: &'static str,
    icon: G,
    arrow: bool,
    sub: &'static [Menu],
    /// `<item>.Size` width of the submenu's own rows.
    sub_w: f32,
}

const fn m(label: &'static str, icon: G) -> Menu {
    Menu { label, icon, arrow: false, sub: &[], sub_w: 0.0 }
}

/// An item whose submenu ShareX fills at runtime: the arrow is real, the rows
/// are not ours to invent.
const fn m_dyn(label: &'static str, icon: G) -> Menu {
    Menu { label, icon, arrow: true, sub: &[], sub_w: 0.0 }
}

const fn m_sub(label: &'static str, icon: G, sub: &'static [Menu], sub_w: f32) -> Menu {
    Menu { label, icon, arrow: true, sub, sub_w }
}

const MSEP: Menu = Menu { label: "", icon: G::None, arrow: false, sub: &[], sub_w: 0.0 };

/// A row of `tsMain`. An empty label is a separator; a non-empty `menu` or a
/// set `drop` flag makes it a `ToolStripDropDownButton` instead of a
/// `ToolStripButton`.
struct Row {
    label: &'static str,
    icon: G,
    drop: bool,
    menu: &'static [Menu],
    /// `<item>.Size` width of the dropdown's rows.
    menu_w: f32,
}

const fn tsb(label: &'static str, icon: G) -> Row {
    Row { label, icon, drop: false, menu: &[], menu_w: 0.0 }
}

const fn tsddb(label: &'static str, icon: G, menu: &'static [Menu], menu_w: f32) -> Row {
    Row { label, icon, drop: true, menu, menu_w }
}

/// A dropdown ShareX builds from the user's configuration, so the designer
/// gives it no items and neither do we.
const fn tsddb_dyn(label: &'static str, icon: G) -> Row {
    Row { label, icon, drop: true, menu: &[], menu_w: 0.0 }
}

const RSEP: Row = Row { label: "", icon: G::None, drop: false, menu: &[], menu_w: 0.0 };

/// `tsmiScreenshotDelay.DropDownItems`, rows 126 wide.
const DELAY: &[Menu] = &[
    m("No delay", G::None),
    m("1 second", G::None),
    m("2 seconds", G::None),
    m("3 seconds", G::None),
    m("4 seconds", G::None),
    m("5 seconds", G::None),
];

/// `tsddbCapture.DropDownItems`, rows 191 wide.
const CAPTURE: &[Menu] = &[
    m("Fullscreen", G::Screen),
    m("Window", G::Window),
    m("Monitor", G::File("monitor-image")),
    m("Region", G::File("layer-shape")),
    m("Region (Light)", G::DashRect),
    m("Region (Transparent)", G::DashRect),
    m("Last region", G::File("layers-stack-arrange")),
    m("Screen recording", G::Film),
    m("Screen recording (GIF)", G::Film),
    m("Scrolling capture...", G::Scroll),
    m("Auto capture...", G::Clock),
    MSEP,
    m("Show cursor", G::File("cursor")),
    m_sub("Screenshot delay", G::Clock, DELAY, 126.0),
];

/// `tsddbUpload.DropDownItems`, rows 203 wide.
const UPLOAD: &[Menu] = &[
    m("Upload file...", G::Folder),
    m("Upload folder...", G::Folder),
    m("Upload from clipboard...", G::File("clipboard")),
    m("Upload text...", G::Note),
    m("Upload from URL...", G::File("drive-globe")),
    m("Drag and drop upload...", G::Tray),
    m("Shorten URL...", G::Link),
];

/// `tsddbTools.DropDownItems`, rows 194 wide.
const TOOLS: &[Menu] = &[
    m("Color picker...", G::Palette),
    m("Screen color picker...", G::Pipette),
    m("Ruler...", G::Ruler),
    m("Pin to screen...", G::Pin),
    MSEP,
    m("Image editor...", G::File("image--pencil")),
    m("Image beautifier...", G::Sunset),
    m("Image effects...", G::File("layer-shade-white")),
    m("Image viewer...", G::File("image-empty")),
    m("Background remover...", G::Wand),
    m("Image comparer...", G::Split),
    m("Image combiner...", G::File("image--plus")),
    m("Image splitter...", G::File("layer--minus")),
    m("Image thumbnailer...", G::File("image-resize-actual")),
    MSEP,
    m("Video converter...", G::Film),
    m("Video thumbnailer...", G::File("image-resize")),
    MSEP,
    m("Analyze image...", G::Robot),
    m("OCR...", G::LetterA),
    m("QR code...", G::File("grid-white")),
    m("Hash checker...", G::Hash),
    m("Metadata...", G::Note),
    m("Directory indexer...", G::Folder),
    MSEP,
    m("Clipboard viewer...", G::File("clipboard")),
    m("Borderless window...", G::Window),
    m("Inspect window...", G::File("magnifier-zoom")),
    m("Monitor test...", G::File("monitor-image")),
];

/// `tsddbDestinations.DropDownItems`, rows 181 wide. Every one of them opens a
/// list of services that ShareX assembles at runtime.
const DESTINATIONS: &[Menu] = &[
    m_dyn("Image uploaders", G::Picture),
    m_dyn("Text uploaders", G::Note),
    m_dyn("File uploaders", G::Doc),
    m_dyn("URL shorteners", G::Link),
    m_dyn("URL sharing services", G::File("drive-globe")),
];

/// `tsddbDebug.DropDownItems`, rows 172 wide.
const DEBUG: &[Menu] = &[
    m("Debug log...", G::Doc),
    m("Test image upload", G::Picture),
    m("Test text upload", G::Note),
    m("Test file upload", G::Doc),
    m("Test URL shortener", G::Link),
    m("Test URL sharing", G::File("drive-globe")),
];

/// `tsMain.Items` in the designer's order.
const TOOLBAR: &[Row] = &[
    tsddb("Capture", G::File("camera"), CAPTURE, 191.0),
    tsddb("Upload", G::Up, UPLOAD, 203.0),
    tsddb_dyn("Workflows", G::Layers),
    tsddb("Tools", G::Toolbox, TOOLS, 194.0),
    RSEP,
    tsddb_dyn("After capture tasks", G::ImgOut),
    tsddb_dyn("After upload tasks", G::CloudUp),
    tsddb("Destinations", G::File("drive-globe"), DESTINATIONS, 181.0),
    RSEP,
    tsb("Application settings...", G::File("wrench-screwdriver")),
    tsb("Task settings...", G::File("gear")),
    tsb("Hotkey settings...", G::Keyboard),
    tsb("Destination settings...", G::GlobePen),
    tsb("Custom uploader settings...", G::Cloud),
    RSEP,
    tsb("Screenshots folder...", G::File("folder-open-image")),
    tsb("History...", G::Doc),
    tsb("Image history...", G::Picture),
    RSEP,
    tsddb("Debug", G::Cone, DEBUG, 172.0),
    tsb("Donate...", G::Heart),
    tsb("Follow @ShareX...", G::XLogo),
    tsb("Discord...", G::Discord),
    tsb("About...", G::Crown),
];

/// `lvUploads.Columns`: text and `<column>.Width`. `chStatus` carries no width
/// in the resx, so it keeps the WinForms default of 60.
const COLUMNS: &[(&str, f32)] = &[
    ("Filename", 150.0),
    ("Status", 60.0),
    ("Progress", 125.0),
    ("Speed", 75.0),
    ("Elapsed", 45.0),
    ("Remaining", 45.0),
    ("URL", 145.0),
];

// ------------------------------------------------------------------ drawing

/// A 16 x 16 grid over an icon slot, the same grid the Fugue PNGs are cut on,
/// so a drawn glyph sits at the same weight as a loaded one.
struct Pen {
    p: egui::Painter,
    /// The top-left of the slot; every coordinate below is an offset from it.
    o: Pos2,
}

impl Pen {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x, y)
    }

    fn fill(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.p.rect_filled(Rect::from_min_max(self.at(x0, y0), self.at(x1, y1)), 0, dim(c));
    }

    fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color32) {
        self.p
            .line_segment([self.at(x0, y0), self.at(x1, y1)], egui::Stroke::new(w, dim(c)));
    }

    fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.p.rect_stroke(
            Rect::from_min_max(self.at(x0, y0), self.at(x1, y1)),
            0,
            egui::Stroke::new(1.0, dim(c)),
            egui::StrokeKind::Inside,
        );
    }

    fn disc(&self, cx: f32, cy: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.at(cx, cy), r, dim(c));
    }

    fn ring(&self, cx: f32, cy: f32, r: f32, w: f32, c: Color32) {
        self.p
            .circle_stroke(self.at(cx, cy), r, egui::Stroke::new(w, dim(c)));
    }

    fn poly(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p
            .add(egui::Shape::convex_polygon(v, dim(c), egui::Stroke::NONE));
    }

    fn glyph(&self, s: &str, x: f32, y: f32, size: f32, c: Color32) {
        self.p.text(
            self.at(x, y),
            Align2::CENTER_CENTER,
            s,
            font::font_id(size, true),
            dim(c),
        );
    }
}

/// Icons are drawn at the alpha a disabled ToolStrip image gets in WinForms.
fn dim(c: Color32) -> Color32 {
    c.gamma_multiply(ICON_A)
}

const I_BLUE: Color32 = Color32::from_rgb(0x6E, 0xA8, 0xDC);
const I_GREEN: Color32 = Color32::from_rgb(0x7F, 0xBF, 0x5F);
const I_GREY: Color32 = Color32::from_rgb(0xB4, 0xB4, 0xB4);
const I_DARK: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const I_YELLOW: Color32 = Color32::from_rgb(0xE0, 0xB4, 0x50);
const I_RED: Color32 = Color32::from_rgb(0xD0, 0x60, 0x60);
const I_WHITE: Color32 = Color32::from_rgb(0xE7, 0xE9, 0xEA);

/// Paints one icon into a 16 x 16 slot. The `File` arm hands the slot to a
/// Fugue PNG; the rest are the drawn approximations.
fn icon(ui: &egui::Ui, icons: &Icons, g: G, r: Rect) {
    if let G::File(name) = g {
        icons
            .img(name)
            .tint(Color32::from_white_alpha((255.0 * ICON_A) as u8))
            .paint_at(ui, r);
        return;
    }
    let k = Pen { p: ui.painter().with_clip_rect(ui.clip_rect()), o: r.min };
    match g {
        G::File(_) | G::None => {}
        G::Up => {
            k.poly(&[(8.0, 2.0), (14.0, 9.0), (10.0, 9.0), (6.0, 9.0), (2.0, 9.0)], I_GREEN);
            k.fill(6.0, 9.0, 10.0, 14.0, I_GREEN);
        }
        G::Layers => {
            k.fill(2.0, 3.0, 14.0, 6.0, I_BLUE);
            k.fill(2.0, 7.0, 14.0, 10.0, I_GREY);
            k.fill(2.0, 11.0, 14.0, 14.0, I_YELLOW);
        }
        G::Toolbox => {
            k.fill(2.0, 6.0, 14.0, 14.0, I_RED);
            k.fill(6.0, 3.0, 10.0, 6.0, I_GREY);
            k.fill(2.0, 9.0, 14.0, 10.0, I_DARK);
        }
        G::ImgOut => {
            k.frame(1.0, 3.0, 9.0, 11.0, I_GREY);
            k.poly(&[(3.0, 10.0), (5.5, 6.0), (8.0, 10.0)], I_GREEN);
            k.poly(&[(9.0, 12.0), (15.0, 12.0), (15.0, 9.0), (15.0, 15.0)], I_BLUE);
            k.fill(9.0, 11.5, 13.0, 12.5, I_BLUE);
        }
        G::CloudUp => {
            k.disc(6.0, 9.0, 4.0, I_GREY);
            k.disc(10.5, 9.5, 3.5, I_GREY);
            k.fill(4.0, 9.0, 13.0, 13.0, I_GREY);
            k.poly(&[(8.5, 2.0), (12.0, 6.5), (5.0, 6.5)], I_GREEN);
        }
        G::Cloud => {
            k.disc(6.0, 8.0, 4.0, I_BLUE);
            k.disc(10.5, 8.5, 3.5, I_BLUE);
            k.fill(4.0, 8.0, 13.0, 12.0, I_BLUE);
        }
        G::Keyboard => {
            k.fill(1.0, 5.0, 15.0, 12.0, I_GREY);
            for c in 0..5 {
                k.fill(2.5 + c as f32 * 2.4, 6.5, 3.9 + c as f32 * 2.4, 7.8, I_DARK);
            }
            k.fill(4.0, 9.0, 12.0, 10.5, I_DARK);
        }
        G::GlobePen => {
            k.ring(7.0, 7.5, 5.0, 1.2, I_BLUE);
            k.line(2.0, 7.5, 12.0, 7.5, 1.0, I_BLUE);
            k.line(7.0, 2.5, 7.0, 12.5, 1.0, I_BLUE);
            k.poly(&[(9.5, 14.5), (15.0, 9.0), (16.0, 10.0), (10.5, 15.5)], I_YELLOW);
        }
        G::Doc => {
            k.fill(3.0, 1.5, 13.0, 14.5, I_WHITE);
            for i in 0..4 {
                k.fill(5.0, 4.0 + i as f32 * 2.5, 11.0, 5.0 + i as f32 * 2.5, I_DARK);
            }
        }
        G::Picture => {
            k.fill(1.5, 3.0, 14.5, 13.0, I_BLUE);
            k.disc(5.0, 6.5, 1.6, I_YELLOW);
            k.poly(&[(3.0, 12.0), (8.0, 6.0), (13.0, 12.0)], I_GREEN);
        }
        G::Cone => {
            k.poly(&[(8.0, 1.5), (13.5, 12.5), (2.5, 12.5)], I_YELLOW);
            k.fill(4.5, 8.0, 11.5, 9.5, I_WHITE);
            k.fill(1.0, 12.5, 15.0, 14.5, I_DARK);
        }
        G::Heart => {
            k.disc(5.5, 6.0, 3.6, I_RED);
            k.disc(10.5, 6.0, 3.6, I_RED);
            k.poly(&[(2.2, 7.5), (13.8, 7.5), (8.0, 14.5)], I_RED);
        }
        G::XLogo => {
            k.line(3.0, 3.0, 13.0, 13.0, 2.0, I_WHITE);
            k.line(13.0, 3.0, 3.0, 13.0, 2.0, I_WHITE);
        }
        G::Discord => {
            k.disc(5.0, 8.0, 3.5, I_BLUE);
            k.disc(11.0, 8.0, 3.5, I_BLUE);
            k.fill(5.0, 4.5, 11.0, 11.5, I_BLUE);
            k.disc(6.0, 8.0, 1.2, BG);
            k.disc(10.0, 8.0, 1.2, BG);
        }
        G::Crown => {
            k.poly(
                &[(1.5, 12.0), (1.5, 4.0), (5.5, 8.0), (8.0, 3.5), (10.5, 8.0), (14.5, 4.0), (14.5, 12.0)],
                I_YELLOW,
            );
            k.fill(1.5, 12.0, 14.5, 14.0, I_YELLOW);
        }
        G::Screen => {
            k.fill(1.0, 2.5, 15.0, 13.5, I_BLUE);
            k.frame(1.0, 2.5, 15.0, 13.5, I_WHITE);
        }
        G::Window => {
            k.fill(1.5, 2.5, 14.5, 13.5, I_WHITE);
            k.fill(1.5, 2.5, 14.5, 5.5, I_BLUE);
        }
        G::DashRect => {
            let c = I_WHITE;
            for i in 0..4 {
                let a = 2.0 + i as f32 * 3.2;
                k.fill(a, 3.0, a + 2.0, 4.0, c);
                k.fill(a, 12.0, a + 2.0, 13.0, c);
            }
            for i in 0..3 {
                let b = 4.5 + i as f32 * 2.8;
                k.fill(2.0, b, 3.0, b + 1.8, c);
                k.fill(13.0, b, 14.0, b + 1.8, c);
            }
        }
        G::Film => {
            k.fill(1.0, 3.0, 15.0, 13.0, I_DARK);
            for i in 0..5 {
                let a = 1.6 + i as f32 * 2.7;
                k.fill(a, 3.6, a + 1.6, 5.0, I_WHITE);
                k.fill(a, 11.0, a + 1.6, 12.4, I_WHITE);
            }
            k.fill(2.5, 6.0, 13.5, 10.0, I_GREY);
        }
        G::Scroll => {
            k.frame(2.0, 1.5, 14.0, 14.5, I_GREY);
            k.fill(11.0, 1.5, 14.0, 14.5, I_GREY);
            k.poly(&[(12.5, 12.5), (10.8, 9.5), (14.2, 9.5)], I_BLUE);
        }
        G::Clock => {
            k.disc(8.0, 8.0, 6.5, I_WHITE);
            k.ring(8.0, 8.0, 6.5, 1.0, I_DARK);
            k.line(8.0, 8.0, 8.0, 4.0, 1.2, I_DARK);
            k.line(8.0, 8.0, 11.0, 9.0, 1.2, I_DARK);
        }
        G::Folder => {
            k.fill(1.0, 3.0, 7.5, 5.0, I_YELLOW);
            k.fill(1.0, 4.5, 15.0, 13.5, I_YELLOW);
        }
        G::Note => {
            k.fill(2.5, 1.5, 13.5, 14.5, I_WHITE);
            k.fill(2.5, 1.5, 4.5, 14.5, I_GREEN);
            for i in 0..4 {
                k.fill(5.5, 4.0 + i as f32 * 2.5, 12.0, 5.0 + i as f32 * 2.5, I_DARK);
            }
        }
        G::Tray => {
            k.poly(&[(1.0, 7.0), (15.0, 7.0), (15.0, 13.5), (1.0, 13.5)], I_GREY);
            k.fill(1.0, 9.5, 15.0, 10.5, I_DARK);
            k.poly(&[(8.0, 6.5), (11.5, 2.5), (4.5, 2.5)], I_BLUE);
        }
        G::Link => {
            k.ring(5.5, 10.5, 3.4, 1.6, I_BLUE);
            k.ring(10.5, 5.5, 3.4, 1.6, I_BLUE);
            k.line(6.5, 9.5, 9.5, 6.5, 1.6, I_BLUE);
        }
        G::Palette => {
            k.fill(1.5, 1.5, 8.0, 8.0, I_RED);
            k.fill(8.0, 1.5, 14.5, 8.0, I_GREEN);
            k.fill(1.5, 8.0, 8.0, 14.5, I_BLUE);
            k.fill(8.0, 8.0, 14.5, 14.5, I_YELLOW);
        }
        G::Pipette => {
            k.poly(&[(9.0, 2.0), (14.0, 7.0), (11.5, 9.5), (6.5, 4.5)], I_GREY);
            k.poly(&[(6.5, 5.5), (10.5, 9.5), (4.0, 14.0), (2.0, 12.0)], I_BLUE);
        }
        G::Ruler => {
            k.poly(&[(1.5, 11.0), (11.0, 1.5), (14.5, 5.0), (5.0, 14.5)], I_YELLOW);
            for i in 0..4 {
                let a = 3.5 + i as f32 * 2.2;
                k.line(a, 12.5 - i as f32 * 2.2, a + 2.0, 10.5 - i as f32 * 2.2, 1.0, I_DARK);
            }
        }
        G::Pin => {
            k.poly(&[(9.5, 1.5), (14.5, 6.5), (11.5, 8.0), (8.0, 5.0)], I_RED);
            k.poly(&[(4.0, 6.0), (10.5, 12.0), (5.0, 13.0), (3.0, 11.0)], I_GREY);
            k.line(4.0, 12.0, 1.5, 14.5, 1.2, I_DARK);
        }
        G::Sunset => {
            k.fill(1.5, 2.5, 14.5, 13.5, I_BLUE);
            k.disc(8.0, 7.0, 3.0, I_YELLOW);
            k.fill(1.5, 10.0, 14.5, 13.5, I_GREEN);
        }
        G::Wand => {
            k.poly(&[(3.0, 14.0), (11.5, 5.5), (13.0, 7.0), (4.5, 15.5)], I_GREY);
            k.glyph("+", 12.5, 3.0, 9.0, I_YELLOW);
            k.glyph("+", 5.0, 3.5, 7.0, I_YELLOW);
        }
        G::Robot => {
            k.fill(2.5, 4.5, 13.5, 13.0, I_GREY);
            k.disc(6.0, 8.0, 1.4, I_DARK);
            k.disc(10.0, 8.0, 1.4, I_DARK);
            k.fill(7.5, 1.5, 8.5, 4.5, I_GREY);
            k.disc(8.0, 1.5, 1.2, I_BLUE);
        }
        G::LetterA => {
            k.glyph("A", 8.0, 8.0, 14.0, I_WHITE);
        }
        G::Hash => {
            k.glyph("#", 8.0, 8.0, 14.0, I_GREEN);
        }
        G::Split => {
            k.frame(1.0, 2.5, 15.0, 13.5, I_GREY);
            k.fill(1.0, 2.5, 7.5, 13.5, I_BLUE);
            k.fill(8.5, 2.5, 15.0, 13.5, I_GREEN);
        }
    }
}

/// The three-point arrow WinForms paints on a `ToolStripDropDownButton` and on
/// an item that carries a submenu.
fn arrow(p: &egui::Painter, c: Pos2, right: bool, col: Color32) {
    let pts = if right {
        vec![c + vec2(-2.0, -3.5), c + vec2(2.0, 0.0), c + vec2(-2.0, 3.5)]
    } else {
        vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.0)]
    };
    p.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
}

// -------------------------------------------------------------- the window

pub struct MainWindow {
    icons: Icons,
    /// Index into `TOOLBAR` of the dropdown that is open, if any.
    open: Option<usize>,
    /// Index into that dropdown of the submenu that is open, if any.
    sub: Option<usize>,
}

impl MainWindow {
    pub fn new(ctx: &egui::Context) -> Self {
        Self { icons: Icons::load(ctx), open: None, sub: None }
    }

    /// `form` is the client area, which the caller knows: the live window hands
    /// over its root `Ui`, the screenshot mode a rectangle of the form's own
    /// size.
    pub fn ui(&mut self, ctx: &egui::Context, form: Rect) {
        style(ctx);
        // a click that lands on nothing of ours closes whatever is open, the way
        // a WinForms dropdown dismisses itself
        let mut hit = false;
        egui::Area::new(egui::Id::new("form"))
            .order(egui::Order::Background)
            .fixed_pos(form.min)
            .show(ctx, |ui: &mut egui::Ui| {
                let (all, _) = ui.allocate_exact_size(form.size(), Sense::hover());
                ui.painter().rect_filled(all, 0, BG);
                let bar = Rect::from_min_size(all.min, vec2(BAR_W, all.height()));
                let main = Rect::from_min_max(pos2(all.min.x + BAR_W, all.min.y), all.max);
                self.toolbar(ui, bar, &mut hit);
                task_area(ui, main);
            });
        self.menus(ctx, form, &mut hit);
        if !hit && ctx.input(|i| i.pointer.any_click()) {
            self.open = None;
            self.sub = None;
        }
    }

    /// `pToolbars` and the `tsMain` inside it: a vertical stack of 171 x 20
    /// rows, ended on the right by the border `ToolStripBorderRight` draws.
    fn toolbar(&mut self, ui: &mut egui::Ui, bar: Rect, hit: &mut bool) {
        let p = ui.painter().with_clip_rect(bar);
        p.rect_filled(bar, 0, BG);
        p.line_segment(
            [bar.right_top(), bar.right_bottom()],
            egui::Stroke::new(1.0, BORDER),
        );
        let grey = ui.visuals().gray_out(TEXT);
        let mut y = bar.top() + BAR_PAD_TOP;
        for (i, row) in TOOLBAR.iter().enumerate() {
            let x = bar.left() + BAR_PAD_X;
            if row.label.is_empty() {
                separator(&p, pos2(x, y + SEP_H * 0.5), ITEM_W, false);
                y += SEP_H;
                continue;
            }
            let r = Rect::from_min_size(pos2(x, y), vec2(ITEM_W, ITEM_H));
            // the pressed look of an open dropdown is the only state the shell has
            if self.open == Some(i) {
                p.rect_filled(r, 0, SEL_BG);
                p.rect_stroke(r, 0, egui::Stroke::new(1.0, SEL_BORDER), egui::StrokeKind::Inside);
            }
            let ir = Rect::from_min_size(pos2(r.left() + 2.0, r.center().y - 8.0), Vec2::splat(16.0));
            icon(ui, &self.icons, row.icon, ir);
            p.text(
                pos2(ir.right() + 5.0, r.center().y),
                Align2::LEFT_CENTER,
                row.label,
                font::font_id(FONT, false),
                grey,
            );
            if row.drop {
                arrow(&p, pos2(r.right() - 8.0, r.center().y), false, grey);
                let resp = ui.interact(r, egui::Id::new(("tsddb", i)), Sense::click());
                if resp.clicked() {
                    *hit = true;
                    self.open = if self.open == Some(i) { None } else { Some(i) };
                    self.sub = None;
                }
                if resp.hovered() && self.open != Some(i) {
                    p.rect_filled(r, 0, HOVER_BG);
                }
            }
            y += ITEM_H;
        }
    }

    /// The open dropdown and, if one is open too, its submenu.
    fn menus(&mut self, ctx: &egui::Context, form: Rect, hit: &mut bool) {
        let Some(i) = self.open else { return };
        let row = &TOOLBAR[i];
        if row.menu.is_empty() {
            return;
        }
        // a vertical ToolStrip drops its menus to the right, level with the button
        let top = form.top() + BAR_PAD_TOP + strip_offset(i);
        let left = form.left() + BAR_PAD_X + ITEM_W;
        let picked = self.menu(ctx, "menu", pos2(left, top), row.menu, row.menu_w, self.sub, hit);
        if let Some(k) = picked {
            self.sub = if self.sub == Some(k) { None } else { Some(k) };
        }
        let Some(k) = self.sub else { return };
        let it = &row.menu[k];
        if it.sub.is_empty() {
            return;
        }
        let sy = top + menu_offset(row.menu, k);
        let sx = left + row.menu_w + 2.0;
        self.menu(ctx, "submenu", pos2(sx, sy), it.sub, it.sub_w, None, hit);
    }

    /// One dropdown panel. Returns the index of a submenu row that was clicked.
    fn menu(
        &self,
        ctx: &egui::Context,
        id: &str,
        at: Pos2,
        rows: &'static [Menu],
        w: f32,
        open_sub: Option<usize>,
        hit: &mut bool,
    ) -> Option<usize> {
        let h: f32 = rows.iter().map(|r| if r.label.is_empty() { SEP_H } else { MENU_H }).sum();
        let mut picked = None;
        egui::Area::new(egui::Id::new(id))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui: &mut egui::Ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(w + 2.0, h + 4.0), Sense::click());
                let p = ui.painter().with_clip_rect(rect);
                p.rect_filled(rect, 0, BG);
                p.rect_stroke(rect, 0, egui::Stroke::new(1.0, SEL_BORDER), egui::StrokeKind::Inside);
                let grey = ui.visuals().gray_out(TEXT);
                let mut y = rect.top() + 2.0;
                for (j, it) in rows.iter().enumerate() {
                    let x = rect.left() + 1.0;
                    if it.label.is_empty() {
                        separator(&p, pos2(x + MENU_TEXT_X, y + SEP_H * 0.5), w - MENU_TEXT_X, true);
                        y += SEP_H;
                        continue;
                    }
                    let r = Rect::from_min_size(pos2(x, y), vec2(w, MENU_H));
                    if open_sub == Some(j) {
                        p.rect_filled(r, 0, HOVER_BG);
                    }
                    let ir = Rect::from_min_size(
                        pos2(r.left() + MENU_ICON_X, r.center().y - 8.0),
                        Vec2::splat(16.0),
                    );
                    icon(ui, &self.icons, it.icon, ir);
                    p.text(
                        pos2(r.left() + MENU_TEXT_X, r.center().y),
                        Align2::LEFT_CENTER,
                        it.label,
                        font::font_id(FONT, false),
                        grey,
                    );
                    if it.arrow {
                        arrow(&p, pos2(r.right() - 9.0, r.center().y), true, grey);
                    }
                    if !it.sub.is_empty() {
                        let resp = ui.interact(r, egui::Id::new((id, j)), Sense::click());
                        if resp.clicked() {
                            *hit = true;
                            picked = Some(j);
                        }
                    }
                    y += MENU_H;
                }
                if ui.rect_contains_pointer(rect) && ctx.input(|i| i.pointer.any_click()) {
                    *hit = true;
                }
            });
        picked
    }
}

/// A `ToolStripSeparator`: the dark line of `SeparatorDarkColor` with the
/// `SeparatorLightColor` highlight under it that the professional renderer
/// draws. In a menu it starts after the image margin.
fn separator(p: &egui::Painter, at: Pos2, w: f32, in_menu: bool) {
    let a = if in_menu { SEP_LIGHT } else { BORDER };
    p.line_segment([at, at + vec2(w, 0.0)], egui::Stroke::new(1.0, a));
    if !in_menu {
        p.line_segment(
            [at + vec2(0.0, 1.0), at + vec2(w, 1.0)],
            egui::Stroke::new(1.0, SEP_LIGHT),
        );
    }
}

/// The y of toolbar row `i`, counted from the top of `tsMain`'s padding.
fn strip_offset(i: usize) -> f32 {
    TOOLBAR[..i]
        .iter()
        .map(|r| if r.label.is_empty() { SEP_H } else { ITEM_H })
        .sum()
}

/// The same for a menu row, plus the dropdown's own top padding.
fn menu_offset(rows: &[Menu], k: usize) -> f32 {
    2.0 + rows[..k]
        .iter()
        .map(|r| if r.label.is_empty() { SEP_H } else { MENU_H })
        .sum::<f32>()
}

/// `pMain`: the split container, `lvUploads` on the left and `pbPreview` on
/// the right. The list holds no rows — nothing has been captured — and the
/// preview has nothing to show.
fn task_area(ui: &mut egui::Ui, main: Rect) {
    let p = ui.painter().with_clip_rect(main);
    let list = Rect::from_min_size(main.min, vec2(SPLIT, main.height()));
    let gap = Rect::from_min_size(pos2(list.right(), main.top()), vec2(SPLIT_W, main.height()));
    let prev = Rect::from_min_max(pos2(gap.right(), main.top()), main.max);

    // the header row of the MyListView, in the designer's column order
    let head = Rect::from_min_size(list.min, vec2(list.width(), HEAD_H));
    p.rect_filled(head, 0, BG);
    p.rect_filled(
        Rect::from_min_max(pos2(list.left(), head.bottom()), list.max),
        0,
        DARK_BG,
    );
    let grey = ui.visuals().gray_out(TEXT);
    let hp = p.with_clip_rect(head);
    // `lvUploads.AutoFillColumn` gives the leftover width to the last column
    let fixed: f32 = COLUMNS[..COLUMNS.len() - 1].iter().map(|c| c.1).sum();
    let last = (list.width() - fixed).max(COLUMNS[COLUMNS.len() - 1].1);
    let mut x = list.left();
    for (n, (name, w)) in COLUMNS.iter().enumerate() {
        let w = if n + 1 == COLUMNS.len() { last } else { *w };
        hp.text(
            pos2(x + 6.0, head.center().y),
            Align2::LEFT_CENTER,
            *name,
            font::font_id(FONT, false),
            grey,
        );
        x += w;
        hp.line_segment(
            [pos2(x, head.top() + 3.0), pos2(x, head.bottom() - 3.0)],
            egui::Stroke::new(1.0, SEL_BORDER),
        );
    }
    p.line_segment(
        [head.left_bottom(), head.right_bottom()],
        egui::Stroke::new(1.0, BORDER),
    );

    // `SplitContainerCustomSplitter` paints a thin line down the middle of the
    // six points it reserves
    p.rect_filled(gap, 0, BG);
    p.line_segment(
        [pos2(gap.center().x, gap.top() + 2.0), pos2(gap.center().x, gap.bottom() - 2.0)],
        egui::Stroke::new(1.0, SEL_BORDER),
    );

    // the seven columns add up to 645 points against the 335 the panel has, and
    // `FillColumn` gives up when the leftover is negative, so the real control
    // ends with a horizontal scrollbar and the last four headers off-screen
    let sum: f32 = COLUMNS.iter().map(|c| c.1).sum::<f32>() - COLUMNS[COLUMNS.len() - 1].1 + last;
    if sum > list.width() {
        let track = Rect::from_min_max(pos2(list.left(), list.bottom() - 15.0), list.max);
        p.rect_filled(track, 0, BG);
        let thumb = Rect::from_min_size(
            track.min + vec2(1.0, 3.0),
            vec2((track.width() * list.width() / sum).max(20.0), 9.0),
        );
        p.rect_filled(thumb, 2, SEL_BORDER);
    }

    p.rect_filled(prev, 0, DARK_BG);
}

/// The dark theme, so the shell and the region editor read as one program.
fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.panel_fill = BG;
        s.visuals.window_fill = BG;
        s.visuals.extreme_bg_color = DARK_BG;
        s.visuals.override_text_color = Some(TEXT);
        // `gray_out` tints towards this, which is what turns a label into a
        // disabled label; pointing it at the form background gives the WinForms
        // grey
        s.visuals.widgets.noninteractive.weak_bg_fill = BG;
    });
}

impl eframe::App for MainWindow {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        font::sync();
        let form = ui.max_rect();
        self.ui(&ctx, form);
    }
}

pub fn run() -> Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([FORM_W, FORM_H])
            .with_min_inner_size([MIN_W, MIN_H])
            .with_title("ShareX"),
        ..Default::default()
    };
    eframe::run_native(
        "sxr",
        opts,
        Box::new(move |cc| {
            font::install(&cc.egui_ctx);
            Ok(Box::new(MainWindow::new(&cc.egui_ctx)) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

/// `--mainwin-shot`: the shell at the form's own client size, then the same
/// shell with a dropdown open, so the menus can be held next to the real
/// ShareX without starting a window.
pub fn shot(path: &str) -> Result<()> {
    let stem = path.strip_suffix(".png").unwrap_or(path);
    one(path, FORM_H, None, None)?;
    one(&format!("{stem}-capture.png"), FORM_H, Some(0), None)?;
    one(&format!("{stem}-capture-delay.png"), FORM_H, Some(0), Some(13))?;
    one(&format!("{stem}-upload.png"), FORM_H, Some(1), None)?;
    // the Tools dropdown is 662 points tall against the form's 531, so the real
    // one grows scroll arrows; the shot stretches the form instead, since the
    // point here is to read all 29 rows at once
    one(&format!("{stem}-tools.png"), 690.0, Some(3), None)
}

fn one(path: &str, h: f32, open: Option<usize>, sub: Option<usize>) -> Result<()> {
    let ctx = egui::Context::default();
    font::install(&ctx);
    let mut w = MainWindow::new(&ctx);
    w.open = open;
    w.sub = sub;
    let form = Rect::from_min_size(Pos2::ZERO, vec2(FORM_W, h));
    shot_ctx(path, FORM_W as u32, h as u32, &ctx, |c| {
        // the shot has no pointer, so the state set above is what gets drawn
        w.ui(c, form);
    })
}
