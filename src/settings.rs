//! ShareX's Application settings window, reproduced as an inert shell.
//!
//! Every geometry number below comes from `ShareX/Forms/ApplicationSettingsForm.resx`
//! on the `master` branch, where the designer's `ApplyResources` calls leave the
//! sizes and the labels; the control tree comes from the `>>name.Parent` and
//! `>>name.ZOrder` entries of the same file, so the tables are in the designer's
//! own order. The controls are drawn, not wired: a button that would reset the
//! settings here does nothing, and says so by rendering greyed out.
//!
//! The one thing the designer does not put on screen is a tab strip.
//! `tcSettings` carries all twelve pages but is `Visible = False`; the form
//! shows a `TabToTreeView` docked over the whole client area, which lists the
//! page names in a tree on the left and hosts the selected page on the right.
//! That is why the pages start at x = 173 in the resx and the form is 737 wide.
//!
//! The labels are ShareX's own English control names, kept as literals rather
//! than going through `i18n`: they name controls that do nothing yet, and
//! `--i18n-check` asks every language to carry every key. They move into
//! `i18n` on the day a control becomes live.

use anyhow::Result;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{HOVER_BG, SEL_BG, SEL_BORDER, shot_ctx};
use crate::font;

// ------------------------------------------------------------------ palette
//
// `ShareXTheme.DarkTheme` in `ShareX.HelpersLib/ShareXTheme.cs`, handed to the
// controls by `ShareXResources.ApplyCustomThemeToControl`. Three of its entries
// are already in `app`, because the region editor's toolbar uses them, and the
// two windows have to look like one application.

/// `BackgroundColor` — the form, the tab page and the splitter.
const BG: Color32 = Color32::from_rgb(0x27, 0x27, 0x27);
/// `DarkBackgroundColor` — `TabToTreeView.LeftPanelBackColor`, so the tree.
const DARK_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
/// `TextColor`.
const TEXT: Color32 = Color32::from_rgb(0xE7, 0xE9, 0xEA);
/// `BorderColor` — also `SeparatorDarkColor`, which paints `pSeparator`, and
/// the flat border `ApplyTheme` gives every button.
const BORDER: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
/// `LightBackgroundColor`: what `ApplyTheme` fills a button, a text box, a
/// combo box, a list view and a property grid's view with. `app` re-exports the
/// same colour under the name the editor's toolbar uses it for.
const LIGHT_BG: Color32 = HOVER_BG;

// ----------------------------------------------------------------- geometry

/// `$this.ClientSize`, plus the `PAGE_EXTRA` the Upload tab needs. Everything
/// is laid out from the left, so the form grows by exactly what that page took.
pub const FORM_W: f32 = 737.0 + PAGE_EXTRA;
pub const FORM_H: f32 = 402.0;
/// How much wider than ShareX the tab page has to be. Segoe UI 9pt fits
/// "Secondary image uploaders" inside a 168-point group box; DejaVu, the face
/// we ship, wants 180 for the same caption at the same size, and the Upload tab
/// stands three of those boxes side by side. Twelve points each pushes the last
/// one from 536 out to 572, and the page grows by that plus the 23 points of
/// right margin ShareX left beside them. The alternative was to shrink the
/// caption, which reads as the wrong font size long before anyone notices
/// thirty-six points of window.
const PAGE_EXTRA: f32 = 36.0;
/// `$this.MinimumSize` is the outer window, frame included. egui is given the
/// client area instead, and for the height the frame already accounts for the
/// whole difference, so the form's own height is the floor.
const MIN_W: f32 = 640.0 + PAGE_EXTRA;
const MIN_H: f32 = FORM_H;
/// `tttvMain.TreeViewSize`, which the control forwards to
/// `scMain.SplitterDistance`, and `scMain.SplitterWidth`.
const TREE_W: f32 = 175.0;
const SPLIT_W: f32 = 3.0;
/// `pLeft.Padding` — the inset of `tvMain` inside the left panel.
const TREE_PAD: f32 = 8.0;
/// `tvMain.ItemHeight`.
const ROW_H: f32 = 25.0;
/// A `TreeView`'s default `Indent`. `ShowLines` and `ShowPlusMinus` are both
/// off, but `ShowRootLines` is not, so the room for the glyph is still taken.
const INDENT: f32 = 19.0;
/// Where the tab page starts: `Panel2` of the split container, holding the
/// `TablessControl` that traps `TCM_ADJUSTRECT`, so the page fills it whole.
/// leaving the page 559 x 402 — plus `PAGE_EXTRA` — rather than the 556 x 376
/// the designer drew.
const PAGE_X: f32 = TREE_W + SPLIT_W;
/// `AutoScaleDimensions` is 96, 96 and the form's font is Segoe UI 9pt, which
/// is 12 points at 96 dpi. DejaVu, the face we ship, is wider than Segoe UI, so
/// every container was measured against its longest label before being
/// trusted; on this form none of them had to grow.
const FONT: f32 = 12.0;
/// `tttvMain.TreeViewFont` — Microsoft Sans Serif 9.75pt, 13 points at 96 dpi.
const TREE_FONT: f32 = 13.0;
/// A `ListView` header row at 96 dpi, the same one `mainwin` draws.
const HEAD_H: f32 = 22.0;
/// The check box glyph WinForms paints, and the gap before the label.
const BOX: f32 = 13.0;
const BOX_GAP: f32 = 4.0;
/// A `ComboBox` drop button and a `NumericUpDown` spin column.
const DROP_W: f32 = 17.0;
const SPIN_W: f32 = 16.0;
/// A `PropertyGrid`'s tool strip and its description pane, top and bottom of
/// the control.
const GRID_BAR_H: f32 = 25.0;
const GRID_HELP_H: f32 = 60.0;

