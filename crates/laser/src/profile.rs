//! Built-in laser / engraver machine profiles (operator metadata for LightBurn handoff).
//!
//! Beds and device hints are taken from vendor manuals and LightBurn device docs — not
//! invented. Cheap ChinaCNCzone routers are Mach3-class (DXF still useful for laser heads
//! or CAM); Snapmaker Artisan family is Marlin with vendor `.lbdev` files.

use serde::{Deserialize, Serialize};

/// Stable profile id used in `laser-job.json` and commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileId {
    // ComMarker (JCZ / EzCAD)
    Commarker60wMopa,
    CommarkerB4,
    CommarkerB6,
    CommarkerOmniUv,
    // xTool (LightBurn native xTool type)
    XtoolF1,
    XtoolF1Ultra,
    XtoolD1,
    XtoolD1Pro,
    XtoolS1,
    XtoolP2,
    // Snapmaker (Marlin / vendor lbdev)
    SnapmakerArtisan,
    SnapmakerA350,
    SnapmakerA250,
    SnapmakerRay,
    // Makera (diode on CNC; LightBurn)
    MakeraCarvera,
    MakeraCarveraAir,
    // ChinaCNCzone / china-cncrouter.com (Mach3-class beds)
    Chinacnc3040,
    Chinacnc6040,
    Chinacnc6090,
}

impl ProfileId {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileId::Commarker60wMopa => "commarker-60w-mopa",
            ProfileId::CommarkerB4 => "commarker-b4",
            ProfileId::CommarkerB6 => "commarker-b6",
            ProfileId::CommarkerOmniUv => "commarker-omni-uv",
            ProfileId::XtoolF1 => "xtool-f1",
            ProfileId::XtoolF1Ultra => "xtool-f1-ultra",
            ProfileId::XtoolD1 => "xtool-d1",
            ProfileId::XtoolD1Pro => "xtool-d1-pro",
            ProfileId::XtoolS1 => "xtool-s1",
            ProfileId::XtoolP2 => "xtool-p2",
            ProfileId::SnapmakerArtisan => "snapmaker-artisan",
            ProfileId::SnapmakerA350 => "snapmaker-a350",
            ProfileId::SnapmakerA250 => "snapmaker-a250",
            ProfileId::SnapmakerRay => "snapmaker-ray",
            ProfileId::MakeraCarvera => "makera-carvera",
            ProfileId::MakeraCarveraAir => "makera-carvera-air",
            ProfileId::Chinacnc3040 => "chinacnc-3040",
            ProfileId::Chinacnc6040 => "chinacnc-6040",
            ProfileId::Chinacnc6090 => "chinacnc-6090",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let key = s.trim().to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "commarker-60w-mopa" | "commarker60wmopa" => Some(ProfileId::Commarker60wMopa),
            "commarker-b4" | "commarkerb4" | "b4" => Some(ProfileId::CommarkerB4),
            "commarker-b6" | "commarkerb6" | "b6" => Some(ProfileId::CommarkerB6),
            "commarker-omni-uv" | "commarker-omni" | "omni-uv" | "omni" => Some(ProfileId::CommarkerOmniUv),
            "xtool-f1" | "xtoolf1" | "f1" => Some(ProfileId::XtoolF1),
            "xtool-f1-ultra" | "xtoolf1ultra" | "f1-ultra" | "f1ultra" => Some(ProfileId::XtoolF1Ultra),
            "xtool-d1" | "xtoold1" | "d1" => Some(ProfileId::XtoolD1),
            "xtool-d1-pro" | "xtoold1pro" | "d1-pro" | "d1pro" => Some(ProfileId::XtoolD1Pro),
            "xtool-s1" | "xtools1" | "s1" => Some(ProfileId::XtoolS1),
            "xtool-p2" | "xtoolp2" | "p2" | "xtool-p2s" | "p2s" => Some(ProfileId::XtoolP2),
            "snapmaker-artisan" | "artisan" => Some(ProfileId::SnapmakerArtisan),
            "snapmaker-a350" | "a350" | "a350t" | "f350" => Some(ProfileId::SnapmakerA350),
            "snapmaker-a250" | "a250" | "a250t" | "f250" => Some(ProfileId::SnapmakerA250),
            "snapmaker-ray" | "ray" => Some(ProfileId::SnapmakerRay),
            "makera-carvera" | "carvera" => Some(ProfileId::MakeraCarvera),
            "makera-carvera-air" | "carvera-air" => Some(ProfileId::MakeraCarveraAir),
            "chinacnc-3040" | "cnc-3040" | "3040" => Some(ProfileId::Chinacnc3040),
            "chinacnc-6040" | "cnc-6040" | "6040" => Some(ProfileId::Chinacnc6040),
            "chinacnc-6090" | "cnc-6090" | "6090" => Some(ProfileId::Chinacnc6090),
            _ => None,
        }
    }
}

