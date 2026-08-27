use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;

/// Iconițele barei — aceleași fișiere Fugue Icons pe care le folosește ShareX
/// (Yusuke Kamiyamane, CC-BY 3.0), preluate din depozitul lor.
macro_rules! icons {
    ($($n:literal),* $(,)?) => {
        const RAW: &[(&str, &[u8])] = &[
            $(($n, include_bytes!(concat!("../assets/icons/", $n, ".png")))),*
        ];
    };
}

icons![
    "cursor",
    "layer-shape",
    "layer-shape-ellipse",
    "pencil",
    "pencil--arrow",
    "layer-shape-line-white",
    "layer-shape-arrow-white",
    "edit-outline-white",
    "edit-shade-white",
    "balloon-box-left",
    "counter-reset",
    "magnifier-zoom",
    "folder-open-image",
    "monitor-image",
    "smiley-yell",
    "stamp-cursor",
    "eraser",
    "layer-shade-white",
    "grid-white",
    "highlighter-text",
    "flashlight-shine",
    "image-crop",
    "table-delete-column",
    "layer--pencil",
    "wrench-screwdriver",
    "image--pencil",
    "tick",
    "disk-black",
    "disks-black",
    "clipboard",
    "drive-globe",
    "printer",
    "arrow-circle-225-left",
    "arrow-circle-315",
    "document-copy",
    "layer--minus",
    "layers-stack-arrange",
    "layers-arrange",
    "layers-arrange-back",
    "layers-stack-arrange-back",
    "image-empty",
    "image--plus",
    "camera",
    "image-select",
    "image-resize",
    "image-resize-actual",
    "arrow-circle",
    "arrow-circle-135-left",
];

pub struct Icons(HashMap<&'static str, TextureHandle>);

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        let mut m = HashMap::new();
        for (name, bytes) in RAW {
            let Ok(img) = image::load_from_memory(bytes) else { continue };
            let img = img.to_rgba8();
            let ci = ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            m.insert(*name, ctx.load_texture(*name, ci, TextureOptions::LINEAR));
        }
        Self(m)
    }

    pub fn img(&self, name: &str) -> egui::Image<'static> {
        let tex = self.0.get(name).expect("iconiță lipsă");
        egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
            .fit_to_exact_size(egui::Vec2::splat(16.0))
    }
}