// ---------------------------------------------------------------- controls

/// What a control is. Only the shapes this form uses are here.
enum K {
    /// `Label`, one line.
    Lbl,
    /// `Label` with `AutoSize` off, so WinForms wraps it inside its box.
    Wrap,
    /// `CheckBox`, with the state the designer sets.
    Chk(bool),
    /// `Button`, flat, as `ApplyTheme` leaves it.
    Btn,
    /// `MenuButton` — a button that drops a `ContextMenuStrip`.
    MenuBtn,
    /// `ComboBox`, every one of them `DropDownList`.
    Combo,
    /// `TextBox`.
    Edit,
    /// `NumericUpDown`.
    Num,
    /// `GroupBox`, the only kind with children.
    Group,
    /// `MyListView` in `Details` view: the columns and whether it shows a
    /// header (`HeaderStyle` is `None` on the secondary uploader lists).
    List(&'static [(&'static str, f32)], bool),
    /// `PropertyGrid`.
    Grid,
    /// `pbExportImportNote`, a `PictureBox` holding ShareX's `exclamation`
    /// image.
    Warn,
}

/// One control: its kind, its `.Text`, and its `.Location` and `.Size` inside
/// whatever holds it.
struct C {
    kind: K,
    text: &'static str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    kids: &'static [C],
}

const fn c(kind: K, text: &'static str, x: f32, y: f32, w: f32, h: f32) -> C {
    C { kind, text, x, y, w, h, kids: &[] }
}

const fn g(text: &'static str, x: f32, y: f32, w: f32, h: f32, kids: &'static [C]) -> C {
    C { kind: K::Group, text, x, y, w, h, kids }
}
/// `tpGeneral`.
const GENERAL: &[C] = &[
    c(K::Combo, "", 248.0, 276.0, 144.0, 21.0),
    c(K::Lbl, "Update channel:", 13.0, 280.0, 86.0, 13.0),
    c(K::Chk(false), "Automatically check for updates", 16.0, 256.0, 177.0, 17.0),
    c(K::Chk(false), "Use white ShareX icon", 16.0, 120.0, 134.0, 17.0),
    c(K::Btn, "Install dev build...", 16.0, 304.0, 288.0, 32.0),
    c(K::Combo, "", 248.0, 188.0, 288.0, 21.0),
    c(K::Lbl, "On tray icon middle click:", 13.0, 192.0, 125.0, 13.0),
    c(K::Combo, "", 248.0, 164.0, 288.0, 21.0),
    c(K::Lbl, "On tray icon double left click:", 13.0, 168.0, 144.0, 13.0),
    c(K::Combo, "", 248.0, 140.0, 288.0, 21.0),
    c(K::Lbl, "On tray icon left click:", 13.0, 144.0, 109.0, 13.0),
    c(K::Btn, "Edit quick task menu...", 16.0, 216.0, 288.0, 32.0),
    c(K::Chk(false), "Show tray icon", 16.0, 48.0, 96.0, 17.0),
    c(K::Chk(false), "Show progress in tray icon", 16.0, 72.0, 150.0, 17.0),
    c(K::MenuBtn, "", 248.0, 16.0, 288.0, 23.0),
    c(K::Chk(false), "Remember main window position", 16.0, 96.0, 180.0, 17.0),
    c(K::Chk(false), "Minimize to tray on start", 248.0, 48.0, 136.0, 17.0),
    c(K::Chk(false), "Show progress in taskbar button", 248.0, 72.0, 178.0, 17.0),
    c(K::Chk(false), "Remember main window size", 248.0, 96.0, 162.0, 17.0),
    c(K::Lbl, "Language:", 13.0, 16.0, 58.0, 13.0),
];

