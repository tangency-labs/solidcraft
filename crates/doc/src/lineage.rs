//! What each face of a body a feature made came from, in the model's terms.
//!
//! The kernel reports, for every operation, which faces and edges of its operands each face and
//! edge of its result came from ([`History`]). A feature may run several operations (a tool
//! built from a sketch, then cut from each target, then split into lumps) whose operands are
//! the feature's tools, intermediate bodies or bodies of the model before it. A [`Lineage`]
//! composes those histories into one over the operands that names can be read from: bodies of
//! the model before the feature (whose faces already have names), moved copies of them, and
//! bodies the feature built itself (whose faces are named by their role: the sketch curve they
//! sweep, the cap they are). Naming (`history_names`) reads it; nothing here samples geometry.

use std::sync::{Arc, OnceLock};

use solidcraft_geom::{Region2, Vec3};
use solidcraft_kernel::Body;
use solidcraft_kernel::history::{BodyHistory, Change, FaceOrigin, FaceProv, History};

use crate::SolvedSketch;

/// Something a body's faces came from, and how its faces are named.
#[derive(Debug)]
pub(crate) enum Operand {
    /// A body of the model before the feature (by name): its faces keep their names.
    Body(String),
    /// A moved copy of a body of the model before the feature, instance `instance` of a pattern
    /// or mirror: its faces are named after their sources (`<name>@F<feature>.<instance>`).
    Copy { body: String, instance: usize },
    /// The cavity side of a shell grown from a body's faces (by name): each face is the inner
    /// wall of the face it offsets (`F<feature>:inner:<name>`).
    Inner(String),
    /// A body the feature built: each face's role (`side:<curve>`, `top`, `+x`, …; the first is
    /// its name, the others aliases), empty where the role is not known exactly.
    Made(Vec<Vec<String>>),
    /// A moved copy of a body another feature (`feature`) builds, instance `instance` of a
    /// pattern or mirror of that feature: its faces named by their roles in that feature
    /// (`F<feature>:<role>@F<pattern>.<instance>`).
    CopiedTool { roles: Vec<Vec<String>>, feature: u64, instance: usize },
    /// A body the feature made on the way to its result (a cut body before it falls into lumps),
    /// named as a whole first so pieces of one face in different results are told apart.
    Stage(Box<Stage>),
}

/// An intermediate body of a feature, with its lineage and, once worked out, its names.
#[derive(Debug)]
pub(crate) struct Stage {
    /// The whole body (a stand-in model body: its mesh is worked out once for every lump).
    pub body: crate::ModelBody,
    pub lineage: Lineage,
    pub names: OnceLock<Arc<crate::history_names::Names>>,
}

/// The history of a body over operands that can be named.
#[derive(Clone, Debug)]
pub(crate) struct Lineage {
    pub(crate) history: History,
    pub(crate) operands: Vec<Arc<Operand>>,
}

impl Lineage {
    /// `body` as it is: every face and edge its own, named as `operand` names them.
    pub(crate) fn of(body: &Body, operand: Operand) -> Lineage {
        Lineage { history: History::identity(body), operands: vec![Arc::new(operand)] }
    }

    /// The lineage of `result`, made by one kernel operation from operands whose lineages are
    /// `inputs` (in the operation's operand order). `None` where the operation reports no
    /// history or its history cannot be composed with theirs: the result is then named from
    /// geometry, flagged as unverified.
    pub(crate) fn step(result: &Body, inputs: &[&Lineage]) -> Option<Lineage> {
        let mut history = result.history()?.clone();
        if history.operands != inputs.len() {
            return None;
        }
        // From the last operand back: each one's operands come in at its place.
        for (k, input) in inputs.iter().enumerate().rev() {
            if let Err(e) = history.substitute(k, &input.history) {
                log::debug!("no lineage: {e}");
                return None;
            }
        }
        let operands: Vec<Arc<Operand>> = inputs.iter().flat_map(|l| l.operands.iter().cloned()).collect();
        (history.operands == operands.len()).then_some(Lineage { history, operands })
    }