/// Operator-facing machine preset (not a controller driver).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaserProfile {
    pub id: ProfileId,
    pub name: String,
    pub bed_width_mm: f64,
    pub bed_height_mm: f64,
    pub field_options_mm: Vec<f64>,
    pub lightburn_device: String,
    pub lightburn_model: String,
    pub source: String,
    pub notes: String,
}

impl LaserProfile {
    pub fn default_field_mm(&self) -> f64 {
        self.bed_width_mm.max(self.bed_height_mm)
    }
}

fn p(id: ProfileId, name: &str, w: f64, h: f64, lenses: &[f64], device: &str, model: &str, source: &str, notes: &str) -> LaserProfile {
    LaserProfile {
        id,
        name: name.into(),
        bed_width_mm: w,
        bed_height_mm: h,
        field_options_mm: lenses.to_vec(),
        lightburn_device: device.into(),
        lightburn_model: model.into(),
        source: source.into(),
        notes: notes.into(),
    }
}

/// Every built-in profile.
pub fn builtin_profiles() -> &'static [LaserProfile] {
    static PROFILES: std::sync::OnceLock<Vec<LaserProfile>> = std::sync::OnceLock::new();
    PROFILES.get_or_init(|| {
        vec![
            // —— ComMarker (JCZFiber) ——
            p(
                ProfileId::Commarker60wMopa,
                "ComMarker 60W MOPA",
                110.0,
                110.0,
                &[70.0, 110.0, 150.0, 175.0, 200.0, 300.0],
                "JCZFiber",
                "B6 MOPA",
                "ComMarker B6 MOPA manual / LightBurn JCZFiber guide",
                "Create Manually → JCZFiber; import EzCAD markcfg + COR. MOPA: Freq 1–4000 kHz, Galvo 2, Q-Pulse (JPT/IPG_YLP). Set X/Y to your F-theta (field_mm override).",
            ),
            p(
                ProfileId::CommarkerB4,
                "ComMarker B4",
                110.0,
                110.0,
                &[110.0, 150.0, 200.0, 300.0, 400.0],
                "JCZFiber",
                "B4",
                "ComMarker B4 user manual (working area 110×110 / optional larger lenses); LightBurn JCZFiber",
                "Fiber galvo. LightBurn: JCZFiber + B4 EzCAD folder (110mm or 200mm Plug/markcfg7, matching .cor). Freq ~20–200 kHz Q-switched; Galvo 2.",
            ),
            p(
                ProfileId::CommarkerB6,
                "ComMarker B6",
                150.0,
                150.0,
                &[110.0, 150.0, 200.0, 300.0],
                "JCZFiber",
                "B6",
                "ComMarker B6 user manual (standard 150×150 mm; optional 110/200/300); LightBurn JCZFiber",
                "Fiber / MOPA galvo family. LightBurn: JCZFiber + B6 EzCAD (JCZ15.cor typical). Non-MOPA freq ~20–200; MOPA uses 1–4000 + Q-pulse.",
            ),
            p(
                ProfileId::CommarkerOmniUv,
                "ComMarker Omni UV",
                150.0,
                150.0,
                &[70.0, 150.0],
                "JCZFiber",
                "Omni UV",
                "ComMarker Omni UV guide (70×70 and 150×150 mm); LightBurn JCZFiber",
                "355 nm UV galvo. LightBurn: JCZFiber + Omni EzCAD config; set axis length to installed lens (70 or 150).",
            ),
            // —— xTool ——
            p(
                ProfileId::XtoolF1,
                "xTool F1",
                115.0,
                115.0,
                &[115.0],
                "xTool",
                "F1",
                "xTool F1 FAQ (115×115); LightBurn Device Types → xTool",
                "Galvo dual-source (10W diode + 2W IR). LB 2.2+ native xTool / Model F1; switch Laser 1/2 per layer.",
            ),
            p(
                ProfileId::XtoolF1Ultra,
                "xTool F1 Ultra",
                220.0,
                220.0,
                &[220.0],
                "xTool",
                "F1 Ultra",
                "xTool F1 Ultra user guide (220×220×145 mm); LightBurn xTool",
                "Galvo 20W fiber + 20W diode. LB: xTool / F1 Ultra; dual-source per cut layer.",
            ),
            p(
                ProfileId::XtoolD1,
                "xTool D1",
                432.0,
                406.0,
                &[],
                "xTool",
                "D1",
                "xTool D1 pre-sales (432×406 mm); LightBurn xTool USB",
                "Gantry diode. LB: xTool / D1; Serial/USB; work area 432×406 mm.",
            ),
            p(
                ProfileId::XtoolD1Pro,
                "xTool D1 Pro",
                432.0,
                406.0,
                &[],
                "xTool",
                "D1 Pro",
                "xTool D1 size (432×406); LightBurn lists D1 Pro / D1 Pro 2.0",
                "Gantry diode. LB: xTool / D1 Pro; Serial/USB.",
            ),
            p(
                ProfileId::XtoolS1,
                "xTool S1",
                498.0,
                319.0,
                &[],
                "xTool",
                "S1",
                "xTool S1 user guide (498×319 mm); LightBurn xTool USB",
                "Enclosed diode. LB: xTool / S1; Serial/USB.",
            ),
            p(
                ProfileId::XtoolP2,
                "xTool P2",
                600.0,
                305.0,
                &[],
                "xTool",
                "P2",
                "xTool P2 processing area 600×305 mm; LightBurn P2/P2S",
                "CO₂ flat. LB: xTool / P2 or P2S (flat; camera limited). Optional vendor .lbdev.",
            ),
            // —— Snapmaker (Marlin; vendor .lbdev) ——
            p(
                ProfileId::SnapmakerArtisan,
                "Snapmaker Artisan",
                400.0,
                400.0,
                &[],
                "Marlin",
                "Snapmaker Artisan",
                "Snapmaker Artisan specs (laser 400×400 mm); wiki LightBurn .lbdev (artisan-*-v1)",
                "3-in-1; laser work area 400×400. Firmware is Marlin — use Snapmaker Artisan LightBurn device / .lbdev (not plain GRBL) for best results. Serial only to LB.",
            ),
            p(
                ProfileId::SnapmakerA350,
                "Snapmaker 2.0 A350",
                320.0,
                350.0,
                &[],
                "Marlin",
                "Snapmaker 2.0",
                "Snapmaker 20W/40W laser module page (320×350 mm on A350/A350T/F350)",
                "2.0 family laser bed 320×350. LB: Snapmaker 2.0 .lbdev; Marlin. Also covers F350.",
            ),
            p(
                ProfileId::SnapmakerA250,
                "Snapmaker 2.0 A250",
                230.0,
                250.0,
                &[],
                "Marlin",
                "Snapmaker 2.0",
                "Snapmaker laser module page (230×250 mm on A250/A250T/F250)",
                "2.0 compact laser bed 230×250. LB: Snapmaker 2.0 .lbdev; Marlin.",
            ),
            p(
                ProfileId::SnapmakerRay,
                "Snapmaker Ray",
                400.0,
                400.0,
                &[],
                "GRBL",
                "Snapmaker Ray",
                "Snapmaker LightBurn wiki (ray-*-v1.lbdev); Ray is GRBL (Artisan/2.0 are Marlin)",
                "Dedicated diode laser. LB: Snapmaker Ray .lbdev (GRBL-class). Confirm bed in Device Settings if your Ray variant differs.",
            ),
            // —— Makera ——
            p(
                ProfileId::MakeraCarvera,
                "Makera Carvera",
                360.0,
                240.0,
                &[],
                "GRBL",
                "Carvera",
                "Makera Carvera product page (360×240×140 mm); wiki laser engraving uses LightBurn",
                "Desktop CNC with built-in 2.5W diode. Work area 360×240 for laser/CNC. LB: create GRBL (or vendor guidance) at 360×240; often run G-code via Makera control after export.",
            ),
            p(
                ProfileId::MakeraCarveraAir,
                "Makera Carvera Air",
                300.0,
                200.0,
                &[],
                "GRBL",
                "Carvera Air",
                "Makera Carvera Air (300×200×130 mm); optional 5W laser",
                "CNC + optional 5W diode. LB work area 300×200 mm.",
            ),
            // —— ChinaCNCzone (china-cncrouter.com), Mach3-class ——
            p(
                ProfileId::Chinacnc3040,
                "ChinaCNC 3040",
                300.0,
                400.0,
                &[],
                "Mach3",
                "3040",
                "china-cncrouter.com / ChinaCNCzone HY-3040 (effective 300×400×150 mm); Mach3",
                "Popular cheap mini router (Mach3 USB/parallel). Bed 300×400. DXF handoff for laser head or CAM; LB Mach3 if a laser is fitted. Confirm travel on your clone.",
            ),
            p(
                ProfileId::Chinacnc6040,
                "ChinaCNC 6040",
                400.0,
                600.0,
                &[],
                "Mach3",
                "6040",
                "ChinaCNCzone CNC 6040 Mach3 (effective 400×600×100 mm)",
                "Mach3 mini router 400×600. Same story as 3040 — DXF for laser/CAM; verify Z travel on your frame.",
            ),
            p(
                ProfileId::Chinacnc6090,
                "ChinaCNC 6090",
                600.0,
                900.0,
                &[],
                "Mach3",
                "6090",
                "ChinaCNCzone 6090 (table ~600×900; effective travel often ~580×880 on DSP builds)",
                "Larger Mach3/DSP router. Export uses 600×900; shrink field_mm or edit bed if your effective travel is 580×880.",
            ),
        ]
    })
}

