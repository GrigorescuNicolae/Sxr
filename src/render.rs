use anyhow::{Context, Result};
use eframe::egui::{Color32, Pos2, Rect, Vec2};
use image::RgbaImage;
use tiny_skia::{
    LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke as TsStroke,
    Transform,
};

use crate::font;
use crate::shape::{
    self, arrow_geom, arrow_head, balloon_tail, block_avg, blur_region, box_shape, curve_poly, highlight_paint,
    magnify_blocks, outline_offsets, spotlight_at, spotlight_cover, spotlight_rects, step_radius, text_shape,
    Shape, SPOTLIGHT_DIM,
};

fn paint_of(c: Color32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(tiny_skia::Color::from_rgba8(c.r(), c.g(), c.b(), c.a()));
    p.anti_alias = true;
    p
}

fn stroke_of(width: f32) -> TsStroke {
    TsStroke {
        width: width.max(0.5),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..TsStroke::default()
    }
}

fn ts_rect(r: Rect) -> Option<tiny_skia::Rect> {
    let n = Rect::from_two_pos(r.min, r.max);
    tiny_skia::Rect::from_ltrb(n.min.x, n.min.y, n.max.x, n.max.y)
}

fn line_path(a: Pos2, b: Pos2) -> Option<tiny_skia::Path> {
    poly_path(&[a, b])
}

/// A shape's polyline (straight line, discretized spline, or arrow tail).
/// With two points it comes out as exactly the old path.
fn poly_path(poly: &[Pos2]) -> Option<tiny_skia::Path> {
    let (first, rest) = poly.split_first()?;
    let mut pb = PathBuilder::new();
    pb.move_to(first.x, first.y);
    for q in rest {
        pb.line_to(q.x, q.y);
    }
    pb.finish()
}

/// Rectangle with rounded corners (ShareX draws shapes with radius 3).
fn round_rect_path(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let r = Rect::from_two_pos(r.min, r.max);
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return None;
    }
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    if rad <= 0.5 {
        return ts_rect(r).map(PathBuilder::from_rect);
    }
    // 0.5523 = the Bézier approximation of a quarter circle
    let k = rad * 0.5523;
    let (l, t, rr, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(rr - rad, t);
    pb.cubic_to(rr - rad + k, t, rr, t + rad - k, rr, t + rad);
    pb.line_to(rr, b - rad);
    pb.cubic_to(rr, b - rad + k, rr - rad + k, b, rr - rad, b);
    pb.line_to(l + rad, b);
    pb.cubic_to(l + rad - k, b, l, b - rad + k, l, b - rad);
    pb.line_to(l, t + rad);
    pb.cubic_to(l, t + rad - k, l + rad - k, t, l + rad, t);
    pb.close();
    pb.finish()
}

