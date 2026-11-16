//! Healing tolerance-scale defects of a body (an imported one, typically) that the kernel's full
//! check fails: edges shorter than their tolerance squeezed into a vertex, the edges that met
//! the removed vertex re-ended there exactly, and the sliver faces that leaves between two
//! coincident edges collapsed (`solidcore::heal::tolerance_defects`, where every edit is
//! decided on a certified bound and no tolerance grows).
//!
//! The recipe: the full check of the body, the heal on a copy, the full check of the copy. The
//! copy is kept only when no edge or vertex tolerance grew, its check fails fewer tests than
//! the body's, and it fails no kind of test (its owner's class and number) more often than the
//! body's did; else the body is given back as it was, with why. The re-check only ever
//! refuses: each edit is accepted on its own certificate, and the check guards against what the
//! edits did not foresee. It counts failures by kind, not by entity, so a new failure that
//! takes the place of one the heal removed, of the same kind, is not told apart.
//!
//! SMLib's own healer (`SmHealData`, `HealBrep`) is not used: on bodies whose edges are shorter
//! than their stated tolerance its tolerance step resets every tolerance from a model-size
//! bucket (the check then passes because the tolerances shrank, and fails on any real gap the
//! old tolerances covered), and its degenerate-edge step finds short edges against the
//! context's tolerance, not the edge's own.

use std::collections::BTreeMap;

use solidcore::heal::tolerance_defects::{heal_short_edges_and_slivers, Healed, Refusal};
use solidcore::topology::brep::p3::{brep_copy_mapped, edit_brep};
use solidcore::EntityRef;
use solidcore_record::brep::Brep as Record;

use crate::body::{finish, Body, Expect};
use crate::check::{CheckLevel, CheckReport};
use crate::order::Order;
use crate::session::run_on;
use crate::split::copies_of;
use crate::{Error, Result};

/// A healed body ([`heal_tolerance_defects`]).
#[derive(Debug)]
pub struct HealedBody {
    /// The healed body.
    pub body: Body,
    /// Its record, in canonical order.
    pub record: Record,
    /// For each face of [`HealedBody::record`], the index of the record face of the body as it
    /// came (in the order given) that it is: the heal only removes faces, never makes one.
    pub face_source: Vec<usize>,
}

/// What [`heal_tolerance_defects`] found and did.
#[derive(Clone, Debug, PartialEq)]
pub struct HealReport {
    /// The full check of the body as it came.
    pub before: CheckReport,
    /// The full check of the healed copy; `None` when no heal was tried (the body passed, or
    /// the heal changed nothing).
    pub after: Option<CheckReport>,
    /// Whether the healed copy is the result.
    pub kept: bool,
    /// Edges squeezed into a vertex.
    pub squeezed_edges: usize,
    /// Edges re-ended at a vertex a squeeze kept.
    pub re_ended_edges: usize,
    /// Sliver faces between two coincident edges collapsed.
    pub collapsed_faces: usize,
    /// Pieces of no length cut out of edge curves.
    pub cut_pieces: usize,
    /// The largest certified bound behind a squeeze or a collapse: how far the geometry each
    /// removed lay from what stands for it now, mm.
    pub largest_bound: f64,
    /// Short edges and sliver faces left as they were, each with why (no proof found).
    pub refused: Vec<String>,
    /// Why the healed copy was not kept, when it was not.
    pub why_not: Option<String>,
}

/// How many failures of each class (reporting type and number, `SmEdge::Test #7`) a check
/// reports.
fn classes(report: &CheckReport) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for l in report.text.lines() {
        let Some((owner, rest)) = l.split_once("::Test #") else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            *out.entry(format!("{owner}::Test #{digits}")).or_insert(0) += 1;
        }
    }
    out
}

