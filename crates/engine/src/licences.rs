//! SolidCraft's licence texts and the third-party licences, built into every program so a single
//! downloaded executable carries them (Help ▸ About ▸ Licences, `solidcraft-cli licences`).
//! THIRD-PARTY-LICENSES.txt is written by `cargo xtask licences` and checked by `cargo xtask ci`.

pub const LICENSE_MIT: &str = include_str!("../../../LICENSE-MIT");
pub const LICENSE_APACHE: &str = include_str!("../../../LICENSE-APACHE");
pub const NOTICE: &str = include_str!("../../../NOTICE");
pub const THIRD_PARTY: &str = include_str!("../../../THIRD-PARTY-LICENSES.txt");

/// Everything, as one text.
pub fn all() -> String {
    let rule = "=".repeat(80);
    format!(
        "SolidCraft is licensed under the MIT License or the Apache License, Version 2.0, at your option.\n\n{rule}\nNOTICE\n{rule}\n\n{NOTICE}\n{rule}\nLICENSE-MIT\n{rule}\n\n{LICENSE_MIT}\n{rule}\nLICENSE-APACHE\n{rule}\n\n{LICENSE_APACHE}\n{rule}\n{THIRD_PARTY}"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_licences_are_built_in() {
        assert!(super::LICENSE_MIT.contains("MIT License") && super::LICENSE_APACHE.contains("Apache License"));
        assert!(super::THIRD_PARTY.contains("wgpu") && super::THIRD_PARTY.contains("egui"));
        assert!(super::all().contains("NOTICE"));
    }
}
