# sxr

A faithful reimplementation of the **classic ShareX image editor** for Linux,
written in Rust.

It does not try to be all of ShareX. It covers the flow you use every day: hit
the shortcut, pick a region of the screen, the editor opens, and the capture is
already in the clipboard.

## What it does

- **Region capture** through a selector of our own: the screen is frozen first,
  then a dimmed overlay goes over every monitor with crosshair guides and a
  live size readout, and the rectangle is cut out of the frozen image. The
  result is copied to the clipboard automatically right after the selection —
  close without editing and the capture is already there.
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

Requirements: Rust (2024 edition). Region capture needs `spectacle` for the
screen grab and a Wayland compositor that speaks `zwlr_layer_shell_v1` for the
overlay; where the overlay cannot come up, the selection falls back to
spectacle's own selector. The font list in the text window comes from `fc-list`
(fontconfig); without it, DejaVu Sans is used.

## Usage

```sh
sxr             # select a region of the screen, then open the editor
sxr <file>      # open an existing image directly
sxr --main      # the main window (a layout shell for now, see below)
sxr --settings  # the application settings window (a layout shell too)
```

In practice you bind it to a global shortcut (`Ctrl+Print`, for example).

`--main` opens a reproduction of ShareX's main window: the toolbar, its
dropdowns and the task list, laid out from `MainForm.resx`. Every control is
there and every control is disabled — it exists so the layout can be compared
against the real thing while the features behind it are written, and each
control comes alive as its feature lands.

`--settings` does the same for ShareX's Application settings window, laid out
from `ApplicationSettingsForm.resx`: the tree of twelve pages on the left and
each page's controls on the right, all of them disabled, and clicking a page in
the tree is the only thing that answers.

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