/// Blends one pixel over the premultiplied pixmap (used by text rasterization).
fn blend(pm: &mut Pixmap, w: u32, h: u32, x: i32, y: i32, c: Color32, cov: f32) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let a = cov.clamp(0.0, 1.0) * (c.a() as f32 / 255.0);
    if a <= 0.001 {
        return;
    }
    let i = y as usize * w as usize + x as usize;
    let d = pm.pixels()[i];
    let mix = |s: u8, dv: u8| (s as f32 * a + dv as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    let na = (a * 255.0 + d.alpha() as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    let (nr, ng, nb) = (
        mix(c.r(), d.red()).min(na),
        mix(c.g(), d.green()).min(na),
        mix(c.b(), d.blue()).min(na),
    );
    pm.pixels_mut()[i] = PremultipliedColorU8::from_rgba(nr, ng, nb, na).unwrap_or(d);
}

fn draw_text(pm: &mut Pixmap, w: u32, h: u32, text: &str, size: f32, bold: bool, at: Pos2, c: Color32) {
    if text.is_empty() || c.a() == 0 {
        return;
    }
    font::rasterize(text, size, bold, at.x, at.y, |x, y, cov| {
        blend(pm, w, h, x, y, c, cov)
    });
}

/// Already laid-out text rows, plus the underline, shifted by `off`.
/// The same positions as in the preview, since they come from the same
/// `font::layout`.
fn draw_layout(
    pm: &mut Pixmap,
    w: u32,
    h: u32,
    lay: &font::Layout,
    o: &shape::TextOpts,
    uls: &[Rect],
    off: Vec2,
    c: Color32,
) {
    if c.a() == 0 {
        return;
    }
    font::rasterize_layout(lay, o, o.size, off, |x, y, cov| blend(pm, w, h, x, y, c, cov));
    let mut paint = paint_of(c);
    paint.anti_alias = false;
    for u in uls {
        if let Some(r) = ts_rect(u.translate(off)) {
            pm.fill_rect(r, &paint, Transform::identity(), None);
        }
    }
}

/// Draws an inserted image, bilinearly resampled to `rect`.
/// We interpolate in premultiplied alpha: otherwise the color of the fully
/// transparent pixels (usually black) would bleed into the shape's edges and a
/// halo would appear. Only the result is then blended over the background.
fn draw_img(pm: &mut Pixmap, w: u32, h: u32, img: &RgbaImage, rect: Rect) {
    let r = Rect::from_two_pos(rect.min, rect.max);
    let (sw, sh) = (img.width() as i64, img.height() as i64);
    if sw == 0 || sh == 0 || r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let x0 = r.min.x.floor().max(0.0) as i64;
    let y0 = r.min.y.floor().max(0.0) as i64;
    let x1 = (r.max.x.ceil() as i64).min(w as i64);
    let y1 = (r.max.y.ceil() as i64).min(h as i64);
    // the edges are replicated, so the sampling cannot run off the image
    let tap = |x: i64, y: i64| {
        let p = img
            .get_pixel(x.clamp(0, sw - 1) as u32, y.clamp(0, sh - 1) as u32)
            .0;
        let a = p[3] as f32 / 255.0;
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    for y in y0..y1 {
        // the center of the destination pixel, taken into source coordinates
        let v = (y as f32 + 0.5 - r.min.y) / r.height() * sh as f32 - 0.5;
        let (iv, fv) = (v.floor(), v - v.floor());
        for x in x0..x1 {
            let u = (x as f32 + 0.5 - r.min.x) / r.width() * sw as f32 - 0.5;
            let (iu, fu) = (u.floor(), u - u.floor());
            let (ix, iy) = (iu as i64, iv as i64);
            let (c00, c10, c01, c11) = (
                tap(ix, iy),
                tap(ix + 1, iy),
                tap(ix, iy + 1),
                tap(ix + 1, iy + 1),
            );
            let mut c = [0.0f32; 4];
            for k in 0..4 {
                let t = c00[k] + (c10[k] - c00[k]) * fu;
                let b = c01[k] + (c11[k] - c01[k]) * fu;
                c[k] = t + (b - t) * fv;
            }
            if c[3] <= 0.002 {
                continue;
            }
            // `blend` wants an unpremultiplied color plus separate coverage
            let d = |q: f32| (q / c[3]).round().clamp(0.0, 255.0) as u8;
            let col = Color32::from_rgb(d(c[0]), d(c[1]), d(c[2]));
            blend(pm, w, h, x as i32, y as i32, col, c[3]);
        }
    }
}

/// Rasterizes the final image on the CPU and encodes it as PNG.
/// Independent of the preview zoom — the export is always at source resolution.
pub fn compose(src: &RgbaImage, shapes: &[Shape]) -> Result<Vec<u8>> {
    compose_opts(src, shapes, true)
}

/// The variant with options: `shadow` honors the "Shadow" checkbox in the toolbar.
pub fn compose_opts(src: &RgbaImage, shapes: &[Shape], shadow: bool) -> Result<Vec<u8>> {
    let (w, h) = src.dimensions();
    let mut pm = Pixmap::new(w, h).context("could not allocate pixmap")?;

    for (dst, px) in pm.pixels_mut().iter_mut().zip(src.pixels()) {
        let [r, g, b, a] = px.0;
        let m = |v: u8| ((v as u16 * a as u16) / 255) as u8;
        *dst = PremultipliedColorU8::from_rgba(m(r), m(g), m(b), a)
            .unwrap_or(PremultipliedColorU8::TRANSPARENT);
    }

    // the spotlight: a single dark layer, with holes for all the
    // rectangles, placed at the position of the first spotlight in the list
    let spot_at = spotlight_at(shapes);
    let spots = spotlight_rects(shapes);

    for (i, s) in shapes.iter().enumerate() {
        if spot_at == Some(i) {
            draw_spotlight(&mut pm, &spots, w as f32, h as f32);
        }
        if shadow {
            if let Some(sh) = s.shadow_copy() {
                draw_one(&mut pm, w, h, src, &sh);
            }
        }
        draw_one(&mut pm, w, h, src, s);
    }

    let mut out = RgbaImage::new(w, h);
    for (px, sp) in out.pixels_mut().zip(pm.pixels()) {
        let a = sp.alpha();
        px.0 = if a == 0 {
            [0, 0, 0, 0]
        } else {
            let d = |v: u8| ((v as u16 * 255) / a as u16).min(255) as u8;
            [d(sp.red()), d(sp.green()), d(sp.blue()), a]
        };
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    out.write_to(&mut buf, image::ImageFormat::Png)
        .context("codare PNG")?;
    Ok(buf.into_inner())
}

/// Darkens everything outside the union of the spotlight rectangles.
fn draw_spotlight(pm: &mut Pixmap, spots: &[Rect], w: f32, h: f32) {
    let id = Transform::identity();
    let mut paint = paint_of(SPOTLIGHT_DIM);
    paint.anti_alias = false;
    for r in spotlight_cover(spots, w, h) {
        if let Some(rr) = ts_rect(r) {
            pm.fill_rect(rr, &paint, id, None);
        }
    }
}

/// The solid arrowhead: a single closed outline — tiny-skia fills the concave
/// notch shape correctly too.
fn fill_arrow_head(
    pm: &mut tiny_skia::Pixmap,
    [tip, c1, mid, c2]: [eframe::egui::Pos2; 4],
    paint: &tiny_skia::Paint,
    id: tiny_skia::Transform,
) {
    let mut pb = PathBuilder::new();
    pb.move_to(tip.x, tip.y);
    pb.line_to(c1.x, c1.y);
    pb.line_to(mid.x, mid.y);
    pb.line_to(c2.x, c2.y);
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, paint, tiny_skia::FillRule::Winding, id, None);
    }
}

fn draw_one(pm: &mut Pixmap, w: u32, h: u32, src: &RgbaImage, s: &Shape) {
    let id = Transform::identity();
    match s {
        Shape::Rect { rect, border, fill, width, radius } => {
            if let Some(path) = round_rect_path(*rect, *radius) {
                if fill.a() > 0 {
                    pm.fill_path(&path, &paint_of(*fill), tiny_skia::FillRule::Winding, id, None);
                }
                if *width > 0.0 && border.a() > 0 {
                    pm.stroke_path(&path, &paint_of(*border), &stroke_of(*width), id, None);
                }
            }
        }
        Shape::Ellipse { rect, border, fill, width } => {
            if let Some(r) = ts_rect(*rect) {
                if let Some(path) = PathBuilder::from_oval(r) {
                    if fill.a() > 0 {
                        pm.fill_path(&path, &paint_of(*fill), tiny_skia::FillRule::Winding, id, None);
                    }
                    if *width > 0.0 && border.a() > 0 {
                        pm.stroke_path(&path, &paint_of(*border), &stroke_of(*width), id, None);
                    }
                }
            }
        }
        Shape::Line { from, to, mid, curbat, color, width } => {
            if let Some(path) = poly_path(&curve_poly(*from, *to, mid, *curbat)) {
                pm.stroke_path(&path, &paint_of(*color), &stroke_of(*width), id, None);
            }
        }
        Shape::Arrow { from, to, mid, curbat, color, width } => {
            let p = paint_of(*color);
            let (tail, head) = arrow_geom(&curve_poly(*from, *to, mid, *curbat), *width);
            if let Some(path) = poly_path(&tail) {
                pm.stroke_path(&path, &p, &stroke_of(*width), id, None);
            }
            fill_arrow_head(pm, head, &p, id);
        }
        Shape::Free { pts, color, width, arrow } => {
            if pts.len() >= 2 {
                let mut pb = PathBuilder::new();
                pb.move_to(pts[0].x, pts[0].y);
                for q in &pts[1..] {
                    pb.line_to(q.x, q.y);
                }
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint_of(*color), &stroke_of(*width), id, None);
                }
                if *arrow {
                    let (a, b) = (pts[pts.len().saturating_sub(4)], pts[pts.len() - 1]);
                    fill_arrow_head(pm, arrow_head(a, b, *width), &paint_of(*color), id);
                }
            }
        }
        Shape::Text { rect, text, opts, fill, outline, outline_w, radius } => {
            if fill.a() > 0 {
                if let Some(path) = round_rect_path(*rect, *radius) {
                    pm.fill_path(&path, &paint_of(*fill), tiny_skia::FillRule::Winding, id, None);
                }
            }
            if text.is_empty() {
                return;
            }
            let lay = font::layout(text, opts, opts.size, Rect::from_two_pos(rect.min, rect.max));
            let uls = font::underline_rects(&lay, opts);
            for off in outline_offsets(*outline_w) {
                draw_layout(pm, w, h, &lay, opts, &uls, off, *outline);
            }
            draw_layout(pm, w, h, &lay, opts, &uls, Vec2::ZERO, opts.color);
        }
        Shape::Balloon { rect, tail, text, opts, fill, border, width, radius } => {
            // an unfinished draft can have `max < min` (dragging backwards)
            let rect = &Rect::from_two_pos(rect.min, rect.max);
            // the box goes through exactly the same path as the rounded rect
            draw_one(pm, w, h, src, &box_shape(*rect, *fill, *border, *width, *radius));
            let t = balloon_tail(*rect, *tail);
            if fill.a() > 0 {
                let q = t.fill_pts(*width);
                let mut pb = PathBuilder::new();
                pb.move_to(q[0].x, q[0].y);
                pb.line_to(q[1].x, q[1].y);
                pb.line_to(q[2].x, q[2].y);
                pb.close();
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint_of(*fill), tiny_skia::FillRule::Winding, id, None);
                }
            }
            if *width > 0.0 && border.a() > 0 {
                // only the slanted sides: the base would draw a line through the balloon
                for b in t.base {
                    if let Some(path) = line_path(b, t.tip) {
                        pm.stroke_path(&path, &paint_of(*border), &stroke_of(*width), id, None);
                    }
                }
            }
            draw_one(pm, w, h, src, &text_shape(*rect, text, opts));
        }
        Shape::Magnify { rect, strength, border, width } => {
            let rect = &Rect::from_two_pos(rect.min, rect.max);
            for (d, c) in magnify_blocks(src, *rect, *strength) {
                if let Some(rr) = ts_rect(d) {
                    let mut p = paint_of(c);
                    p.anti_alias = false;
                    pm.fill_rect(rr, &p, id, None);
                }
            }
            draw_one(pm, w, h, src, &box_shape(*rect, Color32::TRANSPARENT, *border, *width, 0.0));
        }
        Shape::Step { center, n, size, fill, border, text, width } => {
            let rad = step_radius(*n, *size);
            if let Some(path) = PathBuilder::from_circle(center.x, center.y, rad) {
                if fill.a() > 0 {
                    pm.fill_path(&path, &paint_of(*fill), tiny_skia::FillRule::Winding, id, None);
                }
                if *width > 0.0 && border.a() > 0 {
                    pm.stroke_path(&path, &paint_of(*border), &stroke_of(*width), id, None);
                }
            }
            let label = n.to_string();
            let (tw, th) = font::measure(&label, *size, true);
            let at = Pos2::new(center.x - tw / 2.0, center.y - th / 2.0);
            draw_text(pm, w, h, &label, *size, true, at, *text);
        }
        Shape::Highlight { rect, color } => {
            if let Some(r) = ts_rect(*rect) {
                pm.fill_rect(r, &paint_of(highlight_paint(*color)), id, None);
            }
        }
        Shape::Pixelate { rect, block } => {
            let b = block.max(2.0);
            let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(src.width() as f32, src.height() as f32));
            let r = Rect::from_two_pos(rect.min, rect.max).intersect(clip);
            if !r.is_positive() {
                return;
            }
            let mut y = r.min.y;
            while y < r.max.y {
                let mut x = r.min.x;
                while x < r.max.x {
                    let (x2, y2) = ((x + b).min(r.max.x), (y + b).min(r.max.y));
                    // we sample from the original source, not from the pixmap,
                    // so the shapes drawn underneath are not smeared into blocks
                    let c = block_avg(src, x as i64, y as i64, x2 as i64, y2 as i64);
                    if let Some(rr) = tiny_skia::Rect::from_ltrb(x, y, x2, y2) {
                        let mut p = paint_of(c);
                        p.anti_alias = false;
                        pm.fill_rect(rr, &p, id, None);
                    }
                    x += b;
                }
                y += b;
            }
        }
        Shape::Blur { rect, radius } => {
            let rect = &Rect::from_two_pos(rect.min, rect.max);
            let Some(b) = blur_region(src, *rect, *radius) else { return };
            for y in 0..b.h {
                for x in 0..b.w {
                    let c = b.px[y * b.w + x];
                    let col = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                    blend(pm, w, h, (b.x0 + x as i64) as i32, (b.y0 + y as i64) as i32, col, 1.0);
                }
            }
        }
        // the dark layer is drawn only once, in compose_opts
        Shape::Spotlight { .. } => {}
        Shape::Img { rect, img, .. } => draw_img(pm, w, h, img, Rect::from_two_pos(rect.min, rect.max)),
        Shape::Erase { rect, color } => {
            if let Some(r) = ts_rect(*rect) {
                let mut p = paint_of(*color);
                p.anti_alias = false;
                pm.fill_rect(r, &p, id, None);
            }
        }
    }
}
