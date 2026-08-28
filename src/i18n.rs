//! The interface language. English is the default — the English texts are the
//! ShareX ones wherever ShareX has an equivalent, since sxr is a clone of its
//! classic editor; where it has none (our own status messages), it is plain
//! English.
//!
//! The keys are typed, not strings: there is no lookup at runtime and no
//! misspelled key that gets past the compiler. The table below generates the
//! `enum Msg`, the `Msg::ALL` list and both exhaustive `match`es on the language
//! alike, so a key without a translation simply cannot be written.
//!
//! Texts with values inserted into them (paths, sizes, errors) are not keys but
//! functions returning `String` — see the bottom of the file.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::config;

// ---------------------------------------------------------------- language

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ro,
}

impl Lang {
    /// The languages we offer, in the order the language selector shows them.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ro];

    /// The code written in the config file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ro => "ro",
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "ro" => Some(Lang::Ro),
            _ => None,
        }
    }

    /// The language's name in itself, as in any language selector: it is not
    /// translated, so it looks the same whatever the current language is.
    pub fn label(self) -> Msg {
        match self {
            Lang::En => Msg::LangEnglish,
            Lang::Ro => Msg::LangRomanian,
        }
    }
}

/// The current language, kept global so it need not thread through every
/// signature. Reading it is a single relaxed `load`: it is called dozens of
/// times per frame.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The key in the config file.
pub const LANG_KEY: &str = "lang";

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 { Lang::Ro } else { Lang::En }
}

/// Changes the language in memory only. It takes effect from the next frame.
pub fn set_lang(l: Lang) {
    CURRENT.store(if l == Lang::Ro { 1 } else { 0 }, Ordering::Relaxed);
}

/// Reads the language from the config file. With no file, a broken file or an
/// unknown code it stays English — the system's `LANG` does not count.
pub fn init() {
    let l = config::get(LANG_KEY)
        .and_then(|v| Lang::from_code(&v))
        .unwrap_or(Lang::En);
    set_lang(l);
}

/// Changes the language and remembers it. A failed write does not stop the
/// change: the interface is translated anyway, only a restart brings back the
/// saved one.
pub fn set_lang_saved(l: Lang) {
    set_lang(l);
    config::set(LANG_KEY, l.code());
}

/// The key's text in the current language.
pub fn t(m: Msg) -> &'static str {
    m.text(lang())
}

// ------------------------------------------------------------------- keys

macro_rules! messages {
    ($( $name:ident => ($en:expr, $ro:expr) ),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Msg { $($name),* }

        impl Msg {
            /// All the keys, in table order. `--i18n-check` walks it.
            pub const ALL: &'static [Msg] = &[ $(Msg::$name),* ];

            /// The key's name, for the check reports.
            pub fn key(self) -> &'static str {
                match self { $(Msg::$name => stringify!($name)),* }
            }

            /// The text in a given language, whatever the current one is.
            pub fn text(self, l: Lang) -> &'static str {
                match l {
                    Lang::En => match self { $(Msg::$name => $en),* },
                    Lang::Ro => match self { $(Msg::$name => $ro),* },
                }
            }
        }
    };
}