/// `tpTheme`.
const THEME: &[C] = &[
    c(K::Btn, "Reset...", 208.0, 48.0, 104.0, 24.0),
    c(K::Btn, "Remove", 128.0, 16.0, 104.0, 24.0),
    c(K::Btn, "Add", 16.0, 16.0, 104.0, 24.0),
    c(K::Combo, "", 240.0, 18.0, 304.0, 21.0),
    c(K::Grid, "", 16.0, 80.0, 528.0, 312.0),
    c(K::MenuBtn, "Export", 16.0, 48.0, 88.0, 24.0),
    c(K::MenuBtn, "Import", 112.0, 48.0, 88.0, 24.0),
];

/// `tpIntegration`.
const INTEGRATION: &[C] = &[
    g("Firefox addon", 8.0, 240.0, 536.0, 88.0, &[
        c(K::Chk(false), "Enable Firefox addon support", 16.0, 24.0, 164.0, 17.0),
        c(K::Btn, "Install ShareX Firefox addon...", 16.0, 48.0, 288.0, 23.0),
    ]),
    g("Steam", 8.0, 336.0, 536.0, 56.0, &[
        c(K::Chk(false), "While ShareX is open, show \"In-App\" in Steam", 16.0, 24.0, 247.0, 17.0),
    ]),
    g("Chrome extension", 8.0, 144.0, 536.0, 88.0, &[
        c(K::Chk(false), "Enable Chrome extension support", 16.0, 24.0, 184.0, 17.0),
        c(K::Btn, "Install ShareX Chrome extension...", 16.0, 48.0, 288.0, 23.0),
    ]),
    g("Windows", 8.0, 8.0, 536.0, 128.0, &[
        c(K::Chk(false), "Show \"Edit with ShareX\" button in Windows Explorer context menu", 16.0, 72.0, 343.0, 17.0),
        // `AutoSize` is on and the designer left the text empty, so the resx
        // size is the bare glyph; the text arrives at run time from
        // `ApplicationSettingsForm_cbStartWithWindows_Text` and the control
        // grows to it — 222 points wide in DejaVu.
        c(K::Chk(false), "Run ShareX when Windows starts", 16.0, 24.0, 222.0, 17.0),
        c(K::Chk(false), "Show ShareX in \"Send to\" menu", 16.0, 96.0, 181.0, 17.0),
        c(K::Chk(false), "Show \"Upload with ShareX\" button in Windows Explorer context menu", 16.0, 48.0, 359.0, 17.0),
    ]),
];

/// `tpPaths`.
const PATHS: &[C] = &[
    c(K::Edit, "", 16.0, 232.0, 408.0, 20.0),
    c(K::Lbl, "Sub folder pattern for window:", 13.0, 216.0, 148.0, 13.0),
    c(K::Btn, "Apply", 16.0, 56.0, 96.0, 23.0),
    c(K::Btn, "Open...", 16.0, 184.0, 96.0, 23.0),
    c(K::Lbl, "...", 224.0, 61.0, 16.0, 13.0),
    c(K::Btn, "Browse...", 432.0, 31.0, 96.0, 23.0),
    c(K::Lbl, "ShareX personal folder:", 13.0, 16.0, 117.0, 13.0),
    c(K::Edit, "", 16.0, 32.0, 408.0, 20.0),
    c(K::Btn, "Browse...", 432.0, 111.0, 96.0, 23.0),
    c(K::Btn, "Open...", 120.0, 56.0, 96.0, 23.0),
    c(K::Edit, "", 16.0, 112.0, 408.0, 20.0),
    c(K::Chk(false), "Use custom screenshots folder:", 16.0, 88.0, 174.0, 17.0),
    c(K::Lbl, "Sub folder pattern:", 13.0, 144.0, 94.0, 13.0),
    c(K::Lbl, "...", 120.0, 189.0, 16.0, 13.0),
    c(K::Edit, "", 16.0, 160.0, 408.0, 20.0),
];

