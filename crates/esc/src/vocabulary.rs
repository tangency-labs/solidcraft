//! The built-in operations: where each may appear and the arguments it takes. Names and
//! arguments follow SolidCraft's commands; an operation's arguments are fixed when its geometry
//! is built.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Part,
    Assembly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// An entry's value, in the files listed.
    Entry(&'static [FileKind]),
    /// A sketch line's value.
    SketchLine,
    /// The operation of a `step`.
    StepOp,
    /// Inside another value.
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    Any,
    Length,
    Angle,
    Number,
    /// A plain number or any quantity.
    Quantity,
    Text,
    Direction,
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct Op {
    pub name: &'static str,
    pub place: Place,
    pub positional: &'static [Arg],
    pub required: usize,
    pub named: &'static [(&'static str, Arg)],
    /// Takes any named argument (`import`'s parameter values).
    pub open: bool,
}

const ANY: &[FileKind] = &[FileKind::Part, FileKind::Assembly];
const PART: &[FileKind] = &[FileKind::Part];
const ASSEMBLY: &[FileKind] = &[FileKind::Assembly];

const MODE: Arg = Arg::Choice(&["add", "cut"]);
const KINDS: Arg = Arg::Choice(&["line", "circle", "arc", "ellipse", "spline", "plane", "cylinder", "cone", "sphere", "torus"]);
const MOVE: &[(&str, Arg)] = &[("joint", Arg::Any), ("joints", Arg::Any), ("from", Arg::Any), ("distance", Arg::Length)];
const QUERY: &[(&str, Arg)] = &[
    ("parallel", Arg::Direction),
    ("perpendicular", Arg::Direction),
    ("between", Arg::Any),
    ("from", Arg::Any),
    ("made_by", Arg::Any),
    ("kind", KINDS),
    ("radius", Arg::Length),
    ("extreme", Arg::Direction),
    ("pick", Arg::Choice(&["outermost", "innermost"])),
    ("nearest", Arg::Any),
    ("count", Arg::Number),
];

const fn op(name: &'static str, place: Place, positional: &'static [Arg], named: &'static [(&'static str, Arg)]) -> Op {
    Op { name, place, positional, required: positional.len(), named, open: false }
}

const fn optional(mut o: Op, required: usize) -> Op {
    o.required = required;
    o
}

const TWO: &[Arg] = &[Arg::Any, Arg::Any];
const ONE: &[Arg] = &[Arg::Any];

