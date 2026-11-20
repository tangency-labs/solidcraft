//! What laying a sheet out flat gives: where each panel's chart lies, each panel and bend zone
//! in the flat frame, what each joint trims, and the cuts. Plain data, beside the record's own
//! (`solidcore-record`'s `develop`, which `solidcore-convert` copies field by field).
//!
//! Not upstream (SolidCraft's sheet development, moved into the kernel, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::Rigid2;
use crate::math::Vec2;
use crate::prism::profile::Region2;

/// A sheet laid out flat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Development {
    /// Where each panel's neutral chart lies in the flat frame (a planar panel's neutral chart
    /// is its chart).
    pub placements: Vec<Option<Rigid2>>,
    /// Each panel with material, in the flat frame.
    pub panels: Vec<Option<FlatPanel>>,
    /// The pieces of the flat pattern: the panels that bends and seams hold together. The pieces
    /// with an anchored panel come first and lie where their anchors put them; the others are
    /// laid beside them, left to right.
    pub pieces: Vec<Vec<usize>>,
    /// Each bend's zone in the flat frame.
    pub zones: Vec<Zone>,
    /// What to take out of each panel before the pieces are joined: its material between its
    /// tangent line and a bend's anchor, along the zone and, where its tail is trimmed, beyond
    /// it. A rip's cut is no trim: it is in its panels' outlines (see [`FlatPanel`]).
    pub trims: Vec<Trim>,
    /// Bends and seams that close a loop, where the layout reached both panels another way.
    pub loops: Vec<Mismatch>,
    /// What is cut from the flat, from every piece it reaches: the corners' reliefs, then the
    /// sheet's cuts.
    pub cuts: Vec<Region2>,
    /// The bend ends a panel runs on past where the bend's relief has no width to cut: the
    /// bend's joint, the panel, and where along the joint the end lies.
    pub unrelieved: Vec<(usize, usize, f64)>,
}

/// A panel laid flat, its rips' cuts taken off. For each loop of its region, `origins` gives what
/// each segment is in the sheet (see [`EdgeFrom`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatPanel {
    /// Its material, in the flat frame.
    pub region: Region2,
    /// What each segment of each of its loops is in the sheet.
    pub origins: Vec<Vec<EdgeFrom>>,
}

/// What an edge of a flat panel is in its sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFrom {
    /// Segment `k` of the same loop of the panel's own region, or what a rip left of it.
    Own(usize),
    /// The panel's new edge along joint `j`'s rip, with what joins it to the edges beside it.
    Rip(usize),
}

/// A bend's zone in the flat frame: its region, its edge on the `a` panel's side (`near`) and on
/// the `b` panel's side (`far`), each from the end at the anchors' origin to the other, and the
/// panels on those sides. Its ends join `near[k]` to `far[k]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// The bend's joint.
    pub joint: usize,
    /// Its material, in the flat frame.
    pub region: Region2,
    /// Its edge on the `a` panel's side.
    pub near: [Vec2; 2],
    /// Its edge on the `b` panel's side.
    pub far: [Vec2; 2],
    /// The `a` and `b` panels.
    pub panels: [usize; 2],
}

/// What a joint takes from a panel: the region to cut, and the line along which the panel's
/// material ends once it is cut (its tangent line at a bend, its cut edge at a rip).
#[derive(Clone, Debug, PartialEq)]
pub struct Trim {
    /// The panel it cuts.
    pub panel: usize,
    /// The joint it is for.
    pub joint: usize,
    /// What it cuts.
    pub region: Region2,
    /// Where the panel's material ends once cut.
    pub line: [Vec2; 2],
}

/// How far a joint that closes a loop is from fitting: laid out through the rest of the loop,
/// its second panel lands `distance` (mm) and `angle` (radians) from where the joint itself
/// would put it. A ring of bends and seams never fits; it needs a rip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mismatch {
    /// The joint that closes the loop.
    pub joint: usize,
    /// How far (mm) its second panel lands from where the joint would put it.
    pub distance: f64,
    /// How far (radians) it is turned from there.
    pub angle: f64,
}