messages! {
    // ---- the window
    WindowTitle => ("sxr — image editor", "sxr — editor"),

    // ---- the action buttons in the toolbar
    TipApply     => ("Apply changes & close (Enter)", "Aplică și închide (Enter)"),
    TipSave      => ("Save image (Ctrl + S)", "Salvează (Ctrl+S)"),
    TipSaveAs    => ("Save image as... (Ctrl + Shift + S)", "Salvează ca... (Ctrl+Shift+S)"),
    TipCopy      => ("Copy image to clipboard (Ctrl + C)", "Copiază în clipboard (Ctrl+C)"),
    TipUpload    => ("Upload image (Ctrl + U)", "Încarcă (Ctrl+U)"),
    TipPrint     => ("Print image... (Ctrl + P)", "Tipărește (Ctrl+P)"),

    // ---- the color buttons in the toolbar
    TipBorderColor    => ("Border color", "Culoare contur"),
    TipFillColor      => ("Fill color", "Culoare umplere"),
    TipHighlightColor => ("Highlight color", "Culoare evidențiere"),

    // ---- the tools, in toolbar order (ShareX: `ShapeType`)
    ToolSelect         => ("Select and move (M)", "Mutare și redimensionare (M)"),
    ToolRect           => ("Rectangle (R)", "Dreptunghi (R)"),
    ToolEllipse        => ("Ellipse (E)", "Elipsă (E)"),
    ToolFreehand       => ("Freehand (F)", "Desen liber (F)"),
    ToolFreehandArrow  => ("Freehand arrow", "Săgeată liberă"),
    ToolLine           => ("Line (L)", "Linie (L)"),
    ToolArrow          => ("Arrow (A)", "Săgeată (A)"),
    ToolTextOutline    => ("Text (Outline) (O)", "Text (contur) (O)"),
    ToolTextBackground => ("Text (Background) (T)", "Text (fundal) (T)"),
    ToolSpeechBalloon  => ("Speech balloon (S)", "Balon de dialog (S)"),
    ToolStep           => ("Step (I)", "Numărător de pași (I)"),
    ToolMagnify        => ("Magnify", "Lupă"),
    ToolImageFile      => ("Image (File)", "Imagine (fișier)"),
    ToolImageScreen    => ("Image (Screen)", "Imagine (ecran)"),
    ToolSticker        => ("Sticker", "Sticker"),
    ToolCursor         => ("Cursor", "Cursor"),
    ToolSmartEraser    => ("Smart eraser", "Gumă inteligentă"),
    ToolBlur           => ("Blur (B)", "Blur (B)"),
    ToolPixelate       => ("Pixelate (P)", "Pixelare (P)"),
    ToolHighlight      => ("Highlight (H)", "Evidențiere (H)"),
    ToolSpotlight      => ("Spotlight", "Reflector"),
    ToolCrop           => ("Crop image (C)", "Decupare (C)"),
    ToolCutOut         => ("Cut out (X)", "Tăiere fâșie (X)"),

    // ---- the "Tool options" menu
    MenuToolOptions   => ("Tool options", "Opțiuni unealtă"),
    SldBorderSize     => ("Border size", "Grosime contur"),
    SldCornerRadius   => ("Corner radius", "Rază colț"),
    SldCenterPoints   => ("Center points", "Puncte de curbură linie"),
    SldPixelSize      => ("Pixel size", "Dimensiune pixelare"),
    SldBlurStrength   => ("Blur strength", "Rază blur"),
    SldMagnifyStrength=> ("Magnify strength", "Putere lupă"),
    SldFontSize       => ("Font size", "Dimensiune font text"),
    SldStepFontSize   => ("Step font size", "Dimensiune font numărător"),
    SldStepStart      => ("Value of first step", "Valoare de pornire numărător"),
    ChkDropShadow     => ("Drop shadow", "Umbră"),
    LangSelector      => ("Language / Limbă", "Language / Limbă"),
    LangEnglish       => ("English", "English"),
    LangRomanian      => ("Română", "Română"),

    // ---- the "Edit" menu
    MenuEdit     => ("Edit", "Editare"),
    ItUndo       => ("Undo (Ctrl+Z)", "Anulează (Ctrl+Z)"),
    ItRedo       => ("Redo (Ctrl+Y)", "Refă (Ctrl+Y)"),
    ItDuplicate  => ("Duplicate (Ctrl+D)", "Duplică (Ctrl+D)"),
    ItDelete     => ("Delete (Delete)", "Șterge (Delete)"),
    ItDeleteAll  => ("Delete all (Shift+Delete)", "Șterge tot (Shift+Delete)"),
    ItToFront    => ("Bring to front (Home)", "Adu în față (Home)"),
    ItForward    => ("Bring forward (PageUp)", "Adu mai în față (PageUp)"),
    ItBackward   => ("Send backward (PageDown)", "Trimite mai în spate (PageDown)"),
    ItToBack     => ("Send to back (End)", "Trimite în spate (End)"),

    // ---- the "Image" menu
    MenuImage        => ("Image", "Imagine"),
    ItNewImage       => ("New image...", "Imagine nouă"),
    ItOpenImage      => ("Open image file...", "Deschide fișier imagine"),
    ItInsertFile     => ("Insert image file...", "Inserează fișier imagine"),
    ItInsertScreen   => ("Insert image from screen...", "Inserează imagine din ecran"),
    ItImageSize      => ("Image size...", "Dimensiune imagine"),
    ItCanvasSize     => ("Canvas size...", "Dimensiune pânză"),
    ItCropImage      => ("Crop image...", "Decupează imaginea"),
    ItAutoCrop       => ("Auto crop image...", "Decupare automată"),
    ItRotateRight    => ("Rotate 90° clockwise", "Rotește 90° dreapta"),
    ItRotateLeft     => ("Rotate 90° counter clockwise", "Rotește 90° stânga"),

    // ---- the size dialogs
    DlgNewTitle    => ("New image", "Imagine nouă"),
    DlgSizeTitle   => ("Image size", "Dimensiune imagine"),
    DlgCanvasTitle => ("Canvas size", "Dimensiune pânză"),
    BtnCreate      => ("Create", "Creează"),
    BtnOk          => ("OK", "OK"),
    BtnCancel      => ("Cancel", "Renunță"),
    LblWidth       => ("Width", "Lățime"),
    LblHeight      => ("Height", "Înălțime"),
    LblAspect      => ("Aspect ratio", "Proporții"),
    ChkKeepAspect  => ("Maintain aspect ratio", "Păstrează proporțiile"),
    LblBackground  => ("Background", "Fundal"),
    LblCanvasFill  => ("Canvas color", "Umplere"),

    // ---- the text input dialog
    // the title follows the ShareX form: "ShareX - Text input"
    DlgTextTitle       => ("sxr - Text input", "sxr - Introducere text"),
    LblFont            => ("Font:", "Font:"),
    LblTextSize        => ("Size:", "Dimensiune:"),
    TipTextColor       => ("Text color", "Culoarea textului"),
    TipBold            => ("Bold", "Aldin"),
    TipItalic          => ("Italic", "Cursiv"),
    TipUnderline       => ("Underline", "Subliniat"),
    TipAlignHoriz      => ("Horizontal alignment", "Aliniere pe orizontală"),
    TipAlignVert       => ("Vertical alignment", "Aliniere pe verticală"),
    // the bottom-left button (`btnSwapEnterKey`) toggles between the two hints
    TextInputHint      => ("New line: Ctrl + Enter, OK: Enter", "Rând nou: Ctrl + Enter, OK: Enter"),
    TextInputHintSwap  => ("New line: Enter, OK: Ctrl + Enter", "Rând nou: Enter, OK: Ctrl + Enter"),
    TipSwapEnterKey    => ("Swap Enter key behavior", "Schimbă rolul tastei Enter"),
    AlignLeft          => ("Left", "Stânga"),
    AlignCenter        => ("Center", "Centru"),
    AlignRight         => ("Right", "Dreapta"),
    AlignTop           => ("Top", "Sus"),
    AlignMiddle        => ("Middle", "Mijloc"),
    AlignBottom        => ("Bottom", "Jos"),

    // ---- the sticker picker dialog (ShareX: StickerForm)
    // the original title is "ShareX - Sticker picker"; our app has another name
    DlgStickerTitle  => ("Sticker picker", "Alegere sticker"),
    LblSearch        => ("Search:", "Caută:"),
    LblStickerPack   => ("Stickers:", "Stickere:"),
    LblStickerSize   => ("Size:", "Dimensiune:"),
    // the root of the sticker folder, the one with no pack around it
    StickerAllPacks  => ("All stickers", "Toate stickerele"),
    TipStickerFolder => ("Open the sticker folder", "Deschide folderul cu stickere"),
    StNoStickerMatch => ("no sticker matches the search", "niciun sticker nu se potrivește"),

    // ---- the status bar, the messages with no inserted values
    StNothingToUndo   => ("nothing to undo", "nimic de anulat"),
    StNothingToRedo   => ("nothing to redo", "nimic de refăcut"),
    StEmptyImage      => ("the image is empty", "imaginea e goală"),
    StSelectRegion    => ("select the region on the screen", "selectează regiunea de pe ecran"),
    StNoShapeSelected => ("no shape selected", "nicio formă selectată"),
    StNoUniformEdges  => ("there are no uniform edges to crop", "nu există margini uniforme de tăiat"),
    StCopied          => ("✓ copied to clipboard", "✓ copiat în clipboard"),
    StUploadMissing   => ("uploading is not implemented", "încărcarea nu e implementată"),
    StPrintMissing    => ("printing is not included in sxr", "tipărirea nu e inclusă în sxr"),
    StDragToCrop      => ("drag a rectangle to crop", "trage un dreptunghi ca să decupezi"),
    StFlattened       => ("the shapes were applied to the image", "formele au fost aplicate pe imagine"),

    // ---- the command line (`-h` / `--help`)
    HelpTitle  => ("sxr — capture and annotate", "sxr — captură și adnotare"),
    HelpRegion => (
        "  sxr              select a region on the screen, then open the editor",
        "  sxr              selectează o regiune de pe ecran, apoi deschide editorul"
    ),
    HelpFile   => (
        "  sxr <file>       open an existing image directly",
        "  sxr <fișier>     deschide direct o imagine existentă"
    ),
}

