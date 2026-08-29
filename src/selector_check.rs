//! Two things `--sel-flow` cannot check from the outside.
//!
//! The first is correctness of the damage tracking. [`Painter`] only repairs
//! the rectangles it thinks moved, so a frame is built partly out of what the
//! slot already held — and a damage set that is one rectangle short leaves a
//! smear nobody notices until it is on screen. The test here renders every
//! frame of a long script twice, once incrementally into alternating slots and
//! once from scratch, and demands they come out byte for byte the same.
//!
//! The second is what a frame costs. Damage tracking is the whole reason the
//! overlay can keep up with a pointer, so the numbers are worth printing:
//! `cargo test --release -- --nocapture`.
use crate::selector::*;
use image::RgbaImage;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

fn screen() -> RgbaImage {
    RgbaImage::from_fn(3840, 1080, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
    })
}

fn run(name: &str, wins: Vec<Box2>, drive: &dyn Fn(&mut Sel, u64)) {
    let scr = screen();
    let outs = vec![Box2::new(0, 0, 1920, 1080), Box2::new(1920, 0, 1920, 1080)];
    let mut sel = Sel::new(Box2::new(0, 0, 3840, 1080), outs, wins);
    let mut p = Painter::new(&scr, Box2::new(0, 0, 1920, 1080));
    let mut pms: Vec<Pixmap> = (0..2).map(|_| Pixmap::new(1920, 1080).unwrap()).collect();
    // warm both slots up with a full repaint
    for s in 0..2 {
        p.render(&mut pms[s], &scr, &sel, Duration::from_millis(600), None);
    }
    let n = 120u64;
    let t0 = Instant::now();
    let mut px = 0usize;
    for f in 0..n {
        drive(&mut sel, f);
        let slot = (f % 2) as usize;
        let d = p.render(&mut pms[slot], &scr, &sel, Duration::from_millis(600 + f * 16), Some(2));
        px += d.iter().map(|r| (r.w as usize) * (r.h as usize)).sum::<usize>();
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("{name:<28} {ms:6.2} ms/frame   {:>9} px damage/frame", px / n as usize);
}

#[test]
fn probe() {
    let a = Box2::new(0, 0, 1920, 1080);
    let b = Box2::new(1920, 0, 1920, 1080);
    let small = Box2::new(100, 100, 400, 300);

    run("idle, nothing hovered", vec![], &|s, f| {
        s.feed(Input::Motion(Pos { x: 900 + (f % 3) as i32, y: 500 }), Duration::from_millis(600 + f * 16));
    });
    run("hover maximized window", vec![a, b], &|s, f| {
        s.feed(Input::Motion(Pos { x: 900 + (f % 3) as i32, y: 500 }), Duration::from_millis(600 + f * 16));
    });
    run("hover small window", vec![small], &|s, f| {
        s.feed(Input::Motion(Pos { x: 200 + (f % 3) as i32, y: 200 }), Duration::from_millis(600 + f * 16));
    });
    run("switching between windows", vec![a, b], &|s, f| {
        let x = if (f / 6) % 2 == 0 { 900 } else { 2900 };
        s.feed(Input::Motion(Pos { x, y: 500 }), Duration::from_millis(600 + f * 16));
    });
}

// ------------------------------------------------- incremental == from scratch

/// One frame of a script: what the user did just before it, and when it is
/// drawn — the moment matters, because the dashes and the hover tween both run
/// off the clock rather than off the input.
struct Frame {
    inputs: Vec<Input>,
    at: Duration,
}

/// A named run of frames over one selector state.
struct Scene {
    name: &'static str,
    sel: Sel,
    frames: Vec<Frame>,
}

const DESKTOP: Box2 = Box2 { x: 0, y: 0, w: 3840, h: 1080 };
const LEFTALT: u32 = 56;

/// The whole scripted run: idle, a drag out and back in, hovers big and small,
/// the tween between two windows, the Alt previews, the guides, the magnifier
/// in a corner and the cursor walking across a region's border.
fn script() -> Vec<Scene> {
    let outs = vec![Box2::new(0, 0, 1920, 1080), Box2::new(1920, 0, 1920, 1080)];
    let big = Box2::new(0, 0, 1920, 1080);
    let far = Box2::new(2100, 200, 900, 600);
    let small = Box2::new(200, 400, 400, 300);
    let sel = |wins: Vec<Box2>| Sel::new(DESKTOP, outs.clone(), wins);
    // the clock only ever moves forward, so the ants are at a different phase
    // in every single frame of the run
    let mut t = Duration::from_millis(600);
    let mut tick = move || {
        t += Duration::from_millis(17);
        t
    };
    let motion = |x, y| vec![Input::Motion(Pos { x, y })];

    let mut v = Vec::new();

    v.push(Scene {
        name: "idle, cursor wandering",
        sel: sel(vec![]),
        frames: (0..12)
            .map(|i| Frame { inputs: motion(900 + i * 7, 500 + i * 3), at: tick() })
            .collect(),
    });

    // a drag that grows, holds still while the ants march, then shrinks
    let mut frames = vec![Frame {
        inputs: vec![Input::Motion(Pos { x: 300, y: 300 }), Input::Button { btn: Btn::Left, down: true }],
        at: tick(),
    }];
    frames.extend((1..14).map(|i| Frame { inputs: motion(300 + i * 60, 300 + i * 40), at: tick() }));
    frames.extend((0..6).map(|_| Frame { inputs: vec![], at: tick() }));
    frames.extend((0..14).rev().map(|i| Frame { inputs: motion(300 + i * 60, 300 + i * 40), at: tick() }));
    v.push(Scene { name: "a drag growing then shrinking", sel: sel(vec![]), frames });

    v.push(Scene {
        name: "hover on a small window",
        sel: sel(vec![small]),
        frames: (0..14)
            .map(|i| Frame { inputs: motion(300 + i % 4, 500 + i % 3), at: tick() })
            .collect(),
    });

    // the hover jumps between two disjoint windows and back; the empty frames
    // are where the tween slides the border across on its own
    let mut frames = Vec::new();
    for round in 0..3 {
        let (x, y) = if round % 2 == 0 { (2500, 500) } else { (300, 500) };
        frames.push(Frame { inputs: motion(x, y), at: tick() });
        frames.extend((0..9).map(|_| Frame { inputs: vec![], at: tick() }));
    }
    v.push(Scene { name: "hover tweening between windows", sel: sel(vec![far, small]), frames });

    v.push(Scene {
        name: "hover on a maximized window",
        sel: sel(vec![big]),
        frames: (0..10).map(|i| Frame { inputs: motion(900 + i % 3, 500), at: tick() }).collect(),
    });

    // Alt on and off in the middle of a drag: the red snap previews appear over
    // the bright region and have to leave it clean again
    let mut frames = vec![Frame {
        inputs: vec![Input::Motion(Pos { x: 300, y: 200 }), Input::Button { btn: Btn::Left, down: true }],
        at: tick(),
    }];
    frames.push(Frame { inputs: motion(1000, 620), at: tick() });
    for down in [true, false, true, false, true] {
        frames.push(Frame { inputs: vec![Input::Key { code: LEFTALT, down }], at: tick() });
        frames.extend((0..3).map(|_| Frame { inputs: vec![], at: tick() }));
        frames.push(Frame { inputs: motion(1000 + i32::from(down) * 30, 620), at: tick() });
    }
    v.push(Scene { name: "Alt snap previews on and off", sel: sel(vec![]), frames });

    let mut s = sel(vec![small]);
    s.show_crosshair = true;
    v.push(Scene {
        name: "the crosshair guides",
        sel: s,
        frames: (0..12)
            .map(|i| Frame { inputs: motion(1400 - i * 90, 600 - i * 20), at: tick() })
            .collect(),
    });

    // the magnifier flipping around the bottom-right corner of the desktop
    v.push(Scene {
        name: "the magnifier in a corner",
        sel: sel(vec![]),
        frames: (0..12)
            .map(|i| Frame { inputs: motion(3700 + i * 8, 1000 + i * 5), at: tick() })
            .collect(),
    });

    let mut s = sel(vec![]);
    s.square_magnifier = true;
    v.push(Scene {
        name: "the square magnifier",
        sel: s,
        frames: (0..8).map(|i| Frame { inputs: motion(2860 + i * 5, 520), at: tick() }).collect(),
    });

    // the cursor walks straight across the border of a settled region, so the
    // magnifier and the label cross from the dimmed side to the bright one
    let mut frames = vec![
        Frame {
            inputs: vec![
                Input::Motion(Pos { x: 500, y: 300 }),
                Input::Button { btn: Btn::Left, down: true },
                Input::Motion(Pos { x: 1200, y: 800 }),
                Input::Button { btn: Btn::Left, down: false },
            ],
            at: tick(),
        },
    ];
    frames.extend((0..24).map(|i| Frame { inputs: motion(400 + i * 40, 300 + i * 25), at: tick() }));
    v.push(Scene { name: "the cursor crossing a border", sel: sel(vec![]), frames });

    v
}

/// Renders the whole script twice for each output: once incrementally, cycling
/// through `slots` buffers the way the compositor hands them back, and once
/// from scratch with `age = None`. Nothing about the incremental path is
/// allowed to show: the two pixmaps must come out byte for byte the same.
fn identity(slots: usize) -> usize {
    let scr = screen();
    let mut compared = 0usize;
    for out in [Box2::new(0, 0, 1920, 1080), Box2::new(1920, 0, 1920, 1080)] {
        let mut p = Painter::new(&scr, out);
        let mut fresh = Painter::new(&scr, out);
        let mut pms: Vec<Pixmap> =
            (0..slots).map(|_| Pixmap::new(out.w as u32, out.h as u32).unwrap()).collect();
        let mut want = Pixmap::new(out.w as u32, out.h as u32).unwrap();
        // the painter carries its history across scene boundaries on purpose:
        // an abrupt cut is exactly where a damage set goes wrong
        let mut n = 0usize;
        for mut sc in script() {
            for f in &sc.frames {
                for ev in &f.inputs {
                    sc.sel.feed(*ev, f.at);
                }
                // the first `slots` frames have nothing to build on
                let age = (n >= slots).then_some(slots as u32);
                let slot = n % slots;
                p.render(&mut pms[slot], &scr, &sc.sel, f.at, age);
                fresh.render(&mut want, &scr, &sc.sel, f.at, None);
                assert_eq!(
                    pms[slot].data(),
                    want.data(),
                    "frame {n} of {:?} in scene {:?} differs from a full repaint",
                    f.at,
                    sc.name
                );
                compared += 1;
                n += 1;
            }
        }
    }
    compared
}

#[test]
fn incremental_matches_a_full_repaint() {
    let two = identity(2);
    let three = identity(3);
    println!("pixel identity: {two} frames double buffered, {three} triple buffered, all identical");
}
