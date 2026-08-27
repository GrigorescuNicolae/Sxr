mod app;
mod capture;
mod clip;
mod font;
mod icons;
mod render;
mod shape;

fn main() {
    if let Err(e) = real_main() {
        eprintln!("sxr: {e:#}");
        std::process::exit(1);
    }
}

use anyhow::Context as _;

fn real_main() -> anyhow::Result<()> {
    let arg = std::env::args().nth(1);

    // moduri interne, nu apar in --help
    if arg.as_deref() == Some(clip::SERVE_FLAG) {
        return clip::serve_from_stdin();
    }
    if arg.as_deref() == Some("--paste-check") {
        for m in clip::mime_types()? {
            println!("{m}");
        }
        return Ok(());
    }
    if arg.as_deref() == Some("--paste-save") {
        let path = std::env::args().nth(2).context("lipseste calea")?;
        let n = clip::paste_to_file(&path)?;
        println!("{n} octeti salvati in {path}");
        return Ok(());
    }
    if arg.as_deref() == Some("--render-test") {
        let path = std::env::args().nth(2).context("lipseste calea")?;
        return render_test(&path);
    }
    if arg.as_deref() == Some("--arrow-test") {
        let out = std::env::args().nth(2).unwrap_or_else(|| "/tmp/arrow.png".into());
        return arrow_test(&out);
    }
    if arg.as_deref() == Some("--step-test") {
        let out = std::env::args().nth(2).unwrap_or_else(|| "/tmp/step.png".into());
        return step_test(&out);
    }
    if arg.as_deref() == Some("--flip-test") {
        let dir = std::env::args().nth(2).context("lipseste directorul de iesire")?;
        return flip_test(&dir);
    }
    if arg.as_deref() == Some("--curve-test") {
        let dir = std::env::args().nth(2).context("lipseste directorul de iesire")?;
        return curve_test(&dir);
    }
    if arg.as_deref() == Some("--text-test") {
        let dir = std::env::args().nth(2).context("lipseste directorul de iesire")?;
        return text_test(&dir);
    }
    if arg.as_deref() == Some("--cutout-test") {
        let src = std::env::args().nth(2).context("lipseste calea de intrare")?;
        let dst = std::env::args().nth(3).context("lipseste calea de iesire")?;
        return cutout_test(&src, &dst);
    }
    if arg.as_deref() == Some("--img-test") {
        let src = std::env::args().nth(2).context("lipseste calea de intrare")?;
        let dir = std::env::args().nth(3).context("lipseste directorul de iesire")?;
        return img_test(&src, &dir);
    }
    if arg.as_deref() == Some("--copy-test") {
        let path = std::env::args().nth(2).context("lipseste calea")?;
        let img = image::open(&path)?.to_rgba8();
        return clip::copy_png(render::compose(&img, &[])?);
    }

    let img = match arg.as_deref() {
        Some("-h") | Some("--help") => {
            println!("sxr — captura si adnotare\n");
            println!("  sxr              selecteaza o regiune de pe ecran, apoi deschide editorul");
            println!("  sxr <fisier>     deschide direct o imagine existenta");
            return Ok(());
        }
        Some(path) => image::open(path)
            .map_err(|e| anyhow::anyhow!("nu pot deschide {path}: {e}"))?
            .to_rgba8(),
        None => {
            let img = capture::select_region()?;
            // auto-copy imediat: daca inchizi fara sa editezi, captura e deja in clipboard
            match render::compose(&img, &[]).and_then(clip::copy_png) {
                Ok(()) => {}
                Err(e) => eprintln!("sxr: auto-copy esuat: {e:#}"),
            }
            img
        }
    };
    app::run(img)
}

/// Mod ascuns de verificare: construieste cate o forma din fiecare varianta
/// peste o imagine generata si scrie PNG-ul rezultat. Neinteractiv, fara fereastra.
/// Randează numărătorul pentru mai multe valori, pe fundal alb, ca să se poată
/// măsura programatic spațiul dintre cifră și marginea cercului.
/// Săgeți la mai multe grosimi, pe fundal alb, ca să se poată măsura capul.