// ----------------------------------------- texts with inserted values

// We have no formatting engine: every text with holes in it is a function that
// takes exactly what goes into them and returns a `String`.

/// The chosen tool is in the toolbar, but it does not draw anything yet.
pub fn tool_not_ready(tool: &str) -> String {
    match lang() {
        Lang::En => format!("the {tool} tool is not implemented yet"),
        Lang::Ro => format!("unealta {tool} nu e implementată încă"),
    }
}

pub fn image_inserted(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("image inserted ({w}x{h})"),
        Lang::Ro => format!("imagine inserată ({w}x{h})"),
    }
}

pub fn cannot_open(path: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!("cannot open {path}: {err}"),
        Lang::Ro => format!("nu pot deschide {path}: {err}"),
    }
}

pub fn cannot_decode_cursor(err: &str) -> String {
    match lang() {
        Lang::En => format!("cursor.png cannot be decoded: {err}"),
        Lang::Ro => format!("cursor.png nu se poate decoda: {err}"),
    }
}

pub fn cannot_create(path: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!("cannot create {path}: {err}"),
        Lang::Ro => format!("nu pot crea {path}: {err}"),
    }
}

pub fn put_png_files_in(path: &str) -> String {
    match lang() {
        Lang::En => format!("put PNG files in {path}"),
        Lang::Ro => format!("pune fișiere PNG în {path}"),
    }
}