    /// This lineage for a moved copy of what feature `feature` built (instance `instance` of a
    /// pattern or mirror of it): the faces it built are named as that feature's, copied.
    pub(crate) fn copied(&self, feature: u64, instance: usize) -> Lineage {
        let operands = self
            .operands
            .iter()
            .map(|op| match op.as_ref() {
                Operand::Made(roles) => Arc::new(Operand::CopiedTool { roles: roles.clone(), feature, instance }),
                _ => op.clone(),
            })
            .collect();
        Lineage { history: self.history.clone(), operands }
    }

    /// Whether each face of the body this is the lineage of came, in whole or in part, from a
    /// face or edge of `name`, a body of the model before the feature.
    pub(crate) fn faces_from_body(&self, name: &str) -> Vec<bool> {
        let from: Vec<bool> = self.operands.iter().map(|op| matches!(op.as_ref(), Operand::Body(n) if n == name)).collect();
        self.history
            .faces
            .iter()
            .map(|face| {
                face.origins.iter().any(|origin| match *origin {
                    FaceOrigin::Face { operand, .. } | FaceOrigin::Edge { operand, .. } | FaceOrigin::Offset { operand, .. } => {
                        from.get(operand) == Some(&true)
                    }
                    FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => false,
                })
            })
            .collect()
    }

    /// A tool of `faces` faces and no edges (a mesh's triangles) whose faces have no role.
    pub(crate) fn unnamed_faces(faces: usize) -> Lineage {
        let faces = (0..faces).map(|face| FaceProv { origins: vec![FaceOrigin::Face { operand: 0, face }], change: Change::Kept }).collect();
        Lineage { history: History { operands: 1, faces, edges: Vec::new() }, operands: vec![Arc::new(Operand::Made(Vec::new()))] }
    }

    /// The lineages of `pieces`, the lumps of `whole` (whose lineage is `lineage`): their faces
    /// named after the whole's, worked out once for the whole, so pieces of one face that end up
    /// in different bodies are told apart. `None` for a piece whose history is not known.
    /// A body in one piece is its own lump: its lineage as it is.
    pub(crate) fn lumps(whole: &Body, lineage: Option<&Lineage>, pieces: &[Body]) -> Vec<Option<Lineage>> {
        let Some(lineage) = lineage else { return vec![None; pieces.len()] };
        if pieces.len() < 2 {
            return vec![Some(lineage.clone()); pieces.len()];
        }
        let body = crate::ModelBody::new(String::new(), whole.clone(), 0);
        let stage = Arc::new(Operand::Stage(Box::new(Stage { body, lineage: lineage.clone(), names: OnceLock::new() })));
        pieces
            .iter()
            .map(|piece| {
                let history = piece.history()?.clone();
                (history.operands == 1).then(|| Lineage { history, operands: vec![stage.clone()] })
            })
            .collect()
    }
}

/// A body made during a feature, with its lineage (`None`: not known; it is then named from
/// geometry).
#[derive(Clone, Debug)]
pub(crate) struct Traced {
    pub body: Body,
    pub lineage: Option<Lineage>,
}

impl Traced {
    /// A body the feature built whose faces' roles are not known: its faces are named from
    /// geometry, those of the bodies it is combined with by their history.
    pub fn unnamed(body: Body) -> Traced {
        let roles = vec![Vec::new(); body.face_count()];
        Traced { lineage: Some(Lineage::of(&body, Operand::Made(roles))), body }
    }

    /// A body the feature built, its faces' roles given (one per face), else not known.
    pub fn made(body: Body, roles: Option<Vec<Vec<String>>>) -> Traced {
        match roles.filter(|r| r.len() == body.face_count()) {
            Some(roles) => Traced { lineage: Some(Lineage::of(&body, Operand::Made(roles))), body },
            None => Traced::unnamed(body),
        }
    }

    /// `copy`, this tool moved: instance `instance` of a pattern or mirror of feature `feature`,
    /// which built it.
    pub fn copied(&self, copy: Body, feature: u64, instance: usize) -> Traced {
        let lineage = self.lineage.as_ref().and_then(|l| Lineage::step(&copy, &[&l.copied(feature, instance)]));
        Traced { body: copy, lineage }
    }