fn refusal_text(r: &Refusal) -> String {
    match r {
        Refusal::Closed => "the edge is closed".into(),
        Refusal::NotEnclosed => "no enclosure of the edge's curve".into(),
        Refusal::TooLong(d) => format!(
            "the edge reaches {d:e} mm from the midpoint of its vertices, past its tolerance"
        ),
        Refusal::Parallel(_) => "another edge joins the same two vertices".into(),
        Refusal::NeighbourClosed(_) => "an edge at one of its vertices is closed there".into(),
        Refusal::NeighbourKind(_) => {
            "an edge at one of its vertices has no curve that can be re-ended".into()
        }
        Refusal::NeighbourOffFace(_, _, Some(g)) => {
            format!("an edge re-ended at the new vertex lies up to {g:e} mm off its face, past half its tolerance")
        }
        Refusal::NeighbourOffFace(_, _, None) => {
            "an edge re-ended at the new vertex has no certified bound to its face".into()
        }
        Refusal::NotZeroArea => {
            "the collapse's area test did not find the face of zero area".into()
        }
        Refusal::VertexOffFace(_, g) => format!(
            "the new vertex would lie {g:e} mm off a face around it, past half its tolerance"
        ),
        Refusal::NeighbourEnd(_, d) => {
            format!("an edge re-ended at the new vertex ends {d:e} mm from it")
        }
        Refusal::NotCoincident(d) => {
            format!("the sliver's two edges are up to {d:e} mm apart, past the tolerance")
        }
        Refusal::KeptOffFace(_, Some(g)) => {
            format!("the sliver's kept edge lies up to {g:e} mm off the face across it")
        }
        Refusal::KeptOffFace(_, None) => {
            "the sliver's kept edge has no certified bound to the face across it".into()
        }
        Refusal::WouldGrow => "the sliver's kept edge has the smaller tolerance".into(),
        Refusal::PieceTooLong(d) => {
            format!("cutting a piece of no length out of the edge's curve moves it {d:e} mm")
        }
        Refusal::CutOffFace(_, Some(g)) => format!(
            "the edge's curve, a piece of no length cut out, lies up to {g:e} mm off its face"
        ),
        Refusal::CutEnd(d) => format!(
            "the edge's curve, a piece of no length cut out, ends {d:e} mm from its vertex, past its tolerance"
        ),
        Refusal::CutOffFace(_, None) => {
            "the edge's curve, a piece of no length cut out, has no certified bound to its face"
                .into()
        }
    }
}

fn summarise(healed: &Healed, report: &mut HealReport) {
    report.squeezed_edges = healed.squeezed.len();
    report.re_ended_edges = healed.squeezed.iter().map(|s| s.re_ended.len()).sum();
    report.collapsed_faces = healed.collapsed.len();
    report.cut_pieces = healed.cut_pieces.len();
    report.largest_bound = healed
        .squeezed
        .iter()
        .map(|s| s.bound)
        .chain(healed.collapsed.iter().map(|(_, d)| *d))
        .chain(healed.cut_pieces.iter().map(|(_, d)| *d))
        .fold(0.0, f64::max);
    report.refused = healed
        .refused_edges
        .iter()
        .map(|(_, r)| format!("short edge left: {}", refusal_text(r)))
        .chain(
            healed
                .refused_faces
                .iter()
                .map(|(_, r)| format!("sliver face left: {}", refusal_text(r))),
        )
        .collect();
}

