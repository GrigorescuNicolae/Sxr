use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use eframe::egui::containers::menu::MenuButton;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Vec2};
use image::RgbaImage;

use crate::clip;
use crate::config;
use crate::font;
use crate::i18n::{self, Msg, t};
use crate::icons::Icons;
use crate::render;
use crate::shape::{self, Shape, Tool};

/// Toolbar width in points — the window never goes below it, otherwise the
/// menus on the right would fall outside the frame (Wayland ignores a resize
/// requested by the application after creation).
pub const BAR_W: f32 = 1112.0;

/// The resize node, like ShareX's `Resources/CircleNode.png`: a solid white
/// disc of 18px, in a 24px box. The outlined variant drawn by
/// `ResizeNode.OnDraw` is used only when `UseLightResizeNodes` is on, and that
/// option is off by default, so it is never seen under normal use.
const NODE: f32 = 18.0;
/// The side of the node box — and the radius within which the mouse grabs it.
const NODE_HIT: f32 = 24.0;

/// The active tool in the toolbar, as in ShareX: not a block of color, but a
/// discreet background plus a 1px border. The region editor's toolbar gets
/// `ToolStripDarkRenderer` (`ShareXResources.ApplyTheme`), that is
/// `DarkColorTable` over `ShareXTheme.DarkTheme`; from there come
/// `ButtonCheckedGradient*` = `MenuCheckBackgroundColor` = #333333 and
/// `ButtonSelectedBorder` = `MenuHighlightBorderColor` = #3F3F3F, and hover =
/// `MenuHighlightColor` = #2E2E2E. We take them as they are, not as a
/// difference from the background: our panel is darker than the ShareX bar
/// (#1B1B1B against #272727), so the transposed difference would give an
/// almost invisible background.
pub const SEL_BG: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const SEL_BORDER: Color32 = Color32::from_rgb(0x3F, 0x3F, 0x3F);
pub const HOVER_BG: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);

pub fn run(img: RgbaImage) -> Result<()> {
    let (w, h) = img.dimensions();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([
                (w as f32 + 24.0).clamp(BAR_W, 1750.0),
                (h as f32 + 108.0).min(980.0),
            ])
            .with_min_inner_size([BAR_W, 340.0])
            .with_title(t(Msg::WindowTitle)),
        ..Default::default()
    };
    eframe::run_native(
        "sxr",
        opts,
        Box::new(move |cc| Ok(Box::new(Editor::new(&cc.egui_ctx, img)) as Box<dyn eframe::App>)),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

/// Non-interactive measurement of the toolbar width, for `--i18n-check`: the
/// bar holds only icons and icon menus, so the language should not change it —
/// but `BAR_W` is the window's minimum width, so it is worth checking. egui
/// lays everything out on the CPU, so no window is needed.
pub fn bar_width(l: i18n::Lang) -> f32 {
    i18n::set_lang(l);
    let ctx = egui::Context::default();
    font::install(&ctx);
    let mut ed = Editor::new(&ctx, RgbaImage::new(16, 16));
    // the first frame loads the fonts and textures; the good measure comes after
    for _ in 0..3 {
        let _ = ctx.run_ui(Default::default(), |ui| {
            let icons = ed.icons.clone();
            let mut acts = Vec::new();
            ed.toolbar(ui, &icons, &mut acts);
        });
    }
    ed.bar_w
}

/// Non-interactive render of the text window, for `--dlg-test`: the same
/// `text_dialog_ui`, in an egui context that works only on the CPU — the
/// technique from `bar_width`, plus our own rasterizer for the triangles egui
/// emits. The background stays magenta, so the window edges can be measured.
pub fn text_dialog_shot(path: &str) -> Result<()> {
    shot(path, 660, 540, |ctx, ed| {
        let mut acts = Vec::new();
        ed.text_dialog_ui(ctx, &mut acts);
    })
}

/// The same render, but for the toolbar: `--bar-test`.
pub fn toolbar_shot(path: &str) -> Result<()> {
    shot(path, 1160, 60, |ctx, ed| {
        let icons = ed.icons.clone();
        let mut acts = Vec::new();
        // the panels need a `Ui`, which we do not have here, so we put the bar in
        // an area with the panel background, so it looks like it does in the app
        egui::Area::new("bar".into())
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let fill = ui.visuals().panel_fill;
                egui::Frame::new()
                    .fill(fill)
                    .inner_margin(6)
                    .show(ui, |ui| ed.toolbar(ui, &icons, &mut acts));
            });
    })
}

/// Rendering of a speech balloon on the PREVIEW path (`Shape::draw`, that is
/// `egui::Painter`), for `--balloon-test`. The project's rule is that preview
/// and export draw the same, so the mode writes both images, from the same
/// shape and over the same background, so they can be compared pixel by pixel
/// from outside.
pub fn balloon_shot(dir: &str) -> Result<()> {
    let (w, h) = (300u32, 220u32);
    let bg = RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
    let s = demo_balloon();

    shot(&format!("{dir}/balloon-preview.png"), w, h, |ctx, _| {
        // our own black background: `shot` would otherwise leave magenta, and the
        // comparison with the export needs exactly the same pixels under the shape
        egui::Area::new("balloon".into())
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let p = ui.painter();
                p.rect_filled(
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32, h as f32)),
                    egui::CornerRadius::ZERO,
                    Color32::BLACK,
                );
                s.draw(p, |q| q, 1.0, &RgbaImage::new(1, 1));
            });
    })?;

    let path = format!("{dir}/balloon-export.png");
    // no shadow: the canvas preview draws it separately, not in `Shape::draw`
    let png = render::compose_opts(&bg, std::slice::from_ref(&s), false)?;
    std::fs::write(&path, &png).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    println!("wrote {path}");
    Ok(())
}

/// A solid PNG, written on the spot for the check modes: the user's stickers
/// are never touched, not even for reading.
fn write_test_png(path: &Path, side: u32, rgb: [u8; 3]) -> Result<()> {
    let img = RgbaImage::from_pixel(side, side, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    img.save(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(())
}

/// The test tree of the check modes: the root with loose files, plus two
/// packs. Rebuilt from scratch on every run.
fn make_test_stickers(root: &Path, loose: &[&str]) -> Result<()> {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root)?;
    for (i, n) in loose.iter().enumerate() {
        let c = [
            (40 + i * 37 % 200) as u8,
            (200 - i * 53 % 180) as u8,
            (90 + i * 71 % 160) as u8,
        ];
        write_test_png(&root.join(n), 96, c)?;
    }
    for (pack, names) in [("animals", ["cat.png", "dog.png"]), ("symbols", ["check.png", "cross.png"])] {
        let dir = root.join(pack);
        std::fs::create_dir_all(&dir)?;
        for (i, n) in names.iter().enumerate() {
            write_test_png(&dir.join(n), 96, [220, (60 + i * 90) as u8, 40])?;
        }
    }
    Ok(())
}

/// Non-interactive render of the sticker window (`--sticker-shot`), on a test
/// folder in the temporary directory. The configuration is redirected to
/// `/tmp` as well, so the image comes out the same whatever size the user
/// picked last time — and so their own file stays untouched.
pub fn sticker_shot(path: &str) -> Result<()> {
    let tmp = std::env::temp_dir().join("sxr-sticker-shot");
    let cfg = tmp.join("config-home");
    std::fs::create_dir_all(&cfg)?;
    // SAFETY: there are no other threads yet that could read the environment
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &cfg) };
    let root = tmp.join("stickers");
    make_test_stickers(
        &root,
        &[
            "blobbanhammer.png",
            "blobconcernedreading.png",
            "check.png",
            "warning.png",
            "fire.png",
            "beacon.png",
            "heart.png",
            "ok.png",
            "laugh.png",
            "star.png",
            "sad.png",
            "victory.png",
        ],
    )?;
    shot(path, 700, 560, move |ctx, ed| {
        if ed.dialog.is_none() {
            ed.open_sticker(root.clone(), Pos2::new(10.0, 10.0), false);
        }
        let mut acts = Vec::new();
        ed.sticker_dialog_ui(ctx, &mut acts);
    })
}

/// Non-interactive check of the sticker window (`--sticker-flow`): it goes
/// through pack enumeration, filtering, Enter, the remembered size and the
/// actual insertion, without opening any window. The configuration goes to a
/// temporary directory, so the user's own file is not touched.
pub fn sticker_flow_test() -> Result<()> {
    let tmp = std::env::temp_dir().join("sxr-sticker-flow");
    let cfg = tmp.join("config-home");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&cfg)?;
    // SAFETY: there are no other threads yet that could read the environment
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &cfg) };
    println!("test configuration: {}", config::path().display());
    let root = tmp.join("stickers");
    make_test_stickers(&root, &["hedgehog.png", "whale.png", "Zebra.png"])?;
    let mut fail = 0usize;

    // 1. the packs: root first, then the subfolders in alphabetical order
    let packs = sticker_packs(&root);
    let names: Vec<&str> = packs.iter().map(|(n, _)| n.as_str()).collect();
    let ok = names == [t(Msg::StickerAllPacks), "animals", "symbols"];
    ck(&mut fail, ok, format!("1 packs: {names:?}"));

    // 2. a root with no loose files is not a pack any more
    let empty = tmp.join("stickers-empty");
    std::fs::create_dir_all(empty.join("one"))?;
    write_test_png(&empty.join("one").join("x.png"), 32, [10, 20, 30])?;
    let pg = sticker_packs(&empty);
    let p2: Vec<&str> = pg.iter().map(|(n, _)| n.as_str()).collect();
    let ok = p2 == ["one"];
    ck(&mut fail, ok, format!("2 root without loose files is not a pack: {p2:?}"));

    // a folder with no stickers in it is not a pack either: otherwise the tool
    // would think the folder is not empty and open a window on an empty grid
    std::fs::create_dir_all(empty.join("nothing"))?;
    std::fs::write(empty.join("nothing").join("readme.txt"), b"not a sticker")?;
    let pg = sticker_packs(&empty);
    let p2: Vec<&str> = pg.iter().map(|(n, _)| n.as_str()).collect();
    let ok = p2 == ["one"];
    ck(&mut fail, ok, format!("2 a folder without stickers is not a pack: {p2:?}"));

    let barren = tmp.join("stickers-barren");
    std::fs::create_dir_all(barren.join("nothing"))?;
    let pg = sticker_packs(&barren);
    ck(&mut fail, pg.is_empty(), format!("2 only empty folders means no pack: {}", pg.len()));

    // 3. the files of one pack, sorted
    let files = sticker_files(&root);
    ck(&mut fail, files.len() == 3, format!("3 root has {} files", files.len()));
    let sorted = files.windows(2).all(|w| w[0] <= w[1]);
    ck(&mut fail, sorted, format!("3 list is sorted: {sorted}"));

    // 4. filtering: over the whole path, case-insensitive
    let animals = sticker_files(&root.join("animals"));
    let h = sticker_filter(&animals, "CAT");
    let ok = h.len() == 1 && animals[h[0]].ends_with("cat.png");
    ck(&mut fail, ok, format!("4 CAT finds cat: {} results", h.len()));
    let h = sticker_filter(&animals, "animals");
    ck(&mut fail, h.len() == 2, format!("4 the pack name takes the whole pack: {}", h.len()));
    let h = sticker_filter(&animals, "zzz");
    ck(&mut fail, h.is_empty(), format!("4 search with no result: {}", h.len()));
    let h = sticker_filter(&animals, "   ");
    ck(&mut fail, h.len() == 2, format!("4 an empty search shows everything: {}", h.len()));

    // 5. after each key the first result is preselected, and Enter takes it
    let ctx = egui::Context::default();
    let mut ed = Editor::new(&ctx, RgbaImage::new(400, 300));
    ed.open_sticker(root.clone(), Pos2::new(40.0, 50.0), false);
    let d = ed.dialog.as_mut().expect("sticker window");
    d.sticker.sel = 2;
    d.sticker.query = "WHA".into();
    d.sticker.refilter();
    let sd = &d.sticker;
    let ok = sd.hits.len() == 1 && sd.sel == 0;
    ck(&mut fail, ok, format!("5 preselection: {} results, sel={}", sd.hits.len(), sd.sel));
    let first = sd.first_hit();
    let ok = first.as_deref().is_some_and(|p| p.ends_with("whale.png"));
    ck(&mut fail, ok, format!("5 Enter takes the first result: {first:?}"));

    // 6. the size: saved and read back; anything outside 16..=256 falls to 64
    let mut d = ed.dialog.take().expect("sticker window");
    d.sticker.size = 128.0;
    d.sticker.picked = first.clone();
    ed.sticker_done(&ctx, d.sticker, false);
    let v = sticker_size_saved();
    ck(&mut fail, v == 128.0, format!("6 the saved size is read back: {v}"));
    for (written, expected) in [("999", 64.0), ("0", 64.0), ("abc", 64.0), ("16", 16.0), ("256", 256.0)] {
        config::set(K_STICKER_SIZE, written);
        let v = sticker_size_saved();
        ck(&mut fail, v == expected, format!("6 size written {written} -> {v} (expected {expected})"));
    }
    ck(&mut fail, ed.shapes.is_empty(), format!("6 cancelling does not insert: {} shapes", ed.shapes.len()));

    // 7. the pack is remembered by name, not by index
    config::set(K_STICKER_PACK, "symbols");
    ed.open_sticker(root.clone(), Pos2::ZERO, false);
    let i = ed.dialog.as_ref().map(|d| d.sticker.pack).unwrap_or(9);
    ck(&mut fail, i == 2, format!("7 the saved pack is selected: {i}"));
    config::set(K_STICKER_PACK, "a-deleted-folder");
    ed.open_sticker(root.clone(), Pos2::ZERO, false);
    let i = ed.dialog.as_ref().map(|d| d.sticker.pack).unwrap_or(9);
    ck(&mut fail, i == 0, format!("7 the missing pack falls back to the first: {i}"));

    // 8. picking inserts a square of the requested side, at the click point
    ed.open_sticker(root.clone(), Pos2::new(40.0, 50.0), false);
    let mut d = ed.dialog.take().expect("sticker window");
    d.sticker.size = 48.0;
    d.sticker.picked = Some(root.join("hedgehog.png"));
    ed.sticker_done(&ctx, d.sticker, true);
    ck(&mut fail, ed.shapes.len() == 1, format!("8 after picking: {} shapes", ed.shapes.len()));
    match ed.shapes.first() {
        Some(Shape::Img { rect, .. }) => {
            let ok = (rect.min.x - 40.0).abs() < 0.01
                && (rect.min.y - 50.0).abs() < 0.01
                && (rect.width() - 48.0).abs() < 0.01
                && (rect.height() - 48.0).abs() < 0.01;
            ck(&mut fail, ok, format!("8 48px square at the click point: {rect:?}"));
        }
        _ => ck(&mut fail, false, "8 the inserted shape is not an image".into()),
    }

    // 9. the request from the menu recenters the shape on the image
    ed.delete_all();
    ed.open_sticker(root.clone(), Pos2::new(0.0, 0.0), true);
    let mut d = ed.dialog.take().expect("sticker window");
    d.sticker.size = 64.0;
    d.sticker.picked = Some(root.join("hedgehog.png"));
    ed.sticker_done(&ctx, d.sticker, true);
    match ed.shapes.first() {
        Some(Shape::Img { rect, .. }) => {
            let c = rect.center();
            let ok = (c.x - 200.0).abs() < 0.01 && (c.y - 150.0).abs() < 0.01;
            ck(&mut fail, ok, format!("9 centered on the image: {c:?}"));
        }
        _ => ck(&mut fail, false, "9 no shape after the menu request".into()),
    }

    // 10. cancelling, with a sticker already under the cursor, inserts nothing
    ed.delete_all();
    ed.open_sticker(root.clone(), Pos2::new(10.0, 10.0), false);
    let mut d = ed.dialog.take().expect("sticker window");
    d.sticker.picked = Some(root.join("hedgehog.png"));
    ed.sticker_done(&ctx, d.sticker, false);
    ck(&mut fail, ed.shapes.is_empty(), format!("10 cancel: {} shapes", ed.shapes.len()));

    if fail > 0 {
        anyhow::bail!("{fail} checks failed");
    }
    println!("all good");
    Ok(())
}

