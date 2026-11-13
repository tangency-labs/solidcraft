#![forbid(unsafe_code)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/app/windows/exports/solidcraft-executable.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let copy = out.join("solidcraft.ico");
    std::fs::copy(icon, &copy)?;
    let rc = out.join("solidcraft.rc");
    let icon_line = format!("1 ICON \"{}\"\n", copy.display().to_string().replace('\\', "/"));
    std::fs::write(&rc, format!("{icon_line}{}", version_resource(&std::env::var("CARGO_PKG_VERSION")?)))?;
    embed_resource::compile_for(&rc, ["solidcraft"], embed_resource::NONE).manifest_required()?;
    Ok(())
}

/// The Windows version resource (what Explorer's Details tab shows and what a launcher reads).
/// `FileVersion` and `ProductVersion` are the package version verbatim (`0.25.1`, `0.26.0-rc.1`):
/// ArtCraft's launcher parses the `ProductVersion` string as semver, so a fourth `.0` would not
/// read. The binary `FILEVERSION` fields need four numbers and take the leading `major.minor.patch`.
fn version_resource(version: &str) -> String {
    let mut numbers = version.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || numbers.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    format!(
        "1 VERSIONINFO\nFILEVERSION {major},{minor},{patch},0\nPRODUCTVERSION {major},{minor},{patch},0\nFILEOS 0x40004\nFILETYPE 0x1\n{{\nBLOCK \"StringFileInfo\"\n{{\nBLOCK \"040904B0\"\n{{\nVALUE \"CompanyName\", \"SolidCraft contributors\"\nVALUE \"FileDescription\", \"SolidCraft, parametric 3D CAD\"\nVALUE \"FileVersion\", \"{version}\"\nVALUE \"InternalName\", \"solidcraft\"\nVALUE \"OriginalFilename\", \"solidcraft.exe\"\nVALUE \"ProductName\", \"SolidCraft\"\nVALUE \"ProductVersion\", \"{version}\"\n}}\n}}\nBLOCK \"VarFileInfo\"\n{{\nVALUE \"Translation\", 0x0409, 1200\n}}\n}}\n"
    )
}