pub fn cannot_open_folder(path: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!("cannot open the folder {path}: {err}"),
        Lang::Ro => format!("nu pot deschide folderul {path}: {err}"),
    }
}

pub fn capture_cancelled(err: &str) -> String {
    match lang() {
        Lang::En => format!("capture cancelled: {err}"),
        Lang::Ro => format!("captură anulată: {err}"),
    }
}

pub fn cropped_to(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("cropped to {w}x{h}"),
        Lang::Ro => format!("decupat la {w}x{h}"),
    }
}

pub fn cut_out_to(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("cut out to {w}x{h}"),
        Lang::Ro => format!("tăiat la {w}x{h}"),
    }
}

pub fn cannot_apply_shapes(err: &str) -> String {
    match lang() {
        Lang::En => format!("cannot apply the shapes: {err}"),
        Lang::Ro => format!("nu pot aplica formele: {err}"),
    }
}

pub fn new_image(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("new image {w}x{h}"),
        Lang::Ro => format!("imagine nouă {w}x{h}"),
    }
}

pub fn opened(path: &str, w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("opened {path} ({w}x{h})"),
        Lang::Ro => format!("deschis {path} ({w}x{h})"),
    }
}

pub fn resized_to(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("resized to {w}x{h}"),
        Lang::Ro => format!("redimensionat la {w}x{h}"),
    }
}

pub fn canvas_to(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("canvas {w}x{h}"),
        Lang::Ro => format!("pânză {w}x{h}"),
    }
}

pub fn auto_cropped_to(w: u32, h: u32) -> String {
    match lang() {
        Lang::En => format!("auto cropped to {w}x{h}"),
        Lang::Ro => format!("decupat automat la {w}x{h}"),
    }
}

pub fn rotated(right: bool, w: u32, h: u32) -> String {
    match lang() {
        Lang::En if right => format!("rotated 90° clockwise to {w}x{h}"),
        Lang::En => format!("rotated 90° counter clockwise to {w}x{h}"),
        Lang::Ro if right => format!("rotit 90° dreapta la {w}x{h}"),
        Lang::Ro => format!("rotit 90° stânga la {w}x{h}"),
    }
}