/// `tpSettings`.
const SETTINGS: &[C] = &[
    c(K::Chk(false), "Automatically cleanup old log files", 16.0, 240.0, 184.0, 17.0),
    c(K::Num, "", 16.0, 280.0, 72.0, 20.0),
    c(K::Lbl, "Number of files to keep:", 13.0, 264.0, 119.0, 13.0),
    c(K::Chk(false), "Automatically cleanup old backup files", 16.0, 216.0, 206.0, 17.0),
    c(K::Warn, "", 16.0, 16.0, 32.0, 32.0),
    c(K::Chk(true), "History", 16.0, 88.0, 58.0, 17.0),
    c(K::Chk(true), "Settings", 16.0, 64.0, 64.0, 17.0),
    c(K::Wrap, "Note: Do not share the exported file with anyone because it might contain private information such as account details and your upload history.", 56.0, 16.0, 488.0, 32.0),
    c(K::Btn, "Reset settings...", 16.0, 176.0, 184.0, 24.0),
    c(K::Btn, "Export...", 16.0, 112.0, 184.0, 23.0),
    c(K::Btn, "Import...", 16.0, 144.0, 184.0, 23.0),
];

/// `tpMainWindow`.
const MAIN_WINDOW: &[C] = &[
    g("List view", 16.0, 200.0, 432.0, 104.0, &[
        c(K::Combo, "", 176.0, 68.0, 160.0, 21.0),
        c(K::Lbl, "Image preview location:", 13.0, 72.0, 119.0, 13.0),
        c(K::Combo, "", 176.0, 44.0, 160.0, 21.0),
        c(K::Lbl, "Image preview visibility:", 13.0, 48.0, 117.0, 13.0),
        c(K::Chk(false), "Show columns", 16.0, 24.0, 95.0, 17.0),
    ]),
    g("Thumbnail view", 16.0, 64.0, 432.0, 128.0, &[
        c(K::Btn, "Reset", 344.0, 67.0, 75.0, 23.0),
        c(K::Lbl, "x", 250.0, 72.0, 12.0, 13.0),
        c(K::Num, "", 272.0, 68.0, 64.0, 20.0),
        c(K::Num, "", 176.0, 68.0, 64.0, 20.0),
        c(K::Combo, "", 176.0, 92.0, 160.0, 21.0),
        c(K::Lbl, "Thumbnail click action:", 13.0, 96.0, 116.0, 13.0),
        c(K::Lbl, "Thumbnail size:", 13.0, 72.0, 80.0, 13.0),
        c(K::Combo, "", 176.0, 44.0, 160.0, 21.0),
        c(K::Lbl, "Title location:", 13.0, 48.0, 70.0, 13.0),
        c(K::Chk(false), "Show title", 16.0, 24.0, 72.0, 17.0),
    ]),
    c(K::Chk(false), "Show menu", 16.0, 16.0, 82.0, 17.0),
    c(K::Combo, "", 176.0, 36.0, 160.0, 21.0),
    c(K::Lbl, "Task view mode:", 13.0, 40.0, 88.0, 13.0),
];

/// `tpClipboardFormats`.
const CLIPBOARD: &[C] = &[
    c(K::Lbl, "These formats will appear under \"Copy\" sub-menu in the main window context menu.", 13.0, 16.0, 406.0, 13.0),
    c(K::Btn, "Edit...", 120.0, 40.0, 96.0, 23.0),
    c(K::Btn, "Remove", 224.0, 40.0, 96.0, 23.0),
    c(K::Btn, "Add...", 16.0, 40.0, 96.0, 23.0),
    // the only anchored control on the form (`Top, Bottom, Left, Right`): the
    // designer sized it 520 x 288 for a 556 x 376 page, and the page it really
    // gets is 559 x 402.
    c(K::List(&[("Description", 135.0), ("Format", 320.0)], true), "", 16.0, 72.0, 523.0 + PAGE_EXTRA, 314.0),
];