/// `body` (its record faces in `order`, as [`Body::from_record`] gives it) healed of its
/// tolerance-scale defects (module docs), or `None` (keep `body`) when it
/// passes the full check, when nothing could be healed, or when the healed copy's check is not
/// better (fewer failed tests, no kind failed more often); the report says which and why.
/// `body` is unchanged.
///
/// # Errors
/// [`Error::Argument`] for an `order` not of `body`; [`Error::Cancelled`] when the caller's
/// token stops it; those of the check ([`Body::check`]), of the copy and of the record
/// conversion. A heal edit that fails otherwise is not an error: the body is kept, with why.
pub fn heal_tolerance_defects(
    body: &Body,
    order: &Order,
) -> Result<(Option<HealedBody>, HealReport)> {
    let before = body.check(CheckLevel::All)?;
    let mut report = HealReport {
        before: before.clone(),
        after: None,
        kept: false,
        squeezed_edges: 0,
        re_ended_edges: 0,
        collapsed_faces: 0,
        cut_pieces: 0,
        largest_bound: 0.0,
        refused: Vec::new(),
        why_not: None,
    };
    if before.is_valid() {
        report.why_not = Some("the body passes the check".into());
        return Ok((None, report));
    }
    let made = run_on("heal_tolerance_defects", &[("brep", body)], |k| {
        let (mut a, copies, tolerances) = {
            let g = body.lock();
            if order.faces.iter().any(|&f| g.face(f).is_err()) {
                return Ok(Err(Error::Argument));
            }
            let (a, map) = brep_copy_mapped(k, &g)?;
            let Some(copies) = copies_of(&map, &order.faces, EntityRef::Face, |e| match e {
                EntityRef::Face(f) => Some(f),
                _ => None,
            }) else {
                return Ok(Err(Error::Exception));
            };
            // every edge's and vertex's tolerance, by its copy, to show none grew
            let mut tolerances = Vec::new();
            for &e in g.edge_list() {
                if let Ok(Some(EntityRef::Edge(c))) = map.get_at(Some(EntityRef::Edge(e))) {
                    tolerances.push((EntityRef::Edge(c), g.edge(e)?.tolerance));
                }
            }
            for &v in g.vertex_list() {
                if let Ok(Some(EntityRef::Vertex(c))) = map.get_at(Some(EntityRef::Vertex(v))) {
                    tolerances.push((EntityRef::Vertex(c), g.vertex(v)?.tolerance));
                }
            }
            (a, copies, tolerances)
        };
        let expect = Expect::of(k, &mut a)?;
        let healed = match edit_brep(k, &mut a, |k, b, scope| {
            heal_short_edges_and_slivers(k, b, scope.env)
        }) {
            Ok(h) => h,
            // a stopped heal is the caller's stop, never "not kept"
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => return Ok(Ok(Err(format!("the heal failed: {e}")))),
        };
        // an entity the heal removed is gone from its arena: nothing to compare
        let mut grew = 0usize;
        for (x, before) in tolerances {
            let after = match x {
                EntityRef::Edge(e) => a.edge(e).ok().map(|r| r.tolerance),
                EntityRef::Vertex(v) => a.vertex(v).ok().map(|r| r.tolerance),
                _ => None,
            };
            if after.is_some_and(|t| t > before || t.is_nan()) {
                grew += 1;
            }
        }
        if grew > 0 {
            return Ok(Ok(Err(format!("the heal would grow {grew} tolerances"))));
        }
        Ok(Ok(match finish(k, a, expect)? {
            Ok(a) => Ok((a, healed, copies)),
            Err(e) => Err(format!("the healed copy fails the result contract: {e:?}")),
        }))
    })??;
    let (healed_brep, healed, copies) = match made {
        Ok(made) => made,
        Err(why) => {
            report.why_not = Some(why);
            return Ok((None, report));
        }
    };
    summarise(&healed, &mut report);
    if healed.squeezed.is_empty() && healed.collapsed.is_empty() && healed.cut_pieces.is_empty() {
        report.why_not = Some("nothing the heal takes could be proved".into());
        return Ok((None, report));
    }
    let candidate = Body::new(healed_brep);
    let after = candidate.check(CheckLevel::All)?;
    let was = classes(&before);
    let new: Vec<String> = classes(&after)
        .into_iter()
        .filter(|(class, n)| *n > was.get(class).copied().unwrap_or(0))
        .map(|(class, _)| class)
        .collect();
    report.after = Some(after.clone());
    if !new.is_empty() {
        report.why_not = Some(format!(
            "the healed body fails more tests of a kind than the body: {}",
            new.join(", ")
        ));
        return Ok((None, report));
    }
    if after.failures >= before.failures {
        report.why_not = Some(format!(
            "the healed body fails {} tests, the body {}",
            after.failures, before.failures
        ));
        return Ok((None, report));
    }
    // each face of the healed record by the record face of `body` it was
    let (record, healed_order) = candidate.to_record()?;
    let face_source = healed_order
        .faces
        .iter()
        .map(|f| copies.iter().position(|c| c == f))
        .collect::<Option<Vec<usize>>>()
        .ok_or(Error::Exception)?;
    report.kept = true;
    Ok((
        Some(HealedBody {
            body: candidate,
            record,
            face_source,
        }),
        report,
    ))
}