    /// `result`, made by one kernel operation from `inputs` (in its operand order).
    pub fn step(result: Body, inputs: &[&Traced]) -> Traced {
        let lineages: Option<Vec<&Lineage>> = inputs.iter().map(|t| t.lineage.as_ref()).collect();
        let lineage = lineages.and_then(|l| Lineage::step(&result, &l));
        Traced { body: result, lineage }
    }

    /// `result`, made by one kernel operation from this body alone. Where that operation's
    /// history can't be composed with this body's (it blends an edge the same feature made: a
    /// boss's root fillet), this body is a stage of the feature, its faces and edges named as a
    /// whole first.
    pub fn then(&self, result: Body) -> Traced {
        let step = Traced::step(result.clone(), &[self]);
        if step.lineage.is_some() {
            return step;
        }
        let lineage = self.lineage.as_ref().and_then(|l| {
            let history = result.history()?.clone();
            if history.operands != 1 {
                return None;
            }
            let body = crate::ModelBody::new(String::new(), self.body.clone(), 0);
            let stage = Arc::new(Operand::Stage(Box::new(Stage { body, lineage: l.clone(), names: OnceLock::new() })));
            Some(Lineage { history, operands: vec![stage] })
        });
        Traced { body: result, lineage }
    }
}

/// How the bodies a feature builds from a profile name their faces: what each profile segment's
/// face is called, and each cap.
pub(crate) struct ProfileRoles<'a> {
    /// The sketch the profile is in (its curves name the side faces), if any.
    pub sketch: Option<&'a SolvedSketch>,
    /// The region the body was built from, in the sketch's coordinates; its segments (outer loop,
    /// then holes) number [`FaceOrigin::Profile`].
    pub region: Option<&'a Region2>,
    /// The start and end caps' roles (`None`: not known).
    pub caps: [Option<String>; 2],
    /// A thin wall: its sides lie off the sketch curves, and are named by the curve they run
    /// beside.
    pub thin: Option<&'a ThinWallRoles>,
}

/// What names the sides of a thin extrude's wall: the curves it follows and how far its sides lie
/// from them (the thickness either side).
pub(crate) struct ThinWallRoles {
    pub curves: Vec<String>,
    pub distances: [f64; 2],
}

impl<'a> ProfileRoles<'a> {
    /// The roles of the faces of a body built from the profile, whose history (over no
    /// operands: it was built, not changed) is `history`. A face swept from profile segments is
    /// `side:<curve>` of the sketch curve each lies on exactly; a cap is its cap's role. Unknown
    /// (empty) where a segment lies on no single sketch curve, or the history is not a builder's.
    pub fn of(&self, history: &History) -> Option<Vec<Vec<String>>> {
        if history.operands != 0 {
            return None;
        }
        let segments: Vec<&solidcraft_geom::Seg2> =
            self.region.map(|r| std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs).collect()).unwrap_or_default();
        let curves = self.sketch.map(crate::config::ExactSketchCurves::new);
        // Every curve the segment is drawn on (one, or several coincident or collinear ones).
        let curve = |segment: usize| -> Vec<String> {
            let (Some(curves), Some(segment)) = (curves.as_ref(), segments.get(segment)) else { return Vec::new() };
            let drawn: Vec<String> = curves.find_all_along(segment).into_iter().map(|id| format!("side:{id}")).collect();
            match self.thin {
                // A thin wall's side beside its curve (the other side may lie on it).
                Some(thin) if drawn.is_empty() => curves.find_offset(segment, &thin.curves, thin.distances),
                _ => drawn,
            }
        };
        let roles = history
            .faces
            .iter()
            .map(|face| {
                let mut roles: Vec<String> = Vec::new();
                for origin in &face.origins {
                    let found = match *origin {
                        FaceOrigin::Profile { segment } => curve(segment),
                        FaceOrigin::Cap { end } => self.caps.get(usize::from(end)).cloned().flatten().into_iter().collect(),
                        _ => Vec::new(),
                    };
                    // One unknown origin leaves the face unknown: never a partial guess.
                    if found.is_empty() {
                        return Vec::new();
                    }
                    for role in found {
                        if !roles.contains(&role) {
                            roles.push(role);
                        }
                    }
                }
                roles
            })
            .collect();
        Some(roles)
    }

    /// A body swept from a profile (revolve, sweep, loft): its sides named by the sketch curves,
    /// its caps `start` and `end`.
    pub fn swept(sketch: Option<&'a SolvedSketch>, region: Option<&'a Region2>) -> ProfileRoles<'a> {
        ProfileRoles { sketch, region, caps: [Some("start".into()), Some("end".into())], thin: None }
    }

    /// A tool built from the profile (and then moved, from `built`, when given), its faces named
    /// by their roles where its history gives them.
    pub fn tool(&self, body: Body, built: Option<&Body>) -> Traced {
        let history = match built {
            Some(built) => built_history(&body, built),
            None => body.history().cloned(),
        };
        Traced::made(body, history.and_then(|h| self.of(&h)))
    }
}