fn arrow_test(path: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2};
    use shape::Shape;

    let mut img = image::RgbaImage::new(520, 300);
    for px in img.pixels_mut() {
        px.0 = [255, 255, 255, 255];
    }
    let red = Color32::from_rgb(220, 30, 30);
    let shapes: Vec<Shape> = [2.0f32, 4.0, 8.0]
        .iter()
        .enumerate()
        .map(|(i, w)| Shape::Arrow {
            from: Pos2::new(40.0, 50.0 + i as f32 * 100.0),
            to: Pos2::new(460.0, 50.0 + i as f32 * 100.0),
            mid: Vec::new(),
            curbat: false,
            color: red,
            width: *w,
        })
        .collect();
    let png = render::compose_opts(&img, &shapes, false)?;
    std::fs::write(path, png)?;
    println!("scris {path}");
    Ok(())
}

fn step_test(path: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2};
    use shape::Shape;

    let (w, h) = (700u32, 140u32);
    let mut img = image::RgbaImage::new(w, h);
    for px in img.pixels_mut() {
        px.0 = [255, 255, 255, 255];
    }
    let red = Color32::from_rgb(220, 30, 30);
    let shapes: Vec<Shape> = [1u32, 9, 10, 25, 100]
        .iter()
        .enumerate()
        .map(|(i, n)| Shape::Step {
            center: Pos2::new(80.0 + i as f32 * 130.0, 70.0),
            n: *n,
            size: 22.0,
            fill: red,
            border: Color32::TRANSPARENT,
            text: Color32::WHITE,
            width: 0.0,
        })
        .collect();
    for n in [1u32, 9, 10, 25, 100] {
        println!("n={n} raza={:.2}", shape::step_radius(n, 22.0));
    }
    let png = render::compose_opts(&img, &shapes, false)?;
    std::fs::write(path, png)?;
    println!("scris {path}");
    Ok(())
}

