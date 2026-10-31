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

use solidcraft_geom::Region2;
use solidcraft_kernel::Body;
use solidcraft_kernel::history::{Change, FaceOrigin, FaceProv, History};

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

/// The roles of a cone primitive's faces, from the body revolved from its half section (base
/// side first, then the sloped side, then the top side of a cut-off cone; the side on the axis
/// makes no face): `bottom`, `side` and `top`, as a cylinder's are (#7476). `None` when the body
/// has no history of its own.
pub(crate) fn cone_roles(body: &Body) -> Option<Vec<Vec<String>>> {
    body.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Profile { segment: 0 }] => vec!["bottom".to_string()],
                [FaceOrigin::Profile { segment: 1 }] => vec!["side".to_string()],
                [FaceOrigin::Profile { segment: 2 }] => vec!["top".to_string()],
                _ => Vec::new(),
            })
            .collect()
    })
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
        if members.iter().any(|i| bodies.get(*i).map(|b| b.feature) != feature) {
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
        let names: Vec<String> = members.iter().filter_map(|i| bodies.get(*i).map(|b| b.name.clone())).collect();
        for (name, (_, i)) in names.into_iter().zip(keys) {
            if let Some(b) = bodies.get_mut(i) {
                b.name = name;
            }
        }
    }
}