/// The status message, with the note about flattening appended after it.
pub fn with_flattened(msg: &str) -> String {
    format!("{msg} · {}", t(Msg::StFlattened))
}

pub fn copy_failed(err: &str) -> String {
    match lang() {
        Lang::En => format!("✗ copy failed: {err}"),
        Lang::Ro => format!("✗ copiere eșuată: {err}"),
    }
}

pub fn saved_to(path: &str) -> String {
    match lang() {
        Lang::En => format!("✓ saved: {path}"),
        Lang::Ro => format!("✓ salvat: {path}"),
    }
}

pub fn save_failed(err: &str) -> String {
    match lang() {
        Lang::En => format!("✗ save failed: {err}"),
        Lang::Ro => format!("✗ salvare eșuată: {err}"),
    }
}

pub fn auto_copy_failed(err: &str) -> String {
    match lang() {
        Lang::En => format!("auto-copy failed: {err}"),
        Lang::Ro => format!("auto-copy eșuat: {err}"),
    }
}

// ---- the messages coming out of font reading (they land in the status bar too)

pub fn fc_list_failed(status: &str) -> String {
    match lang() {
        Lang::En => format!("fc-list failed (code {status})"),
        Lang::Ro => format!("fc-list a eșuat (cod {status})"),
    }
}

pub fn fc_list_missing(err: &str) -> String {
    match lang() {
        Lang::En => format!("fc-list is missing: {err}"),
        Lang::Ro => format!("fc-list lipsește: {err}"),
    }
}

pub fn family_missing(family: &str, fallback: &str) -> String {
    match lang() {
        Lang::En => format!("font family “{family}” does not exist; using {fallback}"),
        Lang::Ro => format!("familia „{family}” nu există; folosesc {fallback}"),
    }
}

pub fn cannot_read(path: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!("cannot read {path}: {err}"),
        Lang::Ro => format!("nu pot citi {path}: {err}"),
    }
}

pub fn family_no_variant(family: &str) -> String {
    match lang() {
        Lang::En => format!("“{family}” has no such variant; using the available style"),
        Lang::Ro => format!("„{family}” nu are varianta cerută; folosesc stilul disponibil"),
    }
}

pub fn family_load_failed(family: &str, err: &str, fallback: &str) -> String {
    match lang() {
        Lang::En => format!("“{family}” cannot be loaded ({err}); using {fallback}"),
        Lang::Ro => format!("„{family}” nu poate fi încărcat ({err}); folosesc {fallback}"),
    }
}

// --------------------------------------------------------------- checking

/// Hidden `--i18n-check` mode: walks every key, in both languages, and flags
/// what is worth a look by eye — empty text, English identical to Romanian
/// (sometimes legitimate: "OK") and Romanian diacritics left in the English
/// variant. Non-interactive, no window.
pub fn check() {
    const DIACRITICE: [char; 10] = ['ă', 'â', 'î', 'ș', 'ț', 'Ă', 'Â', 'Î', 'Ș', 'Ț'];

    println!("chei: {}", Msg::ALL.len());
    for l in Lang::ALL {
        let goale = Msg::ALL.iter().filter(|m| m.text(l).is_empty()).count();
        println!("{}: {} chei, {} goale", l.code(), Msg::ALL.len(), goale);
    }

    let goale: Vec<&Msg> = Msg::ALL
        .iter()
        .filter(|m| Lang::ALL.iter().any(|l| m.text(*l).is_empty()))
        .collect();
    println!("\ntext gol: {}", goale.len());
    for m in &goale {
        println!("  {}", m.key());
    }

    let egale: Vec<&Msg> = Msg::ALL
        .iter()
        .filter(|m| m.text(Lang::En) == m.text(Lang::Ro))
        .collect();
    println!("\nengleza identică cu româna: {}", egale.len());
    for m in &egale {
        println!("  {} = {:?}", m.key(), m.text(Lang::En));
    }

    let cu_diacritice: Vec<&Msg> = Msg::ALL
        .iter()
        .filter(|m| m.text(Lang::En).contains(DIACRITICE))
        .collect();
    println!("\ndiacritice românești în varianta engleză: {}", cu_diacritice.len());
    for m in &cu_diacritice {
        println!("  {} = {:?}", m.key(), m.text(Lang::En));
    }

    println!("\nfișier de configurare: {}", config::path().display());
    println!("limba curentă: {}", lang().code());
}
