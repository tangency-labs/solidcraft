//! A sheet metal part's surfaces: its panels, each drawn in its own chart, and the joints where
//! they meet.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use solidcraft_geom::bspline::NurbsSurface;
use solidcraft_geom::{Region2, Seg2, Vec2, Vec3};

/// A sheet metal part's surfaces: the panels and the joints between them. The material lies on
/// the side of the sheet that a chart's `u × v` points to, `thickness` deep.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub thickness: f64,
    /// Where the neutral surface lies through the thickness, as a fraction of it from the
    /// inside of each bend.
    pub k_factor: f64,
    pub panels: Vec<Panel>,
    pub joints: Vec<Joint>,
    /// The reliefs cut where bends meet.
    pub corners: Vec<Corner>,
    /// The cuts that cross a joint (see [`Cut`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<Cut>,
}

/// A cut through the sheet that crosses a joint, so that no one panel's region can hold it: a
/// hole across a bend's tangent line, or across a seam. Its region is drawn in the flat chart of
/// a panel it reaches (the chart the development places: a roll's neutral one), and it is cut
/// from every piece of the flat it reaches, bends' zones included. What it leaves of a zone bends
/// where it still joins both panels, and is a tab of the one panel it joins otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub panel: usize,
    pub region: Region2,
}

/// What is cut where two bends meet at a corner. A round or a square relief is cut from the flat
/// pattern wherever it reaches: both bends' zones and every panel round the corner alike, none of
/// them first. It is centered where the two zones' center lines cross, `size` across: a round,
/// or the four-sided relief whose sides run along the two bends, `size` apart. A notch or a tear
/// is the bends' own ends (see [`Relief`]), and cuts nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    /// The two bend joints.
    pub joints: [usize; 2],
    pub relief: CornerRelief,
    pub size: f64,
}

/// The shape of a corner's relief (see [`Corner`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CornerRelief {
    Notch,
    Tear,
    Round,
    Square,
}

/// A face of the sheet. Its region is drawn in its surface's own parameters, so whatever is drawn
/// on the panel keeps its place on the surface, and the surface's flow (its parameter lines)
/// carries to the flat exactly. An empty region is a panel with no material of its own, which
/// still carries the joints on it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub name: String,
    pub surface: Surface,
    pub region: Region2,
}

/// The surface a panel lies on, and so what its parameters are. A curved panel is a roll: its
/// radius is the panel's own geometry, where a bend's is a value of the joint. It flattens
/// through its neutral surface, as a bend does, so a roll and a bend of one radius give the same
/// flat. The parameters run so that the material lies on the side their `u x v` points to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// A planar panel: its parameters are its plane's coordinates.
    #[default]
    Plane,
    /// A panel rolled on a cylinder of `radius` (the sheet's). Its parameters are length along
    /// the axis and angle round it (radians).
    Cylinder { radius: f64, material_inward: bool },
    /// A panel rolled on a cone whose rulings make `half_angle` (radians) with its axis. Its
    /// parameters are distance from the apex along a ruling and angle round the axis (radians).
    Cone { half_angle: f64, material_inward: bool },
    /// A free-form panel on a spline surface, whose parameters are the surface's own. It
    /// flattens without stretch where it is developable; any stretch it cannot avoid is
    /// reported with the flat, never forced.
    Spline(Arc<NurbsSurface>),
}

/// Where a joint lies on one of its panels: a straight line in the panel's parameters, through
/// `origin` along the unit `dir`. The two anchors of a joint are the same line on the sheet,
/// with the same origin and direction, and the `a` panel's material lies on the left of it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub panel: usize,
    pub origin: Vec2,
    pub dir: Vec2,
}

/// Two panels meeting along a line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub a: Anchor,
    pub b: Anchor,
    pub kind: JointKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum JointKind {
    /// The sheet runs on from one panel to the other round a bend.
    Bend(Bend),
    /// The sheet runs on from one panel into the next with no bend between: where a roll
    /// meets a flat panel or another roll tangent to it.
    Seam,
    /// The panels are cut apart along their shared edge (see [`Rip`]), and the flat pattern lays
    /// them out apart unless other joints hold them together.
    Rip(Rip),
}

/// How a rip parts two panels along the edge they share, whatever the edge's course and the
/// panels' surfaces. At every point of the edge each panel is cut back from it, square to it on
/// its own surface, by as much as its lap asks there: so a rip between two planes, round a
/// roll's rim or along a crease between two free-form panels is the one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rip {
    /// The edge on each panel `(a, b)`, in its parameters: the same points of it, in order from
    /// the joint's start (its anchors' origin) to its end. Two make a straight edge.
    pub edges: (Vec<Vec2>, Vec<Vec2>),
    /// The edge on each panel `(a, b)` exactly, where its source knows it: lines, circular arcs
    /// and free-form spans in the panel's parameters, end to end from the joint's start to its
    /// end. Empty where only the points are known.
    pub curves: (Vec<Seg2>, Vec<Seg2>),
    /// How far the sheet turns across the edge at each of those points (radians, from the `a`
    /// panel to the `b` one, positive toward the material, as a bend turns): 0 where it would run
    /// straight on, so that a cut through a flat panel is a rip too.
    pub turns: Vec<f64>,
    pub lap: Lap,
}

/// How a rip parts its panels: what each is cut back from the edge, given how far the sheet
/// turns there and how thick it is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Lap {
    /// Each panel's metal kept `gap / 2` off the surface halfway between the two, through its
    /// whole thickness: a mitered corner, or a slit `gap` wide through a flat panel.
    Miter { gap: f64 },
    /// Each panel cut back the given distance from the edge, `(a, b)`, whatever the turn.
    Cut(f64, f64),
}

