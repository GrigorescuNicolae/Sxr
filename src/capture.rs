use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::process::Command;

/// Region selection through the native KDE selector.
/// Temporary: will be replaced with our own wlr-layer-shell overlay.
pub fn select_region() -> Result<RgbaImage> {
    let tmp = std::env::temp_dir().join(format!("sxr-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    let status = Command::new("spectacle")
        .args(["-r", "-b", "-n", "-o"])
        .arg(&tmp)
        .status()
        .context("could not start spectacle")?;

    if !status.success() {
        bail!("spectacle exited with code {:?}", status.code());
    }
    if !tmp.exists() {
        bail!("selection cancelled");
    }

    let img = image::open(&tmp).context("could not decode the PNG")?.to_rgba8();
    let _ = std::fs::remove_file(&tmp);
    Ok(img)
}