/// The balloon used by `--balloon-test` and `--balloon-flow`: solid colors and a
/// thick outline, so color measurements from outside do not land on antialiasing.
fn demo_balloon() -> Shape {
    Shape::Balloon {
        rect: Rect::from_min_max(Pos2::new(40.0, 30.0), Pos2::new(250.0, 130.0)),
        tail: Pos2::new(70.0, 200.0),
        text: "Hello".into(),
        opts: shape::TextOpts::default(),
        fill: Color32::from_rgb(0, 0, 255),
        border: Color32::from_rgb(0, 255, 0),
        width: 4.0,
        radius: 0.0,
    }
}

/// The result of one `--balloon-flow` check, on a single line.
fn ck(fail: &mut usize, ok: bool, msg: String) {
    if !ok {
        *fail += 1;
    }
    println!("{} {msg}", if ok { "OK" } else { "FAILED" });
}

/// Non-interactive check of the "speech balloon" tool (`--balloon-flow`): it
/// replays step by step what the mouse and the text window do on the canvas,
/// calling the editor's methods directly. Without it, the click → draft → text
/// window path could only be tried by hand, in a graphical session.
pub fn balloon_flow_test() -> Result<()> {
    let ctx = egui::Context::default();
    let mut ed = Editor::new(&ctx, RgbaImage::new(400, 300));
    let mut fail = 0usize;

    // 1. the ordinary drag: press, drag, release
    ed.pick(Tool::SpeechBalloon);
    ed.draft = ed.new_draft(Pos2::new(40.0, 30.0));
    ed.update_draft(Pos2::new(250.0, 130.0));
    ed.commit_draft();
    ck(&mut fail, ed.shapes.len() == 1, format!("1 drag: {} shapes", ed.shapes.len()));
    let is_balloon = matches!(ed.shapes.first(), Some(Shape::Balloon { .. }));
    ck(&mut fail, is_balloon, format!("1 the shape is a balloon: {is_balloon}"));
    let dlg = ed.dialog.as_ref().map(|d| d.kind == DlgKind::Text) == Some(true);
    ck(&mut fail, dlg, format!("1 text window open: {dlg}"));
    ck(&mut fail, ed.sel.is_none(), format!("1 sel while the window is open: {:?}", ed.sel));

    // 2. closing with text: the shape stays, the text lands in it, nodes return
    let mut d = ed.dialog.take().expect("text window");
    d.text.buf = "Hello".into();
    ed.text_done(d.text, true);
    let txt = matches!(ed.shapes.first(), Some(Shape::Balloon { text, .. }) if text == "Hello");
    ck(&mut fail, txt, format!("2 the text landed in the shape: {txt}"));
    ck(&mut fail, ed.shapes.len() == 1, format!("2 the shape stayed: {} shapes", ed.shapes.len()));
    ck(&mut fail, ed.sel == Some(0), format!("2 sel after OK: {:?}", ed.sel));

    // 3. cancelling on a freshly drawn shape: it goes, nothing else does
    ed.draft = ed.new_draft(Pos2::new(40.0, 160.0));
    ed.update_draft(Pos2::new(250.0, 260.0));
    ed.commit_draft();
    let d = ed.dialog.take().expect("text window");
    ed.text_done(d.text, false);
    let n = ed.shapes.len();
    ck(&mut fail, n == 1, format!("3 after cancelling {n} shapes remain"));
    let keep = matches!(ed.shapes.first(), Some(Shape::Balloon { text, .. }) if text == "Hello");
    ck(&mut fail, keep, format!("3 the earlier balloon is untouched: {keep}"));

    // 4. a plain click, without dragging: the degenerate box gets the
    // default size, exactly as text boxes do
    let click = matches!(Tool::SpeechBalloon, Tool::Step) || Tool::SpeechBalloon.is_text();
    ck(&mut fail, click, format!("4 a single click places the balloon: {click}"));
    let at = Pos2::new(40.0, 160.0);
    ed.draft = ed.new_draft(at);
    ed.commit_draft();
    ck(&mut fail, ed.shapes.len() == 2, format!("4 after the click: {} shapes", ed.shapes.len()));
    if let Some(Shape::Balloon { rect, .. }) = ed.shapes.get(1) {
        let ok = rect.width() > 12.0 && rect.height() > 12.0;
        ck(&mut fail, ok, format!("4 default box {}x{}", rect.width(), rect.height()));
    } else {
        ck(&mut fail, false, "4 the click left no balloon".into());
    }
    if let Some(d) = ed.dialog.take() {
        ed.text_done(d.text, false);
    }

    // 5. the nodes: 8 from the rectangle plus the tail tip
    let s = ed.shapes[0].clone();
    let n = s.handles().len();
    ck(&mut fail, n == 9, format!("5 handles: {n}"));
    let (r0, t0) = balloon_parts(&s);
    let mut s8 = s.clone();
    s8.move_handle(8, Pos2::new(300.0, 280.0));
    let (r8, t8) = balloon_parts(&s8);
    let only_tail = r8 == r0 && t8 == Pos2::new(300.0, 280.0);
    ck(&mut fail, only_tail, format!("5 handle 8 moves only the tail: {r8:?} / {t8:?}"));
    let mut s0 = s.clone();
    s0.move_handle(0, Pos2::new(10.0, 10.0));
    let (r1, t1) = balloon_parts(&s0);
    let only_box = r1 != r0 && t1 == t0;
    ck(&mut fail, only_box, format!("5 handle 0 moves only the box: {r1:?} / {t1:?}"));

    // 6. the tail placed at commit: below the bottom-left corner, with its base
    // on the bottom edge of the box and the tip right at the tail point
    let sub = (t0.y - r0.bottom() - 30.0).abs() < 0.01 && (t0.x - r0.left()).abs() < 0.01;
    let lb = r0.left_bottom();
    ck(&mut fail, sub, format!("6 tail {t0:?} below the bottom-left corner {lb:?}"));
    let tl = shape::balloon_tail(r0, t0);
    let base_ok = tl.base[0] != tl.base[1]
        && (tl.base[0].y - r0.bottom()).abs() < 0.01
        && (tl.base[1].y - r0.bottom()).abs() < 0.01;
    let (b0, b1) = (tl.base[0], tl.base[1]);
    ck(&mut fail, base_ok, format!("6 base on the bottom edge: {b0:?} - {b1:?}"));
    ck(&mut fail, tl.tip == t0, format!("6 tip is the tail itself: {:?}", tl.tip));

    if fail > 0 {
        anyhow::bail!("{fail} checks failed");
    }
    println!("all good");
    Ok(())
}

/// The box and the tail tip of a balloon, for the checks above.
fn balloon_parts(s: &Shape) -> (Rect, Pos2) {
    match s {
        Shape::Balloon { rect, tail, .. } => (*rect, *tail),
        _ => (Rect::NOTHING, Pos2::ZERO),
    }
}

fn shot(path: &str, sw: u32, sh: u32, mut draw: impl FnMut(&egui::Context, &mut Editor)) -> Result<()> {
    let ctx = egui::Context::default();
    font::install(&ctx);
    let mut ed = Editor::new(&ctx, RgbaImage::new(16, 16));
    shot_ctx(path, sw, sh, &ctx, |c| draw(c, &mut ed))
}