/// How a surface built from chains of sketch segments (Surface Extrude, Revolve, Sweep) names
/// its walls: each `side:<curve>` of the sketch curve its segment lies on exactly, segments
/// numbered through the chains in order. It has no caps.
pub(crate) struct ChainRoles<'a> {
    pub sketch: &'a SolvedSketch,
    pub chains: &'a [Vec<solidcraft_geom::Seg2>],
}

impl ChainRoles<'_> {
    /// The surface `body`, its walls named by their roles where its history gives them.
    pub fn tool(&self, body: Body) -> Traced {
        let roles = body.history().filter(|h| h.operands == 0).map(|h| self.of(h));
        Traced::made(body, roles)
    }

    fn of(&self, history: &History) -> Vec<Vec<String>> {
        let segments: Vec<&solidcraft_geom::Seg2> = self.chains.iter().flatten().collect();
        let curves = crate::config::ExactSketchCurves::new(self.sketch);
        history
            .faces
            .iter()
            .map(|face| {
                let mut roles: Vec<String> = Vec::new();
                for origin in &face.origins {
                    let role = match *origin {
                        FaceOrigin::Profile { segment } => segments.get(segment).and_then(|s| curves.find_along(s)).map(|id| format!("side:{id}")),
                        _ => None,
                    };
                    match role {
                        Some(role) if !roles.contains(&role) => roles.push(role),
                        Some(_) => {}
                        // One unknown origin leaves the face unknown: never a partial guess.
                        None => return Vec::new(),
                    }
                }
                roles
            })
            .collect()
    }
}

/// How a loft names its faces: each side by the sketch curves of the sections it runs between
/// (`side:<curve>` for the first section's, `side:s<k>.<curve>` for section `k`'s), named after
/// the section with the most segments (the one each side spans exactly one segment of) and
/// answering to the others; its caps `start` and `end`.
pub(crate) struct SectionRoles<'a> {
    /// Each section's sketch and loops (outer, then holes), in the order given.
    pub sections: Vec<(&'a SolvedSketch, Vec<&'a solidcraft_geom::Loop2>)>,
}

impl SectionRoles<'_> {
    /// The loft `body`, its faces named by their roles where its history gives them.
    pub fn tool(&self, body: Body) -> Traced {
        let roles = body.history().filter(|h| h.operands == 0).map(|h| self.of(h));
        Traced::made(body, roles)
    }

    fn of(&self, history: &History) -> Vec<Vec<String>> {
        // Each profile segment (numbered through the sections in order): its section and role.
        let mut segments: Vec<(usize, Option<String>)> = Vec::new();
        for (k, (sketch, loops)) in self.sections.iter().enumerate() {
            let curves = crate::config::ExactSketchCurves::new(sketch);
            for seg in loops.iter().flat_map(|lp| &lp.segs) {
                let role = curves.find_along(seg).map(|id| if k == 0 { format!("side:{id}") } else { format!("side:s{k}.{id}") });
                segments.push((k, role));
            }
        }
        let lead = self
            .sections
            .iter()
            .enumerate()
            .max_by_key(|(k, (_, loops))| (loops.iter().map(|lp| lp.segs.len()).sum::<usize>(), std::cmp::Reverse(*k)))
            .map_or(0, |(k, _)| k);
        history
            .faces
            .iter()
            .map(|face| {
                let mut roles: Vec<(bool, usize, String)> = Vec::new();
                for origin in &face.origins {
                    let (k, role) = match *origin {
                        FaceOrigin::Cap { end } => (0, Some(if end { "end" } else { "start" }.to_string())),
                        FaceOrigin::Profile { segment } => match segments.get(segment) {
                            Some((k, role)) => (*k, role.clone()),
                            None => (0, None),
                        },
                        _ => (0, None),
                    };
                    // One unknown origin leaves the face unknown: never a partial guess.
                    let Some(role) = role else { return Vec::new() };
                    roles.push((k != lead, k, role));
                }
                roles.sort();
                let mut out: Vec<String> = Vec::new();
                for (.., role) in roles {
                    if !out.contains(&role) {
                        out.push(role);
                    }
                }
                out
            })
            .collect()
    }
}

