//! The shaft spline's record data and the kernel's (`solidcore::teeth`), field for field: the
//! numbers go to the kernel, the plan and its outlines come back, and an outline's pieces go
//! both ways.

use solidcore::teeth as k;
use solidcore::teeth::master_spline::Span;
use solidcore::teeth::outline::SpaceSection as KernelSection;
use solidcore::teeth::shaft_spline as ks;
use solidcore_record::shaft_spline::{
    MasterArc, MasterPlaced, OutlinePiece, SpaceSection, SplineCorner, SplineForm, SplineMade,
    SplineMake, SplineMaster, SplineNumbers, SplinePlan, SplineRefusal, ToothOutline, ToothRole,
};

/// A record kind of teeth as the kernel's.
#[must_use]
pub fn form(f: SplineForm) -> ks::SplineForm {
    match f {
        SplineForm::Involute => ks::SplineForm::Involute,
        SplineForm::Straight => ks::SplineForm::Straight,
        SplineForm::Serration => ks::SplineForm::Serration,
    }
}

/// A record choice of the new part as the kernel's.
#[must_use]
pub fn make(m: SplineMake) -> ks::SplineMake {
    match m {
        SplineMake::Shaft => ks::SplineMake::Shaft,
        SplineMake::Hub => ks::SplineMake::Hub,
        SplineMake::Both => ks::SplineMake::Both,
    }
}

/// A record tip corner as the kernel's.
#[must_use]
pub fn corner(c: SplineCorner) -> ks::SplineCorner {
    match c {
        SplineCorner::Plain => ks::SplineCorner::Plain,
        SplineCorner::Chamfered => ks::SplineCorner::Chamfered,
        SplineCorner::Rounded => ks::SplineCorner::Rounded,
    }
}

/// A record master spline as the kernel's.
#[must_use]
pub fn master(m: SplineMaster) -> ks::SplineMaster {
    match m {
        SplineMaster::None => ks::SplineMaster::None,
        SplineMaster::ToothOut => ks::SplineMaster::ToothOut,
        SplineMaster::WideTooth => ks::SplineMaster::WideTooth,
    }
}

/// The record's numbers as the kernel's. Which parts take the spline (`with_hub`, `with_shaft`)
/// is the caller's own business and has no counterpart in the kernel.
#[must_use]
pub fn numbers(n: &SplineNumbers) -> ks::SplineNumbers {
    ks::SplineNumbers {
        form: form(n.form),
        make: make(n.make),
        teeth: n.teeth,
        length: n.length,
        module: n.module,
        pressure_angle: n.pressure_angle,
        addendum: n.addendum,
        dedendum: n.dedendum,
        root_fillet: n.root_fillet,
        profile_shift: n.profile_shift,
        thinning: n.thinning,
        hub_addendum: n.hub_addendum,
        tip_clearance: n.tip_clearance,
        form_clearance: n.form_clearance,
        major_diameter: n.major_diameter,
        minor_diameter: n.minor_diameter,
        tooth_width: n.tooth_width,
        flank_angle: n.flank_angle,
        root_round: n.root_round,
        tip_corner: corner(n.tip_corner),
        tip_size: n.tip_size,
        major_clearance: n.major_clearance,
        minor_clearance: n.minor_clearance,
        hub_tip_round: n.hub_tip_round,
        side_clearance: n.side_clearance,
        hub_root_round: n.hub_root_round,
        hub_diameter: n.hub_diameter,
        master: master(n.master),
        master_angle: n.master_angle,
        cutter_diameter: n.cutter_diameter,
        end_chamfer: n.end_chamfer,
        entry_chamfer: n.entry_chamfer,
    }
}

/// The kernel's report of what a spline comes to as the record's.
#[must_use]
pub fn record_made(m: &ks::SplineMade) -> SplineMade {
    SplineMade {
        teeth: m.teeth,
        shaft_major_diameter: m.shaft_major_diameter,
        shaft_minor_diameter: m.shaft_minor_diameter,
        hub_major_diameter: m.hub_major_diameter,
        hub_minor_diameter: m.hub_minor_diameter,
        pitch_diameter: m.pitch_diameter,
        base_diameter: m.base_diameter,
        form_diameter: m.form_diameter,
        undercut: m.undercut,
        sharp_tip_diameter: m.sharp_tip_diameter,
        side_clearance_each: m.side_clearance_each,
        tip_gap: m.tip_gap,
        root_gap: m.root_gap,
        form_gap: m.form_gap,
        turning_play: m.turning_play,
        positions: m.positions,
        master_tooth: m.master_tooth,
        master_turn: m.master_turn,
        flank_tolerance: m.flank_tolerance,
    }
}

