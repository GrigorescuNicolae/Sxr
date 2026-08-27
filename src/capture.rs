use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::process::Command;

/// Selecție de regiune prin selectorul nativ KDE.
/// Temporar: va fi înlocuit cu overlay propriu pe wlr-layer-shell.
pub fn select_region() -> Result<RgbaImage> {
    let tmp = std::env::temp_dir().join(format!("sxr-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    let status = Command::new("spectacle")
        .args(["-r", "-b", "-n", "-o"])
        .arg(&tmp)
        .status()
        .context("nu am putut porni spectacle")?;

    if !status.success() {
        bail!("spectacle a returnat cod {:?}", status.code());
    }
    if !tmp.exists() {
        bail!("selectie anulata");
    }

    let img = image::open(&tmp).context("nu am putut decoda PNG-ul")?.to_rgba8();
    let _ = std::fs::remove_file(&tmp);
    Ok(img)
}