/// The rasterizer behind every `--*-shot` mode: it drives an egui context on
/// the CPU for a few frames and paints the triangles it emits into a PNG. The
/// context comes from the caller because the state a mode draws is usually
/// built from it (`Editor::new` wants one for its textures), and it has to be
/// the same context the frames run on.
pub fn shot_ctx(
    path: &str,
    sw: u32,
    sh: u32,
    ctx: &egui::Context,
    mut draw: impl FnMut(&egui::Context),
) -> Result<()> {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(sw as f32, sh as f32))),
        ..Default::default()
    };
    // texture id -> (width, height, premultiplied pixels)
    let mut tex: std::collections::HashMap<egui::TextureId, (usize, usize, Vec<Color32>)> =
        std::collections::HashMap::new();
    let mut prims = Vec::new();
    // the first frames load the fonts, place the window and finish its fade-in
    // (`Area::fade_in`); the good drawing comes only after them
    for _ in 0..16 {
        font::sync();
        ctx.begin_pass(input.clone());
        draw(ctx);
        let out = ctx.end_pass();
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                let src = match &delta.image {
                    egui::epaint::ImageData::Color(i) => i,
                };
                let [dw, dh] = src.size;
                match delta.pos {
                    None => {
                        tex.insert(*id, (dw, dh, src.pixels.clone()));
                    }
                    Some([ox, oy]) => {
                        if let Some((w, _, buf)) = tex.get_mut(id) {
                            for row in 0..dh {
                                for col in 0..dw {
                                    buf[(oy + row) * *w + ox + col] = src.pixels[row * dw + col];
                                }
                            }
                        }
                    }
                }
            }
        }
        prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    }

    let mut img = RgbaImage::from_pixel(sw, sh, image::Rgba([255, 0, 255, 255]));
    for cp in &prims {
        let egui::epaint::Primitive::Mesh(m) = &cp.primitive else { continue };
        let Some((tw, th, tp)) = tex.get(&m.texture_id) else { continue };
        for tri in m.indices.chunks_exact(3) {
            let v = [
                m.vertices[tri[0] as usize],
                m.vertices[tri[1] as usize],
                m.vertices[tri[2] as usize],
            ];
            let area = edge(v[0].pos, v[1].pos, v[2].pos);
            if area.abs() < 1e-6 {
                continue;
            }
            let xs = [v[0].pos.x, v[1].pos.x, v[2].pos.x];
            let ys = [v[0].pos.y, v[1].pos.y, v[2].pos.y];
            let lo = |a: [f32; 3], min: f32| a.iter().cloned().fold(f32::MAX, f32::min).max(min);
            let hi = |a: [f32; 3], max: f32| a.iter().cloned().fold(f32::MIN, f32::max).min(max);
            let x0 = lo(xs, cp.clip_rect.left().max(0.0)).floor() as i32;
            let x1 = hi(xs, cp.clip_rect.right().min(sw as f32)).ceil() as i32;
            let y0 = lo(ys, cp.clip_rect.top().max(0.0)).floor() as i32;
            let y1 = hi(ys, cp.clip_rect.bottom().min(sh as f32)).ceil() as i32;
            for y in y0.max(0)..y1.min(sh as i32) {
                for x in x0.max(0)..x1.min(sw as i32) {
                    let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let l0 = edge(v[1].pos, v[2].pos, p) / area;
                    let l1 = edge(v[2].pos, v[0].pos, p) / area;
                    let l2 = 1.0 - l0 - l1;
                    if l0 < -1e-4 || l1 < -1e-4 || l2 < -1e-4 {
                        continue;
                    }
                    let u = l0 * v[0].uv.x + l1 * v[1].uv.x + l2 * v[2].uv.x;
                    let w = l0 * v[0].uv.y + l1 * v[1].uv.y + l2 * v[2].uv.y;
                    let texel = sample(tp, *tw, *th, u, w);
                    let chan = |f: fn(&Color32) -> u8| {
                        l0 * f(&v[0].color) as f32
                            + l1 * f(&v[1].color) as f32
                            + l2 * f(&v[2].color) as f32
                    };
                    // both are premultiplied, so the multiplication is per channel
                    let src = [
                        chan(|c| c.r()) * texel[0] / 255.0,
                        chan(|c| c.g()) * texel[1] / 255.0,
                        chan(|c| c.b()) * texel[2] / 255.0,
                        chan(|c| c.a()) * texel[3] / 255.0,
                    ];
                    let inv = 1.0 - src[3] / 255.0;
                    let px = img.get_pixel_mut(x as u32, y as u32);
                    for k in 0..3 {
                        px.0[k] = (src[k] + px.0[k] as f32 * inv).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
    img.save(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    println!("wrote {path}");
    Ok(())
}

/// Twice the signed area of triangle `a b c`. Zero = the points are collinear.
fn edge(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Bilinear read from the texture atlas, with normalized coordinates.
fn sample(px: &[Color32], w: usize, h: usize, u: f32, v: f32) -> [f32; 4] {
    if w == 0 || h == 0 {
        return [0.0; 4];
    }
    let fx = (u * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
    let fy = (v * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let mut out = [0.0f32; 4];
    for (c, wt) in [
        (px[y0 * w + x0], (1.0 - tx) * (1.0 - ty)),
        (px[y0 * w + x1], tx * (1.0 - ty)),
        (px[y1 * w + x0], (1.0 - tx) * ty),
        (px[y1 * w + x1], tx * ty),
    ] {
        out[0] += c.r() as f32 * wt;
        out[1] += c.g() as f32 * wt;
        out[2] += c.b() as f32 * wt;
        out[3] += c.a() as f32 * wt;
    }
    out
}

/// Snapshot for undo. The image is cloned only on crop, so that we do not
/// copy a few MB for every line drawn.
struct Snap {
    shapes: Vec<Shape>,
    img: Option<RgbaImage>,
}

enum Drag {
    Move { last: Pos2 },
    Handle { idx: usize },
}

/// Action deferred by one frame: the minimize command has to reach the
/// compositor first, otherwise our window would show up in the capture.
enum Pending {
    /// Region capture, to be stamped at the given point. `center` = the request
    /// comes from the menu, so the shape is recentered on the image after insertion.
    Screen { at: Pos2, center: bool },
}

/// What the modal dialog opened over the canvas is asking for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DlgKind {
    New,
    Size,
    Canvas,
    /// The text input window (ShareX: TextDrawingInputBox).
    Text,
    /// The sticker picker window (ShareX: StickerForm).
    Sticker,
}

/// The text window's state. The numeric fields below have nothing to do with it.
#[derive(Default)]
struct TextDlg {
    /// The shape being edited.
    idx: usize,
    /// The shape has just been created: if we cancel, it disappears entirely.
    fresh: bool,
    /// Outlined text (not text with a background): decides where defaults are saved.
    outline: bool,
    buf: String,
    opts: shape::TextOpts,
    /// The outline, or the box background — ShareX's "secondary color".
    color2: Color32,
    /// First frame: we move the focus into the text area.
    focus: bool,
}

/// The sticker picker window's state. It lives exactly as long as the window:
/// the file list and the thumbnails uploaded to the GPU go away with it.
#[derive(Default)]
struct StickerDlg {
    /// The click point: that is where the sticker's top-left corner lands.
    at: Pos2,
    /// Request from the menu, not from a canvas click: the shape gets recentered.
    center: bool,
    /// The root in use — `stickers_dir()` in the app, another one in the checks.
    root: PathBuf,
    /// The packs found: (display name, folder), root first.
    packs: Vec<(String, PathBuf)>,
    /// The chosen pack, an index into `packs`.
    pack: usize,
    /// The chosen pack's files. Read once, on opening and on switching packs;
    /// filtering works on this list, not on disk.
    files: Vec<PathBuf>,
    /// Indices into `files`, in the order they are shown in the grid.
    hits: Vec<usize>,
    query: String,
    /// The selection, as an index into `hits`.
    sel: usize,
    /// The thumbnail side, which is also the side the sticker is inserted at.
    size: f32,
    /// The thumbnails already uploaded. `None` = a file that does not decode.
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    /// First frame: the focus goes into the search field.
    focus: bool,
    /// The chosen sticker; without it, closing inserts nothing.
    picked: Option<PathBuf>,
}

impl StickerDlg {
    /// Reads the chosen pack and rebuilds the filtered list.
    fn reload(&mut self) {
        self.files = self
            .packs
            .get(self.pack)
            .map(|(_, p)| sticker_files(p))
            .unwrap_or_default();
        // the old thumbnails belong to another pack: no reason to keep them in the GPU
        self.thumbs.clear();
        self.refilter();
    }

    /// Rebuilds the filtered list. The first result stays preselected, as in
    /// ShareX: after each key, Enter already has something to fall on.
    fn refilter(&mut self) {
        self.hits = sticker_filter(&self.files, &self.query);
        self.sel = 0;
    }

    /// The first search result — what Enter picks.
    fn first_hit(&self) -> Option<PathBuf> {
        self.files.get(*self.hits.first()?).cloned()
    }

    /// The chosen pack's folder, the one the gear button opens.
    fn pack_dir(&self) -> PathBuf {
        self.packs
            .get(self.pack)
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| self.root.clone())
    }
}

/// Our own dialog: kdialog has neither numeric fields nor a text window.
struct Dialog {
    kind: DlgKind,
    w: u32,
    h: u32,
    /// The background of the new image, or the fill of the added canvas.
    color: Color32,
    /// "Keep aspect ratio" — only for Image size.
    keep: bool,
    /// Height / width at opening: recomputation always starts from here, so the
    /// ratio does not drift after several changes.
    ratio: f32,
    /// The values from the previous frame, so we know which side the user changed.
    last_w: u32,
    last_h: u32,
    text: TextDlg,
    sticker: StickerDlg,
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
            sticker: StickerDlg::default(),
        }
    }
}

/// The toolbar and keyboard commands. We collect them into a list and run them
/// after the bar is done being built, so we do not fight over borrowing `self`.
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

/// The defaults from AnnotationOptions.cs, field by field.
struct Opts {
    border: Color32,
    fill: Color32,
    border_size: f32,
    corner_radius: f32,
    shadow: bool,
    /// How many curvature nodes new lines and arrows get
    /// (ShareX: `LineCenterPointCount`, 1 by default).
    line_mid: usize,

    text_fill: Color32,
    text_border: Color32,
    text_border_w: f32,
    /// The defaults of the text box with a background (and of the balloon).
    text: shape::TextOpts,

    outline_border: Color32,
    outline_w: f32,
    /// The defaults of outlined text.
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
            // ShareX writes outlined text larger and in bold
            outline: shape::TextOpts {
                size: 25.0,
                bold: true,
                ..shape::TextOpts::default()
            },

            step_fill: shape::PRIMARY,
            step_border: shape::SECONDARY,
            step_text: Color32::WHITE,
            // ShareX starts from 18; the digits came out too small, so ~20% bigger
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
    /// The number the next step counter placed will get.
    step_next: u32,
    last_save: Option<std::path::PathBuf>,
    status: String,
    /// The real width of the button row, measured on the previous frame.
    /// We use it to center the bar, as in ShareX.
    bar_w: f32,
    /// The screen capture requested on the previous frame, not yet taken.
    pending: Option<Pending>,
    /// A counter for texture names: two inserted images must not get the same
    /// name, otherwise the second overwrites the first.
    img_seq: u32,
    /// The built-in arrow, decoded on first use.
    cursor: Option<Arc<RgbaImage>>,
    /// The modal dialog that is open, if there is one.
    dialog: Option<Dialog>,
}

/// ~/Pictures if it exists, otherwise the home directory.
fn pictures_dir() -> PathBuf {
    let p = config::home().join("Pictures");
    if p.is_dir() { p } else { config::home() }
}

/// ~/.local/share/sxr/stickers, honoring XDG_DATA_HOME.
fn stickers_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| config::home().join(".local/share"))
        .join("sxr")
        .join("stickers")
}

/// The extensions read as stickers: exactly the formats `image` decodes with
/// our default configuration.
const STICKER_EXT: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

/// The keys remembered between sessions for the sticker window.
const K_STICKER_SIZE: &str = "sticker_size";
const K_STICKER_PACK: &str = "sticker_pack";

/// The limits of the `Size:` field in `StickerForm`: from 16 to 256 in steps of
/// 16, 64 by default. The same value is both the thumbnail side and the side of
/// the inserted sticker — in the original the ratio is 1:1, so a square.
const STICKER_MIN: f32 = 16.0;
const STICKER_MAX: f32 = 256.0;
const STICKER_STEP: f32 = 16.0;
const STICKER_DEF: f32 = 64.0;

/// The saved size. A corrupted or out-of-range value is not an error to show
/// anyone: we simply fall back to the default.
fn sticker_size_saved() -> f32 {
    config::get(K_STICKER_SIZE)
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| (STICKER_MIN..=STICKER_MAX).contains(v))
        .unwrap_or(STICKER_DEF)
}

fn is_sticker(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| STICKER_EXT.contains(&e.to_ascii_lowercase().as_str()))
        && p.is_file()
}

/// The images in a single folder, without descending into subfolders. Sorted,
/// otherwise the grid would look different on every opening: `read_dir`
/// promises no order at all.
fn sticker_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_sticker(p))
        .collect();
    v.sort();
    v
}

/// The sticker packs. In ShareX a pack is a folder, so every direct subfolder
/// is one. The root goes into the list too — first — but only if it has loose
/// files: that is where stickers land while nobody has grouped them yet, and
/// there would be no other way to reach them.
///
/// A subfolder with no stickers in it is left out. Otherwise it would count as
/// a pack, the tool would decide the folder is not empty, and the window would
/// open on a grid with nothing in it.
fn sticker_packs(root: &Path) -> Vec<(String, PathBuf)> {
    let mut subs: Vec<(String, PathBuf)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| !sticker_files(p).is_empty())
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().into_owned(), p)))
        .collect();
    subs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    let mut out = Vec::with_capacity(subs.len() + 1);
    if !sticker_files(root).is_empty() {
        out.push((t(Msg::StickerAllPacks).to_owned(), root.to_owned()));
    }
    out.append(&mut subs);
    out
}