fn render_test(path: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2, Rect};
    use shape::Shape;
    use std::sync::Arc;

    let (w, h) = (800u32, 600u32);
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        // degrade + tabla de sah pe toate canalele: asa exista si detaliu de
        // frecventa inalta, nu doar gradient, ca pixelarea, estomparea si
        // evidentierea sa aiba ce amesteca. Gradientele stau sub 160 ca sa
        // ramana loc pentru tabla de sah fara saturare.
        let chk = if ((x / 20) + (y / 20)) % 2 == 0 { 70 } else { 0 };
        let mix = |v: u32| (v + chk).min(255) as u8;
        px.0 = [mix(x * 160 / w), mix(y * 160 / h), mix(60), 255];
    }

    // Cursorul: îl punem mărit de 4 ori (16x24 -> 64x96) ca să se vadă și
    // scalarea, și că zonele transparente rămân transparente. Stă în gaura
    // reflectorului, unde fundalul e neatins, ca verificarea alfa să fie curată.
    let cursor = image::load_from_memory(include_bytes!("../assets/cursor.png"))
        .context("cursor.png")?
        .to_rgba8();
    // context fără fereastră: `load_texture` lucrează doar pe CPU, deci merge
    let ctx = eframe::egui::Context::default();
    let cursor_tex = ctx.load_texture(
        "cursor",
        eframe::egui::ColorImage::from_rgba_unmultiplied(
            [cursor.width() as usize, cursor.height() as usize],
            cursor.as_raw(),
        ),
        eframe::egui::TextureOptions::LINEAR,
    );

    let red = shape::PRIMARY;
    let white = shape::SECONDARY;
    // guma isi ia culoarea din inelul din jurul dreptunghiului, o singura data
    let erase_rect = Rect::from_min_max(Pos2::new(240.0, 440.0), Pos2::new(420.0, 580.0));
    let shapes = vec![
        // reflectorul primul: stratul intunecat ramane sub restul formelor
        Shape::Spotlight {
            rect: Rect::from_min_max(Pos2::new(460.0, 150.0), Pos2::new(770.0, 250.0)),
        },
        Shape::Rect {
            rect: Rect::from_min_max(Pos2::new(20.0, 20.0), Pos2::new(220.0, 140.0)),
            border: red,
            fill: Color32::TRANSPARENT,
            width: 4.0,
            radius: 3.0,
        },
        Shape::Ellipse {
            rect: Rect::from_min_max(Pos2::new(250.0, 20.0), Pos2::new(450.0, 140.0)),
            border: Color32::from_rgb(40, 200, 90),
            fill: Color32::from_rgb(20, 60, 30),
            width: 4.0,
        },
        Shape::Line {
            from: Pos2::new(480.0, 30.0),
            to: Pos2::new(760.0, 130.0),
            mid: Vec::new(),
            curbat: false,
            color: Color32::from_rgb(60, 140, 255),
            width: 5.0,
        },
        Shape::Arrow {
            from: Pos2::new(480.0, 130.0),
            to: Pos2::new(760.0, 30.0),
            mid: Vec::new(),
            curbat: false,
            color: red,
            width: 5.0,
        },
        Shape::Free {
            pts: (0..40)
                .map(|i| {
                    let t = i as f32;
                    Pos2::new(30.0 + t * 5.0, 200.0 + (t / 3.0).sin() * 30.0)
                })
                .collect(),
            color: Color32::from_rgb(255, 180, 40),
            width: 4.0,
            arrow: true,
        },
        Shape::Text {
            rect: Rect::from_min_max(Pos2::new(30.0, 270.0), Pos2::new(360.0, 330.0)),
            text: "Contur".into(),
            opts: shape::TextOpts { size: 25.0, bold: true, ..Default::default() },
            fill: Color32::TRANSPARENT,
            outline: red,
            outline_w: 5.0,
            radius: 0.0,
        },
        Shape::Text {
            rect: Rect::from_min_max(Pos2::new(400.0, 270.0), Pos2::new(720.0, 330.0)),
            text: "Fundal".into(),
            opts: shape::TextOpts::default(),
            fill: red,
            outline: Color32::TRANSPARENT,
            outline_w: 0.0,
            radius: 3.0,
        },
        // lupa peste zona cu tabla de sah si degrade, ca maritura sa se vada
        Shape::Magnify {
            rect: Rect::from_min_max(Pos2::new(250.0, 160.0), Pos2::new(450.0, 250.0)),
            strength: 200.0,
            border: white,
            width: 3.0,
        },
        Shape::Step {
            center: Pos2::new(80.0, 400.0),
            n: 1,
            size: 18.0,
            fill: red,
            border: white,
            text: Color32::WHITE,
            width: 0.0,
        },
        Shape::Highlight {
            rect: Rect::from_min_max(Pos2::new(150.0, 370.0), Pos2::new(420.0, 430.0)),
            color: Color32::YELLOW,
        },
        Shape::Pixelate {
            rect: Rect::from_min_max(Pos2::new(460.0, 360.0), Pos2::new(760.0, 560.0)),
            block: 15.0,
        },
        Shape::Blur {
            rect: Rect::from_min_max(Pos2::new(20.0, 440.0), Pos2::new(220.0, 580.0)),
            radius: 35.0,
        },
        Shape::Erase {
            rect: erase_rect,
            color: shape::ring_avg(&img, erase_rect),
        },
        Shape::Img {
            rect: Rect::from_min_max(Pos2::new(620.0, 152.0), Pos2::new(684.0, 248.0)),
            img: Arc::new(cursor),
            tex: cursor_tex,
        },
        // balonul ultimul: sta deasupra pixelarii, cu coada in jos
        Shape::Balloon {
            rect: Rect::from_min_max(Pos2::new(470.0, 370.0), Pos2::new(700.0, 430.0)),
            tail: Pos2::new(470.0, 460.0),
            text: "Salut!".into(),
            opts: shape::TextOpts::default(),
            fill: red,
            border: white,
            width: 2.0,
            radius: 3.0,
        },
    ];

    let png = render::compose(&img, &shapes)?;
    std::fs::write(path, &png).with_context(|| format!("scriere {path}"))?;
    println!("{} octeti scrisi in {path}", png.len());
    Ok(())
}