impl Lap {
    /// What each panel `(a, b)` is cut back from the edge, square to it on its own surface,
    /// where the sheet turns `turn` across it and is `thickness` thick.
    pub fn cuts(&self, turn: f64, thickness: f64) -> (f64, f64) {
        match *self {
            Lap::Cut(a, b) => (a.max(0.0), b.max(0.0)),
            Lap::Miter { gap } => {
                // In the `a` panel's frame at the edge: y toward its material, z on toward the
                // `b` panel. Into `a` is -z; into `b`, turned by `turn` toward the material, and
                // `b`'s material normal turns with it.
                let (sin, cos) = turn.sin_cos();
                let (into_a, into_b, up_b) = (Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, sin, cos), Vec3::new(0.0, cos, -sin));
                // The plane halfway between them holds the edge; its normal toward `a`.
                let half = into_a + into_b;
                let m = match Vec3::new(0.0, -half.z, half.y).normalized() {
                    Some(m) if m.dot(into_a) >= 0.0 => m,
                    Some(m) => -m,
                    None => into_a,
                };
                let cut = |into: Vec3, up: Vec3, m: Vec3| {
                    let along = into.dot(m);
                    if along > 1e-9 { ((gap / 2.0 - (thickness * up.dot(m)).min(0.0)) / along).max(0.0) } else { 0.0 }
                };
                (cut(into_a, Vec3::Y, m), cut(into_b, up_b, -m))
            }
        }
    }
}

/// A bend between two panels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bend {
    /// How far the sheet turns, in radians, more than 0 and at most a half turn.
    pub angle: f64,
    /// The inside radius.
    pub radius: f64,
    /// The bend turns toward the material side of the sheet, so the sheet runs round the
    /// outside of the bend; otherwise round the inside.
    pub toward_material: bool,
    /// What the anchors mark on the panels.
    pub datum: Datum,
    /// How long the edge is where the two panels meet, along the anchors from their origin: the
    /// zone's ends lie within it, where the corners and rips at its ends leave them.
    pub length: f64,
    /// The bend zone's ends along the joint.
    pub ends: ZoneEnds,
    /// What each panel keeps beyond the zone's ends, between its anchor and where the bend
    /// would start: `(a, b)`.
    pub tails: (Tail, Tail),
    /// What is cut at each of the zone's ends, `(start, end)` along the joint.
    pub relief: (Relief, Relief),
}

/// What is cut at one end of a bend's zone, from both its panels alike. Reliefs are the bend's,
/// never drawn on its panels: they move with its radius, its thickness and its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Relief {
    /// Nothing: past the end each panel keeps or trims its material as its tail says, so a kept
    /// tail is torn from the bend along the end.
    #[default]
    None,
    /// Past the end both panels stop at their tangent lines: where the bend meets another at a
    /// corner, or where a rip cuts the panels beside it.
    Notch,
    /// A bend relief in each panel that runs on past the end: a slot `width` along past the
    /// end, cut from `depth` inside the panel's tangent line out through its edge. None is cut
    /// in a panel that runs on less far than that.
    Slot { width: f64, depth: f64 },
    /// Torn tight against a surface across the bend, as where a rip parts the panels beyond a
    /// corner: past the end both panels stop at their tangent lines, as at a notch, and the
    /// zone's end runs from its given end on the `a` panel's tangent line to its given end on
    /// the `b` panel's, its metal kept off the surface by as much as at those two ends and, in
    /// between, in proportion to how far the bend has turned, so that it follows the surface
    /// all the way round. The surface is the plane of the points `p` with `p · normal = offset`
    /// in the bend's frame at its `a` anchor (`x` along the joint, `y` toward the material, `z`
    /// toward the `b` panel), `normal` toward the bend's side; a lap or a free-form rip will
    /// part the panels along other surfaces.
    Tear { normal: Vec3, offset: f64 },
}

/// The line the anchors mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Datum {
    /// The virtual sharp, where the two panels' surfaces would meet if the bend had no radius.
    /// Changing the radius moves neither panel. A half turn has no sharp.
    Sharp,
    /// The tangent line, where each panel's flat material ends and the bend starts. A hem,
    /// whose panels never meet, is anchored here.
    Tangent,
}

/// The bend zone along its joint, in lengths along the anchors from their origin: where its
/// edge on the `a` side starts and ends (`near`), and where its edge on the `b` side does
/// (`far`). A square end has the same length in both; a torn end does not.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneEnds {
    pub near: (f64, f64),
    pub far: (f64, f64),
}

/// What a panel keeps beyond the bend zone's ends, between its anchor and its tangent line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tail {
    /// The panel's own outline: a base that runs on beside a shorter flange keeps its edge.
    Keep,
    /// Nothing: past the zone's ends the panel starts where the bend would.
    Trim,
}

impl Bend {
    /// The flat width of the bend zone: the length of its neutral arc.
    pub fn allowance(&self, thickness: f64, k_factor: f64) -> f64 {
        self.angle * (self.radius + k_factor * thickness)
    }

    /// How far each anchor lies from its panel's tangent line, toward the other panel.
    pub fn setback(&self, thickness: f64) -> f64 {
        match self.datum {
            Datum::Tangent => 0.0,
            Datum::Sharp => {
                let surface_radius = if self.toward_material { self.radius + thickness } else { self.radius };
                surface_radius * (self.angle / 2.0).tan()
            }
        }
    }
}