/// `tpUpload`.
const UPLOAD: &[C] = &[
    // 168 wide in the resx, 180 here so DejaVu's caption fits, and the two
    // boxes to its left push it 24 points further right; see `PAGE_EXTRA`.
    g("Secondary file uploaders", 392.0, 184.0, 180.0, 208.0, &[
        c(K::List(&[("", 60.0)], false), "", 3.0, 18.0, 174.0, 187.0),
    ]),
    c(K::Lbl, "Simultaneous upload limit:", 13.0, 16.0, 128.0, 13.0),
    g("Secondary image uploaders", 16.0, 184.0, 180.0, 208.0, &[
        c(K::List(&[("", 60.0)], false), "", 3.0, 18.0, 174.0, 187.0),
    ]),
    g("Secondary text uploaders", 204.0, 184.0, 180.0, 208.0, &[
        c(K::List(&[("", 60.0)], false), "", 3.0, 18.0, 174.0, 187.0),
    ]),
    c(K::Num, "", 16.0, 32.0, 56.0, 20.0),
    c(K::Chk(false), "Use secondary uploaders order of preference when retrying", 16.0, 160.0, 305.0, 17.0),
    c(K::Lbl, "0 - 25 (0 disables)", 80.0, 36.0, 90.0, 13.0),
    c(K::Lbl, "Number of times to retry if upload fails:", 13.0, 112.0, 185.0, 13.0),
    c(K::Lbl, "Buffer size:", 13.0, 64.0, 59.0, 13.0),
    c(K::Num, "", 16.0, 128.0, 56.0, 20.0),
    c(K::Combo, "", 16.0, 80.0, 76.0, 21.0),
];

/// `tpHistory`.
const HISTORY: &[C] = &[
    g("History", 8.0, 8.0, 536.0, 80.0, &[
        c(K::Chk(false), "Only save if URL is not empty", 16.0, 48.0, 165.0, 17.0),
        c(K::Chk(false), "Save tasks to history", 16.0, 24.0, 124.0, 17.0),
    ]),
    g("Recent tasks", 8.0, 96.0, 536.0, 176.0, &[
        c(K::Chk(false), "In tray menu show most recent tasks first", 16.0, 144.0, 217.0, 17.0),
        c(K::Lbl, "Maximum number of tasks to save:", 13.0, 48.0, 170.0, 13.0),
        c(K::Num, "", 16.0, 64.0, 56.0, 20.0),
        c(K::Chk(false), "Show recent tasks in tray menu", 16.0, 120.0, 174.0, 17.0),
        c(K::Chk(false), "Show recent tasks in main window on startup", 16.0, 96.0, 239.0, 17.0),
        c(K::Chk(false), "Save recent tasks", 16.0, 24.0, 112.0, 17.0),
    ]),
];

/// `tpPrint`.
const PRINT: &[C] = &[
    c(K::Lbl, "Default printer override:", 13.0, 96.0, 117.0, 13.0),
    c(K::Edit, "", 16.0, 112.0, 352.0, 20.0),
    c(K::Chk(false), "Don't show Windows print dialog", 16.0, 72.0, 180.0, 17.0),
    c(K::Chk(false), "Don't show image print settings dialog", 16.0, 48.0, 203.0, 17.0),
    c(K::Btn, "Image print settings...", 16.0, 16.0, 208.0, 23.0),
];

/// `tpProxy`.
const PROXY: &[C] = &[
    c(K::Combo, "", 16.0, 32.0, 136.0, 21.0),
    c(K::Lbl, "Proxy configuration:", 13.0, 16.0, 100.0, 13.0),
    c(K::Lbl, "Host:", 13.0, 64.0, 32.0, 13.0),
    c(K::Edit, "", 16.0, 80.0, 232.0, 20.0),
    c(K::Num, "", 256.0, 80.0, 64.0, 20.0),
    c(K::Lbl, "Port:", 253.0, 64.0, 29.0, 13.0),
    c(K::Lbl, "Password:", 13.0, 160.0, 56.0, 13.0),
    c(K::Edit, "", 16.0, 176.0, 232.0, 20.0),
    c(K::Lbl, "Username:", 13.0, 112.0, 58.0, 13.0),
    c(K::Edit, "", 16.0, 128.0, 232.0, 20.0),
];

/// `tpAdvanced`.
const ADVANCED: &[C] = &[
    // `pgSettings` is docked `Fill`, so it takes the page less its 3 point
    // padding: 553 x 396, not the 550 x 370 the designer laid out at 556 x 376.
    c(K::Grid, "", 3.0, 3.0, 553.0 + PAGE_EXTRA, 396.0),
];