/// Mod ascuns de verificare pentru dreptunghiurile trase invers: construieste
/// fiecare forma bazata pe dreptunghi de doua ori peste acelasi fundal — o data
/// cu `min`/`max` in ordine normala si o data inversate, exact ce lasa in urma o
/// tragere de la dreapta la stanga — apoi scrie cate un PNG pentru fiecare.
/// Cele doua trebuie sa iasa pixel cu pixel identice. Tot aici numaram si cate
/// primitive scoate calea de previzualizare (`Shape::draw`) in fiecare caz:
/// preview-ul si exportul trebuie sa se comporte la fel.
fn flip_test(dir: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2, Rect};
    use shape::Shape;
    use std::sync::Arc;

    let (w, h) = (320u32, 240u32);
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let chk = if ((x / 16) + (y / 16)) % 2 == 0 { 70 } else { 0 };
        let mix = |v: u32| (v + chk).min(255) as u8;
        px.0 = [mix(x * 160 / w), mix(y * 160 / h), mix(60), 255];
    }

    let cursor = image::load_from_memory(include_bytes!("../assets/cursor.png"))
        .context("cursor.png")?
        .to_rgba8();
    // context fara fereastra: layout-ul si texturile lucreaza doar pe CPU
    let ctx = eframe::egui::Context::default();
    font::install(&ctx);
    let cursor_tex = ctx.load_texture(
        "cursor",
        eframe::egui::ColorImage::from_rgba_unmultiplied(
            [cursor.width() as usize, cursor.height() as usize],
            cursor.as_raw(),
        ),
        eframe::egui::TextureOptions::LINEAR,
    );
    let cursor = Arc::new(cursor);

    let red = shape::PRIMARY;
    let white = shape::SECONDARY;
    let (a, b) = (Pos2::new(60.0, 50.0), Pos2::new(260.0, 190.0));

    // acelasi dreptunghi, o data normal si o data cu colturile schimbate intre ele
    let build = |r: Rect| -> Vec<(&'static str, Shape)> {
        vec![
            ("rect", Shape::Rect { rect: r, border: red, fill: Color32::TRANSPARENT, width: 4.0, radius: 3.0 }),
            ("elipsa", Shape::Ellipse { rect: r, border: red, fill: Color32::from_rgb(20, 60, 30), width: 4.0 }),
            ("text", Shape::Text {
                rect: r,
                text: "Salut".into(),
                opts: shape::TextOpts { size: 25.0, bold: true, ..Default::default() },
                fill: red,
                outline: white,
                outline_w: 3.0,
                radius: 3.0,
            }),
            ("balon", Shape::Balloon {
                rect: r,
                tail: Pos2::new(40.0, 220.0),
                text: "Salut".into(),
                opts: shape::TextOpts::default(),
                fill: red,
                border: white,
                width: 2.0,
                radius: 3.0,
            }),
            ("lupa", Shape::Magnify { rect: r, strength: 200.0, border: white, width: 3.0 }),
            ("evidentiere", Shape::Highlight { rect: r, color: Color32::YELLOW }),
            ("pixelare", Shape::Pixelate { rect: r, block: 15.0 }),
            ("blur", Shape::Blur { rect: r, radius: 35.0 }),
            ("reflector", Shape::Spotlight { rect: r }),
            ("guma", Shape::Erase { rect: r, color: shape::ring_avg(&img, r) }),
            ("imagine", Shape::Img { rect: r, img: cursor.clone(), tex: cursor_tex.clone() }),
        ]
    };

    std::fs::create_dir_all(dir).with_context(|| format!("creare {dir}"))?;
    let save = |name: &str, data: &[u8]| -> anyhow::Result<()> {
        let p = std::path::Path::new(dir).join(name);
        std::fs::write(&p, data).with_context(|| format!("scriere {}", p.display()))?;
        Ok(())
    };
    save("fundal.png", &render::compose(&img, &[])?)?;

    let normal = build(Rect::from_min_max(a, b));
    let flipped = build(Rect::from_min_max(b, a));
    for ((name, ns), (_, fs)) in normal.into_iter().zip(flipped) {
        save(&format!("{name}-normal.png"), &render::compose(&img, &[ns.clone()])?)?;
        save(&format!("{name}-inversat.png"), &render::compose(&img, &[fs.clone()])?)?;
        println!(
            "{name} preview normal={} inversat={}",
            preview_count(&ctx, &img, &ns),
            preview_count(&ctx, &img, &fs)
        );
    }
    println!("scris in {dir}");
    Ok(())
}