/// A box primitive, its faces named by the side they face (`+x`, `-z`, …), as its construction
/// records them.
pub(crate) fn box_tool(body: Body) -> Traced {
    let roles: Option<Vec<Vec<String>>> = body.face_sources().map(|sources| {
        sources
            .iter()
            .map(|source| match source {
                Some(solidcraft_kernel::FaceSource::Box { axis, high, .. }) => {
                    ["x", "y", "z"].get(usize::from(*axis)).map(|a| vec![format!("{}{a}", if *high { "+" } else { "-" })]).unwrap_or_default()
                }
                _ => Vec::new(),
            })
            .collect()
    });
    Traced::made(body, roles)
}

/// A cylinder primitive: its wall `side`, its caps `bottom` (at the base) and `top`.
pub(crate) fn cylinder_tool(body: Body) -> Traced {
    let roles = body.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Cap { end }] => vec![if *end { "top" } else { "bottom" }.to_string()],
                origins if !origins.is_empty() && origins.iter().all(|o| matches!(o, FaceOrigin::Profile { .. })) => vec!["side".to_string()],
                _ => Vec::new(),
            })
            .collect()
    });
    Traced::made(body, roles)
}

/// A tool a feature builds without a sketch (a boss's post, a rib, a lip's band), its faces named
/// by the part it makes: `<part>:top` (its end cap), `<part>:base` (its start cap) and
/// `<part>:side:<k>` (swept from segment `k` of its profile), as its construction records them.
pub(crate) fn part_tool(body: Body, part: &str) -> Traced {
    let roles = body.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Cap { end }] => vec![format!("{part}:{}", if *end { "top" } else { "base" })],
                [FaceOrigin::Profile { segment }] => vec![format!("{part}:side:{segment}")],
                _ => Vec::new(),
            })
            .collect()
    });
    Traced::made(body, roles)
}

/// Faces named by their place in a body built as a whole (an imported part's faces in the
/// file's order): `face:<k>`.
pub(crate) fn numbered_tool(body: Body) -> Traced {
    linked_tool(body, 0, &[])
}

/// [`numbered_tool`] for a body of feature `fid` imported from a linked design, whose faces that
/// design named `carried` (by face, empty where not known): such a face is named after its name
/// there ([`link_role`]), so a reference to it follows that name, not the face's place in the
/// file, when the design changes; it still answers to `face:<k>`.
pub(crate) fn linked_tool(body: Body, fid: u64, carried: &[String]) -> Traced {
    let roles = (0..body.face_count())
        .map(|k| {
            let numbered = format!("face:{k}");
            match carried.get(k).filter(|n| !n.is_empty()) {
                Some(name) => vec![link_role(fid, name, 0), numbered],
                None => vec![numbered],
            }
        })
        .collect();
    Traced::made(body, Some(roles))
}

/// Nesting of piece qualifiers a carried name is read through (deeper ones are digested whole).
const MAX_LINK_DEPTH: usize = 32;

/// The role of a face a linked design named `name`: `link:<digest>`, a digest of the name (which
/// may hold anything: `@`, `|` and feature numbers of that design mean something else here). A
/// piece keeps its qualifier, with the faces around it named the same way (`F<fid>:link:…`), so
/// its pieces are told apart and found as they are in that design.
pub(crate) fn link_role(fid: u64, name: &str, depth: usize) -> String {
    let Some((stem, around)) = crate::piece_names::piece_parts(name).filter(|_| depth < MAX_LINK_DEPTH) else {
        // FNV-1a: the same digest on every build and platform.
        let digest = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        return format!("link:{digest:016x}");
    };
    let mut role = link_role(fid, stem, depth + 1);
    if name.get(stem.len()..).is_some_and(|rest| rest.starts_with("#[")) {
        let around: Vec<String> = around.iter().map(|a| format!("F{fid}:{}", link_role(fid, a, depth + 1))).collect();
        role.push_str(&format!("#[{}]", around.join(",")));
    }
    if let Some((_, k)) = name.rsplit_once('#').filter(|(_, k)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())) {
        role.push_str(&format!("#{k}"));
    }
    role
}