/// `tcSettings.Controls` in the designer's order, which is the order
/// `TabToTreeView` fills the tree in. The second field is the suffix
/// `--settings-shot` gives the page's PNG.
const TABS: &[(&str, &str, &[C])] = &[
    ("General", "general", GENERAL),
    ("Theme", "theme", THEME),
    ("Integration", "integration", INTEGRATION),
    ("Paths", "paths", PATHS),
    ("Settings", "settings", SETTINGS),
    ("Main window", "main-window", MAIN_WINDOW),
    ("Clipboard formats", "clipboard-formats", CLIPBOARD),
    ("Upload", "upload", UPLOAD),
    ("History", "history", HISTORY),
    ("Print", "print", PRINT),
    ("Proxy", "proxy", PROXY),
    ("Advanced", "advanced", ADVANCED),
];

// ------------------------------------------------------------------ drawing

/// The three-point arrow WinForms paints on a combo box, on a `MenuButton` and
/// on the two halves of a spin box.
fn arrow(p: &egui::Painter, c: Pos2, up: bool, col: Color32) {
    let s = if up { -1.0 } else { 1.0 };
    let pts = vec![
        c + vec2(-3.5, -2.0 * s),
        c + vec2(3.5, -2.0 * s),
        c + vec2(0.0, 2.0 * s),
    ];
    p.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
}

/// A sunken surface with the single-pixel border `ApplyTheme` puts on a text
/// box, a combo box or a property grid's view.
fn sunken(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 0, LIGHT_BG);
    p.rect_stroke(r, 0, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
}

