//! Built-in machine profiles: hobby / desktop first, then easy OEM shop-floor hooks.
//!
//! Hobby / file-or-serial class: GRBL / FluidNC routers, Mach3 China beds, Makera Carvera,
//! Snapmaker Marlin, Repetier senders, BJJCZ/JCZ galvo (LightBurn handoff).
//! OEM class: Haas (MTConnect), Okuma (THINC + MTConnect), Fanuc / DN-class (FOCAS),
//! Mazak Smooth (MTConnect). Excluded on purpose: partner-only CELOS apps, TRUMPF deep
//! stacks, and anything that is not a documented open protocol or file/serial path.

use crate::{
    MachineAxes, MachineIntegration, MachineIntegrationKind, MachineProfile, ProbeCapability, ProbeMacroStyle, SpindleTaper, ToolChanger,
    ToolChangerKind,
};

fn manual_er(name: &str, oem: &str, model: &str, rpm: u32, feed: f64, rapid: f64) -> MachineProfile {
    MachineProfile {
        name: name.into(),
        oem: oem.into(),
        model: model.into(),
        axes: MachineAxes::ThreeAxis,
        spindle_taper: SpindleTaper::ErCollet,
        tool_changer: ToolChanger { kind: ToolChangerKind::Manual, pockets: 1, ..ToolChanger::default() },
        integration: MachineIntegration::default(),
        probe: ProbeCapability::default(),
        max_spindle_rpm: rpm,
        max_feedrate_mmpm: feed,
        rapid_feedrate_mmpm: rapid,
    }
}