/// A kernel refusal as the record's.
#[must_use]
pub fn record_refusal(r: &ks::SplineRefusal) -> SplineRefusal {
    SplineRefusal {
        key: r.key.clone(),
        words: r.words.clone(),
    }
}

/// A record role of an outline's piece as the kernel's.
#[must_use]
pub fn role(r: ToothRole) -> k::Role {
    match r {
        ToothRole::Flank => k::Role::Flank,
        ToothRole::Fillet => k::Role::Fillet,
        ToothRole::Tip => k::Role::Tip,
        ToothRole::Root => k::Role::Root,
    }
}

/// A kernel role of an outline's piece as the record's.
#[must_use]
pub fn record_role(r: k::Role) -> ToothRole {
    match r {
        k::Role::Flank => ToothRole::Flank,
        k::Role::Fillet => ToothRole::Fillet,
        k::Role::Tip => ToothRole::Tip,
        k::Role::Root => ToothRole::Root,
    }
}

/// A record piece of an outline as the kernel's.
#[must_use]
pub fn piece(p: &OutlinePiece) -> k::Piece {
    match p {
        OutlinePiece::Arc {
            centre,
            radius,
            start,
            sweep,
            role: r,
        } => k::Piece::Arc {
            centre: *centre,
            radius: *radius,
            start: *start,
            sweep: *sweep,
            role: role(*r),
        },
        OutlinePiece::Line { from, to, role: r } => k::Piece::Line {
            from: *from,
            to: *to,
            role: role(*r),
        },
        OutlinePiece::Curve { points, role: r } => k::Piece::Curve {
            points: points.clone(),
            role: role(*r),
        },
        OutlinePiece::Spline {
            degree,
            knots,
            control,
            role: r,
        } => k::Piece::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.clone(),
            role: role(*r),
        },
    }
}

/// A kernel piece of an outline as the record's.
#[must_use]
pub fn record_piece(p: &k::Piece) -> OutlinePiece {
    match p {
        k::Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            role,
        } => OutlinePiece::Arc {
            centre: *centre,
            radius: *radius,
            start: *start,
            sweep: *sweep,
            role: record_role(*role),
        },
        k::Piece::Line { from, to, role } => OutlinePiece::Line {
            from: *from,
            to: *to,
            role: record_role(*role),
        },
        k::Piece::Curve { points, role } => OutlinePiece::Curve {
            points: points.clone(),
            role: record_role(*role),
        },
        k::Piece::Spline {
            degree,
            knots,
            control,
            role,
        } => OutlinePiece::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.clone(),
            role: record_role(*role),
        },
    }
}

/// The record's pieces of one tooth as the kernel's.
#[must_use]
pub fn tooth(pieces: &[OutlinePiece]) -> Vec<k::Piece> {
    pieces.iter().map(piece).collect()
}

/// A kernel outline as the record's.
#[must_use]
pub fn record_outline(o: &k::Outline) -> ToothOutline {
    let (least_radius, most_radius) = o.radii();
    ToothOutline {
        teeth: o.teeth(),
        pitch_angle: o.pitch_angle(),
        tooth: o.tooth().iter().map(record_piece).collect(),
        tolerance: o.tolerance(),
        least_radius,
        most_radius,
    }
}

/// A kernel arc of a master spline as the record's.
#[must_use]
pub fn record_arc(s: &Span) -> MasterArc {
    MasterArc {
        radius: s.radius,
        start: s.start,
        end: s.end,
    }
}

/// A kernel plan as the record's.
#[must_use]
pub fn record_plan(p: &ks::SplinePlan) -> SplinePlan {
    SplinePlan {
        shaft: record_outline(&p.pair.first.outline),
        hub: record_outline(&p.pair.second.outline),
        hub_start: p.pair.second_start,
        made: record_made(&p.made),
        master: p
            .master
            .as_ref()
            .zip(p.hub_arc_on_shaft.as_ref())
            .map(|(m, on_shaft)| MasterPlaced {
                tooth: m.placement.tooth,
                turn: m.placement.turn,
                middle: m.middle,
                shaft_arc: record_arc(&m.shaft_arc),
                hub_arc: record_arc(&m.hub_arc),
                hub_arc_on_shaft: record_arc(on_shaft),
            }),
        tooth_pitch: p.tooth_pitch,
        tooth_at: p.tooth_at,
    }
}

/// A kernel section of a tooth space as the record's.
#[must_use]
pub fn record_section(s: &KernelSection) -> SpaceSection {
    SpaceSection {
        floor: s.floor.iter().map(record_piece).collect(),
        start: s.corners.0,
        end: s.corners.1,
        apart: s.apart,
    }
}