pub const OPS: &[Op] = &[
    op("param", Place::Entry(ANY), &[Arg::Quantity], &[]),
    Op { name: "import", place: Place::Entry(ANY), positional: &[Arg::Text], required: 1, named: &[], open: true },
    op("connector", Place::Entry(ANY), &[], &[("origin", Arg::Any), ("z", Arg::Direction), ("x", Arg::Direction)]),
    op("import_step", Place::Entry(PART), &[Arg::Text], &[]),
    op("sketch", Place::Entry(PART), &[], &[("on", Arg::Any)]),
    op("extrude", Place::Entry(PART), &[Arg::Any, Arg::Length], &[("mode", MODE)]),
    op("revolve", Place::Entry(PART), ONE, &[("axis", Arg::Any), ("angle", Arg::Angle), ("mode", MODE)]),
    op("fillet", Place::Entry(PART), ONE, &[("r", Arg::Length)]),
    op("chamfer", Place::Entry(PART), ONE, &[("d", Arg::Length), ("angle", Arg::Angle)]),
    op("hole", Place::Entry(PART), ONE, &[("d", Arg::Length), ("at", Arg::Any), ("along", Arg::Direction), ("depth", Arg::Length)]),
    op("shell", Place::Entry(PART), ONE, &[("faces", Arg::Any), ("thickness", Arg::Length)]),
    op("mirror", Place::Entry(PART), ONE, &[("plane", Arg::Any)]),
    op(
        "pattern_rectangular",
        Place::Entry(PART),
        ONE,
        &[
            ("along", Arg::Direction),
            ("count", Arg::Number),
            ("spacing", Arg::Length),
            ("along2", Arg::Direction),
            ("count2", Arg::Number),
            ("spacing2", Arg::Length),
        ],
    ),
    op("pattern_circular", Place::Entry(PART), ONE, &[("axis", Arg::Any), ("count", Arg::Number), ("angle", Arg::Angle)]),
    op("combine", Place::Entry(PART), TWO, &[("mode", Arg::Choice(&["union", "cut", "intersect"]))]),
    op("box", Place::Entry(PART), &[], &[("w", Arg::Length), ("h", Arg::Length), ("d", Arg::Length), ("at", Arg::Any)]),
    op("cylinder", Place::Entry(PART), &[], &[("r", Arg::Length), ("h", Arg::Length), ("at", Arg::Any), ("axis", Arg::Direction)]),
    op("instance", Place::Entry(ASSEMBLY), ONE, &[]),
    op("rigid", Place::Entry(ASSEMBLY), TWO, &[("angle", Arg::Angle), ("offset", Arg::Length)]),
    op("revolute", Place::Entry(ASSEMBLY), TWO, &[("angle", Arg::Angle)]),
    op("slider", Place::Entry(ASSEMBLY), TWO, &[("angle", Arg::Angle), ("offset", Arg::Length)]),
    op("step", Place::Entry(ASSEMBLY), ONE, &[("after", Arg::Any)]),
    op("place", Place::StepOp, ONE, &[]),
    op("insert", Place::StepOp, ONE, MOVE),
    op("press", Place::StepOp, ONE, MOVE),
    op("snap", Place::StepOp, ONE, MOVE),
    op("screw", Place::StepOp, ONE, &[("joint", Arg::Any), ("joints", Arg::Any), ("from", Arg::Any), ("distance", Arg::Length), ("tool", Arg::Text)]),
    optional(op("plane", Place::Value, ONE, &[("x", Arg::Length), ("y", Arg::Length), ("z", Arg::Length), ("normal", Arg::Direction)]), 0),
    op("point", Place::Value, &[], &[("x", Arg::Length), ("y", Arg::Length), ("z", Arg::Length)]),
    op("dir", Place::Value, &[Arg::Number, Arg::Number, Arg::Number], &[]),
    op("axis", Place::Value, &[Arg::Any, Arg::Direction], &[]),
    op("edges", Place::Value, ONE, QUERY),
    op("faces", Place::Value, ONE, QUERY),
    op("center_of", Place::Value, ONE, &[]),
    op("line", Place::SketchLine, TWO, &[]),
    op("arc", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Any], &[]),
    op("circle", Place::SketchLine, ONE, &[]),
    op("coincident", Place::SketchLine, TWO, &[]),
    op("on_curve", Place::SketchLine, TWO, &[]),
    op("along", Place::SketchLine, TWO, &[("at", Arg::Number)]),
    optional(op("horizontal", Place::SketchLine, TWO, &[]), 1),
    optional(op("vertical", Place::SketchLine, TWO, &[]), 1),
    op("parallel", Place::SketchLine, TWO, &[]),
    op("perpendicular", Place::SketchLine, TWO, &[]),
    op("collinear", Place::SketchLine, TWO, &[]),
    op("tangent", Place::SketchLine, TWO, &[]),
    op("equal", Place::SketchLine, TWO, &[]),
    op("concentric", Place::SketchLine, TWO, &[]),
    op("midpoint", Place::SketchLine, TWO, &[]),
    op("symmetric", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Any], &[]),
    op("distance", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("distance_x", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("distance_y", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("point_line_distance", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("line_circle_distance", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("point_circle_distance", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("circle_circle_distance", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
    op("length", Place::SketchLine, &[Arg::Any, Arg::Length], &[]),
    op("line_angle", Place::SketchLine, &[Arg::Any, Arg::Angle], &[]),
    op("angle", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Angle], &[]),
    op("radius", Place::SketchLine, &[Arg::Any, Arg::Length], &[]),
    op("diameter", Place::SketchLine, &[Arg::Any, Arg::Length], &[]),
    op("arc_sweep", Place::SketchLine, &[Arg::Any, Arg::Angle], &[]),
    op("linear_diameter", Place::SketchLine, &[Arg::Any, Arg::Any, Arg::Length], &[]),
];

/// Formula functions. They return a value instead of making geometry.
pub const MATH: &[&str] = &["min", "max", "abs", "sqrt", "sin", "cos", "tan", "atan2"];

/// Sketch elements, whose points may carry seeds, and the points each one has.
pub const ELEMENTS: &[(&str, &[&str])] = &[("line", &["start", "end"]), ("arc", &["center", "start", "end"]), ("circle", &["center"])];

pub const AXES: &[&str] = &["X", "Y", "Z"];
pub const PLANES: &[&str] = &["XY", "YZ", "XZ"];
pub const ORIGIN: &str = "origin";
pub const NOTE_KINDS: &[&str] = &["why", "todo"];

pub fn find(name: &str) -> Option<&'static Op> {
    OPS.iter().find(|o| o.name == name)
}

pub fn is_reserved(name: &str) -> bool {
    AXES.contains(&name) || PLANES.contains(&name) || name == ORIGIN || crate::units::is_unit_word(name)
}

pub fn points_of(element: &str) -> Option<&'static [&'static str]> {
    ELEMENTS.iter().find(|(e, _)| *e == element).map(|(_, p)| *p)
}
