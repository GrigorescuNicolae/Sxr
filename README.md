# sxr

A faithful reimplementation of the **classic ShareX image editor** for Linux,
written in Rust.

It does not try to be all of ShareX. It covers the flow you use every day: hit
the shortcut, pick a region of the screen, the editor opens, and the capture is
already in the clipboard.

## What it does

- **Region capture**, copied to the clipboard automatically right after the
  selection — close without editing and the capture is already there.
- **The classic editor**, with the toolbar in ShareX's order and the same icons:
  rectangle, ellipse, freehand, freehand arrow, line, arrow, text with outline,
  text with background, speech balloon, step counter, magnifier, image from
  file, image from screen, sticker, cursor, smart eraser, blur, pixelate,
  highlight, spotlight, crop, cut out.
- **Sticker picker** like ShareX's: packs (subfolders), search across the whole
  path, a grid of lazily loaded thumbnails, and a size that doubles as the size
  the sticker is inserted at.
- **Text input window** like ShareX's: font family from the system, size, color,
  secondary color, bold / italic / underline, alignment on both axes.
  `Enter` = OK, `Ctrl+Enter` = new line, `Esc` = cancel.
- **Curvable line and arrow**: the shape gets `2 + N` nodes, and the middle node
  bends it along a cardinal spline, exactly like `LineDrawingShape`.
- **Image menu**: image size, canvas size, crop, auto crop, rotate left and
  right.
- Undo / redo, layer ordering, drop shadow, duplicate.

Not included: uploading to services, OCR, workflows, history, printing.

## Building

```sh
cargo build --release
install -m755 target/release/sxr ~/.local/bin/sxr
```

Requirements: Rust (2024 edition) and, for region capture, a Wayland session
with the screenshot portal. The font list in the text window comes from
`fc-list` (fontconfig); without it, DejaVu Sans is used.

## Usage

```sh
sxr             # select a region of the screen, then open the editor
sxr <file>      # open an existing image directly
```

In practice you bind it to a global shortcut (`Ctrl+Print`, for example).

Stickers are read from `~/.local/share/sxr/stickers/` — drop images there
(`png`, `jpg`, `jpeg`, `webp`, `bmp`, `gif`) and they show up in the sticker
tool. Each subfolder is a pack; files left directly in the root make up the
"All stickers" pack. The tool opens a window of its own, like ShareX's
`StickerForm`: search, the pack list, a button to the folder, and the sticker
size (16–256, in steps of 16), remembered along with the selected pack. Click a
thumbnail — or press `Enter`, which takes the first search result — to insert
the sticker at the click point; `Esc` cancels.

## Relationship to ShareX

The project is inspired by [ShareX](https://github.com/ShareX/ShareX) and
follows the behavior of its classic editor closely, but it is written from
scratch in Rust. **Not a single line of code is copied from ShareX.** The
behavior, the order of the tools and the default values were reproduced by
observing the application and reading its public documentation.

It is not affiliated with the ShareX project, nor endorsed by it. ShareX is
© ShareX Team, licensed under GPL-3.0; that license does not apply here.

## License

The code is under the [MIT](LICENSE) license.

The icons, fonts and stickers have licenses of their own — see
[ATTRIBUTION.md](ATTRIBUTION.md).