/// Cate primitive de desen scoate calea de previzualizare pentru o forma.
/// Rulam de doua ori: primul cadru doar aseaza fonturile si layout-ul.
fn preview_count(ctx: &eframe::egui::Context, img: &image::RgbaImage, s: &shape::Shape) -> usize {
    let mut n = 0;
    for _ in 0..2 {
        let out = ctx.run_ui(Default::default(), |ui| {
            let p = ui.painter().clone();
            s.draw(&p, |q| q, 1.0, img);
        });
        n = out.shapes.len();
    }
    n
}

/// Mod ascuns de verificare pentru taierea de fasie: o taietura orizontala
/// si una verticala peste imaginea data, apoi scrie rezultatul.
fn cutout_test(src: &str, dst: &str) -> anyhow::Result<()> {
    use eframe::egui::{Pos2, Rect};

    let img = image::open(src)
        .map_err(|e| anyhow::anyhow!("nu pot deschide {src}: {e}"))?
        .to_rgba8();
    let (w0, h0) = img.dimensions();
    println!("intrare {w0}x{h0}");

    // fasie orizontala: mai lata decat inalta, deci scade inaltimea
    let hr = Rect::from_min_max(
        Pos2::new(10.0, 100.0),
        Pos2::new(w0 as f32 - 10.0, 140.0),
    );
    let (img, horiz, end, band) = app::cut_out(&img, hr).context("taietura orizontala esuata")?;
    println!("orizontala={horiz} grosime={band} sfarsit={end} -> {}x{}", img.width(), img.height());

    // fasie verticala: mai inalta decat lata, deci scade latimea
    let vr = Rect::from_min_max(
        Pos2::new(60.0, 5.0),
        Pos2::new(85.0, img.height() as f32 - 5.0),
    );
    let (img, horiz, end, band) = app::cut_out(&img, vr).context("taietura verticala esuata")?;
    println!("orizontala={horiz} grosime={band} sfarsit={end} -> {}x{}", img.width(), img.height());

    img.save(dst).with_context(|| format!("scriere {dst}"))?;
    println!("iesire {}x{} in {dst}", img.width(), img.height());
    Ok(())
}

/// Mod ascuns de verificare pentru operatiile din meniul „Imagine": ruleaza pe
/// rand rotirile, decuparea automata, injumatatirea si marirea panzei, scriind
/// cate un PNG in directorul dat. Neinteractiv, fara fereastra.
fn img_test(src: &str, dir: &str) -> anyhow::Result<()> {
    use eframe::egui::Color32;

    let img = image::open(src)
        .map_err(|e| anyhow::anyhow!("nu pot deschide {src}: {e}"))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    std::fs::create_dir_all(dir).with_context(|| format!("creare {dir}"))?;
    println!("intrare {w}x{h}");

    let save = |name: &str, im: &image::RgbaImage| -> anyhow::Result<()> {
        let p = std::path::Path::new(dir).join(name);
        im.save(&p).with_context(|| format!("scriere {}", p.display()))?;
        println!("{name} {}x{}", im.width(), im.height());
        Ok(())
    };

    save("rot-dreapta.png", &image::imageops::rotate90(&img))?;
    save("rot-stanga.png", &image::imageops::rotate270(&img))?;
    match app::auto_crop_rect(&img) {
        Some((x, y, cw, ch)) => {
            println!("decupare automata: x={x} y={y} {cw}x{ch}");
            save("decupare-auto.png", &image::imageops::crop_imm(&img, x, y, cw, ch).to_image())?;
        }
        None => println!("decupare automata: nu exista margini uniforme de taiat"),
    }
    save("jumatate.png", &app::resize_img(&img, w / 2, h / 2))?;
    save(
        "panza.png",
        &app::canvas_img(&img, w + 200, h + 200, Color32::from_rgb(255, 0, 255)),
    )?;
    Ok(())
}