/// The indices of the files that pass the search. ShareX searches the whole
/// path, not just the file name, so a pack's name finds everything inside it.
/// Case-insensitive.
fn sticker_filter(files: &[PathBuf], q: &str) -> Vec<usize> {
    let q = q.trim().to_lowercase();
    files
        .iter()
        .enumerate()
        .filter(|(_, p)| q.is_empty() || p.to_string_lossy().to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

/// A file's thumbnail, decoded only once. `None` = it does not decode; we skip
/// it silently, so that one broken file does not stop the grid. The shrinking
/// is done on the CPU, before upload: at 64 points there is no point keeping a
/// 512 PNG in the GPU. We never enlarge: a small sticker stays as it is and is
/// stretched only when drawn.
fn thumb(
    ctx: &egui::Context,
    cache: &mut HashMap<PathBuf, Option<TextureHandle>>,
    p: &Path,
    side: f32,
) -> Option<TextureHandle> {
    if let Some(v) = cache.get(p) {
        return v.clone();
    }
    let tex = image::open(p).ok().map(|i| {
        let img = i.to_rgba8();
        let (w, h) = (img.width() as f32, img.height() as f32);
        let k = (side / w).min(side / h).min(1.0);
        let small = if k < 1.0 {
            resize_img(&img, (w * k).round() as u32, (h * k).round() as u32)
        } else {
            img
        };
        ctx.load_texture(
            format!("sxr-thumb-{}", p.display()),
            to_color_image(&small),
            egui::TextureOptions::LINEAR,
        )
    });
    cache.insert(p.to_owned(), tex.clone());
    tex
}

/// Opens a folder in the file manager. `xdg-open` leaves in the background from
/// an `sh` that exits right away, so the interface does not wait on it and no
/// zombie process is left to reap. `command -v` first, otherwise a missing
/// `xdg-open` would pass as success: the shell exits with zero anyway, the
/// backgrounded job being its business, not ours.
fn open_dir(dir: &Path) -> anyhow::Result<()> {
    let st = std::process::Command::new("sh")
        .arg("-c")
        .arg(r#"command -v xdg-open >/dev/null || exit 127; xdg-open "$0" >/dev/null 2>&1 &"#)
        .arg(dir)
        .status()?;
    if !st.success() {
        anyhow::bail!("xdg-open: {st}");
    }
    Ok(())
}

/// The native KDE open dialog. `None` if the user cancels or if `kdialog` is
/// missing — in both cases nothing happens at all.
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

/// The color button's icon, redrawn after ShareX
/// (`ImageHelpers.DrawColorPickerIcon`): a filled square with a 1px black
/// border. If `hole > 0`, the middle stays empty and gets a border of its own —
/// that is how the "outline color" button looks, as opposed to the fill one.
/// The transparent color shows as a checkerboard, again as in ShareX.
/// A contrasting background for the writing area, as in
/// `ColorHelpers.VisibleColor`: white under dark text, dark gray under light.
// --------------------------------------------------------- text input window

/// The side of the square buttons in the text window. In
/// `TextDrawingInputBox.Designer.cs` they all have `Size = 24, 24`:
/// the color, B / I / U, the two alignments and the bottom-left button.
const TDLG_BTN: f32 = 24.0;
/// The width of the font list (`cbFonts.Size = 158, 21`).
const TDLG_FONT_W: f32 = 158.0;
/// The width of the numeric field without the arrows (`nudTextSize.Size = 55, 20`).
const TDLG_NUM_W: f32 = 40.0;
/// The width of the arrows glued to the right of the numeric field.
const TDLG_ARROW_W: f32 = 14.0;
/// The width of the OK and Cancel buttons (`btnOK.Size = 104, 24`).
const TDLG_OK_W: f32 = 104.0;
/// The width of the original window's client area (`$this.ClientSize = 534, 361`);
/// below it the top bar no longer fits on one row, so it is also the minimum width.
const TDLG_W: f32 = 534.0;
/// The window at opening. `egui::Window` draws its own title bar, so we start
/// from the original's outer size, not from the client area: 547x421, which is
/// what `TextDrawingInputBox` measures on screen.
const TDLG_WIN_W: f32 = 547.0;
const TDLG_WIN_H: f32 = 421.0;

// -------------------------------------------------- sticker picker window

/// The window at opening. ShareX's `StickerForm` opens on a client area wide
/// enough for about seven 64px thumbnails in a row; we take that as the
/// measure, not the designer's exact figure, because our top bar is written in
/// a different font and would give a different minimum.
const SDLG_WIN_W: f32 = 620.0;
const SDLG_WIN_H: f32 = 500.0;
/// Below this the top bar would no longer fit on one row.
const SDLG_MIN_W: f32 = 470.0;
/// The space around a thumbnail, inside its cell.
const SDLG_PAD: f32 = 6.0;
/// The search field (`txtSearch`) and the pack list (`cbStickerPacks`).
const SDLG_SEARCH_W: f32 = 130.0;
const SDLG_PACK_W: f32 = 150.0;

/// The 24x24 square button used by every tool with a pictogram.
fn square_button<'a>() -> egui::Button<'a> {
    egui::Button::new("").min_size(Vec2::splat(TDLG_BTN))
}

/// B / I / U: a square button with the letter on it, pressed while the style is on.
/// In the original these are `CheckBox`es with `Appearance.Button`, which is just this.
fn style_toggle(ui: &mut egui::Ui, on: &mut bool, letter: egui::RichText, tip: Msg) {
    let r = ui.add(
        egui::Button::new(letter)
            .selected(*on)
            .min_size(Vec2::splat(TDLG_BTN)),
    );
    if r.clicked() {
        *on = !*on;
    }
    r.on_hover_text(t(tip));
}

/// WinForms' `NumericUpDown`: egui has nothing like it, so we glue a narrow
/// `DragValue` to two small arrow buttons, in a column without spacing.
/// `step` is how much one arrow moves; when it is larger than 1, the value is
/// also rounded to it, like the original's `NumericUpDown.Increment`.
fn num_up_down(ui: &mut egui::Ui, v: &mut f32, lo: f32, hi: f32, step: f32) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.horizontal(|ui| {
            // the text sits centered: `DragValue` lays it out like a button's label
            let dv = egui::DragValue::new(v)
                .range(lo..=hi)
                .speed(step.max(1.0) * 0.2)
                .fixed_decimals(0);
            ui.add_sized([TDLG_NUM_W, TDLG_BTN], dv);

            // The two arrows are ONE single widget, split into halves: as
            // separate buttons, each asks for its own minimum height and the
            // column comes out taller than the field, lowered relative to it.
            ui.add_space(3.0);
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(TDLG_ARROW_W, TDLG_BTN), Sense::click());
            let up_r = Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y));
            let down_r = Rect::from_min_max(egui::pos2(rect.min.x, rect.center().y), rect.max);
            let hovered = resp.hover_pos();
            for (half, up) in [(up_r, true), (down_r, false)] {
                let hot = hovered.is_some_and(|p| half.contains(p));
                let vis = if hot {
                    &ui.style().visuals.widgets.hovered
                } else {
                    &ui.style().visuals.widgets.inactive
                };
                ui.painter()
                    .rect_filled(half.shrink(0.5), vis.corner_radius, vis.bg_fill);
                paint_spinner(ui.painter(), half, up, vis.fg_stroke.color);
            }
            if resp.clicked() {
                let up = resp.interact_pointer_pos().is_some_and(|p| up_r.contains(p));
                *v = if up { *v + step } else { *v - step };
            }
            if step > 1.0 {
                *v = (*v / step).round() * step;
            }
            *v = v.clamp(lo, hi);
        });
    });
}

/// The arrow of a `NumericUpDown` button: a filled triangle, up or down.
fn paint_spinner(p: &egui::Painter, rect: Rect, up: bool, c: Color32) {
    let m = rect.center();
    let (w, h) = (3.5, 2.0);
    let y = if up { -h } else { h };
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(m.x - w, m.y - y),
            egui::pos2(m.x + w, m.y - y),
            egui::pos2(m.x, m.y + y),
        ],
        c,
        egui::Stroke::NONE,
    ));
}

/// The horizontal alignment pictogram, after `edit-alignment*` from the Fugue
/// set (it is not among the icons already brought into `assets/icons`, so we
/// draw it): four bars, long and short alternately, pushed to the chosen edge.
fn paint_align_h(p: &egui::Painter, rect: Rect, a: shape::Align, c: Color32) {
    let b = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    for (i, long) in [true, false, true, false].into_iter().enumerate() {
        let w = if long { b.width() } else { b.width() * 0.6 };
        let x = match a {
            _ if long => b.left(),
            shape::Align::Near => b.left(),
            shape::Align::Center => b.left() + (b.width() - w) / 2.0,
            shape::Align::Far => b.right() - w,
        };
        let y = b.top() + i as f32 * 4.0;
        let bar = Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, 2.0));
        p.rect_filled(bar, 0.0, c);
    }
}

/// The vertical alignment pictogram, after `edit-vertical-alignment*`:
/// one long bar on the chosen edge and two short ones on the other side.
fn paint_align_v(p: &egui::Painter, rect: Rect, a: shape::Align, c: Color32) {
    let b = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    let bar = |y: f32, w: f32| {
        let x = b.left() + (b.width() - w) / 2.0;
        p.rect_filled(
            Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, 2.0)),
            0.0,
            c,
        );
    };
    let short = b.width() * 0.6;
    match a {
        shape::Align::Near => {
            bar(b.top(), b.width());
            bar(b.top() + 5.0, short);
            bar(b.top() + 9.0, short);
        }
        shape::Align::Center => {
            bar(b.top() + 1.0, short);
            bar(b.top() + 6.0, b.width());
            bar(b.top() + 11.0, short);
        }
        shape::Align::Far => {
            bar(b.top() + 3.0, short);
            bar(b.top() + 7.0, short);
            bar(b.bottom() - 2.0, b.width());
        }
    }
}

/// The bottom-left button's pictogram (`btnSwapEnterKey`, the Fugue icon
/// `keyboard-enter`): the Enter arrow, with the elbow at the top right.
fn paint_enter_key(p: &egui::Painter, rect: Rect, c: Color32) {
    let b = Rect::from_center_size(rect.center(), Vec2::splat(12.0));
    let s = egui::Stroke::new(1.5, c);
    let corner = egui::pos2(b.right(), b.bottom() - 3.0);
    let tip = egui::pos2(b.left() + 1.0, b.bottom() - 3.0);
    p.line_segment([egui::pos2(b.right(), b.top()), corner], s);
    p.line_segment([corner, tip], s);
    p.line_segment([tip, egui::pos2(tip.x + 4.0, tip.y - 4.0)], s);
    p.line_segment([tip, egui::pos2(tip.x + 4.0, tip.y + 4.0)], s);
}

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
    // the solid part: the whole square, minus the hole in the middle
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