pub fn profile(id: ProfileId) -> &'static LaserProfile {
    match builtin_profiles().iter().find(|p| p.id == id) {
        Some(p) => p,
        None => &builtin_profiles()[0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_profiles_parse_and_cite_source() {
        let mut seen = std::collections::BTreeSet::new();
        for pr in builtin_profiles() {
            assert!(seen.insert(pr.id.as_str()), "dup {}", pr.id.as_str());
            assert_eq!(ProfileId::parse(pr.id.as_str()), Some(pr.id));
            assert!(pr.bed_width_mm > 0.0 && pr.bed_height_mm > 0.0);
            assert!(!pr.source.is_empty());
        }
        assert!(seen.len() >= 18);
    }

    #[test]
    fn brand_spot_checks() {
        assert_eq!(profile(ProfileId::SnapmakerArtisan).bed_width_mm, 400.0);
        assert_eq!(profile(ProfileId::SnapmakerArtisan).lightburn_device, "Marlin");
        assert_eq!(profile(ProfileId::MakeraCarvera).bed_width_mm, 360.0);
        assert_eq!(profile(ProfileId::Chinacnc6040).lightburn_device, "Mach3");
        assert_eq!(profile(ProfileId::CommarkerB6).bed_width_mm, 150.0);
        assert_eq!(ProfileId::parse("artisan"), Some(ProfileId::SnapmakerArtisan));
        assert_eq!(ProfileId::parse("6040"), Some(ProfileId::Chinacnc6040));
        assert_eq!(ProfileId::parse("carvera-air"), Some(ProfileId::MakeraCarveraAir));
    }
}