/// Mod ascuns de verificare a textului: aceeași propoziție randată în toate
/// combinațiile de stil și aliniere, o dată cu DejaVu Sans și o dată cu prima
/// familie de sistem găsită, ca să se vadă că fontul chiar s-a încărcat.
/// Mod ascuns pentru liniile și săgețile curbate: aceleași capete, o dată
/// drept și o dată cu nodul din mijloc tras în sus, pe fundal alb, ca
/// diferențele să se poată măsura programatic.
fn curve_test(dir: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2};
    use shape::Shape;

    let dir = std::path::Path::new(dir);
    std::fs::create_dir_all(dir)?;

    let mut img = image::RgbaImage::new(500, 220);
    for px in img.pixels_mut() {
        px.0 = [255, 255, 255, 255];
    }
    let red = Color32::from_rgb(220, 30, 30);
    let (from, to) = (Pos2::new(60.0, 160.0), Pos2::new(440.0, 160.0));
    let nod = Pos2::new(250.0, 50.0);
    let lat = 6.0f32;

    // linia dreaptă are și ea un nod de mijloc, doar că neatins: se așază
    // singur pe coardă, exact ca `AutoPositionCenterPoints` din ShareX
    let mut drept = vec![Pos2::ZERO];
    shape::auto_mid(from, to, &mut drept, false);
    println!("nod auto pe linia dreapta: ({:.1}, {:.1})", drept[0].x, drept[0].y);
    println!("nod tras: ({:.1}, {:.1})", nod.x, nod.y);

    let save = |name: &str, s: Shape| -> anyhow::Result<()> {
        let png = render::compose_opts(&img, &[s], false)?;
        let p = dir.join(name);
        std::fs::write(&p, png)?;
        println!("scris {}", p.display());
        Ok(())
    };

    let linie = |mid: Vec<Pos2>, curbat: bool| Shape::Line { from, to, mid, curbat, color: red, width: lat };
    let sageata = |mid: Vec<Pos2>, curbat: bool| Shape::Arrow { from, to, mid, curbat, color: red, width: lat };
    // trei noduri de curbură, trase alternativ sus și jos
    let trei = vec![Pos2::new(155.0, 60.0), Pos2::new(250.0, 195.0), Pos2::new(345.0, 60.0)];

    save("linie-dreapta.png", linie(drept.clone(), false))?;
    save("linie-curbata.png", linie(vec![nod], true))?;
    save("sageata-dreapta.png", sageata(drept.clone(), false))?;
    save("sageata-curbata.png", sageata(vec![nod], true))?;
    save("curba-3.png", linie(trei.clone(), true))?;

    // cifrele geometrice, ca scriptul de verificare sa aiba cu ce compara
    let lung = |p: &[Pos2]| -> f32 { p.windows(2).map(|w| w[0].distance(w[1])).sum() };
    let po_d = shape::curve_poly(from, to, &drept, false);
    let po_c = shape::curve_poly(from, to, &[nod], true);
    let po_3 = shape::curve_poly(from, to, &trei, true);
    println!(
        "lungime curba: dreapta={:.1} curbata={:.1} trei-noduri={:.1}",
        lung(&po_d),
        lung(&po_c),
        lung(&po_3)
    );
    // cat de aproape trece curba de nodul tras (ar trebui sa fie zero)
    let apropiere = po_c
        .iter()
        .map(|q| q.distance(nod))
        .fold(f32::INFINITY, f32::min);
    println!("distanta curbei pana la nodul tras: {apropiere:.3} px");

    let unghi = |c: [Pos2; 4]| ((c[0].y - c[2].y).atan2(c[0].x - c[2].x)).to_degrees();
    let (coada_d, cap_d) = shape::arrow_geom(&po_d, lat);
    let (coada_c, cap_c) = shape::arrow_geom(&po_c, lat);
    println!(
        "unghi cap sageata: dreapta={:.2} grade, curbata={:.2} grade",
        unghi(cap_d),
        unghi(cap_c)
    );
    println!(
        "coada se opreste la: dreapta=({:.1}, {:.1}) curbata=({:.1}, {:.1})",
        coada_d[coada_d.len() - 1].x,
        coada_d[coada_d.len() - 1].y,
        coada_c[coada_c.len() - 1].x,
        coada_c[coada_c.len() - 1].y
    );
    println!(
        "retragere coada pe curba: {:.2} px (asteptat {:.2})",
        lung(&po_c) - lung(&coada_c),
        5.0 * lat
    );
    Ok(())
}