/// The role of an extrude's cap at `height` along the sketch normal: on the sketch plane, above
/// it or below it.
pub(crate) fn extrude_cap_role(height: f64) -> String {
    let tol = 1e-6 * (1.0 + height.abs());
    if height.abs() <= tol {
        "start".into()
    } else if height > 0.0 {
        "top".into()
    } else {
        "bottom".into()
    }
}

/// The history over no operands of a body built and then moved (or otherwise rebuilt in place)
/// by `moved`'s operation from `built`.
pub(crate) fn built_history(moved: &Body, built: &Body) -> Option<History> {
    let mut history = moved.history()?.clone();
    if history.operands == 1 {
        history.substitute(0, built.history()?).ok()?;
        Some(history)
    } else {
        (history.operands == 0).then_some(history)
    }
}

/// How a body of the model being evaluated came to be during the feature being evaluated.
#[derive(Clone, Debug, Default)]
pub(crate) enum Made {
    /// Made or changed by the feature, how not known: named from geometry.
    #[default]
    Unknown,
    /// As it was before the feature.
    Before,
    /// Made or changed by the feature, with this lineage.
    Traced(Arc<Lineage>),
}

impl crate::ModelBody {
    /// A body made or changed by the feature, with what it came from.
    pub(crate) fn traced_new(name: String, made: Traced, feature: u64) -> Self {
        crate::ModelBody::new(name, made.body, feature).with_lineage(made.lineage)
    }

    /// This body with what it came from during the feature (`None`: not known).
    pub(crate) fn with_lineage(mut self, lineage: Option<Lineage>) -> Self {
        self.made = match lineage {
            Some(l) => Made::Traced(Arc::new(l)),
            None => Made::Unknown,
        };
        self
    }

    /// This body as an operand of the feature being evaluated: a body of the model before it, or
    /// what the feature made of one so far.
    pub(crate) fn traced(&self) -> Traced {
        let lineage = match &self.made {
            Made::Before => Some(Lineage::of(&self.body, Operand::Body(self.name.clone()))),
            Made::Traced(l) => Some(l.as_ref().clone()),
            Made::Unknown => None,
        };
        Traced { body: self.body.clone(), lineage }
    }

    /// This body, as it was before the feature, as an operand named by `operand` (a moved copy of
    /// it, the cavity side of a shell grown from it); `None` once the feature has changed it.
    pub(crate) fn before_as(&self, operand: Operand) -> Traced {
        let lineage = matches!(self.made, Made::Before).then(|| Lineage::of(&self.body, operand));
        Traced { body: self.body.clone(), lineage }
    }

    /// The lineage a feature left this body with, when it made or changed it with one.
    pub(crate) fn lineage(&self) -> Option<Arc<Lineage>> {
        match &self.made {
            Made::Traced(l) => Some(l.clone()),
            Made::Before | Made::Unknown => None,
        }
    }
}

/// The pieces feature `f` (saved before format 14) split a body into, named so the later
/// features of the design find them. Those builds named the pieces in truck's shell order, which
/// SMLib's need not follow, and a later feature names the piece it works on (`body`) and keeps
/// the points it picked on it: each name goes to the piece nearest the points the features
/// naming it picked. Pieces no later feature names keep the order they came in; so do all of
/// them where the picks do not tell the pieces apart.
pub(crate) fn order_legacy_lumps(bodies: &mut [crate::ModelBody], before: &crate::ModelState, doc: &crate::Document, f: &crate::Feature) {
    for members in made_families(bodies, before) {
        order_legacy_family(bodies, &members, doc, f);
    }
}