/// Built-in machine presets (document keys for `ManufactureSettings::machines`).
///
/// Hobby / desktop entries are listed first so Setup pickers surface them before shop OEM mills.
pub fn preset_machines() -> Vec<(&'static str, MachineProfile)> {
    let mut out = Vec::new();

    // ── Hobby / desktop / file-or-serial ─────────────────────────────────────
    {
        let mut m = manual_er("GRBL Router", "GRBL", "Generic GRBL / FluidNC", 24_000, 3_000.0, 5_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::SerialGcode,
            default_port: 0,
            notes: "Post GRBL G-code to a file, then send with Universal G-code Sender, OpenBuilds CONTROL, or FluidNC web UI. No MTConnect.".into(),
            source: "GRBL / grblHAL / FluidNC public docs; SolidCraft GRBL post".into(),
        };
        out.push(("GRBL Router", m));
    }
    {
        let mut m = manual_er("ChinaCNC 3040", "ChinaCNCzone", "3040 (Mach3)", 24_000, 2_000.0, 3_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes:
                "Typical 300×400 China bed on Mach3/Mach4. Post Mach3 NC file; load on the PC control. Laser head → LightBurn chinacnc-3040 profile."
                    .into(),
            source: "china-cncrouter / ChinaCNCzone bed sizes; SolidCraft Mach3 post".into(),
        };
        out.push(("ChinaCNC 3040", m));
    }
    {
        let mut m = manual_er("ChinaCNC 6040", "ChinaCNCzone", "6040 (Mach3)", 24_000, 2_500.0, 4_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes: "600×400 Mach3-class bed. File post only — no shop-floor agent.".into(),
            source: "china-cncrouter bed sizes; SolidCraft Mach3 post".into(),
        };
        out.push(("ChinaCNC 6040", m));
    }
    {
        let mut m = manual_er("ChinaCNC 6090", "ChinaCNCzone", "6090 (Mach3)", 24_000, 2_500.0, 4_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes: "600×900 Mach3-class bed. Post NC file to the control PC.".into(),
            source: "china-cncrouter bed sizes; SolidCraft Mach3 post".into(),
        };
        out.push(("ChinaCNC 6090", m));
    }
    {
        let mut m = manual_er("Makera Carvera", "Makera", "Carvera", 15_000, 2_000.0, 3_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes: "Desktop CNC (360×240). Post GRBL-class G-code; run via Makera control / sender. Diode laser → LightBurn makera-carvera.".into(),
            source: "Makera Carvera public specs; SolidCraft GRBL post".into(),
        };
        out.push(("Makera Carvera", m));
    }
    {
        let mut m = manual_er("Makera Carvera Air", "Makera", "Carvera Air", 15_000, 2_000.0, 3_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes: "Air variant (300×200). File G-code handoff; not MTConnect.".into(),
            source: "Makera Carvera Air public specs; SolidCraft GRBL post".into(),
        };
        out.push(("Makera Carvera Air", m));
    }
    {
        let mut m = manual_er("Snapmaker Artisan", "Snapmaker", "Artisan (Marlin)", 12_000, 3_000.0, 3_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::SerialGcode,
            default_port: 0,
            notes: "Marlin firmware (not GRBL). Post Marlin G-code / use vendor app; laser → LightBurn Marlin + Artisan .lbdev.".into(),
            source: "Snapmaker Artisan docs; SolidCraft Marlin post; LightBurn Snapmaker device".into(),
        };
        out.push(("Snapmaker Artisan", m));
    }
    {
        let mut m = manual_er("Snapmaker A350", "Snapmaker", "2.0 A350 (Marlin)", 12_000, 3_000.0, 3_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::SerialGcode,
            default_port: 0,
            notes: "Snapmaker 2.0 Marlin. File or Luban / serial; laser bed 320×350 → LightBurn Snapmaker 2.0.".into(),
            source: "Snapmaker 2.0 docs; SolidCraft Marlin post".into(),
        };
        out.push(("Snapmaker A350", m));
    }
    {
        let mut m = manual_er("Repetier / Marlin sender", "Repetier", "Repetier-Host / Firmware", 24_000, 3_000.0, 5_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::SerialGcode,
            default_port: 0,
            notes: "Use when the shop runs Repetier-Host (or Repetier-Firmware) over USB/serial. Post Repetier or Marlin G-code to a file, then open in the sender — no companion endpoint required.".into(),
            source: "Repetier-Host / Repetier-Firmware public docs; SolidCraft Repetier post".into(),
        };
        out.push(("Repetier", m));
    }
    {
        out.push((
            "BJJCZ JCZ",
            MachineProfile {
                name: "BJJCZ / JCZ Fiber".into(),
                oem: "BJJCZ".into(),
                model: "JCZ / EzCAD galvo".into(),
                axes: MachineAxes::ThreeAxis,
                spindle_taper: SpindleTaper::Unknown,
                tool_changer: ToolChanger {
                    kind: ToolChangerKind::None,
                    pockets: 0,
                    ..ToolChanger::default()
                },
                integration: MachineIntegration {
                    kind: MachineIntegrationKind::JczLaser,
                    default_port: 0,
                    notes: "Galvo fiber (ComMarker-class). Manufacture mill posts do not drive the card — use laser.exportPackage / LightBurn JCZFiber (or EzCAD). SolidCraft does not speak BJJCZ USB directly in v1.".into(),
                    source: "BJJCZ/JCZ + LightBurn JCZFiber; SolidCraft laser LightBurn handoff".into(),
                },
                probe: ProbeCapability::default(),
                max_spindle_rpm: 0,
                max_feedrate_mmpm: 0.0,
                rapid_feedrate_mmpm: 0.0,
            },
        ));
    }
    {
        let mut m = manual_er("LinuxCNC Desktop", "LinuxCNC", "Generic", 24_000, 5_000.0, 8_000.0);
        m.integration = MachineIntegration {
            kind: MachineIntegrationKind::FileGcode,
            default_port: 0,
            notes: "Post LinuxCNC G-code to a file; run under LinuxCNC AXIS / Gmoccapy. Optional later: network DNC.".into(),
            source: "LinuxCNC docs; SolidCraft LinuxCNC post".into(),
        };
        out.push(("LinuxCNC", m));
    }

    // ── Shop OEM (easy open protocols) ───────────────────────────────────────
    out.push((
        "Haas VF-2",
        MachineProfile {
            name: "Haas VF-2".into(),
            oem: "Haas".into(),
            model: "VF-2 NGC".into(),
            axes: MachineAxes::ThreeAxis,
            spindle_taper: SpindleTaper::Cat40,
            tool_changer: ToolChanger {
                kind: ToolChangerKind::SideMount,
                pockets: 20,
                random_access: true,
                dual_arm: false,
                change_time_s: 4.2,
                max_tool_diameter_mm: 76.0,
                max_tool_length_mm: 279.0,
                max_tool_weight_kg: 5.4,
            },
            integration: MachineIntegration {
                kind: MachineIntegrationKind::MtConnect,
                default_port: 8082,
                notes: "Haas NGC MTConnect agent over HTTP. Probe/Current/Sample at http://<ip>:8082/<MODEL>/…".into(),
                source: "Haas NGC Machine Data Collection (MTConnect port 8082)".into(),
            },
            probe: ProbeCapability {
                present: true,
                style: ProbeMacroStyle::HaasInspectionPlus,
                tool_number: 25,
                notes: "Renishaw Inspection Plus on Haas (G65 P981x; P9832 on / P9833 off). Confirm pocket + calib.".into(),
            },
            max_spindle_rpm: 8_100,
            max_feedrate_mmpm: 25_400.0,
            rapid_feedrate_mmpm: 25_400.0,
        },
    ));
    out.push((
        "Haas UMC-750",
        MachineProfile {
            name: "Haas UMC-750".into(),
            oem: "Haas".into(),
            model: "UMC-750".into(),
            axes: MachineAxes::FiveAxis,
            spindle_taper: SpindleTaper::Cat40,
            tool_changer: ToolChanger {
                kind: ToolChangerKind::SideMount,
                pockets: 40,
                random_access: true,
                dual_arm: false,
                change_time_s: 4.5,
                max_tool_diameter_mm: 76.0,
                max_tool_length_mm: 279.0,
                max_tool_weight_kg: 5.4,
            },
            integration: MachineIntegration {
                kind: MachineIntegrationKind::MtConnect,
                default_port: 8082,
                notes: "Same NGC MTConnect surface as other Haas mills; model path often UMC.".into(),
                source: "Haas NGC Machine Data Collection (MTConnect port 8082)".into(),
            },
            probe: ProbeCapability {
                present: true,
                style: ProbeMacroStyle::HaasInspectionPlus,
                tool_number: 40,
                notes: "UMC typically carries a spindle probe in a high ATC pocket; verify local pocket map.".into(),
            },
            max_spindle_rpm: 8_100,
            max_feedrate_mmpm: 22_900.0,
            rapid_feedrate_mmpm: 22_900.0,
        },
    ));
    out.push((
        "Okuma GENOS M560",
        MachineProfile {
            name: "Okuma GENOS M560".into(),
            oem: "Okuma".into(),
            model: "GENOS M560-V".into(),
            axes: MachineAxes::ThreeAxis,
            spindle_taper: SpindleTaper::Cat40,
            tool_changer: ToolChanger {
                kind: ToolChangerKind::Arm,
                pockets: 32,
                random_access: true,
                dual_arm: true,
                change_time_s: 1.3,
                max_tool_diameter_mm: 80.0,
                max_tool_length_mm: 300.0,
                max_tool_weight_kg: 8.0,
            },
            integration: MachineIntegration {
                kind: MachineIntegrationKind::Thinc,
                default_port: 5000,
                notes: "Prefer Okuma THINC Open API on the OSP-P control; MTConnect agent/adapter also available (often :5000).".into(),
                source: "Okuma Open API SDK (THINC); Okuma MTConnect Agent & Adapter".into(),
            },
            probe: ProbeCapability::default(),
            max_spindle_rpm: 15_000,
            max_feedrate_mmpm: 40_000.0,
            rapid_feedrate_mmpm: 40_000.0,
        },
    ));
    out.push((
        "Fanuc 0i-MF",
        MachineProfile {
            name: "Fanuc 0i-MF class".into(),
            oem: "Fanuc".into(),
            model: "0i-MF".into(),
            axes: MachineAxes::ThreeAxis,
            spindle_taper: SpindleTaper::Bt40,
            tool_changer: ToolChanger {
                kind: ToolChangerKind::Carousel,
                pockets: 24,
                random_access: true,
                dual_arm: false,
                change_time_s: 2.5,
                max_tool_diameter_mm: 80.0,
                max_tool_length_mm: 300.0,
                max_tool_weight_kg: 7.0,
            },
            integration: MachineIntegration {
                kind: MachineIntegrationKind::Focas,
                default_port: 8193,
                notes: "Fanuc FOCAS Ethernet. Covers most DN Solutions / Doosan Fanuc-control mills — treat DN as this class, not a separate SDK."
                    .into(),
                source: "FANUC FOCAS / FASMTC MTConnect Server (FOCAS → MTConnect)".into(),
            },
            probe: ProbeCapability {
                present: true,
                style: ProbeMacroStyle::FanucInspectionPlus,
                tool_number: 24,
                notes: "Inspection Plus O98xx when Renishaw macros are installed; pocket varies by builder.".into(),
            },
            max_spindle_rpm: 12_000,
            max_feedrate_mmpm: 30_000.0,
            rapid_feedrate_mmpm: 30_000.0,
        },
    ));
    out.push((
        "Mazak VCN-530C",
        MachineProfile {
            name: "Mazak VCN-530C".into(),
            oem: "Mazak".into(),
            model: "VCN-530C Smooth".into(),
            axes: MachineAxes::ThreeAxis,
            spindle_taper: SpindleTaper::Cat40,
            tool_changer: ToolChanger {
                kind: ToolChangerKind::Carousel,
                pockets: 30,
                random_access: true,
                dual_arm: false,
                change_time_s: 2.0,
                max_tool_diameter_mm: 80.0,
                max_tool_length_mm: 300.0,
                max_tool_weight_kg: 8.0,
            },
            integration: MachineIntegration {
                kind: MachineIntegrationKind::MtConnect,
                default_port: 5000,
                notes: "Mazak Smooth native MTConnect (SmoothX/G/C). Read-oriented; tooling data items are sparse.".into(),
                source: "Mazak Smooth CNC MTConnect functionality (MTConnect 1.3)".into(),
            },
            probe: ProbeCapability::default(),
            max_spindle_rpm: 12_000,
            max_feedrate_mmpm: 42_000.0,
            rapid_feedrate_mmpm: 42_000.0,
        },
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_include_hobby_and_oem() {
        let presets = preset_machines();
        assert!(presets.len() >= 15);
        for (id, m) in &presets {
            assert!(!id.is_empty());
            assert!(!m.name.is_empty());
            assert!(!m.oem.is_empty());
            assert!(!matches!(m.integration.kind, MachineIntegrationKind::None));
            assert!(!m.integration.source.is_empty());
        }
        assert!(presets.iter().any(|(id, _)| *id == "GRBL Router"));
        assert!(presets.iter().any(|(id, _)| *id == "Repetier"));
        assert!(presets.iter().any(|(id, _)| *id == "Snapmaker Artisan"));
        assert!(presets.iter().any(|(id, _)| *id == "BJJCZ JCZ"));
        assert!(presets.iter().any(|(id, _)| *id == "ChinaCNC 3040"));
        assert!(presets.iter().any(|(id, _)| *id == "Haas VF-2"));
        assert!(presets.iter().any(|(_, m)| m.integration.kind == MachineIntegrationKind::SerialGcode));
        assert!(presets.iter().any(|(_, m)| m.integration.kind == MachineIntegrationKind::FileGcode));
        assert!(presets.iter().any(|(_, m)| m.integration.kind == MachineIntegrationKind::JczLaser));
        assert!(presets.iter().any(|(_, m)| m.integration.kind == MachineIntegrationKind::Thinc));
        assert!(presets.iter().any(|(_, m)| m.integration.kind == MachineIntegrationKind::Focas));
    }
}