/// One control and, if it is a group box, everything inside it. `o` is the
/// top-left of whatever holds it, in form coordinates.
fn control(p: &egui::Painter, o: Pos2, it: &C, grey: Color32) {
    let r = Rect::from_min_size(o + vec2(it.x, it.y), vec2(it.w, it.h));
    let f = font::font_id(FONT, false);
    match &it.kind {
        K::Lbl => {
            p.text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, it.text, f, grey);
        }
        K::Wrap => {
            let gal = p.layout(it.text.to_string(), f, grey, it.w);
            p.galley(pos2(r.left(), r.center().y - gal.size().y * 0.5), gal, grey);
        }
        K::Chk(on) => {
            let b = Rect::from_min_size(
                pos2(r.left(), r.center().y - BOX * 0.5),
                Vec2::splat(BOX),
            );
            // ShareX leaves the glyph itself to the OS, which paints it light
            // even under the dark theme; here it is drawn in the theme's own
            // colours and greyed, because every control on this form is inert.
            p.rect_filled(b, 0, LIGHT_BG);
            p.rect_stroke(b, 0, egui::Stroke::new(1.0, grey), egui::StrokeKind::Inside);
            if *on {
                let s = egui::Stroke::new(1.6, grey);
                p.line_segment([b.min + vec2(2.5, 6.5), b.min + vec2(5.0, 9.5)], s);
                p.line_segment([b.min + vec2(5.0, 9.5), b.min + vec2(10.5, 3.5)], s);
            }
            p.text(
                pos2(b.right() + BOX_GAP, r.center().y),
                Align2::LEFT_CENTER,
                it.text,
                f,
                grey,
            );
        }
        K::Btn => {
            p.rect_filled(r, 0, LIGHT_BG);
            p.rect_stroke(r, 0, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, it.text, f, grey);
        }
        K::MenuBtn => {
            // `TextAlign` is `MiddleLeft` on every one of them, and the menu
            // arrow sits at the right edge
            p.rect_filled(r, 0, LIGHT_BG);
            p.rect_stroke(r, 0, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            p.text(
                pos2(r.left() + 6.0, r.center().y),
                Align2::LEFT_CENTER,
                it.text,
                f,
                grey,
            );
            arrow(p, pos2(r.right() - 10.0, r.center().y), false, grey);
        }
        K::Combo => {
            sunken(p, r);
            let d = Rect::from_min_max(pos2(r.right() - DROP_W, r.top()), r.max);
            p.line_segment(
                [d.left_top() + vec2(0.0, 1.0), d.left_bottom() - vec2(0.0, 1.0)],
                egui::Stroke::new(1.0, BORDER),
            );
            arrow(p, d.center(), false, grey);
        }
        K::Edit => sunken(p, r),
        K::Num => {
            sunken(p, r);
            // the spin buttons sit on the control's own background, which
            // `ApplyTheme` leaves at `BackgroundColor`: a `NumericUpDown` is
            // not a `TextBox`, only the edit inside it is
            let s = Rect::from_min_max(pos2(r.right() - SPIN_W, r.top() + 1.0), r.max - vec2(1.0, 1.0));
            p.rect_filled(s, 0, BG);
            arrow(p, pos2(s.center().x, s.top() + s.height() * 0.28), true, grey);
            arrow(p, pos2(s.center().x, s.bottom() - s.height() * 0.28), false, grey);
        }
        K::Group => {
            // WinForms breaks the frame for the caption and starts it half a
            // line down from the top of the box
            let top = r.top() + FONT * 0.5;
            let gal = p.layout_no_wrap(it.text.to_string(), f, grey);
            let tw = gal.size().x;
            let st = egui::Stroke::new(1.0, SEL_BORDER);
            p.line_segment([pos2(r.left(), top), pos2(r.left() + 7.0, top)], st);
            p.line_segment([pos2(r.left() + 11.0 + tw, top), pos2(r.right(), top)], st);
            p.line_segment([pos2(r.left(), top), r.left_bottom()], st);
            p.line_segment([pos2(r.right(), top), r.right_bottom()], st);
            p.line_segment([r.left_bottom(), r.right_bottom()], st);
            p.galley(pos2(r.left() + 9.0, r.top()), gal, grey);
            for k in it.kids {
                control(p, r.min, k, grey);
            }
        }
        K::List(cols, head) => {
            p.rect_filled(r, 0, LIGHT_BG);
            if !*head {
                return;
            }
            p.rect_stroke(r, 0, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            let hr = Rect::from_min_size(r.min, vec2(r.width(), HEAD_H));
            p.rect_filled(hr, 0, BG);
            let hp = p.with_clip_rect(hr);
            // `AutoFillColumn` hands the leftover width to the last column
            let fixed: f32 = cols[..cols.len() - 1].iter().map(|c| c.1).sum();
            let mut x = r.left();
            for (n, (name, w)) in cols.iter().enumerate() {
                let w = if n + 1 == cols.len() {
                    (r.width() - fixed).max(*w)
                } else {
                    *w
                };
                hp.text(
                    pos2(x + 6.0, hr.center().y),
                    Align2::LEFT_CENTER,
                    *name,
                    font::font_id(FONT, false),
                    grey,
                );
                x += w;
                hp.line_segment(
                    [pos2(x, hr.top() + 3.0), pos2(x, hr.bottom() - 3.0)],
                    egui::Stroke::new(1.0, SEL_BORDER),
                );
            }
            p.line_segment(
                [hr.left_bottom(), hr.right_bottom()],
                egui::Stroke::new(1.0, BORDER),
            );
        }
        K::Grid => {
            // A `PropertyGrid` shows the properties of whatever object it is
            // given, and this shell has none to give: inventing rows would be
            // inventing ShareX's settings. What is drawn is the control's own
            // furniture — the tool strip, the view with `ViewBackColor` and
            // `ViewBorderColor`, the description pane with `HelpBackColor` —
            // left empty.
            p.rect_filled(r, 0, BG);
            let bar = Rect::from_min_size(r.min, vec2(r.width(), GRID_BAR_H));
            p.line_segment(
                [bar.left_bottom(), bar.right_bottom()],
                egui::Stroke::new(1.0, BORDER),
            );
            let help = Rect::from_min_max(pos2(r.left(), r.bottom() - GRID_HELP_H), r.max);
            p.rect_stroke(help, 0, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            let view = Rect::from_min_max(bar.left_bottom(), pos2(r.right(), help.top() - 4.0));
            sunken(p, view);
        }
        K::Warn => {
            // ShareX's `exclamation` image, drawn rather than copied: the
            // repository it comes from is GPL and this one is MIT.
            let cx = r.center();
            p.circle_filled(cx, r.width() * 0.44, grey);
            p.text(
                cx,
                Align2::CENTER_CENTER,
                "!",
                font::font_id(r.height() * 0.7, true),
                BG,
            );
        }
    }
}

/// The dark theme, so the shell and the region editor read as one program.
fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.panel_fill = BG;
        s.visuals.window_fill = BG;
        s.visuals.extreme_bg_color = LIGHT_BG;
        s.visuals.override_text_color = Some(TEXT);
        // `gray_out` tints towards this, which is what turns a label into a
        // disabled label; pointing it at the form background gives the WinForms
        // grey
        s.visuals.widgets.noninteractive.weak_bg_fill = BG;
    });
}