/// The families of [`lump_families`] the feature that left `bodies` after `before` made: a family
/// it left as it was keeps its names.
fn made_families(bodies: &[crate::ModelBody], before: &crate::ModelState) -> Vec<Vec<usize>> {
    lump_families(bodies)
        .into_iter()
        .filter(|members| members.iter().filter_map(|i| bodies.get(*i)).any(|b| !before.bodies.iter().any(|o| o.body.same(&b.body))))
        .collect()
}

/// What [`order_legacy_lumps`] named the pieces of `bodies` (the state feature `f` left after
/// `before`) by: a digest of each piece name with the points the later features of `doc` naming
/// it picked, 0 where `f` was saved since format 14 or split nothing. Those features come after
/// `f`, so the state is `f`'s only while this is the same: a script replays them after it, and
/// an edit may change what they pick or name.
pub(crate) fn legacy_picks_digest(bodies: &[crate::ModelBody], before: &crate::ModelState, doc: &crate::Document, f: &crate::Feature) -> u64 {
    use std::hash::{Hash, Hasher};
    if !f.shell_piece_order {
        return 0;
    }
    let families = made_families(bodies, before);
    if families.is_empty() {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for members in families {
        let mut names: Vec<String> = members.iter().filter_map(|i| bodies.get(*i).map(|b| b.name.clone())).collect();
        names.sort();
        for (name, points) in names.iter().zip(later_picks(doc, f, &names)) {
            name.hash(&mut h);
            for p in points {
                [p.x, p.y, p.z].map(f64::to_bits).hash(&mut h);
            }
        }
    }
    // 0 means "nothing named so"; a digest that happens to be 0 must not read as that.
    h.finish().max(1)
}

/// The pieces each body a feature cut in two or more came out as (the bodies whose lineage is
/// that one operand), as [`order_lumps`] finds them.
fn lump_families(bodies: &[crate::ModelBody]) -> Vec<Vec<usize>> {
    let mut families: Vec<(*const Operand, Vec<usize>)> = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        let Made::Traced(l) = &b.made else { continue };
        let [op] = l.operands.as_slice() else { continue };
        if !matches!(op.as_ref(), Operand::Stage(_)) {
            continue;
        }
        let key = Arc::as_ptr(op);
        match families.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(i),
            None => families.push((key, vec![i])),
        }
    }
    families.into_iter().map(|(_, m)| m).filter(|m| m.len() > 1).collect()
}

/// Per name of `names`: the points the features after `f` in `doc` that name that body picked
/// on it.
fn later_picks(doc: &crate::Document, f: &crate::Feature, names: &[String]) -> Vec<Vec<Vec3>> {
    let later = doc.features.iter().skip_while(|g| g.id != f.id).skip(1);
    let mut picks: Vec<Vec<Vec3>> = vec![Vec::new(); names.len()];
    for g in later {
        let Ok(kind) = serde_json::to_value(&g.kind) else { continue };
        let Some(k) = kind.get("body").and_then(|b| b.as_str()).and_then(|b| names.iter().position(|n| n == b)) else { continue };
        for key in ["edges", "faces", "face", "position"] {
            let point = |v: &serde_json::Value| -> Option<Vec3> {
                let a = v.as_array().filter(|a| a.len() == 3)?;
                Some(Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?))
            };
            let found: Vec<Vec3> = match kind.get(key) {
                Some(v) => {
                    point(v).map(|p| vec![p]).unwrap_or_else(|| v.as_array().map(|a| a.iter().filter_map(point).collect()).unwrap_or_default())
                }
                None => Vec::new(),
            };
            if let Some(slot) = picks.get_mut(k) {
                slot.extend(found);
            }
        }
    }
    picks
}