/// A color button with the icon above, which opens the same egui color picker
/// as `color_edit_button_srgba` — only the drawing of the button differs.
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
    fn new(ctx: &egui::Context, img: RgbaImage) -> Self {
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
            self.status = t(Msg::StNothingToUndo).into();
        }
    }

    fn do_redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let back = self.restore(s);
            self.undo.push(back);
        } else {
            self.status = t(Msg::StNothingToRedo).into();
        }
    }

    // ------------------------------------------------------------- drafts

    /// The starting shape for the current tool. `None` = the tool draws nothing.
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
            // the middle nodes all start from the starting point;
            // `update_draft` spreads them along the line as you drag
            Tool::Line => Shape::Line {
                from: at,
                to: at,
                mid: vec![at; o.line_mid],
                curved: false,
                color: o.border,
                width: o.border_size,
            },
            Tool::Arrow => Shape::Arrow {
                from: at,
                to: at,
                mid: vec![at; o.line_mid],
                curved: false,
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
            // the eraser's color is computed from the ring around it; on the draft it
            // is still empty, update_draft rebuilds it as you drag with the mouse
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
            // the balloon's tail is placed only at commit, when the box is final
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
            // crop and cut out use a plain frame, not the annotation colors
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
                Shape::Arrow { from, to, mid, curved, .. } | Shape::Line { from, to, mid, curved, .. },
            ) => {
                *to = at;
                shape::auto_mid(*from, *to, mid, *curved);
            }
            Some(Shape::Free { pts, .. }) => {
                if pts.last().is_none_or(|l| l.distance(at) > 1.0) {
                    pts.push(at);
                }
            }
            Some(Shape::Step { center, .. }) => *center = at,
            None => {}
        }
        // the smart eraser takes its color from the ring around the rectangle;
        // we recompute it while you drag, then it stays fixed even if the shape moves
        if let Some(Shape::Erase { rect, .. }) = &self.draft {
            let c = shape::ring_avg(&self.img, *rect);
            if let Some(Shape::Erase { color, .. }) = self.draft.as_mut() {
                *color = c;
            }
        }
    }

    /// Is the draft big enough to be worth keeping?
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
            // the step counter is placed with a simple click
            Shape::Step { .. } => true,
        }
    }

    fn commit_draft(&mut self) {
        let Some(mut s) = self.draft.take() else { return };
        // dragging right to left (or bottom to top) leaves `max < min`;
        // `update_draft` must not normalize, otherwise the anchor would migrate
        // along with the cursor, so we straighten the rectangle here, at commit
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

        // a text box (or a balloon) dragged too short gets a default size,
        // otherwise an unsteady click would lose the shape
        if let Shape::Text { rect, opts, .. } | Shape::Balloon { rect, opts, .. } = &mut s {
            let n = Rect::from_two_pos(rect.min, rect.max);
            *rect = if n.width() < 12.0 || n.height() < 12.0 {
                Rect::from_min_size(n.min, Vec2::new(220.0, opts.size * 1.8))
            } else {
                n
            };
        }
        // the tail starts 30px below the box's bottom-left corner
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
        // as in ShareX: the freshly drawn shape stays active, with its nodes visible,
        // so you can adjust it right away without switching to the selection tool
        self.sel = Some(i);
        if is_text {
            // as in ShareX: the freshly drawn shape opens the text window
            self.open_text(i, true);
        }
        if is_step {
            self.step_next += 1;
        }
    }

    // ------------------------------------------------------ inserted images

    /// Inserts an image as a new shape: at native size, with the top-left corner
    /// at the click point, shrunk proportionally if it would not fit in the
    /// background image.
    fn stamp(&mut self, ctx: &egui::Context, at: Pos2, img: Arc<RgbaImage>) {
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let k = (self.img.width() as f32 / iw.max(1.0))
            .min(self.img.height() as f32 / ih.max(1.0))
            .min(1.0);
        self.stamp_rect(ctx, Rect::from_min_size(at, Vec2::new(iw * k, ih * k)), img);
    }

    /// Inserts an image into a square of the given side, with the corner at the
    /// click point: the stickers' path, where the size chosen in the window beats
    /// the file's native size.
    fn stamp_sized(&mut self, ctx: &egui::Context, at: Pos2, img: Arc<RgbaImage>, side: f32) {
        self.stamp_rect(ctx, Rect::from_min_size(at, Vec2::splat(side)), img);
    }

    /// The common part: uploads the texture, adds the shape and switches to the
    /// selection tool.
    fn stamp_rect(&mut self, ctx: &egui::Context, rect: Rect, img: Arc<RgbaImage>) {
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        if iw < 1.0 || ih < 1.0 {
            self.status = t(Msg::StEmptyImage).into();
            return;
        }
        self.img_seq += 1;
        let tex = ctx.load_texture(
            format!("sxr-img-{}", self.img_seq),
            to_color_image(&img),
            egui::TextureOptions::LINEAR,
        );
        self.push_undo(false);
        self.shapes.push(Shape::Img { rect, img, tex });
        // we switch to the selection tool: otherwise `sel` would be cleared on the
        // next frame (see `ui`) and the shape just placed could no longer be dragged
        self.tool = Tool::Select;
        self.sel = Some(self.shapes.len() - 1);
        self.status = i18n::image_inserted(iw as u32, ih as u32);
    }

    /// Loads a file from disk and stamps it. On error it only reports.
    fn stamp_file(&mut self, ctx: &egui::Context, at: Pos2, path: &Path) {
        match image::open(path) {
            Ok(i) => self.stamp(ctx, at, Arc::new(i.to_rgba8())),
            Err(e) => self.status = i18n::cannot_open(&path.display().to_string(), &e.to_string()),
        }
    }

    /// The classic arrow, embedded in the binary and decoded only once.
    fn cursor_img(&mut self) -> Option<Arc<RgbaImage>> {
        if self.cursor.is_none() {
            const PNG: &[u8] = include_bytes!("../assets/cursor.png");
            match image::load_from_memory(PNG) {
                Ok(i) => self.cursor = Some(Arc::new(i.to_rgba8())),
                Err(e) => self.status = i18n::cannot_decode_cursor(&e.to_string()),
            }
        }
        self.cursor.clone()
    }

    /// The tools that insert an image: they are placed with a single click.
    /// The dialogs and the capture block the UI thread while they run.
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
                    self.status = i18n::cannot_create(&dir.display().to_string(), &e.to_string());
                    return;
                }
                // the window on an empty folder would have nothing to show
                let packs = sticker_packs(&dir);
                if packs.is_empty() {
                    self.status = i18n::put_png_files_in(&dir.display().to_string());
                    return;
                }
                // the sticker is inserted only after one is chosen: the window
                // is ours, so it does not block the UI thread like kdialog
                self.open_sticker(dir, at, false);
            }
            Tool::Cursor => {
                // the arrow's tip is the very top-left pixel of the image,
                // so the shape's corner falls exactly on the click point
                if let Some(c) = self.cursor_img() {
                    self.stamp(ctx, at, c);
                }
            }
            Tool::ImageScreen => {
                // the window has to get out of the way before spectacle runs:
                // we send the command now, the capture comes on the next frame
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                ctx.request_repaint();
                self.pending = Some(Pending::Screen { at, center: false });
                self.status = t(Msg::StSelectRegion).into();
            }
            _ => {}
        }
    }

    /// Insertion requested from the "Image" menu: the same tools, only started
    /// without a click, and the shape is placed in the center of the image.
    fn stamp_menu(&mut self, ctx: &egui::Context, t: Tool) {
        let prev = self.tool;
        self.tool = t;
        let at = Pos2::new(self.img.width() as f32 / 2.0, self.img.height() as f32 / 2.0);
        if t == Tool::ImageScreen {
            self.stamp_tool(ctx, at);
            // the capture comes only on the next frame; we mark the centering there
            if let Some(Pending::Screen { center, .. }) = self.pending.as_mut() {
                *center = true;
            }
        } else if t == Tool::Sticker {
            self.stamp_tool(ctx, at);
            // the choice comes only a few frames later, from the sticker window;
            // we mark the centering there, as with the screen capture
            if let Some(d) = self.dialog.as_mut() {
                d.sticker.center = true;
            }
        } else {
            let n = self.shapes.len();
            self.stamp_tool(ctx, at);
            if self.shapes.len() > n {
                self.center_last();
            }
        }
        // `stamp` switches to the selection tool on success; otherwise we go back
        if self.tool == t {
            self.tool = prev;
        }
    }

    /// Moves the last inserted shape to the center of the image.
    fn center_last(&mut self) {
        let c = Pos2::new(self.img.width() as f32 / 2.0, self.img.height() as f32 / 2.0);
        if let Some(s) = self.shapes.last_mut() {
            let d = c - s.bounds().center();
            s.translate(d);
        }
    }

    /// The action `stamp_tool` deferred by one frame.
    fn run_pending(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending.take() else { return };
        match p {
            Pending::Screen { at, center } => {
                let shot = crate::capture::select_region();
                // the window comes back whatever came out of the capture
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.request_repaint();
                match shot {
                    Ok(i) => {
                        self.stamp(ctx, at, Arc::new(i));
                        if center {
                            self.center_last();
                        }
                    }
                    Err(e) => self.status = i18n::capture_cancelled(&format!("{e:#}")),
                }
            }
        }
    }

    // ------------------------------------------------------- text window

    /// Opens the text input window for shape `i`.
    /// `fresh` = the shape has just been created (the undo step is already pushed).
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

    /// Closing the window. `ok` = OK was pressed (or Enter).
    fn text_done(&mut self, t: TextDlg, ok: bool) {
        // ShareX calls OnConfigSave after the dialog, whatever the button:
        // the chosen options stay the defaults for the texts that follow
        if t.outline {
            self.opt.outline = t.opts.clone();
            self.opt.outline_border = t.color2;
        } else {
            self.opt.text = t.opts.clone();
            self.opt.text_fill = t.color2;
        }
        let drop = !ok || t.buf.trim().is_empty();
        if drop {
            // cancelling on an existing shape: it stays untouched
            if !ok && !t.fresh {
                return;
            }
            if t.fresh {
                // it never existed with content: we drop the undo step as well
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
        // as in ShareX: after the window closes the box stays active, with its
        // nodes visible, so it can be moved or resized right away
        self.sel = Some(t.idx);
    }

    // ------------------------------------------------------------- editing

    fn delete_sel(&mut self) {
        let Some(i) = self.sel.take() else {
            self.status = t(Msg::StNoShapeSelected).into();
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
            self.status = t(Msg::StNoShapeSelected).into();
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
            self.status = t(Msg::StNoShapeSelected).into();
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
        self.status = i18n::cropped_to(self.img.width(), self.img.height());
    }

    /// Cuts a band out of the image and glues the two halves together. The same
    /// undo mechanics as crop: the step saves the image too.
    fn apply_cutout(&mut self, r: Rect) {
        let Some((img, horiz, end, band)) = cut_out(&self.img, r) else { return };
        self.push_undo(true);
        self.img = img;
        // the shapes beyond the band move closer by its thickness
        let d = if horiz { Vec2::new(0.0, -band) } else { Vec2::new(-band, 0.0) };
        for s in self.shapes.iter_mut() {
            let b = s.bounds();
            if if horiz { b.min.y >= end } else { b.min.x >= end } {
                s.translate(d);
            }
        }
        self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
        self.sel = None;
        self.status = i18n::cut_out_to(self.img.width(), self.img.height());
    }


    // ---------------------------------------------------- image operations

    fn set_img(&mut self, img: RgbaImage) {
        self.img = img;
        self.tex.set(to_color_image(&self.img), egui::TextureOptions::LINEAR);
    }

    /// The image with the shapes drawn into it. `None` if there are no shapes (or
    /// if the render fails, in which case a message is left in the status bar).
    fn flat_img(&mut self) -> Option<RgbaImage> {
        if self.shapes.is_empty() {
            return None;
        }
        match render::compose_opts(&self.img, &self.shapes, self.opt.shadow)
            .and_then(|png| Ok(image::load_from_memory(&png)?.to_rgba8()))
        {
            Ok(i) => Some(i),
            Err(e) => {
                self.status = i18n::cannot_apply_shapes(&format!("{e:#}"));
                None
            }
        }
    }

    /// Draws the shapes into the image and clears the list. Rotating a text box
    /// would need rotated text, which we do not draw; flattening keeps exactly
    /// what the user sees. Called AFTER `push_undo(true)`.
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

    /// The status message, mentioning the flattening if it happened.
    fn note(&mut self, msg: String, flat: bool) {
        self.status = if flat { i18n::with_flattened(&msg) } else { msg };
    }

    fn img_new(&mut self, w: u32, h: u32, c: Color32) {
        self.push_undo(true);
        let flat = self.flatten();
        self.set_img(RgbaImage::from_pixel(w.max(1), h.max(1), rgba(c)));
        self.drop_shapes();
        self.note(i18n::new_image(w.max(1), h.max(1)), flat);
    }

    fn img_open(&mut self) {
        let Some(p) = pick_image(&pictures_dir()) else { return };
        let img = match image::open(&p) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.status = i18n::cannot_open(&p.display().to_string(), &e.to_string());
                return;
            }
        };
        self.push_undo(true);
        let flat = self.flatten();
        let (w, h) = img.dimensions();
        self.set_img(img);
        self.drop_shapes();
        self.last_save = None;
        self.note(i18n::opened(&p.display().to_string(), w, h), flat);
    }

    /// Resize: the only operation that does NOT flatten — the shapes are scaled
    /// by the same factor and stay editable.
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
        self.note(i18n::resized_to(w, h), false);
    }

    fn img_canvas(&mut self, w: u32, h: u32, c: Color32) {
        self.push_undo(true);
        let flat = self.flatten();
        let out = canvas_img(&self.img, w, h, c);
        let (w, h) = out.dimensions();
        self.set_img(out);
        self.note(i18n::canvas_to(w, h), flat);
    }

    fn img_autocrop(&mut self) {
        // the edges are searched on the image with the shapes already drawn:
        // otherwise a shape flush with an edge would be left out after the cut
        let flat = self.flat_img();
        let out = {
            let base = flat.as_ref().unwrap_or(&self.img);
            auto_crop_rect(base)
                .map(|(x, y, w, h)| image::imageops::crop_imm(base, x, y, w, h).to_image())
        };
        let Some(out) = out else {
            self.status = t(Msg::StNoUniformEdges).into();
            return;
        };
        self.push_undo(true);
        let flattened = flat.is_some();
        if flattened {
            self.drop_shapes();
        }
        let (w, h) = out.dimensions();
        self.set_img(out);
        self.note(i18n::auto_cropped_to(w, h), flattened);
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
        self.note(i18n::rotated(right, w, h), flat);
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
        if self.dialog.as_ref().is_some_and(|d| d.kind == DlgKind::Sticker) {
            self.sticker_dialog_ui(ctx, acts);
            return;
        }
        let Some(d) = self.dialog.as_mut() else { return };
        let (title, ok_text) = match d.kind {
            DlgKind::New => (t(Msg::DlgNewTitle), t(Msg::BtnCreate)),
            DlgKind::Size => (t(Msg::DlgSizeTitle), t(Msg::BtnOk)),
            DlgKind::Canvas => (t(Msg::DlgCanvasTitle), t(Msg::BtnOk)),
            DlgKind::Text | DlgKind::Sticker => return,
        };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(240.0);
                egui::Grid::new("sxr-dlg").num_columns(2).show(ui, |ui| {
                    ui.label(t(Msg::LblWidth));
                    ui.add(egui::DragValue::new(&mut d.w).range(1..=20000).suffix(" px"));
                    ui.end_row();
                    ui.label(t(Msg::LblHeight));
                    ui.add(egui::DragValue::new(&mut d.h).range(1..=20000).suffix(" px"));
                    ui.end_row();
                    match d.kind {
                        DlgKind::Size => {
                            ui.label(t(Msg::LblAspect));
                            ui.checkbox(&mut d.keep, t(Msg::ChkKeepAspect));
                        }
                        DlgKind::New => {
                            ui.label(t(Msg::LblBackground));
                            ui.color_edit_button_srgba(&mut d.color);
                        }
                        DlgKind::Canvas => {
                            ui.label(t(Msg::LblCanvasFill));
                            ui.color_edit_button_srgba(&mut d.color);
                        }
                        DlgKind::Text | DlgKind::Sticker => {}
                    }
                    ui.end_row();
                });
                // the changed side pulls the other one after it
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
                    cancel = ui.button(t(Msg::BtnCancel)).clicked();
                });
            });
        if ok {
            acts.push(Act::DlgOk);
        }
        if cancel {
            acts.push(Act::DlgCancel);
        }
    }


    /// The text input window, after `TextDrawingInputBox`. The layout comes from
    /// `TextDrawingInputBox.Designer.cs` and its `.resx`: `flpProperties` (the top
    /// bar) 518x32 at 8,5, `txtInput` 518x281 at 8,40 with a plain border, and at
    /// the bottom `btnSwapEnterKey` 24x24 at 8,328, `lblTip` next to it and
    /// `btnOK` / `btnCancel` of 104x24 glued to the right. Client: 534x361.
    fn text_dialog_ui(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let Some(d) = self.dialog.as_mut() else { return };
        let td = &mut d.text;
        // the chosen family is loaded once and stays in egui for drawing
        font::register(ctx, &td.opts);
        if let Some(n) = font::take_note() {
            self.status = n;
        }
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(t(Msg::DlgTextTitle))
            .collapsible(false)
            .resizable(true)
            .default_size([TDLG_WIN_W, TDLG_WIN_H])
            .min_size([TDLG_W, 200.0])
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                // all the controls of the bar have the same height, as in the original
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.spacing_mut().interact_size = Vec2::splat(TDLG_BTN);
                // ---- toolbar: a single row, like `flpProperties`
                ui.horizontal(|ui| {
                    ui.label(t(Msg::LblFont));
                    egui::ComboBox::from_id_salt("sxr-font")
                        .width(TDLG_FONT_W)
                        .height(360.0)
                        .selected_text(td.opts.family.clone())
                        .show_ui(ui, |ui| {
                            for f in font::families() {
                                let on = td.opts.family == f;
                                if ui.selectable_label(on, f).clicked() {
                                    td.opts.family = f.to_owned();
                                }
                            }
                        });
                    ui.label(t(Msg::LblTextSize));
                    // `nudTextSize`: minimum 5, maximum 300, in steps of 1
                    num_up_down(ui, &mut td.opts.size, 5.0, 300.0, 1.0);
                    ui.color_edit_button_srgba(&mut td.opts.color)
                        .on_hover_text(t(Msg::TipTextColor));
                    // `btnGradient` lives here too, but `Visible` turns it on only
                    // for shapes with a gradient, and sxr draws no gradients:
                    // one single color button is left, as in the original's screenshot.
                    let rt = egui::RichText::new;
                    style_toggle(ui, &mut td.opts.bold, rt("B").strong(), Msg::TipBold);
                    style_toggle(ui, &mut td.opts.italic, rt("I").italics(), Msg::TipItalic);
                    style_toggle(ui, &mut td.opts.underline, rt("U").underline(), Msg::TipUnderline);
                    // the alignments: square pictogram buttons with a menu on click,
                    // exactly like `btnAlignmentHorizontal` / `btnAlignmentVertical`
                    let (r, _) = MenuButton::from_button(square_button()).ui(ui, |ui| {
                        for a in shape::Align::ALL {
                            if ui.button(a.horiz_name()).clicked() {
                                td.opts.halign = a;
                                ui.close();
                            }
                        }
                    });
                    let c = ui.style().interact(&r).fg_stroke.color;
                    paint_align_h(ui.painter(), r.rect, td.opts.halign, c);
                    r.on_hover_text(t(Msg::TipAlignHoriz));
                    let (r, _) = MenuButton::from_button(square_button()).ui(ui, |ui| {
                        for a in shape::Align::ALL {
                            if ui.button(a.vert_name()).clicked() {
                                td.opts.valign = a;
                                ui.close();
                            }
                        }
                    });
                    let c = ui.style().interact(&r).fg_stroke.color;
                    paint_align_v(ui.painter(), r.rect, td.opts.valign, c);
                    r.on_hover_text(t(Msg::TipAlignVert));
                });
                // ---- the writing area: all the rest of the window, with a border
                let h = (ui.available_height() - TDLG_BTN - ui.spacing().item_spacing.y).max(60.0);
                let bg = visible_bg(td.opts.color);
                {
                    let te = egui::TextEdit::multiline(&mut td.buf)
                        .font(font::opts_font_id(&td.opts, td.opts.size))
                        .text_color(td.opts.color)
                        .horizontal_align(match td.opts.halign {
                            shape::Align::Near => egui::Align::LEFT,
                            shape::Align::Center => egui::Align::Center,
                            shape::Align::Far => egui::Align::RIGHT,
                        })
                        .background_color(bg)
                        .desired_width(f32::INFINITY);
                    let r = ui.add_sized([ui.available_width(), h], te);
                    // first opening: the cursor is right in the text
                    if td.focus {
                        td.focus = false;
                        r.request_focus();
                    }
                }
                // ---- the bottom row
                ui.horizontal(|ui| {
                    let r = ui.add(square_button());
                    let c = ui.style().interact(&r).fg_stroke.color;
                    paint_enter_key(ui.painter(), r.rect, c);
                    if r.clicked() {
                        td.opts.enter_new_line = !td.opts.enter_new_line;
                    }
                    r.on_hover_text(t(Msg::TipSwapEnterKey));
                    ui.label(t(if td.opts.enter_new_line {
                        Msg::TextInputHintSwap
                    } else {
                        Msg::TextInputHint
                    }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = |txt: &str| {
                            egui::Button::new(txt).min_size(egui::vec2(TDLG_OK_W, TDLG_BTN))
                        };
                        cancel = ui.add(btn(t(Msg::BtnCancel))).clicked();
                        ok = ui.add(btn(t(Msg::BtnOk))).clicked();
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

    // -------------------------------------------------- sticker picker window

    /// Opens the sticker picker window over the canvas. `at` = the point where the
    /// sticker lands, `center` = the request comes from the menu.
    fn open_sticker(&mut self, root: PathBuf, at: Pos2, center: bool) {
        let (w, h) = self.img.dimensions();
        let mut d = Dialog::new(DlgKind::Sticker, w, h);
        let packs = sticker_packs(&root);
        // the pack is remembered by name, not by position: folders appear and
        // disappear between sessions, so a saved index would show something else
        let want = config::get(K_STICKER_PACK).unwrap_or_default();
        let pack = packs.iter().position(|(n, _)| *n == want).unwrap_or(0);
        d.sticker = StickerDlg {
            at,
            center,
            root,
            packs,
            pack,
            size: sticker_size_saved(),
            focus: true,
            ..Default::default()
        };
        d.sticker.reload();
        self.dialog = Some(d);
        self.sel = None;
    }

    /// Closing the window. `ok` = a sticker was chosen (click or Enter).
    fn sticker_done(&mut self, ctx: &egui::Context, sd: StickerDlg, ok: bool) {
        // as with the text window: the choices in the bar stay the defaults for
        // next time, whatever button closed it. A single write for both, so the
        // file is not rewritten twice.
        let mut c = config::Config::load();
        c.set(K_STICKER_SIZE, &format!("{}", sd.size as u32));
        if let Some((n, _)) = sd.packs.get(sd.pack) {
            c.set(K_STICKER_PACK, n);
        }
        let _ = c.save();

        if !ok {
            return;
        }
        let Some(p) = sd.picked.as_ref() else { return };
        match image::open(p) {
            Ok(i) => {
                let n = self.shapes.len();
                self.stamp_sized(ctx, sd.at, Arc::new(i.to_rgba8()), sd.size);
                if sd.center && self.shapes.len() > n {
                    self.center_last();
                }
            }
            Err(e) => self.status = i18n::cannot_open(&p.display().to_string(), &e.to_string()),
        }
    }

    /// The sticker picker window, after `StickerForm`: at the top, on a single
    /// row, `Search:` with its field, `Stickers:` with the pack list, the gear
    /// button and `Size:`; below, the scrollable grid of square thumbnails, with
    /// the file name written under each and truncated with "…". Clicking a
    /// thumbnail picks and closes on the spot — the original has no OK button.
    fn sticker_dialog_ui(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let icons = self.icons.clone();
        let Some(d) = self.dialog.as_mut() else { return };
        let sd = &mut d.sticker;
        let (mut reload, mut refilter, mut folder) = (false, false, false);
        let mut chosen: Option<PathBuf> = None;

        egui::Window::new(t(Msg::DlgStickerTitle))
            .collapsible(false)
            .resizable(true)
            .default_size([SDLG_WIN_W, SDLG_WIN_H])
            .min_size([SDLG_MIN_W, 260.0])
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.spacing_mut().interact_size = Vec2::splat(TDLG_BTN);
                // ---- the top bar: a single row, as in the original
                ui.horizontal(|ui| {
                    // the labels have `Margin = Padding(2, 1, 0, 2)`, that is they
                    // sit glued to the control on their right
                    ui.label(t(Msg::LblSearch));
                    // `txtSearch` has `BorderStyle.FixedSingle`: a 1px border, the
                    // same when focused. egui otherwise puts a thick, light ring
                    // there, which would catch the eye more than the grid itself.
                    let v = &mut ui.visuals_mut();
                    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, SEL_BORDER);
                    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_gray(0x5A));
                    v.selection.stroke = egui::Stroke::new(1.0, Color32::from_gray(0x7A));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut sd.query)
                            .desired_width(SDLG_SEARCH_W)
                            .margin(egui::Margin::symmetric(4, 2)),
                    );
                    if r.changed() {
                        refilter = true;
                    }
                    if sd.focus {
                        sd.focus = false;
                        r.request_focus();
                    }
                    ui.label(t(Msg::LblStickerPack));
                    let cur = sd.packs.get(sd.pack).map(|(n, _)| n.clone()).unwrap_or_default();
                    // `DropDownList`: you pick from the list, you do not type in it
                    egui::ComboBox::from_id_salt("sxr-sticker-pack")
                        .width(SDLG_PACK_W)
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            for i in 0..sd.packs.len() {
                                let on = sd.pack == i;
                                if ui.selectable_label(on, &sd.packs[i].0).clicked() && !on {
                                    sd.pack = i;
                                    reload = true;
                                }
                            }
                        });
                    // the gear: in ShareX it opens the pack manager, which sxr
                    // does not have — here it opens the pack's folder, that is
                    // exactly where stickers are added and removed
                    let r = ui
                        .add(egui::Button::image(icons.img("gear")))
                        .on_hover_text(t(Msg::TipStickerFolder));
                    folder = r.clicked();
                    ui.label(t(Msg::LblStickerSize));
                    let before = sd.size;
                    num_up_down(ui, &mut sd.size, STICKER_MIN, STICKER_MAX, STICKER_STEP);
                    if sd.size != before {
                        // the thumbnails were uploaded at the previous size:
                        // at another one they are neither sharp enough nor useful
                        sd.thumbs.clear();
                    }
                });
                ui.separator();

                // ---- the grid: fills the rest of the window
                if sd.hits.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.weak(t(Msg::StNoStickerMatch));
                    });
                    return;
                }
                let line = ui.text_style_height(&egui::TextStyle::Body);
                let cell = Vec2::new(sd.size + SDLG_PAD * 2.0, sd.size + SDLG_PAD * 2.0 + line);
                // the scroll bar on the right eats into the width as well
                let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
                let cols = (((ui.available_width() - bar) / cell.x).floor() as usize).max(1);
                let rows = sd.hits.len().div_ceil(cols);
                let h = ui.available_height().max(cell.y);
                let font = egui::TextStyle::Body.resolve(ui.style());
                let ink = ui.visuals().text_color();
                let StickerDlg { files, hits, thumbs, sel, size, .. } = sd;
                let side = *size;
                ui.scope(|ui| {
                    // the rows have to come exactly every `cell.y` points,
                    // otherwise `show_rows` draws them off to the side
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_height(h)
                        .show_rows(ui, cell.y, rows, |ui, range| {
                            for row in range {
                                ui.horizontal(|ui| {
                                    let pt = ui.painter().clone();
                                    for col in 0..cols {
                                        let k = row * cols + col;
                                        let Some(&fi) = hits.get(k) else { break };
                                        let p = &files[fi];
                                        let (rect, resp) =
                                            ui.allocate_exact_size(cell, Sense::click());
                                        if *sel == k {
                                            pt.rect(
                                                rect.shrink(1.0),
                                                2,
                                                SEL_BG,
                                                egui::Stroke::new(1.0, SEL_BORDER),
                                                egui::StrokeKind::Inside,
                                            );
                                        } else if resp.hovered() {
                                            pt.rect_filled(rect.shrink(1.0), 2, HOVER_BG);
                                        }
                                        let box_r = Rect::from_min_size(
                                            rect.min + Vec2::splat(SDLG_PAD),
                                            Vec2::splat(side),
                                        );
                                        if let Some(tx) = thumb(ui.ctx(), thumbs, p, side) {
                                            let [tw, th] = tx.size();
                                            let (tw, th) = (tw as f32, th as f32);
                                            let z = (side / tw).min(side / th);
                                            pt.image(
                                                tx.id(),
                                                Rect::from_center_size(
                                                    box_r.center(),
                                                    Vec2::new(tw * z, th * z),
                                                ),
                                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                                Color32::WHITE,
                                            );
                                        }
                                        // the name underneath, truncated with "…":
                                        // `max_rows = 1` adds the character itself.
                                        // Without the extension, as in ShareX: the
                                        // label would otherwise be half ".png".
                                        let name = p
                                            .file_stem()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_default();
                                        let mut job = egui::text::LayoutJob::simple_singleline(
                                            name,
                                            font.clone(),
                                            ink,
                                        );
                                        job.wrap.max_width = cell.x - 4.0;
                                        job.wrap.max_rows = 1;
                                        job.wrap.break_anywhere = true;
                                        let g = pt.layout_job(job);
                                        let x = rect.center().x - g.size().x / 2.0;
                                        pt.galley(egui::pos2(x, box_r.bottom()), g, ink);
                                        if resp.clicked() {
                                            *sel = k;
                                            chosen = Some(p.clone());
                                        }
                                    }
                                });
                            }
                        });
                });
            });

        if reload {
            sd.reload();
        } else if refilter {
            sd.refilter();
        }
        if let Some(p) = chosen {
            // the click picks and closes, with no OK button
            sd.picked = Some(p);
            acts.push(Act::DlgOk);
        }
        if folder {
            let dir = sd.pack_dir();
            if let Err(e) = open_dir(&dir) {
                self.status = i18n::cannot_open_folder(&dir.display().to_string(), &format!("{e:#}"));
            }
        }
    }

    // ------------------------------------------------------------- output

    fn png(&self) -> Result<Vec<u8>> {
        render::compose_opts(&self.img, &self.shapes, self.opt.shadow)
    }

    fn copy(&mut self) {
        match self.png().and_then(clip::copy_png) {
            Ok(()) => self.status = t(Msg::StCopied).into(),
            Err(e) => self.status = i18n::copy_failed(&e.to_string()),
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
                self.status = i18n::saved_to(&path.display().to_string());
                self.last_save = Some(path);
            }
            Err(e) => self.status = i18n::save_failed(&e.to_string()),
        }
    }

    // ------------------------------------------------------------- colors

    /// ShareX writes the outline color into the current tool's field, not a single one.
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

    // ------------------------------------------------------------- keyboard

    fn keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::Key as K;
        // the dialog is modal: while it is open no editor shortcut fires,
        // only Enter (OK) and Esc (Cancel)
        if let Some(d) = &self.dialog {
            if d.kind == DlgKind::Text {
                self.text_keys(ctx, acts);
                return;
            }
            if d.kind == DlgKind::Sticker {
                self.sticker_keys(ctx, acts);
                return;
            }
            let typing = ctx.memory(|m| m.focused().is_some());
            ctx.input(|i| {
                if i.key_pressed(K::Escape) {
                    acts.push(Act::DlgCancel);
                }
                // if something is being typed into a field, Enter belongs to the field
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

    /// The text window's keyboard, exactly as in `TextDrawingInputBox`: by default
    /// Enter = OK and Ctrl+Enter = new line, and the bottom-left button
    /// (`btnSwapEnterKey`, that is `Options.EnterKeyNewLine`) swaps them.
    /// Esc = cancel. It runs BEFORE the interface is built, so it takes the
    /// events out of the queue before they reach `TextEdit`.
    fn text_keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::{Event, Key as K};
        // with `EnterKeyNewLine` on, the OK key is Ctrl+Enter, not Enter
        let swap = self.dialog.as_ref().is_some_and(|d| d.text.opts.enter_new_line);
        let (mut ok, mut cancel) = (false, false);
        ctx.input_mut(|i| {
            i.events.retain_mut(|e| match e {
                Event::Key { key: K::Enter, pressed: true, modifiers, .. } => {
                    if (modifiers.ctrl || modifiers.command) == swap {
                        ok = true;
                        false
                    } else {
                        // the new line: we pass it through as a plain Enter, so TextEdit
                        // inserts it right at the cursor position
                        *modifiers = egui::Modifiers::NONE;
                        true
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

    /// The sticker window's keyboard: Enter picks the first search result, Esc
    /// cancels. It runs BEFORE the interface is built, so the keys do not end up
    /// in the search field.
    fn sticker_keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::{Event, Key as K};
        let (mut ok, mut cancel) = (false, false);
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key: K::Enter, pressed: true, .. } => {
                    ok = true;
                    false
                }
                Event::Key { key: K::Escape, pressed: true, .. } => {
                    cancel = true;
                    false
                }
                _ => true,
            });
        });
        if ok {
            if let Some(d) = self.dialog.as_mut() {
                d.sticker.picked = d.sticker.first_hit();
            }
            acts.push(Act::DlgOk);
        }
        if cancel {
            acts.push(Act::DlgCancel);
        }
    }

    fn pick(&mut self, t: Tool) {
        // switching tools drops the active shape; while you stay on the same
        // tool, the freshly drawn shape stays selected, with its nodes visible
        if t != self.tool {
            self.sel = None;
        }
        self.tool = t;
        if !t.ready() {
            self.status = i18n::tool_not_ready(t.tooltip());
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
                Act::Upload => self.status = t(Msg::StUploadMissing).into(),
                // The button stays for fidelity to the ShareX bar,
                // but printing is out of scope for sxr.
                Act::Print => self.status = t(Msg::StPrintMissing).into(),
                Act::Undo => self.do_undo(),
                Act::Redo => self.do_redo(),
                Act::Dup => self.duplicate(),
                Act::Del => self.delete_sel(),
                Act::DelAll => self.delete_all(),
                Act::Front | Act::Forward | Act::Backward | Act::Back => self.reorder(*a),
                Act::CropTool => {
                    self.pick(Tool::Crop);
                    self.status = t(Msg::StDragToCrop).into();
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
                            DlgKind::Sticker => self.sticker_done(ctx, d.sticker, true),
                        }
                    }
                }
                Act::DlgCancel => {
                    if let Some(d) = self.dialog.take() {
                        match d.kind {
                            DlgKind::Text => self.text_done(d.text, false),
                            DlgKind::Sticker => self.sticker_done(ctx, d.sticker, false),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- toolbar

    fn toolbar(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            // no scroll bar: on a narrow window it showed up as a white stripe
            // right under the tools, every time you moved the mouse over there.
            // The bar can still be scrolled with the mouse wheel.
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding = Vec2::new(5.0, 4.0);
                    // In ShareX the icons sit directly on the bar, with no box
                    // around them: the frame and the background appear only on hover
                    // or on the chosen tool. So we only clear the "inactive" state.
                    let v = &mut ui.style_mut().visuals;
                    v.selection.bg_fill = SEL_BG;
                    let w = &mut v.widgets;
                    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                    w.inactive.bg_fill = Color32::TRANSPARENT;
                    w.inactive.bg_stroke = egui::Stroke::NONE;
                    // Hover in ShareX is just a slightly lighter background; we
                    // keep the border for the active tool, otherwise the two
                    // states would look almost the same. Pressing adds the
                    // border, in place of the white frame egui gives.
                    w.hovered.weak_bg_fill = HOVER_BG;
                    w.hovered.bg_fill = HOVER_BG;
                    w.hovered.bg_stroke = egui::Stroke::NONE;
                    w.active.weak_bg_fill = HOVER_BG;
                    w.active.bg_fill = HOVER_BG;
                    w.active.bg_stroke = egui::Stroke::new(1.0, SEL_BORDER);

                    // The bar sits centered across the window width, as in ShareX.
                    let pad = ((ui.available_width() - self.bar_w) * 0.5).max(0.0);
                    if pad > 0.5 {
                        ui.add_space(pad);
                    }
                    let x0 = ui.cursor().min.x;

                    for (name, tip, act) in [
                        ("tick", Msg::TipApply, Act::Apply),
                        ("disk-black", Msg::TipSave, Act::Save),
                        ("disks-black", Msg::TipSaveAs, Act::SaveAs),
                        ("clipboard", Msg::TipCopy, Act::Copy),
                        ("drive-globe", Msg::TipUpload, Act::Upload),
                        ("printer", Msg::TipPrint, Act::Print),
                    ] {
                        if ui
                            .add(egui::Button::image(icons.img(name)))
                            .on_hover_text(t(tip))
                            .clicked()
                        {
                            acts.push(act);
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    for t in Tool::ALL {
                        // `Button::selected` takes the background from
                        // `selection.bg_fill`, but the border still from the widget
                        // state — which we cleared so the icons stay flat. And
                        // `Button::stroke` would inflate the button by 2px, because egui
                        // subtracts the width from the inner margin only for the outline
                        // from the style. So we draw the active tool's border ourselves,
                        // over the button once it is laid out: the bar's width is untouched.
                        let on = self.tool == t;
                        let r = ui
                            .add(egui::Button::image(icons.img(t.icon())).selected(on))
                            .on_hover_text(t.tooltip());
                        if on {
                            let cr = ui.visuals().widgets.inactive.corner_radius;
                            let s = egui::Stroke::new(1.0, SEL_BORDER);
                            ui.painter().rect_stroke(r.rect, cr, s, egui::StrokeKind::Inside);
                        }
                        if r.clicked() {
                            self.pick(t);
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // the outline has a hole in the middle, the fill and the highlight
                    // do not — exactly the three icons from ShareX
                    color_button(ui, "outline", self.border_mut(), 8.0, t(Msg::TipBorderColor));
                    color_button(ui, "fill", self.fill_mut(), 0.0, t(Msg::TipFillColor));
                    color_button(
                        ui,
                        "highlight",
                        &mut self.opt.highlight,
                        0.0,
                        t(Msg::TipHighlightColor),
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
        // the status bar shows up only when it has something to say: an empty
        // row would otherwise leave a useless gap under the image
        if !self.status.is_empty() {
            ui.vertical_centered(|ui| ui.small(self.status.clone()));
        }
    }

    fn menu_opts(&mut self, ui: &mut egui::Ui, icons: &Icons) {
        let o = &mut self.opt;
        let mut start = o.step_start;
        // the language chosen from the menu, applied after the borrow on `self` ends
        let mut pick_lang = None;
        ui.menu_image_button(icons.img("layer--pencil"), |ui| {
            ui.set_min_width(240.0);
            ui.add(egui::Slider::new(&mut o.border_size, 1.0..=32.0).text(t(Msg::SldBorderSize)));
            ui.add(egui::Slider::new(&mut o.corner_radius, 0.0..=32.0).text(t(Msg::SldCornerRadius)));
            ui.add(egui::Slider::new(&mut o.line_mid, 0..=shape::MAX_MID).text(t(Msg::SldCenterPoints)));
            ui.add(egui::Slider::new(&mut o.pixelate, 2.0..=64.0).text(t(Msg::SldPixelSize)));
            ui.add(egui::Slider::new(&mut o.blur, 1.0..=100.0).text(t(Msg::SldBlurStrength)));
            ui.add(egui::Slider::new(&mut o.magnify, 110.0..=800.0).text(t(Msg::SldMagnifyStrength)));
            ui.add(egui::Slider::new(&mut o.text.size, 8.0..=72.0).text(t(Msg::SldFontSize)));
            ui.add(egui::Slider::new(&mut o.step_font, 8.0..=72.0).text(t(Msg::SldStepFontSize)));
            ui.add(egui::Slider::new(&mut start, 1..=100).text(t(Msg::SldStepStart)));
            ui.checkbox(&mut o.shadow, t(Msg::ChkDropShadow));
            // right at the bottom, set apart from the rest: the language choice
            // applies on the spot and is written to the configuration file
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(t(Msg::LangSelector));
                let cur = i18n::lang();
                for l in i18n::Lang::ALL {
                    if ui.selectable_label(cur == l, t(l.label())).clicked() {
                        pick_lang = Some(l);
                        ui.close();
                    }
                }
            });
        })
        .response
        .on_hover_text(t(Msg::MenuToolOptions));
        if start != self.opt.step_start {
            self.opt.step_start = start;
            self.step_next = start;
        }
        if let Some(l) = pick_lang {
            i18n::set_lang_saved(l);
        }
    }

    fn menu_edit(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        ui.menu_image_button(icons.img("wrench-screwdriver"), |ui| {
            let mut item = |ui: &mut egui::Ui, icon: &str, m: Msg, a: Act| {
                if ui
                    .add(egui::Button::image_and_text(icons.img(icon), t(m)))
                    .clicked()
                {
                    acts.push(a);
                    ui.close();
                }
            };
            item(ui, "arrow-circle-225-left", Msg::ItUndo, Act::Undo);
            item(ui, "arrow-circle-315", Msg::ItRedo, Act::Redo);
            item(ui, "document-copy", Msg::ItDuplicate, Act::Dup);
            ui.separator();
            item(ui, "layer--minus", Msg::ItDelete, Act::Del);
            item(ui, "eraser", Msg::ItDeleteAll, Act::DelAll);
            ui.separator();
            item(ui, "layers-stack-arrange", Msg::ItToFront, Act::Front);
            item(ui, "layers-arrange", Msg::ItForward, Act::Forward);
            item(ui, "layers-arrange-back", Msg::ItBackward, Act::Backward);
            item(ui, "layers-stack-arrange-back", Msg::ItToBack, Act::Back);
        })
        .response
        .on_hover_text(t(Msg::MenuEdit));
    }

    fn menu_image(&mut self, ui: &mut egui::Ui, icons: &Icons, acts: &mut Vec<Act>) {
        ui.menu_image_button(icons.img("image--pencil"), |ui| {
            ui.set_min_width(230.0);
            let mut item = |ui: &mut egui::Ui, icon: &str, m: Msg, a: Act| {
                if ui
                    .add(egui::Button::image_and_text(icons.img(icon), t(m)))
                    .clicked()
                {
                    acts.push(a);
                    ui.close();
                }
            };
            item(ui, "image-empty", Msg::ItNewImage, Act::NewImg);
            item(ui, "folder-open-image", Msg::ItOpenImage, Act::OpenImg);
            item(ui, "image--plus", Msg::ItInsertFile, Act::InsertFile);
            item(ui, "camera", Msg::ItInsertScreen, Act::InsertScreen);
            ui.separator();
            item(ui, "image-select", Msg::ItImageSize, Act::ImgSize);
            item(ui, "image-resize", Msg::ItCanvasSize, Act::CanvasSize);
            item(ui, "image-crop", Msg::ItCropImage, Act::CropTool);
            item(ui, "image-resize-actual", Msg::ItAutoCrop, Act::AutoCrop);
            ui.separator();
            item(ui, "arrow-circle", Msg::ItRotateRight, Act::RotRight);
            item(ui, "arrow-circle-135-left", Msg::ItRotateLeft, Act::RotLeft);
        })
        .response
        .on_hover_text(t(Msg::MenuImage));
    }

    // --------------------------------------------------------------- canvas

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (iw, ih) = (self.img.width() as f32, self.img.height() as f32);
        let avail = ui.available_size();
        let zoom = (avail.x / iw).min(avail.y / ih).min(1.0).max(0.05);
        let size = Vec2::new(iw * zoom, ih * zoom);
        // We allocate all the remaining space and center the image in it: on a
        // window resize it stays in the middle, not stuck to the top-left corner.
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let img_rect = Rect::from_center_size(resp.rect.center(), size);
        let origin = img_rect.min;
        let to_screen = move |p: Pos2| origin + p.to_vec2() * zoom;
        // The cursor is clamped to the image, so shapes cannot end up outside it.
        let to_img = move |p: Pos2| ((img_rect.clamp(p) - origin) / zoom).to_pos2();
        let tol = 6.0 / zoom;
        // the node is grabbed over its whole box, as in ShareX
        let htol = NODE_HIT / 2.0 / zoom;

        // The shape under the cursor: it gets the animated border, like `CurrentHoverShape`.
        let hover = resp.hover_pos().and_then(|sp| self.shape_at(to_img(sp), tol));
        // Over a node the cursor becomes an open hand; while you drag it, closed
        // (`SetHandCursor` from ShareX).
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
            // the modal dialog keeps the canvas inactive
        } else if self.tool == Tool::Select {
            self.select_input(&resp, to_img, tol, htol);
        } else {
            // The order on press is the one from `StartRegionSelection`: first
            // the active shape's nodes, then the shape under the cursor — which
            // gets selected and starts moving, whatever the tool — and only if
            // there is nothing there is a new shape started.
            if resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let at = to_img(p);
                    if let Some(i) = self.shape_at(at, tol) {
                        self.sel = Some(i);
                    } else if matches!(self.tool, Tool::Step) || self.tool.is_text() {
                        // the step counter and the text boxes are placed with one click
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
                if let Some(p) = Self::press_at(ui.ctx(), &resp) {
                    let at = to_img(p);
                    if let Some(idx) = self.handle_at(at, htol) {
                        self.push_undo(false);
                        self.drag = Some(Drag::Handle { idx });
                    } else if let Some(i) = self.shape_at(at, tol) {
                        self.sel = Some(i);
                        self.push_undo(false);
                        self.drag = Some(Drag::Move { last: at });
                    } else {
                        self.draft = self.new_draft(at);
                    }
                }
            }
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let at = to_img(p);
                    match self.drag {
                        Some(Drag::Handle { idx }) => {
                            if let Some(s) = self.sel.and_then(|i| self.shapes.get_mut(i)) {
                                s.move_handle(idx, at);
                            }
                        }
                        Some(Drag::Move { last }) => {
                            if let Some(s) = self.sel.and_then(|i| self.shapes.get_mut(i)) {
                                s.translate(at - last);
                            }
                            self.drag = Some(Drag::Move { last: at });
                        }
                        None => self.update_draft(at),
                    }
                }
            }
            if resp.drag_stopped() {
                if self.drag.take().is_none() {
                    self.commit_draft();
                }
            }
        }

        // the spotlight: a single dark layer with holes for all the
        // rectangles, at the position of the first spotlight in the list (the
        // draft being dragged joins the union too)
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

        // ShareX's "marching ants" border: a solid black line, with white
        // dashes of 5 on 5 over it, the offset moving at 15 px/s
        // (`borderDotPen.DashOffset = elapsed * -15`). It shows ONLY while you
        // hold the mouse over the shape, not all the time.
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
            // the offset is kept within a single period (dash + gap = 10): given
            // raw, it would start drawing well before the corner and leave an
            // ever longer white stripe to the left
            let t = (ui.ctx().input(|i| i.time) as f32 * -15.0).rem_euclid(10.0);
            painter.add(egui::Shape::line(pts.to_vec(), egui::Stroke::new(1.0, Color32::BLACK)));
            painter.extend(egui::Shape::dashed_line_with_offset(
                &pts,
                egui::Stroke::new(1.0, Color32::WHITE),
                &[5.0],
                &[5.0],
                t,
            ));
            // the dashes move, so we ask for frames while the border is visible
            ui.ctx().request_repaint();
        }

        if let Some(i) = self.sel {
            if let Some(s) = self.shapes.get(i) {
                // All shapes have the same nodes: the solid white dots from
                // `CircleNode.png`, not squares — ShareX pastes the same image
                // into every corner, whatever the tool.
                for hp in s.handles() {
                    painter.circle_filled(to_screen(hp), NODE / 2.0, Color32::WHITE);
                }
            }
        }
    }

    /// The topmost shape under the given point, like `GetIntersectShape`.
    fn shape_at(&self, p: Pos2, tol: f32) -> Option<usize> {
        self.shapes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, s)| s.hit(p, tol))
            .map(|(i, _)| i)
    }

    /// The place where the button was pressed, not where the cursor ended up:
    /// `drag_started` only fires after the mouse has moved more than 6px from the
    /// press, and `interact_pointer_pos` gives the current position. Without this,
    /// the nodes slip through your fingers and every shape starts 6px further off.
    fn press_at(ctx: &egui::Context, resp: &egui::Response) -> Option<Pos2> {
        ctx.input(|i| i.pointer.press_origin())
            .or_else(|| resp.interact_pointer_pos())
    }

    /// The index of the selected shape's node under the given point, if there is one.
    fn handle_at(&self, p: Pos2, tol: f32) -> Option<usize> {
        let s = self.shapes.get(self.sel?)?;
        // ShareX checks `Rectangle.Contains`, that is the node's whole square
        // box, not a circle — on the diagonal it forgives about 40% more
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
        // a double click on a text box reopens it for editing
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
            if let Some(sp) = Self::press_at(&resp.ctx, resp) {
                let p = to_img(sp);
                let mut started = None;
                // first the handles of the already selected shape, so resizing
                // takes priority over selecting another shape underneath
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
        // a plain click on empty space deselects
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

/// Rescaling with Lanczos3, as in ShareX.
pub fn resize_img(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    image::imageops::resize(img, w.max(1), h.max(1), image::imageops::FilterType::Lanczos3)
}

/// A new `w`x`h` canvas, filled with `fill`, with the old image placed in the
/// center. If the canvas is smaller, the image is cut symmetrically.
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

/// The rectangle left after trimming the uniform edges, taking the top-left
/// corner pixel as reference and a tolerance of 10 per channel. `None` if there
/// is nothing to trim (or if the whole image is uniform).
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

/// Takes a band out of the image and glues the two remaining halves together.
/// The band is horizontal (it shrinks the height) if the rectangle is wider
/// than tall, otherwise vertical. Returns the new image, the orientation, where
/// the band ended in the old coordinates and its thickness; `None` if the band
/// is too thin or would swallow the whole image.
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
        // the fonts sent last frame are now installed in egui
        font::sync();

        // the capture requested on the previous frame: the minimize has now
        // reached the compositor, so our window no longer shows up in the picture
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