fn text_test(dir: &str) -> anyhow::Result<()> {
    use eframe::egui::{Color32, Pos2, Rect};
    use shape::{Align, Shape, TextOpts};

    let dir = std::path::Path::new(dir);
    std::fs::create_dir_all(dir)?;

    // fundal negru: cerneala e albă, deci se numără ușor pixelii aprinși
    let mut img = image::RgbaImage::new(420, 200);
    for px in img.pixels_mut() {
        px.0 = [0, 0, 0, 255];
    }
    let rect = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(410.0, 190.0));
    let txt = "Ana are mere";

    // prima familie de sistem diferită de implicita, ca să se compare cu ea
    let alt = font::families()
        .into_iter()
        .find(|f| *f != font::DEFAULT_FAMILY)
        .unwrap_or(font::DEFAULT_FAMILY)
        .to_owned();
    println!("familie-sistem: {alt}");

    let save = |name: &str, o: &TextOpts| -> anyhow::Result<()> {
        let sh = Shape::Text {
            rect,
            text: txt.into(),
            opts: o.clone(),
            fill: Color32::TRANSPARENT,
            outline: Color32::TRANSPARENT,
            outline_w: 0.0,
            radius: 0.0,
        };
        let png = render::compose_opts(&img, &[sh], false)?;
        let p = dir.join(name);
        std::fs::write(&p, png)?;
        println!("scris {}", p.display());
        Ok(())
    };

    // baza: familia de sistem, 28 px, aliniere stânga-sus, ca diferențele
    // de stil să nu fie amestecate cu cele de așezare
    let base = TextOpts {
        family: alt.clone(),
        size: 28.0,
        color: Color32::WHITE,
        halign: Align::Near,
        valign: Align::Near,
        ..Default::default()
    };
    save("normal.png", &base)?;
    save("bold.png", &TextOpts { bold: true, ..base.clone() })?;
    save("italic.png", &TextOpts { italic: true, ..base.clone() })?;
    save("subliniat.png", &TextOpts { underline: true, ..base.clone() })?;

    // alinierile, fiecare pe axa ei
    for (n, a) in [("stanga", Align::Near), ("centru", Align::Center), ("dreapta", Align::Far)] {
        save(&format!("h-{n}.png"), &TextOpts { halign: a, ..base.clone() })?;
    }
    for (n, a) in [("sus", Align::Near), ("mijloc", Align::Center), ("jos", Align::Far)] {
        save(&format!("v-{n}.png"), &TextOpts { valign: a, ..base.clone() })?;
    }

    // aceeași propoziție cu fontul implicit: imaginea trebuie să difere
    save("dejavu.png", &TextOpts { family: font::DEFAULT_FAMILY.into(), ..base.clone() })?;
    Ok(())
}