fn order_legacy_family(bodies: &mut [crate::ModelBody], members: &[usize], doc: &crate::Document, f: &crate::Feature) {
    if members.len() > 64 {
        return;
    }
    let names: Vec<String> = members.iter().filter_map(|i| bodies.get(*i).map(|b| b.name.clone())).collect();
    let picks = later_picks(doc, f, &names);
    // How far piece `i` is from the points of name `k` (the mesh's nearest triangle).
    let far = |piece: usize, pts: &[Vec3]| -> f64 {
        let Some(b) = members.get(piece).and_then(|i| bodies.get(*i)) else { return f64::INFINITY };
        let m = b.mesh();
        pts.iter()
            .map(|p| {
                m.triangles
                    .iter()
                    .filter_map(|t| m.tri(t))
                    .map(|[a, b, c]| solidcraft_geom::compare::closest_on_triangle(*p, a, b, c).dist(*p))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    };
    let mut order: Vec<Option<usize>> = vec![None; names.len()];
    let mut taken = vec![false; members.len()];
    for (k, pts) in picks.iter().enumerate().filter(|(_, p)| !p.is_empty()) {
        let best =
            (0..members.len()).filter(|i| !taken.get(*i).copied().unwrap_or(true)).map(|i| (far(i, pts), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((d, i)) = best else { return };
        // Only a piece the points lie on: a pick off every piece tells nothing.
        let size = members.get(i).and_then(|m| bodies.get(*m)).map_or(1.0, |b| b.body.size().max(1.0));
        if d > size * 1e-3 {
            return;
        }
        if let (Some(o), Some(t)) = (order.get_mut(k), taken.get_mut(i)) {
            *o = Some(i);
            *t = true;
        }
    }
    let mut free = (0..members.len()).filter(|i| !taken.get(*i).copied().unwrap_or(true));
    let order: Vec<usize> = order.into_iter().map(|o| o.or_else(|| free.next())).collect::<Option<_>>().unwrap_or_default();
    if order.len() != names.len() {
        return;
    }
    for (name, piece) in names.into_iter().zip(order) {
        if let Some(b) = members.get(piece).and_then(|i| bodies.get_mut(*i)) {
            b.name = name;
        }
    }
}

/// Name the pieces a body fell into after their faces: of the bodies in `bodies` that are lumps
/// of one body (made in the same step, from one stage), the one whose least face name sorts first
/// takes the first of their body names, and so on. The kernel's shell order, which named them
/// before, can swap when an upstream edit moves the cut; their faces' names follow history.
/// `origin` is the feature that made them, for naming their faces.
pub(crate) fn order_lumps(bodies: &mut [crate::ModelBody], origin: &Arc<crate::naming::NamingOrigin>) {
    let mut families: Vec<(*const Operand, Vec<usize>)> = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        let Made::Traced(l) = &b.made else { continue };
        let [op] = l.operands.as_slice() else { continue };
        if !matches!(op.as_ref(), Operand::Stage(_)) {
            continue;
        }
        let key = Arc::as_ptr(op);
        match families.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(i),
            None => families.push((key, vec![i])),
        }
    }
    for (_, members) in families.into_iter().filter(|(_, m)| m.len() > 1) {
        let feature = members.first().and_then(|i| bodies.get(*i)).map(|b| b.feature);
        // A cut's pieces past the first belong to the cut itself (#10123), so its family spans
        // two features: the piece that kept the body's name comes from the body's own feature
        // and the rest from this feature. Any other mix is not a split's family: leave it be.
        let same = members.iter().all(|i| bodies.get(*i).map(|b| b.feature) == feature);
        let foreign = members.iter().filter(|i| bodies.get(**i).map(|b| b.feature) != Some(origin.feature.id)).count();
        if !same && foreign > 1 {
            continue;
        }
        let least = |b: &crate::ModelBody| {
            let mut named = b.clone();
            named.names = crate::naming::NameCell::traced(Some(origin.clone()), b.lineage());
            crate::naming::face_names(&named).iter().min().cloned()
        };
        let keys: Option<Vec<(String, usize)>> = members.iter().map(|i| bodies.get(*i).and_then(least).map(|k| (k, *i))).collect();
        let Some(mut keys) = keys else { continue };
        keys.sort();
        // A name moves with the feature it belongs to: in a cut's family the first name is the
        // cut body's own feature's and the rest are the cut's, and `body.rename` stores a name in
        // the `body_names` of the feature a body says made it (#10438).
        let names: Vec<(String, u64)> = members.iter().filter_map(|i| bodies.get(*i).map(|b| (b.name.clone(), b.feature))).collect();
        for ((name, feature), (_, i)) in names.into_iter().zip(keys) {
            if let Some(b) = bodies.get_mut(i) {
                b.name = name;
                b.feature = feature;
            }
        }
    }
}
