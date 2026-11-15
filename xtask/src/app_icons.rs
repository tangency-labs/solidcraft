//! Regenerate all exports of the shared app mark together. Packaging copies these files.
use std::path::Path;

#[path = "../../assets/icons/app/shared/source/app_icon.rs"]
mod app_icon;
#[path = "../../assets/icons/app/shared/source/windows_ico.rs"]
mod windows_ico;

pub fn run(root: &Path) -> Result<(), String> {
    let stage = root.join("target/resources/app-icons");
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let mut exports = Vec::new();
    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        let png = stage.join(format!("{size}.png"));
        draw(&png, size)?;
        exports.push((png, format!("linux/exports/hicolor/{size}x{size}/apps/io.tangencylabs.solidcraft.png")));
    }
    let installer = stage.join("solidcraft.ico");
    draw(&installer, 256)?;
    exports.push((installer, "windows/exports/solidcraft.ico".into()));
    let executable = stage.join("solidcraft-executable.ico");
    std::fs::write(&executable, windows_ico::encode().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    exports.push((executable, "windows/exports/solidcraft-executable.ico".into()));

    // Finish generation before replacing any canonical exports.
    for (source, relative) in exports {
        let target = root.join("assets/icons/app").join(relative);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::copy(source, target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn draw(path: &Path, size: usize) -> Result<(), String> {
    let mut c = crate::cargo();
    c.args(["run", "-q", "--profile", "ci", "-p", "solidcraft-cli", "--", "icon", "--size", &size.to_string(), "--out"]).arg(path);
    crate::run(c, "solidcraft-cli icon")
}