// -------------------------------------------------------------- the window

pub struct SettingsWindow {
    /// Index into `TABS` of the page the tree has selected.
    tab: usize,
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self { tab: 0 }
    }

    /// `form` is the client area, which the caller knows: the live window hands
    /// over its root `Ui`, the screenshot mode a rectangle of the form's own
    /// size.
    pub fn ui(&mut self, ctx: &egui::Context, form: Rect) {
        style(ctx);
        egui::Area::new(egui::Id::new("settings"))
            .order(egui::Order::Background)
            .fixed_pos(form.min)
            .show(ctx, |ui: &mut egui::Ui| {
                let (all, _) = ui.allocate_exact_size(form.size(), Sense::hover());
                ui.painter().rect_filled(all, 0, BG);
                let grey = ui.visuals().gray_out(TEXT);
                let tree = Rect::from_min_size(all.min, vec2(TREE_W, all.height()));
                self.tree(ui, tree, grey);
                let page = Rect::from_min_max(pos2(all.min.x + PAGE_X, all.min.y), all.max);
                let p = ui.painter().with_clip_rect(page);
                for it in TABS[self.tab].2 {
                    control(&p, page.min, it, grey);
                }
            });
    }

    /// `pLeft` and the `tvMain` inside it: one row per tab page, ended on the
    /// right by `pSeparator`. Selecting a row is the only thing on this form
    /// that answers a click.
    fn tree(&mut self, ui: &mut egui::Ui, tree: Rect, grey: Color32) {
        let p = ui.painter().with_clip_rect(tree);
        p.rect_filled(tree, 0, DARK_BG);
        p.line_segment(
            [tree.right_top(), tree.right_bottom()],
            egui::Stroke::new(1.0, BORDER),
        );
        for (i, (name, _, _)) in TABS.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(tree.left() + TREE_PAD, tree.top() + TREE_PAD + i as f32 * ROW_H),
                vec2(tree.width() - TREE_PAD * 2.0, ROW_H),
            );
            // `FullRowSelect` is on and `HideSelection` off; the real control
            // paints the selection with the system highlight, which the shell
            // swaps for the editor's own, so the two windows match
            if i == self.tab {
                p.rect_filled(r, 0, SEL_BG);
                p.rect_stroke(r, 0, egui::Stroke::new(1.0, SEL_BORDER), egui::StrokeKind::Inside);
            }
            p.text(
                pos2(r.left() + INDENT, r.center().y),
                Align2::LEFT_CENTER,
                *name,
                font::font_id(TREE_FONT, false),
                grey,
            );
            let resp = ui.interact(r, egui::Id::new(("tab", i)), Sense::click());
            if resp.clicked() {
                self.tab = i;
            }
            if resp.hovered() && i != self.tab {
                p.rect_filled(r, 0, HOVER_BG);
            }
        }
    }
}

impl eframe::App for SettingsWindow {
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
            .with_title("ShareX - Application settings"),
        ..Default::default()
    };
    eframe::run_native(
        "sxr",
        opts,
        Box::new(move |cc| {
            font::install(&cc.egui_ctx);
            Ok(Box::new(SettingsWindow::new()) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

/// `--settings-shot`: the shell as the window opens at the given path, then one
/// PNG per tab page beside it, so all twelve can be held next to the real
/// ShareX without starting a window.
pub fn shot(path: &str) -> Result<()> {
    let stem = path.strip_suffix(".png").unwrap_or(path);
    one(path, 0)?;
    for (i, (_, slug, _)) in TABS.iter().enumerate() {
        one(&format!("{stem}-{slug}.png"), i)?;
    }
    Ok(())
}

fn one(path: &str, tab: usize) -> Result<()> {
    let ctx = egui::Context::default();
    font::install(&ctx);
    let mut w = SettingsWindow::new();
    w.tab = tab;
    let form = Rect::from_min_size(Pos2::ZERO, vec2(FORM_W, FORM_H));
    shot_ctx(path, FORM_W as u32, FORM_H as u32, &ctx, |c| {
        // the shot has no pointer, so the tab set above is what gets drawn
        w.ui(c, form);
    })
}
