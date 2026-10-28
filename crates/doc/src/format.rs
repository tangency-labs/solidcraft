//! The design file format version and the migrations that bring older files up to date.
//!
//! A design file starts with `"format": "solidcraft/N"`. A file of an older format is read and
//! upgraded step by step (N → N+1 → … → current): JSON-level steps first, for changes in the
//! file's shape, then document-level steps. A file of a newer format than this build knows is
//! refused with a message saying so, never guessed at.
//!
//! History:
//! - 1: every design up to 2026-10-08 (format string since M0). Fields added over time all
//!   have defaults; designs from before feature input names and before occurrences are filled
//!   in on upgrade.
//! - 2: the same shape; files say 2 once they have been upgraded, so later readers can tell.
//! - 3: appearances per body, face and component with names and opacity
//!   (`"appearances": {"bodies": {name: {name, color, opacity}}, "faces": […], "components": {…}}`);
//!   before, `"appearances"` mapped body names to bare `[r, g, b]` colours.
//! - 5: joins, cuts and intersects change only their own component's bodies unless bodies are
//!   named as `participants`; older features in designs with components get `participants:
//!   ["*"]` (every body), which is what they did, so those designs keep their shape.
//! - 4: joint origins mate by default (B's z against A's); `flip` now means
//!   "don't mate". Older joints keep their placement: their `flip` is inverted.
//! - 6: XZ origin planes use sketch y = -Z and normal +Y. Older XZ references become
//!   explicit planes retaining their previous axes, including offset-plane expressions.
//! - 7: configuration tables can include body material and appearance columns. Version 6
//!   rows need no transformation; the version stamp lets older readers request an update
//!   instead of encountering an unknown column variant.
//! - 8: ordered configuration rules and persistent signed line-angle, arc-sweep and slot
//!   dimensions. Version 7 geometry and table values retain their meaning without changes.
//! - 9: configured component/semantic-face styles, associative free-form offsets and
//!   explicit cylindrical emboss targets. Existing version 8 geometry and rules are
//!   preserved; new fields use defaults rather than guessing a target or an association.
//! - 10: timeline body transfers, explicit 3D wire coordinate spaces and certified face
//!   bindings. Legacy ownership maps remain unchanged. Retained wire associations determine
//!   their space; saved coordinates are never rewritten during migration.
//! - 11: durable sketch face anchors, explicitly periodic fit splines and versioned revolve
//!   conventions. Absent markers retain legacy attachment and interpolation behavior.
//! - 12: persistent analytic offset boundary roles and association profile references.
//!   Older analytic chains retain their source/output correspondence and saved geometry;
//!   migration never invents retired entities or renumbers existing curves.
//! - 13: exact planar shell face anchors retain the shell producer, original certified
//!   source and distinct inner/outer role. Version 12 anchors retain their identities;
//!   migration never guesses attachments for older designs.
//! - 14: persistent names follow the kernel's exact face history. Pieces of a split face are
//!   named by the faces around them (`F2:top#[F2:side:l4,F4:side:l4]`) instead of numbered by
//!   where their centres lay (`F2:top#2`). Names stored by older formats were worked out from
//!   geometry, and a few of them name a face that history names differently; they get the
//!   legacy mark (`?F2:top#2`) and are found again as the build that saved them found them.
//!   Pieces a feature splits a body into are named after their faces; features saved before
//!   keep the kernel's shell order (`shell_piece_order`), which their stored body names mean.
//! - 15: sketch face anchors and face bindings can name a cylinder primitive's cap
//!   (`cylinder_cap`), sketches can link lines to their own axes (`sketch_axis`), and sketch
//!   anchors can name a sheet metal panel side (`sheet:<body>:<panel>:<side>`). Version 14
//!   designs need no transformation; the version stamp lets older readers request an update
//!   instead of encountering an unknown certificate or link kind.
//! - 16: sheet metal rules can choose a corner relief shape (`corner_relief_shape`: notch,
//!   round or rectangular). Version 15 designs need no transformation (an absent shape is the
//!   notch they always had); the version stamp lets older readers request an update instead of
//!   silently building notches where a design asks for another shape.
//!   Under the same version, sketch text can be set in a font other than the built-in one
//!   (`font` in the text link's source: an installed family or a font file); a text without
//!   `font` is in the built-in font, as it always was.
//!   A Move feature can also keep its source bodies and transform a copy (`copy: true`); a
//!   move without it saves as before.
//!   A component can link to another design file (`links`: component id → `{path, feature}`,
//!   the path relative to the design's folder); the linked bodies are kept in the component's
//!   Import feature, so an older build opens the design with them and only loses the link.
//!   A design can also define its own physical materials (`user_materials`: name, density in
//!   g/cm³, optional look), which bodies (`materials`) and configuration material cells name
//!   like built-in ones; a design without them saves as before.
//!   A design can also keep what was hidden when it was saved (`visibility`: hidden bodies,
//!   occurrence placements, sketches and origin items; `browser.visibility`). A design without
//!   it shows everything, as it always did; a reader that ignores it loses only the hiding.
//!   Sketch face anchors can name a flat face by its exact history name (a `lineage`
//!   certificate: the target's `name`, e.g. a cut wall `F5:side:l1` or a pattern copy
//!   `F4:top@F6.2`), where no construction certificate names it alone. Anchors with
//!   construction certificates resolve as before.
//! - 17: a `lineage` face name can be a piece's own, qualified by the faces around it
//!   (`F2:top#[F2:side:l4,F4:side:l4]`), where Offset Face moved one piece of a face off the
//!   others' plane; never a numbered piece. Sheet face names can also name a rim, the sheet's
//!   thickness along a panel's straight edge (`sheet:<body>:<panel>:rim.<edge>`, the edge a base
//!   sketch line's id, `edge<k>`, or a flange's `far`, `side0` or `side1`), or a flat end of a
//!   bend (`rim.bend0`, `rim.bend1`). Version 16 designs need no transformation; the version
//!   stamp makes 0.8.0 (which reads up to 16) ask for an update instead of meeting names it
//!   can't resolve.
//!   Convert to Sheet Metal can take bend edges (`bends` in the feature: points on the sharp
//!   edges of a solid or shelled body that become bends). A conversion without `bends` is the
//!   plate conversion it always was and saves as before; 0.8.0 would drop the bend edges and
//!   refuse the body as no plate.
//!   A sketch's link to a projected edge can also keep the edge's persistent name (`edge_name`,
//!   beside `face_name`) and is then found by it, wherever the edge went. A link without it is
//!   the edge nearest its point, as it always was, and takes the name when its sketch is next
//!   edited.
//!   A fillet's `fit` can be `taper` (the radius follows the room along each edge it doesn't
//!   fit). Fillets with `exact` or `reduce` save as before; 0.8.0 would refuse the unknown fit.
//!   Feature-history folders are also first saved in 17 (`timeline_groups`: stable folder id,
//!   name, optional parent folder and feature ids). Version 16 designs have no saved timeline
//!   folders and open with the empty default.
//!   A fillet's `type` can be `spline` (the radius along each edge a B-spline: `start`, `knots`,
//!   and the control radii after `radius` in `radii`). Other fillets save as before; 0.8.0 would
//!   refuse the unknown type.
//!   Under the same version, a sheet metal rule's corner relief shape can also be `tear` (no
//!   relief: each bend is cut straight from the bends' corner to where its flange's end
//!   starts); a rule that says `notch`, `round` or `rectangular` builds what it did.
//!   A sketch's view options can also say `hide_construction` (the Sketch Palette's
//!   Construction Geometries); a sketch that shows its construction geometry saves as before.
//!   A `SurfaceExtrude` feature (Surface Extrude: `sketch`, optional `profiles`, `curves` and
//!   `extent`, walls only) is first saved in 17; 0.8.0 would refuse the unknown feature.
//! - 18: surface features 0.9.0 doesn't know: `Unstitch` (a body's faces taken out as surface
//!   bodies) and `ReverseNormal` (a surface body's faces turned over), each a `body` and
//!   optional `faces` points. Version 17 designs need no transformation; the version stamp makes
//!   0.9.0 (which reads up to 17) ask for an update instead of meeting a feature it can't read.
//!   Under the same version, a sketch text's link source can carry a `style`: `bold` and
//!   `italic` (the font's bold or italic face), `align` (`left`, `center` or `right`),
//!   `letter_spacing` (mm), `line_spacing` (times the font's line distance), `path` (the id of a
//!   curve of the same sketch the baseline follows) and `reverse`. A text without `style` is set
//!   as it always was; 0.9.0 would drop the style and set the text plain, off its curve.
//!   Under the same version, Delete Face can leave the gap open (`heal: false` on the feature:
//!   the faces left are a surface body); a Delete Face without it heals, as it always did, and
//!   saves as before. 0.9.0 would heal a gap the design leaves open.
//!   A `SurfaceRevolve` feature (Surface Revolve: `sketch`, optional `profiles`, `curves`,
//!   `axis`, `angle`, optional `angle2` and `symmetric`) is also first saved in 18.
//!   A `SurfaceSweep` feature (Surface Sweep: `sketch`, optional `profiles` and `curves`,
//!   `path_sketch`, `path`) is also first saved in 18.
//!   A `SurfaceLoft` feature (Surface Loft: `sections`, each a `sketch` with optional `profiles`
//!   and `curves`) is also first saved in 18.
//!   A `SurfaceOffset` feature (Offset Surface: `faces` points, optional `body`, `distance`,
//!   `chain` and `tolerance`) is also first saved in 18.
//!   Nothing new is saved for holes, but a hole on a face is now drilled into the material: a
//!   saved `direction` that leaves the face the hole starts on (designs up to 17 could save one,
//!   taken from another face whose plane passed near the point, such as the far side of a thin
//!   plate or a face of another body at the same height) is mirrored into that face when the
//!   design is evaluated. Holes whose direction enters their face build as before.
//!   Holes at sketch points can be flipped (`flip: true` in the hole's `points`): they drill away
//!   from the side they would go into by themselves. Holes without it save as before; 0.9.0 would
//!   drop the flip and drill the other way.

//! - 19: editable associative offset-distance dimensions. Older offsets retain their saved
//!   fixed distances; migration does not invent dimensions or change geometry.
//!   Under the same version, a fillet's style can be `curvature` (a G2 quintic blend; `radius`
//!   is the equivalent circular setback). Fillets of the other styles save as before; 0.9.0
//!   would refuse the unknown style.
//! - 20: an Extrude can be thin (`thin`: `curves`, `thickness`, optional `side` and
//!   `thickness2`): a wall along sketch curves instead of its profiles. Version 19 designs need
//!   no transformation, and an extrude without `thin` saves as before; the version stamp makes
//!   0.10.0 (which reads up to 19) ask for an update instead of ignoring the wall and filling
//!   the sketch's profiles.
//!   A `sheet_perforate` feature (Perforate: `diameter`, `pitch`, optional `margin`,
//!   `pattern` and `body`): a perforation field stored on a sheet, for the flat pattern, the DXF
//!   and the measure. It is also first saved in 20; 0.10.0 asks for an update instead of
//!   meeting a feature it can't read.
//!   Associative, editable sketch patterns (`patterns` in a sketch) and their point/radius
//!   relations are also first saved in 20. Old sketch copies stay independent: migration cannot
//!   infer which curves formed a pattern.
//!   The saved visibility can also list hidden assembly joints (`visibility.hidden_joints`,
//!   joint ids): their glyphs aren't drawn, but they still constrain and drive the parts. A
//!   design without it shows every joint, as it always did; 0.10.0 would show them all.
//! - 21: named render setups (`render: {setups: {name: setup}}`): renderer-independent render
//!   intent (camera, resolution, exposure, environment). Version 20 designs need no
//!   transformation (an absent field means no setups, and empty render state is not saved); the
//!   version stamp makes 0.11.0 (which reads up to 20) ask for an update instead of silently
//!   dropping saved setups.
//!   Also first saved in 21, a design can carry `metadata` (#1015): design properties
//!   (`project`, `part_number`, `revision`, `author`, `organization`, `description`, `license`,
//!   each text) and other tools' data, each tool's under its own reverse-DNS key
//!   (`com.example.tool`). Every key is kept as it is, and nothing in it is checked when the
//!   design opens. A design without it saves as before. 0.11.x refuses format 21, so no
//!   released build drops the properties when it saves.
//!   Under the same version, looks can explicitly select opaque PBR parameters (`pbr`: metal
//!   fraction, roughness, dielectric IOR). Absent fields keep the scalar shading a look always
//!   had; 0.11.0 asks for an update instead of silently dropping the response.
//!   A `SurfaceUntrim` feature (Untrim: a `face` as `body` and `point`, optional `holes`
//!   points, `outer` and `in_place`) is also first saved in 21; 0.11.0 asks for an update
//!   instead of meeting a feature it can't read.
//!   A `SurfaceRuled` feature (Ruled Surface: `body`, `edges` points, `distance`, optional
//!   `mode`, `angle`, `direction`, `flip`, `corners` and `face`) is also first saved in 21;
//!   0.11.0 asks for an update instead of meeting a feature it can't read.
//!   Under the same version, a `SurfaceExtend` can say how it continues the surface (`mode`:
//!   `tangent` or `curvature`; `natural` when absent), extend up to something instead of its
//!   `distance` (`up_to`: a `plane`, a `face` as `body` and `point`, or `bounds`) and keep the
//!   neighbouring edges as they are (`neighbours: false`). An Extend without them builds the
//!   planar extension it always did and saves as before; 0.11.0 asks for an update instead of
//!   dropping them and extending a plane by the distance.
//!   Under the same version, an extrude's extent records how a symmetric `distance` is read
//!   (`convention`: `total_distance`, split evenly around the start plane, which new solid
//!   extrudes save; or `legacy_per_side`). An extent without it keeps the per-side reading
//!   designs always had, so their thickness and parameters don't change; 0.11.0 asks for an
//!   update instead of doubling a total-distance extrude.
//!   Under the same version, a Convert to Sheet Metal bend point can lie in the middle of a
//!   chamfer (a flat strip between two faces), which then is one bend at the two faces' virtual
//!   sharp; 0.11.0 bent the chamfer's nearest edge instead or found no edge there. Bend points
//!   on edges convert as before.
//!   A bend point can also lie on a bend modelled with its radius (a cylinder between two flat
//!   faces tangent to it), which then is one bend at the two faces' virtual sharp with the
//!   rule's radius; 0.11.0 found no edge there or refused the curved face.
//!   Also under 21, a Sweep and a Surface Sweep can carry `orientation` (`parallel`, or
//!   `{direction: [x, y, z]}`; absent: perpendicular), `twist` (an angle expression) and `scale`
//!   (the end scale, an expression). A sweep without them saves as before; 0.11.x refuses
//!   format 21, so no released build sweeps a twisted or scaled profile straight.
//!   Nothing new is saved for converted sheets' corners and bend ends, but a design converts as
//!   the rule says when it is evaluated: corners between the walls take the rule's corner relief
//!   and a bend ending beside more of the fixed face its bend relief (0.11.0 left every corner a
//!   notch and cut no bend relief), and a wall on part of an edge bends only where it stands
//!   (0.11.0 refused it).
//! - 22: an Import (STEP) feature can list `skip`: indices of the file's bodies it does not
//!   build, because they are later instances of a part the file places more than once and the
//!   other occurrences of that part's component show them (#1386: an imported file becomes a
//!   component, an assembly in it nested components). Version 21 designs need no
//!   transformation: an Import without it builds every body, as it always did, and designs
//!   whose imports sit loose in a component are left as they are. The version stamp makes
//!   0.12.0 (which reads up to 21) ask for an update instead of building those instances again
//!   on top of their occurrences.
//!   Also under 22, a number in an expression can be written with a decimal comma outside
//!   parentheses (`2,5 mm` is `2.5 mm`, #1347; inside a function's parentheses the comma still
//!   separates its arguments). Nothing new is saved, but an expression typed that way is saved as
//!   typed; 0.12.0 asks for an update instead of failing to evaluate it.
//!   Also under 22, a design can hold a `SurfaceBlend` (Blend Surface: `from`/`to` edge chains of
//!   two bodies, optional `continuity`, `bulge`, `align`, `stitch`), and a `Patch` can carry a
//!   `crown` (0 by default, and then not saved: a bump that vanishes on the loop, so the face
//!   still meets the picked edges). Version 21 designs need no transformation; 0.12.0 asks for an
//!   update instead of meeting a feature it can't read or dropping a patch's crown.
//!   Also under 22, a sketch text's link source can say that its place, height or angle are
//!   expressions (`exprs`: `x`, `y`, `height`, `angle`, each an expression over the design's
//!   parameters); the source's `at`, `height` and `angle` then hold the numbers they last came
//!   to. A text without `exprs` is set as it always was; 0.12.0 asks for an update instead of
//!   dropping the expressions and keeping the text at its last numbers.
//!
//! Later formats are one file per change, so changes made side by side never edit the same line
//! (`src/format/pending/README.md`):
//! - `src/format/v<N>/`: the changes format N added, one file each, its `//!` note saying what is
//!   saved and what older designs get, and a `step` when older designs are rewritten as they open
//!   (23: motion link couplings, FreeCAD and Rhino imports; 24: an Edge Flange's own corner relief).
//! - `src/format/pending/`: changes no release has fixed a number for yet. While there are any,
//!   this build writes the next number (which no release reads), with the names of these changes
//!   in `format_pending`, so a later build applies just the steps a file is missing. At release
//!   time `scripts/fold-format-pending.sh` moves them into the next `v<N>/`.

use serde_json::Value;

use crate::{DocError, Document, Result};

/// The format this build writes: the last folded one, or the next while changes are pending.
pub const FORMAT: u32 = FORMATS.current();

/// The last format whose upgrade step is written out in [`JSON_STEPS`]; later formats are made
/// of [`Part`]s (`src/format/`, listed by `build.rs`).
const HAND_WRITTEN: u32 = 22;

/// One change to the file format: a file in `src/format/v<N>/` or `src/format/pending/`.
pub(crate) struct Part {
    /// The file's name, which a file written while the change was pending lists.
    pub name: &'static str,
    /// How a design from before the change is rewritten as it opens, if it is.
    pub step: Option<JsonStep>,
}

include!(concat!(env!("OUT_DIR"), "/format_parts.rs"));

/// Every format this build knows and how to upgrade a design through them.
const FORMATS: Formats<'static> = Formats { hand: &JSON_STEPS, folded: FOLDED, pending: PENDING };

/// The names of the pending changes this build writes into a design (`format_pending`).
pub fn pending_names() -> Vec<String> {
    FORMATS.pending.iter().map(|p| p.name.to_string()).collect()
}

/// The upgrade path: the hand-written steps up to [`HAND_WRITTEN`], the folded formats after it
/// (each a list of changes) and the pending changes, which make the next format.
struct Formats<'a> {
    hand: &'a [JsonStep],
    folded: &'a [(u32, &'a [Part])],
    pending: &'a [Part],
}

impl Formats<'_> {
    const fn current(&self) -> u32 {
        self.hand.len() as u32 + 1 + self.folded.len() as u32 + if self.pending.is_empty() { 0 } else { 1 }
    }

    /// The changes format `version` is made of, if it is made of changes.
    fn parts(&self, version: u32) -> Option<&[Part]> {
        let after = version.checked_sub(self.hand.len() as u32 + 2)? as usize;
        match self.folded.get(after) {
            Some((_, parts)) => Some(parts),
            None if after == self.folded.len() && !self.pending.is_empty() => Some(self.pending),
            None => None,
        }
    }

    /// A concept is implemented only when this reader registers its descriptor.
    fn knows_part_name(&self, name: &str) -> bool {
        self.folded.iter().flat_map(|(_, parts)| *parts).chain(self.pending).any(|part| part.name == name)
    }

    /// Read-only spelling from the personal format-29 renderer. Its card schema is
    /// unchanged; recognizing the alias must never grant support to another reader.
    fn canonical_applied_name<'name>(&self, version: u32, name: &'name str) -> &'name str {
        if version == 29 && name == "physical-material-cards" && self.knows_part_name("material-engineering-cards") {
            "material-engineering-cards"
        } else {
            name
        }
    }

    /// Bring a design of format `version` up to [`Formats::current`]. `applied` is the file's
    /// `format_pending`: the changes of its format it already has, when it was written while they
    /// were pending; the others' steps run first. Recognize changes across the entire history:
    /// a branch's pending change may have landed at a later number. Unknown changes are refused
    /// even after folding, and already-applied changes never run again at their later number.
    fn upgrade(&self, v: &mut Value, version: u32, applied: Option<&[String]>) -> Result<()> {
        if version == 0 || version > self.current() {
            return Err(DocError::Invalid(format!("not a SolidCraft design (format {version})")));
        }
        // A list beside a format that isn't made of changes means nothing (a tool or a test set
        // an older number on a newer file): it is ignored, like any field an upgrade drops.
        let applied = applied.filter(|_| self.parts(version).is_some());
        let already_applied =
            |part: &Part| applied.is_some_and(|names| names.iter().any(|name| self.canonical_applied_name(version, name) == part.name));
        if let Some(applied) = applied {
            if let Some(unknown) = applied.iter().find(|name| !self.knows_part_name(self.canonical_applied_name(version, name))) {
                return Err(DocError::Invalid(format!(
                    "this design was saved by a newer SolidCraft (file format {version} with `{unknown}`, which this version doesn't know): update SolidCraft to open it"
                )));
            }
            for part in self.parts(version).unwrap_or_default().iter().filter(|part| !already_applied(part)) {
                part.step.map_or(Ok(()), |step| step(v))?;
            }
        }
        for from in version..self.current() {
            match self.hand.get(from as usize - 1) {
                Some(step) => step(v)?,
                None => {
                    for part in self.parts(from + 1).unwrap_or_default().iter().filter(|part| !already_applied(part)) {
                        part.step.map_or(Ok(()), |step| step(v))?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Take a file's `format_pending` (the pending changes it has) out of its JSON.
fn take_pending(v: &mut Value) -> Result<Option<Vec<String>>> {
    let Some(list) = v.as_object_mut().and_then(|o| o.remove("format_pending")) else { return Ok(None) };
    let invalid = || DocError::Invalid("damaged design: `format_pending` is not a short list of names".into());
    let list = list.as_array().filter(|l| l.len() <= 1000).ok_or_else(invalid)?;
    list.iter().map(|n| n.as_str().filter(|n| n.len() <= 128).map(str::to_string).ok_or_else(invalid)).collect::<Result<_>>().map(Some)
}

pub fn format_string(n: u32) -> String {
    format!("solidcraft/{n}")
}

/// The format tag of designs saved before the rename. Compatibility shim (docs/compatibility.md):
/// read, never written.
pub const LEGACY_FORMAT_PREFIX: &str = "solvecraft/";

/// The version in a format string (`solidcraft/N`, or the legacy `solvecraft/N`).
pub fn version_of(s: &str) -> Option<u32> {
    s.strip_prefix("solidcraft/").or_else(|| s.strip_prefix(LEGACY_FORMAT_PREFIX))?.parse().ok()
}

/// JSON-level upgrades up to [`HAND_WRITTEN`]: entry `i` turns format `i + 1` into `i + 2`.
/// Never add to it: a new format is a file in `src/format/pending/`.
type JsonStep = fn(&mut Value) -> Result<()>;
const JSON_STEPS: [JsonStep; HAND_WRITTEN as usize - 1] = [
    |_| Ok(()),
    v2_to_v3,
    v3_to_v4,
    v4_to_v5,
    v5_to_v6,
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    v13_to_v14,
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
    |_| Ok(()),
];

/// Stored face and edge names were worked out from geometry before format 14; they get the
/// legacy mark (`?`, see `naming::LEGACY_MARK`) so they are found again as those builds found
/// them, and an older design evaluates as it was saved. The step marks the fields that store
/// such a name, by their place in the schema:
/// - each feature's `face_names`, and fillets' and chamfers' `edge_names` (other features' edge
///   names are sheet metal's own);
/// - a sketch feature's links' `face_name`;
/// - face and edge references (`PlaneRef::Face`, `GeoRef::Face`, `GeoRef::Edge`: tagged
///   `"type": "face"|"edge"`, picked `at` a point), wherever a feature holds one;
/// - joint snaps on a face (`face_center`, `circle_center`), in joints and named origins;
/// - face appearances, but not strict ones, whose names are certified lineage.
///
/// Every feature also keeps naming the pieces it splits a body into in the kernel's shell order
/// (`Feature::shell_piece_order`): the body names stored after it (bodies, targets, participants,
/// materials, appearances, joints) mean the pieces in that order.
fn v13_to_v14(v: &mut Value) -> Result<()> {
    if let Some(Value::Array(features)) = v.get_mut("features") {
        for f in features.iter_mut() {
            let Value::Object(o) = f else { continue };
            let kind = o.get("type").and_then(Value::as_str).map(str::to_string);
            if matches!(kind.as_deref(), Some("fillet" | "chamfer")) {
                mark_all(o.get_mut("edge_names"));
            }
            mark_all(o.get_mut("face_names"));
            if kind.as_deref() == Some("sketch")
                && let Some(Value::Array(links)) = o.get_mut("sketch").and_then(|s| s.get_mut("links"))
            {
                links.iter_mut().filter_map(|l| l.get_mut("face_name")).for_each(mark);
            }
            o.insert("shell_piece_order".into(), Value::Bool(true));
            references(f);
        }
    }
    if let Some(Value::Array(joints)) = v.get_mut("assembly").and_then(|a| a.get_mut("joints")) {
        for j in joints.iter_mut() {
            for side in ["a", "b"] {
                snap(j.get_mut(side));
            }
        }
    }
    if let Some(Value::Array(origins)) = v.get_mut("assembly").and_then(|a| a.get_mut("origins")) {
        origins.iter_mut().for_each(|o| snap(o.get_mut("origin")));
    }
    // Face appearances carry the face's name without a type tag.
    if let Some(Value::Array(faces)) = v.get_mut("appearances").and_then(|a| a.get_mut("faces")) {
        for face in faces.iter_mut().filter(|f| f.get("strict_name").and_then(Value::as_bool) != Some(true)) {
            if let Some(n) = face.get_mut("name") {
                mark(n);
            }
        }
    }
    Ok(())
}

/// Mark the `name` of every face or edge reference in `v` (see [`v13_to_v14`]).
fn references(v: &mut Value) {
    match v {
        Value::Object(o) => {
            let reference = matches!(o.get("type").and_then(Value::as_str), Some("face" | "edge")) && o.contains_key("at");
            for (key, x) in o.iter_mut() {
                if reference && key == "name" {
                    mark(x);
                } else {
                    references(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(references),
        _ => {}
    }
}

/// Mark the face name of a joint origin's snap on a face.
fn snap(origin: Option<&mut Value>) {
    if let Some(snap) = origin.and_then(|o| o.get_mut("snap"))
        && matches!(snap.get("type").and_then(Value::as_str), Some("face_center" | "circle_center"))
        && let Some(n) = snap.get_mut("face")
    {
        mark(n);
    }
}

/// Mark every name in a list of stored names.
fn mark_all(names: Option<&mut Value>) {
    if let Some(Value::Array(names)) = names {
        names.iter_mut().for_each(mark);
    }
}

/// `features` (a design's, as JSON) as an upgrade from before format 14 leaves them: each keeps
/// the shell order of the pieces it makes (see [`v13_to_v14`]).
#[cfg(test)]
pub(crate) fn pieces_in_shell_order(features: &mut Value) {
    for f in features.as_array_mut().into_iter().flatten().filter_map(Value::as_object_mut) {
        f.insert("shell_piece_order".into(), Value::Bool(true));
    }
}

/// A stored name with the legacy mark (an empty name stays empty: no reference).
fn mark(n: &mut Value) {
    if let Value::String(s) = n
        && !s.is_empty()
        && crate::naming::legacy_name(s).is_none()
    {
        s.insert(0, crate::naming::LEGACY_MARK);
    }
}

/// Old XZ references retain their exact frame, including nested offset and angled planes.
/// New origin references use +Y; freezing the old frame avoids changing sketch coordinates,
/// dimensions, profiles, extrude directions or parameter expressions in saved designs.
fn v5_to_v6(v: &mut Value) -> Result<()> {
    fn preserve(v: &mut Value) {
        match v {
            Value::Object(o) => {
                let old_xz = o.get("type").and_then(Value::as_str) == Some("origin")
                    && o.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case("XZ") || n.eq_ignore_ascii_case("FRONT"));
                if old_xz {
                    *v = serde_json::json!({"type": "custom", "plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}});
                } else {
                    o.values_mut().for_each(preserve);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(preserve),
            _ => {}
        }
    }
    preserve(v);
    Ok(())
}

/// Features in designs with components keep reaching every body.
fn v4_to_v5(v: &mut Value) -> Result<()> {
    let has_components = v.get("components").and_then(Value::as_array).is_some_and(|c| !c.is_empty());
    if !has_components {
        return Ok(());
    }
    if let Some(Value::Array(fs)) = v.get_mut("features") {
        for f in fs.iter_mut() {
            if let Some(o) = f.as_object_mut()
                && o.get("type").and_then(Value::as_str) != Some("sketch")
                && o.get("participants").is_none()
            {
                o.insert("participants".into(), serde_json::json!(["*"]));
            }
        }
    }
    Ok(())
}

/// Joints placed with the old convention (aligned unless flipped) keep their placement.
fn v3_to_v4(v: &mut Value) -> Result<()> {
    if let Some(Value::Array(js)) = v.get_mut("assembly").and_then(|a| a.get_mut("joints")) {
        for j in js.iter_mut() {
            if let Some(o) = j.as_object_mut() {
                let f = o.get("flip").and_then(Value::as_bool).unwrap_or(false);
                o.insert("flip".into(), Value::Bool(!f));
            }
        }
    }
    Ok(())
}

/// Body colours `{name: [r, g, b]}` become named, opaque body appearances.
fn v2_to_v3(v: &mut Value) -> Result<()> {
    let Some(Value::Object(old)) = v.get("appearances") else { return Ok(()) };
    if !old.values().all(Value::is_array) {
        return Ok(());
    }
    let bodies: serde_json::Map<String, Value> =
        old.iter().map(|(k, c)| (k.clone(), serde_json::json!({"name": "Custom", "color": c, "opacity": 1.0}))).collect();
    if let Some(o) = v.as_object_mut() {
        o.insert("appearances".into(), serde_json::json!({ "bodies": bodies }));
    }
    Ok(())
}

/// Read a design file's JSON, upgrading older formats.
pub fn read(s: &str) -> Result<Document> {
    let mut v: Value = serde_json::from_str(s).map_err(|e| DocError::Invalid(format!("not a SolidCraft design: {e}")))?;
    let Some(obj) = v.as_object() else {
        return Err(DocError::Invalid("not a SolidCraft design (expected a JSON object)".into()));
    };
    let version = match obj.get("format") {
        None => 1,
        Some(Value::String(f)) => version_of(f).ok_or_else(|| DocError::Invalid(format!("not a SolidCraft design (format `{f}`)")))?,
        Some(_) => return Err(DocError::Invalid("not a SolidCraft design (format is not a string)".into())),
    };
    let applied = take_pending(&mut v)?;
    if version == 0 {
        return Err(DocError::Invalid("not a SolidCraft design (format 0)".into()));
    }
    if version > FORMAT {
        return Err(DocError::Invalid(format!(
            "this design was saved by a newer SolidCraft (file format {version}; this version reads formats up to {FORMAT}): update SolidCraft to open it"
        )));
    }
    check_design_numbers(&v)?;
    crate::config::validate_rules_json(&v)?;
    crate::config::validate_styles_json(&v)?;
    crate::face_anchor::validate_face_anchors_json(&v)?;
    validate_versioned_geometry(&v)?;
    validate_emboss_targets(&v)?;
    validate_freeform_offsets(&v)?;
    validate_sketch_sizes(&v)?;
    crate::format_offsets::validate_raw(&v)?;
    let legacy_wires = validate_wires(&v, version)?;
    validate_body_transfers(&v)?;
    FORMATS.upgrade(&mut v, version, applied.as_deref())?;
    let mut d: Document = serde_json::from_value(v).map_err(|e| DocError::Invalid(format!("design file: {e}")))?;
    check_typed_numbers(&d)?;
    let mut legacy_by_feature = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for (feature, wire) in legacy_wires {
        legacy_by_feature.entry(feature).or_default().push(wire);
    }
    for (feature_index, wire_indices) in legacy_by_feature {
        if let Some(crate::Feature { kind: crate::FeatureKind::Sketch { sketch, .. }, .. }) = d.features.get_mut(feature_index) {
            let mut spaces = std::collections::BTreeMap::new();
            for link in &sketch.links {
                spaces.entry(link.id.as_str()).or_insert_with(|| solidcraft_sketch::WireSpace::from_legacy_link(link));
            }
            for wire_index in wire_indices {
                if let Some(wire) = sketch.wires.get_mut(wire_index) {
                    wire.space = wire.link.as_deref().and_then(|id| spaces.get(id)).copied().unwrap_or_default();
                }
            }
        }
    }
    if d.features.len() > crate::document::MAX_FEATURES
        || d.params.len() > crate::document::MAX_PARAMS
        || d.components.len() > crate::document::MAX_FEATURES
        || d.timeline_groups.len() > crate::document::MAX_FEATURES
        || d.timeline_groups.iter().any(|group| group.features.len() > crate::document::MAX_FEATURES)
    {
        return Err(DocError::Invalid("document too large".into()));
    }
    let feature_ids: std::collections::BTreeSet<_> = d.features.iter().map(|feature| feature.id).collect();
    let mut group_ids = std::collections::BTreeSet::new();
    let mut grouped_features = std::collections::BTreeSet::new();
    for group in &d.timeline_groups {
        if group.name.trim().is_empty() || group.name.len() > 128 || !group_ids.insert(group.id) {
            return Err(DocError::Invalid("damaged design: invalid timeline folder".into()));
        }
        if group.parent.is_some_and(|parent| parent == group.id) {
            return Err(DocError::Invalid("damaged design: timeline folder cannot contain itself".into()));
        }
        if group.features.iter().any(|feature| !feature_ids.contains(feature) || !grouped_features.insert(*feature)) {
            return Err(DocError::Invalid("damaged design: invalid feature in a timeline folder".into()));
        }
    }
    for group in &d.timeline_groups {
        if let Some(parent) = group.parent {
            if !group_ids.contains(&parent) {
                return Err(DocError::Invalid("damaged design: timeline folder has an unknown parent".into()));
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut current = Some(parent);
            while let Some(id) = current {
                if !seen.insert(id) {
                    return Err(DocError::Invalid("damaged design: cyclic timeline folders".into()));
                }
                current = d.timeline_groups.iter().find(|candidate| candidate.id == id).and_then(|candidate| candidate.parent);
            }
        }
    }
    if let Some(id) = crate::non_contiguous_timeline_group(&d.features, &d.timeline_groups) {
        return Err(DocError::Invalid(format!("damaged design: timeline folder {id} is not contiguous")));
    }
    for o in &d.occurrences {
        if !rigid(&o.transform) {
            return Err(DocError::Invalid(format!("damaged design: occurrence `{}` has an invalid placement", o.name)));
        }
    }
    if !all_unique(d.features.iter().map(|f| f.id))
        || !all_unique(d.components.iter().map(|c| c.id))
        || !all_unique(d.occurrences.iter().map(|o| o.id))
    {
        return Err(DocError::Invalid("damaged design: duplicate feature, component or occurrence id".into()));
    }
    let component_ids: std::collections::BTreeSet<_> = d.components.iter().map(|component| component.id).chain(std::iter::once(0)).collect();
    for feature in &d.features {
        if let crate::FeatureKind::Sketch { sketch, .. } = &feature.kind {
            sketch.validate_freeform_offsets().map_err(|e| DocError::Invalid(format!("damaged design: {e}")))?;
            sketch.validate_wires().map_err(|e| DocError::Invalid(format!("damaged design: {e}")))?;
            sketch.validate_patterns().map_err(|e| DocError::Invalid(format!("damaged design: {e}")))?;
            sketch.validate_periodic_splines().map_err(|e| DocError::Invalid(format!("damaged design: {e}")))?;
        }
        if let crate::FeatureKind::BodyTransfer { from, to, .. } = &feature.kind
            && [from, to].into_iter().any(|id| !component_ids.contains(id))
        {
            return Err(DocError::Invalid("damaged design: body transfer references an unknown component".into()));
        }
        if let crate::FeatureKind::BodyTransfer { to, .. } = &feature.kind
            && feature.component != *to
        {
            return Err(DocError::Invalid("damaged design: body transfer owner must match its destination".into()));
        }
    }
    // Fill in what older designs (and hand-edited files) may lack; a no-op otherwise.
    crate::format_offsets::validate_typed(&d)?;
    normalise(&mut d)?;
    // `next_id` is trusted by nothing: a hand-edited or generated file may omit it or leave it stale,
    // and the next feature, component or occurrence would then reuse an id that is already taken.
    let largest =
        d.features.iter().map(|f| f.id).chain(d.components.iter().map(|c| c.id)).chain(d.occurrences.iter().map(|o| o.id)).max().unwrap_or(0);
    let floor = largest.checked_add(1).ok_or_else(|| DocError::Invalid("damaged design: the id space is exhausted".into()))?;
    d.next_id = d.next_id.max(floor).max(1);
    // These lists used to allocate independently. Keep their existing identities, but never
    // let a later deletion make them available again after loading an older design.
    d.next_id = d.next_item_id()?;
    d.format = format_string(FORMAT);
    d.format_pending = pending_names();
    Ok(d)
}

fn all_unique(ids: impl Iterator<Item = u64>) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    ids.into_iter().all(|id| seen.insert(id))
}

/// The caps interactive editing enforces, applied to the raw JSON so an oversized sketch is
/// never deserialised. Every sketch in the timeline is solved when the design opens, and the
/// solver sizes its dense matrices by the sketch, so a file with hundreds of thousands of
/// points would ask the allocator for terabytes and abort before anything could report it.
fn validate_sketch_sizes(value: &Value) -> Result<()> {
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(()) };
    let invalid = || DocError::Invalid("damaged design: sketch too large".into());
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(sketch) = feature.get("sketch") else { continue };
        let len = |key: &str| sketch.get(key).and_then(Value::as_array).map_or(0, Vec::len);
        if len("points") > solidcraft_sketch::MAX_POINTS
            || len("curves") > solidcraft_sketch::MAX_CURVES
            || len("constraints") > solidcraft_sketch::MAX_CONSTRAINTS
            || len("patterns") > solidcraft_sketch::MAX_CONSTRAINTS
        {
            return Err(invalid());
        }
        if let Some(patterns) = sketch.get("patterns").and_then(Value::as_array) {
            let mut points = 0usize;
            let mut curves = 0usize;
            let mut constraints = 0usize;
            for pattern in patterns {
                let Some(entities) = pattern.get("entities").and_then(Value::as_array) else { return Err(invalid()) };
                let Some(instances) = pattern.get("instances").and_then(Value::as_array) else { return Err(invalid()) };
                if entities.len() > solidcraft_sketch::MAX_CURVES + solidcraft_sketch::MAX_POINTS || instances.len() >= 500 {
                    return Err(invalid());
                }
                for instance in instances {
                    let size = |key| instance.get(key).and_then(Value::as_array).map_or(0, Vec::len);
                    points = points.saturating_add(size("points"));
                    curves = curves.saturating_add(size("curves"));
                    constraints = constraints.saturating_add(size("constraints"));
                    if points > solidcraft_sketch::MAX_POINTS
                        || curves > solidcraft_sketch::MAX_CURVES
                        || constraints > solidcraft_sketch::MAX_CONSTRAINTS
                    {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Bound arrays before typed allocation; only old files with absent markers need migration.
fn validate_wires(value: &Value, version: u32) -> Result<Vec<(usize, usize)>> {
    let invalid = || DocError::Invalid("damaged design: 3D curve arrays, ids or coordinate spaces are invalid".into());
    let mut legacy = Vec::new();
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(legacy) };
    if features.len() > crate::document::MAX_FEATURES {
        return Err(invalid());
    }
    for (feature_index, feature) in features.iter().enumerate() {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(wires) = feature.get("sketch").and_then(|sketch| sketch.get("wires")) else { continue };
        let wires = wires.as_array().ok_or_else(invalid)?;
        if wires.len() > 20_000 {
            return Err(invalid());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut work = 0usize;
        for (wire_index, wire) in wires.iter().enumerate() {
            let id = wire.get("id").and_then(Value::as_str).ok_or_else(invalid)?;
            if id.is_empty() || id.len() > 512 || !ids.insert(id) {
                return Err(invalid());
            }
            if let Some(link) = wire.get("link").filter(|link| !link.is_null())
                && link.as_str().is_none_or(|link| link.is_empty() || link.len() > 512)
            {
                return Err(invalid());
            }
            match wire.get("space") {
                None if version < 10 => legacy.push((feature_index, wire_index)),
                None => {}
                Some(Value::String(space)) if matches!(space.as_str(), "world" | "component") => {}
                _ => return Err(invalid()),
            }
            let pts = wire.get("pts").and_then(Value::as_array).ok_or_else(invalid)?;
            let fit = match wire.get("fit") {
                None => &[][..],
                Some(fit) => fit.as_array().ok_or_else(invalid)?.as_slice(),
            };
            work = work.saturating_add(pts.len()).saturating_add(fit.len());
            if work > 1_000_000 || pts.is_empty() || pts.len() > 100_000 || fit.len() > 500 || (pts.len() == 1 && fit.len() != 1) {
                return Err(invalid());
            }
            for point in pts.iter().chain(fit) {
                let point = point.as_array().ok_or_else(invalid)?;
                if point.len() != 3 || point.iter().any(|coordinate| coordinate.as_f64().is_none_or(|n| !n.is_finite() || n.abs() >= MAX_REAL)) {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(legacy)
}

fn validate_body_transfers(value: &Value) -> Result<()> {
    let invalid = || DocError::Invalid("damaged design: body transfer needs bounded unique bodies, components and a rigid placement".into());
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(()) };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("body_transfer") {
            continue;
        }
        let bodies = feature.get("bodies").and_then(Value::as_array).ok_or_else(invalid)?;
        if bodies.is_empty() || bodies.len() > 1000 {
            return Err(invalid());
        }
        let mut names = std::collections::BTreeSet::new();
        for body in bodies {
            let body = body.as_str().ok_or_else(invalid)?;
            if body.trim().is_empty() || body.len() > 512 || body.chars().any(char::is_control) || !names.insert(body) {
                return Err(invalid());
            }
        }
        for key in ["from", "to"] {
            if feature.get(key).and_then(Value::as_u64).is_none() {
                return Err(invalid());
            }
        }
        let owner = match feature.get("component") {
            None => 0,
            Some(value) => value.as_u64().ok_or_else(invalid)?,
        };
        if feature.get("to").and_then(Value::as_u64) != Some(owner) {
            return Err(DocError::Invalid("damaged design: body transfer owner must match its destination".into()));
        }
        let matrix = feature.get("transform").and_then(Value::as_array).ok_or_else(invalid)?;
        if matrix.len() != 4 {
            return Err(invalid());
        }
        let mut placement = [[0.0; 4]; 4];
        for (out, column) in placement.iter_mut().zip(matrix) {
            let column = column.as_array().ok_or_else(invalid)?;
            if column.len() != 4 {
                return Err(invalid());
            }
            for (out, number) in out.iter_mut().zip(column) {
                *out = number.as_f64().ok_or_else(invalid)?;
            }
        }
        if !crate::body_transfer::valid_transfer_matrix(&placement) {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Validate new explicit targets before typed decoding. Older planar features have none.
fn validate_emboss_targets(value: &Value) -> Result<()> {
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(()) };
    if features.len() > crate::document::MAX_FEATURES {
        return Err(DocError::Invalid("document too large".into()));
    }
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("emboss") {
            continue;
        }
        let Some(target) = feature.get("face").filter(|target| !target.is_null()) else { continue };
        let invalid = || DocError::Invalid("damaged design: an emboss face needs a bounded body name and three finite coordinates".into());
        let object = target.as_object().ok_or_else(invalid)?;
        if object.keys().any(|key| !matches!(key.as_str(), "body" | "at" | "path" | "author_path")) {
            return Err(invalid());
        }
        let body = object.get("body").and_then(Value::as_str).ok_or_else(invalid)?;
        let point = object.get("at").and_then(Value::as_array).ok_or_else(invalid)?;
        if body.trim().is_empty()
            || body.len() > 512
            || body.chars().any(char::is_control)
            || point.len() != 3
            || point.iter().any(|coordinate| coordinate.as_f64().is_none_or(|number| !number.is_finite() || number.abs() > MAX_REAL))
        {
            return Err(invalid());
        }
        for path in ["path", "author_path"].into_iter().filter_map(|key| object.get(key)) {
            let path = path.as_array().ok_or_else(invalid)?;
            if path.len() > 64 {
                return Err(invalid());
            }
            let mut ids = std::collections::BTreeSet::new();
            for id in path {
                let id = id.as_u64().filter(|id| *id != 0).ok_or_else(invalid)?;
                if !ids.insert(id) {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

/// Bound explicitly versioned geometry before allocating typed spline point arrays.
fn validate_versioned_geometry(value: &Value) -> Result<()> {
    let invalid = || DocError::Invalid("damaged design: invalid periodic spline or revolve convention".into());
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(()) };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) == Some("revolve")
            && let Some(convention) = feature.get("convention")
            && !matches!(convention.as_str(), Some("legacy_axis_start" | "sketch_normal"))
        {
            return Err(invalid());
        }
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(sketch) = feature.get("sketch") else { continue };
        if let Some(links) = sketch.get("links").and_then(Value::as_array) {
            for link in links {
                if link.get("auto_face").is_some_and(|marker| !marker.is_boolean()) {
                    return Err(invalid());
                }
            }
        }
        let Some(curves) = sketch.get("curves").and_then(Value::as_array) else { continue };
        let points = sketch.get("points").and_then(Value::as_array);
        let mut work = 0usize;
        for curve in curves {
            if curve.get("type").and_then(Value::as_str) != Some("spline") {
                continue;
            }
            let Some(marker) = curve.get("periodic") else { continue };
            let periodic = marker.as_bool().ok_or_else(invalid)?;
            if !periodic {
                continue;
            }
            let indices = curve.get("pts").and_then(Value::as_array).ok_or_else(invalid)?;
            work = work.saturating_add(indices.len());
            if !(4..=solidcraft_sketch::MAX_SPLINE_POINTS).contains(&indices.len())
                || work > 1_000_000
                || curve.get("control").is_some_and(|control| control.as_bool() != Some(false))
                || curve.get("degree").is_some_and(|degree| degree.as_u64() != Some(3))
            {
                return Err(invalid());
            }
            let count = points.ok_or_else(invalid)?.len();
            if indices.iter().any(|index| index.as_u64().is_none_or(|index| index >= count as u64)) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn validate_freeform_offsets(value: &Value) -> Result<()> {
    let Some(features) = value.get("features").and_then(Value::as_array) else { return Ok(()) };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(records) = feature.get("sketch").and_then(|sketch| sketch.get("freeform_offsets")) else { continue };
        let invalid = || DocError::Invalid("damaged design: invalid free-form offset metadata".into());
        let records = records.as_array().ok_or_else(invalid)?;
        if records.len() > 128 {
            return Err(invalid());
        }
        let periodic_outputs: std::collections::BTreeSet<_> = records
            .iter()
            .filter(|record| record.get("certificate").and_then(Value::as_u64) == Some(2))
            .filter_map(|record| record.get("output").and_then(Value::as_str))
            .collect();
        let mut periodic_curves = std::collections::BTreeMap::new();
        if !periodic_outputs.is_empty()
            && let Some(curves) = feature.get("sketch").and_then(|sketch| sketch.get("curves")).and_then(Value::as_array)
        {
            if curves.len() > 1_000_000 {
                return Err(invalid());
            }
            for curve in curves {
                if let Some(id) = curve.get("id").and_then(Value::as_str)
                    && periodic_outputs.contains(id)
                    && periodic_curves.insert(id, curve).is_some()
                {
                    return Err(invalid());
                }
            }
        }
        let mut outputs = std::collections::BTreeSet::new();
        for record in records {
            let object = record.as_object().ok_or_else(invalid)?;
            if object.len() != 9
                || object.keys().any(|key| {
                    !matches!(
                        key.as_str(),
                        "source" | "output" | "forward" | "distance" | "divisions" | "pieces" | "closed" | "tolerance" | "certificate"
                    )
                })
            {
                return Err(invalid());
            }
            let source = record.get("source").and_then(Value::as_str).ok_or_else(invalid)?;
            let output = record.get("output").and_then(Value::as_str).ok_or_else(invalid)?;
            if [source, output].iter().any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
                || source == output
                || !outputs.insert(output)
            {
                return Err(invalid());
            }
            let divisions = record.get("divisions").and_then(Value::as_u64).ok_or_else(invalid)?;
            let pieces = record.get("pieces").and_then(Value::as_u64).ok_or_else(invalid)?;
            let distance = record.get("distance").and_then(Value::as_f64).ok_or_else(invalid)?;
            let tolerance = record.get("tolerance").and_then(Value::as_f64).ok_or_else(invalid)?;
            if !(1..=256).contains(&divisions)
                || !divisions.is_power_of_two()
                || !(1..=256).contains(&pieces)
                || pieces * divisions >= solidcraft_sketch::MAX_SPLINE_POINTS as u64
                || !distance.is_finite()
                || !(1e-9..=MAX_REAL).contains(&distance.abs())
                || !tolerance.is_finite()
                || !(tolerance > 0.0 && tolerance <= 0.002)
                || !matches!(record.get("certificate").and_then(Value::as_u64), Some(1 | 2))
                || record.get("forward").and_then(Value::as_bool).is_none()
                || record.get("closed").and_then(Value::as_bool).is_none()
            {
                return Err(invalid());
            }
            if record.get("certificate").and_then(Value::as_u64) == Some(2) {
                let output_curve = periodic_curves.get(output);
                if record.get("closed").and_then(Value::as_bool) != Some(true)
                    || output_curve.is_none_or(|curve| {
                        curve.get("type").and_then(Value::as_str) != Some("spline") || curve.get("periodic").and_then(Value::as_bool) != Some(true)
                    })
                {
                    return Err(invalid());
                }
            }
        }
        let mut resolved = std::collections::BTreeSet::new();
        for record in records {
            let source = record.get("source").and_then(Value::as_str).ok_or_else(invalid)?;
            let output = record.get("output").and_then(Value::as_str).ok_or_else(invalid)?;
            if outputs.contains(source) && !resolved.contains(source) {
                return Err(invalid());
            }
            resolved.insert(output);
        }
    }
    Ok(())
}

/// Largest coordinate or size a design may hold (mm): a kilometre-scale part is already far
/// outside what the kernel's tolerances serve.
const MAX_REAL: f64 = 1e9;

/// Every number the design itself holds is in range ([`check_numbers`]). `metadata` is left
/// alone: it is other tools' data, kept as it is and never read as a number (#1015); parsing
/// already bounds its nesting. Engineering SI constants have their own bounded admission
/// (e.g. E in Pa): only the exact root user-material card field bypasses the coordinate bound.
fn check_design_numbers(v: &Value) -> Result<()> {
    let Some(root) = v.as_object() else { return check_numbers(v, 0) };
    for (key, value) in root {
        if key == "metadata" {
            continue;
        }
        if key != "user_materials" {
            check_numbers(value, 1)?;
            continue;
        }
        let Some(materials) = value.as_array() else { return check_numbers(value, 1) };
        if materials.len() > crate::MAX_USER_MATERIALS {
            return Err(DocError::Invalid("document exceeds 1000 user materials".into()));
        }
        for material in materials {
            let Some(fields) = material.as_object() else {
                check_numbers(material, 2)?;
                continue;
            };
            for (field, data) in fields {
                // SI engineering constants have separate typed bounds. No other field,
                // including nested fields with this name, bypasses coordinate limits.
                if field == "physical_card" && !data.is_null() {
                    crate::physical_material::validate_card(data)?;
                } else {
                    check_numbers(data, 3)?;
                }
            }
        }
    }
    Ok(())
}

/// The range check again, on the typed design: an integer such as `9223372036854775808` passes
/// [`check_numbers`] in the file (ids and counters are integers and may be large), but once it
/// lands in a coordinate or size it is a float, and floats are written as floats. Integer fields
/// (ids, counters, indexes) stay integers and are never limited, so the field's type decides
/// what is checked, not its name.
fn check_typed_numbers(d: &Document) -> Result<()> {
    let typed = serde_json::to_value(d).map_err(|e| DocError::Invalid(format!("design file: {e}")))?;
    check_design_numbers(&typed)
}

/// Every non-integer number outside separately validated engineering cards is finite and within ±`MAX_REAL` (a damaged or hostile
/// file is refused instead of overflowing the geometry later).
fn check_numbers(v: &Value, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(DocError::Invalid("damaged design: nested too deeply".into()));
    }
    match v {
        Value::Number(n) if !(n.is_u64() || n.is_i64()) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if !(x.is_finite() && x.abs() <= MAX_REAL) {
                return Err(DocError::Invalid(format!("damaged design: the number {n} is out of range")));
            }
        }
        Value::Array(a) => a.iter().try_for_each(|x| check_numbers(x, depth + 1))?,
        Value::Object(m) => m.values().try_for_each(|x| check_numbers(x, depth + 1))?,
        _ => {}
    }
    Ok(())
}

/// A placement: rotation (orthonormal, not mirrored) and a translation in range.
fn rigid(m: &crate::Mat) -> bool {
    let row = |i: usize| m.get(i).copied().unwrap_or_default();
    let (x, y, z, t) = (row(0), row(1), row(2), row(3));
    let dot = |a: [f64; 4], b: [f64; 4]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |a: [f64; 4]| (dot(a, a) - 1.0).abs() < 1e-6;
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    m.iter().flatten().all(|v| v.is_finite())
        && unit(x)
        && unit(y)
        && unit(z)
        && dot(x, y).abs() < 1e-6
        && dot(y, z).abs() < 1e-6
        && dot(x, z).abs() < 1e-6
        && det > 0.0
        && [x[3], y[3], z[3]].iter().all(|v| v.abs() < 1e-9)
        && (t[3] - 1.0).abs() < 1e-9
        && t[..3].iter().all(|v| v.abs() <= MAX_REAL)
}

/// Feature input names and component occurrences, which format 1 files may predate.
fn normalise(d: &mut Document) -> Result<()> {
    d.normalise_component_color_indices()?;
    d.name_all_feature_inputs();
    let missing: Vec<(u64, u64)> =
        d.components.iter().filter(|c| !d.occurrences.iter().any(|o| o.component == c.id)).map(|c| (c.id, c.parent)).collect();
    for (c, p) in missing {
        d.add_occurrence(c, p, crate::IDENTITY)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pending change that records that its step ran, in `trace`.
    fn traced(name: &'static str) -> Part {
        fn step_a(v: &mut Value) -> Result<()> {
            trace(v, "a")
        }
        fn step_b(v: &mut Value) -> Result<()> {
            trace(v, "b")
        }
        fn trace(v: &mut Value, s: &str) -> Result<()> {
            let o = v.as_object_mut().ok_or_else(|| DocError::Invalid("no object".into()))?;
            o.entry("trace").or_insert_with(|| Value::Array(Vec::new())).as_array_mut().into_iter().for_each(|t| t.push(Value::from(s)));
            Ok(())
        }
        Part { name, step: Some(if name.starts_with('a') { step_a } else { step_b }) }
    }

    fn trace_of(v: &Value) -> Vec<&str> {
        v["trace"].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
    }

    /// Formats after the hand-written ones follow on from them, one folder each, and the build
    /// writes the next number exactly while changes are pending, so the last release (which
    /// reads up to 22) refuses what a development build saves.
    #[test]
    fn formats_follow_on_and_pending_changes_take_the_next_number() {
        assert_eq!(JSON_STEPS.len() as u32, HAND_WRITTEN - 1);
        for (i, (version, parts)) in FOLDED.iter().enumerate() {
            assert_eq!(*version, HAND_WRITTEN + 1 + i as u32);
            assert!(!parts.is_empty(), "format {version} has no changes");
        }
        let last_folded = HAND_WRITTEN + FOLDED.len() as u32;
        assert_eq!(FORMAT, last_folded + u32::from(!PENDING.is_empty()));
        const { assert!(FORMAT >= 24, "designs saved as 23 and 24 by nightlies still open") };
        assert_eq!(pending_names(), PENDING.iter().map(|p| p.name).collect::<Vec<_>>());
        assert_eq!(Document::new("A").format_pending, pending_names());
    }

    /// The build lists every change file there is, in file-name order: a change is a file, so
    /// two branches that each add one merge cleanly and the merged build has both.
    #[test]
    fn every_change_file_is_listed_in_name_order() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/format");
        let names = |dir: &str| -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(root.join(dir))
                .map(|d| d.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".rs").map(str::to_string)).collect())
                .unwrap_or_default();
            names.sort();
            names
        };
        assert_eq!(names("pending"), pending_names());
        for (version, parts) in FOLDED {
            assert_eq!(names(&format!("v{version}")), parts.iter().map(|p| p.name).collect::<Vec<_>>());
        }
    }

    /// Two changes pending side by side both run, in name order, after every older format's
    /// steps: an old design upgrades through all of them, and one from the last folded format
    /// gets just the pending ones.
    #[test]
    fn pending_changes_apply_in_order_after_older_formats() {
        let pending = [traced("a-first"), traced("b-second")];
        let formats = Formats { hand: &JSON_STEPS, folded: FOLDED, pending: &pending };
        let next = HAND_WRITTEN + FOLDED.len() as u32 + 1;
        assert_eq!(formats.current(), next);
        for version in [1, HAND_WRITTEN, next - 1] {
            let mut v = serde_json::json!({"features": [], "assembly": {"joints": [{"flip": false}]}});
            formats.upgrade(&mut v, version, None).unwrap();
            assert_eq!(trace_of(&v), ["a", "b"], "from format {version}");
            // The hand-written steps ran too: format 3's joints were flipped on the way up.
            assert_eq!(v["assembly"]["joints"][0]["flip"], Value::Bool(version < 4), "from format {version}");
        }
        // A design of the pending number written before `b-second` existed gets just that step.
        let mut v = serde_json::json!({});
        formats.upgrade(&mut v, next, Some(&["a-first".to_string()])).unwrap();
        assert_eq!(trace_of(&v), ["b"]);
        // One that has both, or that this build wrote, needs nothing.
        let mut v = serde_json::json!({});
        formats.upgrade(&mut v, next, Some(&["a-first".into(), "b-second".into()])).unwrap();
        assert_eq!(trace_of(&v), Vec::<&str>::new());
        // A change this build doesn't know is a newer build's: refused, not guessed at.
        let err = formats.upgrade(&mut serde_json::json!({}), next, Some(&["c-later".into()])).unwrap_err();
        assert!(err.to_string().contains("newer SolidCraft"), "{err}");
        // A list on a format that isn't made of changes means nothing.
        let mut v = serde_json::json!({});
        formats.upgrade(&mut v, HAND_WRITTEN, Some(&["a-first".into()])).unwrap();
        assert_eq!(trace_of(&v), ["a", "b"]);
    }

    /// Folding the pending changes into a number (the release script) keeps every design
    /// opening the same: a file saved while they were pending gets the steps it missed.
    #[test]
    fn folded_changes_upgrade_like_pending_ones() {
        let parts = [traced("a-first"), traced("b-second")];
        let mut folded: Vec<(u32, &[Part])> = FOLDED.to_vec();
        let next = HAND_WRITTEN + folded.len() as u32 + 1;
        folded.push((next, &parts));
        let formats = Formats { hand: &JSON_STEPS, folded: &folded, pending: &[] };
        assert_eq!(formats.current(), next);
        let mut old = serde_json::json!({});
        formats.upgrade(&mut old, HAND_WRITTEN, None).unwrap();
        assert_eq!(trace_of(&old), ["a", "b"]);
        let mut nightly = serde_json::json!({});
        formats.upgrade(&mut nightly, next, Some(&["a-first".into()])).unwrap();
        assert_eq!(trace_of(&nightly), ["b"]);
        let mut released = serde_json::json!({});
        formats.upgrade(&mut released, next, None).unwrap();
        assert_eq!(trace_of(&released), Vec::<&str>::new());
    }

    #[test]
    fn personal_format29_card_alias_requires_actual_canonical_support() {
        let hand: [JsonStep; 27] = [|_| Ok(()); 27];
        let first = [traced("a-first")];
        let cards = [traced("material-engineering-cards")];
        let pending = Formats { hand: &hand, folded: &[(29, &first)], pending: &cards };
        let folded = Formats { hand: &hand, folded: &[(29, &first), (30, &cards)], pending: &[] };
        for formats in [pending, folded] {
            let mut file = serde_json::json!({"trace": ["b"]});
            formats.upgrade(&mut file, 29, Some(&["physical-material-cards".into()])).unwrap();
            assert_eq!(trace_of(&file), ["b", "a"], "never replay existing card semantics");
        }
    }

    #[test]
    fn personal_card_alias_without_support_refuses_before_mutation() {
        let hand: [JsonStep; 27] = [|_| Ok(()); 27];
        let first = [traced("a-first")];
        let formats = Formats { hand: &hand, folded: &[(29, &first)], pending: &[] };
        let mut file = serde_json::json!({"trace": [], "sentinel": 7});
        let original = file.clone();
        let error = formats.upgrade(&mut file, 29, Some(&["physical-material-cards".into()])).unwrap_err();
        assert!(error.to_string().contains("physical-material-cards"), "{error}");
        assert_eq!(file, original);
    }

    #[test]
    fn personal_card_alias_does_not_extend_to_other_format_numbers() {
        let hand: [JsonStep; 26] = [|_| Ok(()); 26];
        let first = [traced("a-first")];
        let second = [traced("b-second")];
        let cards = [traced("material-engineering-cards")];
        let formats = Formats { hand: &hand, folded: &[(28, &first), (29, &second), (30, &cards)], pending: &[] };
        for version in [28, 30] {
            let mut file = serde_json::json!({"trace": []});
            let original = file.clone();
            let error = formats.upgrade(&mut file, version, Some(&["physical-material-cards".into()])).unwrap_err();
            assert!(error.to_string().contains("physical-material-cards"), "{error}");
            assert_eq!(file, original);
        }
    }

    #[test]
    fn personal_card_alias_never_hides_unknown_concepts() {
        let hand: [JsonStep; 27] = [|_| Ok(()); 27];
        let first = [traced("a-first")];
        let cards = [traced("material-engineering-cards")];
        let formats = Formats { hand: &hand, folded: &[(29, &first)], pending: &cards };
        for names in
            [["physical-material-cards".into(), "unknown-card-codec".into()], ["unknown-card-codec".into(), "physical-material-cards".into()]]
        {
            let mut file = serde_json::json!({"trace": ["b"]});
            let original = file.clone();
            let error = formats.upgrade(&mut file, 29, Some(&names)).unwrap_err();
            assert!(error.to_string().contains("unknown-card-codec"), "{error}");
            assert_eq!(file, original);
        }
    }

    #[test]
    fn alias_and_canonical_markers_never_reapply_the_card_migration() {
        let hand: [JsonStep; 27] = [|_| Ok(()); 27];
        let first = [traced("a-first"), traced("material-engineering-cards")];
        let formats = Formats { hand: &hand, folded: &[(29, &first)], pending: &[] };
        for names in [
            vec!["physical-material-cards".into()],
            vec!["physical-material-cards".into(), "material-engineering-cards".into()],
            vec!["material-engineering-cards".into(), "physical-material-cards".into()],
        ] {
            let mut file = serde_json::json!({"trace": ["b"]});
            formats.upgrade(&mut file, 29, Some(&names)).unwrap();
            assert_eq!(trace_of(&file), ["b", "a"]);
        }
    }

    #[test]
    fn ordinary_releases_still_receive_cards_and_metadata_remains_inert() {
        let hand: [JsonStep; 27] = [|_| Ok(()); 27];
        let first = [traced("a-first")];
        let cards = [traced("material-engineering-cards")];
        let formats = Formats { hand: &hand, folded: &[(29, &first)], pending: &cards };
        let mut file = serde_json::json!({"trace": ["a"], "metadata": {"format_pending": ["physical-material-cards"]}});
        let metadata = file["metadata"].clone();
        formats.upgrade(&mut file, 29, None).unwrap();
        assert_eq!(trace_of(&file), ["a", "b"]);
        assert_eq!(file["metadata"], metadata);
    }

    /// An experimental producer can use a number that a release later folds without its
    /// change. Its unknown concept must still be refused before any migration runs.
    #[test]
    fn unknown_pending_concepts_are_refused_after_folding() {
        let parts = [traced("a-first")];
        let later = [traced("b-second")];
        for pending in [&[][..], &later[..]] {
            let formats = Formats { hand: &[], folded: &[(2, &parts)], pending };
            let mut file = serde_json::json!({"trace": []});
            let original = file.clone();
            let err = formats.upgrade(&mut file, 2, Some(&["experimental-card".into()])).unwrap_err();
            assert!(err.to_string().contains("experimental-card") && err.to_string().contains("update SolidCraft"), "{err}");
            assert_eq!(file, original, "refuse before applying even a known migration");
        }
    }

    /// The same concept can land at a later number than an experimental producer used.
    /// Recognize it anywhere in this reader's history and never apply its migration twice.
    #[test]
    fn pending_concepts_moved_to_later_formats_are_not_reapplied() {
        let first = [traced("a-first")];
        let later = [traced("b-second")];
        let pending = Formats { hand: &[], folded: &[(2, &first)], pending: &later };
        let folded = Formats { hand: &[], folded: &[(2, &first), (3, &later)], pending: &[] };
        for formats in [pending, folded] {
            let mut experimental = serde_json::json!({"trace": ["b"]});
            formats.upgrade(&mut experimental, 2, Some(&["b-second".into()])).unwrap();
            assert_eq!(trace_of(&experimental), ["b", "a"]);
            let mut released = serde_json::json!({"trace": ["a"]});
            formats.upgrade(&mut released, 2, None).unwrap();
            assert_eq!(trace_of(&released), ["a", "b"], "ordinary released files still get later migrations");
        }
    }

    /// `format_pending` is read and checked: a damaged list is refused, a list of changes this
    /// build doesn't have is a newer build's, and saving writes this build's own list.
    #[test]
    fn format_pending_in_a_file_is_checked() {
        let mut file: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        assert_eq!(file.get("format_pending").is_some(), !PENDING.is_empty());
        for damaged in [serde_json::json!("x"), serde_json::json!([1]), serde_json::json!(["x".repeat(200)])] {
            file["format_pending"] = damaged;
            assert!(read(&file.to_string()).is_err());
        }
        file["format_pending"] = serde_json::json!(["not-a-change-anyone-made"]);
        assert!(read(&file.to_string()).is_err(), "unknown pending concepts are refused even after folding");
        file["format_pending"] = Value::from(pending_names());
        let opened = read(&file.to_string()).unwrap();
        assert_eq!(opened.format_pending, pending_names());
    }

    /// 24 added the corner relief of an edge flange: a no-op step carries a 23 design up, a flange
    /// without a relief of its own saves as before, and one with it is saved under 24 and kept.
    #[test]
    fn version_twenty_three_designs_open_and_an_edge_flange_relief_is_saved_in_twenty_four() {
        let flange = |shape: Option<&str>, size: Option<&str>| crate::FeatureKind::SheetFlange {
            edges: vec![crate::FlangeEdge::at(solidcraft_geom::Vec3::ZERO)],
            height: "10 mm".into(),
            angle: "90 deg".into(),
            radius: None,
            position: "inside".into(),
            flip: false,
            body: None,
            corner_relief_shape: shape.map(Into::into),
            corner_relief: size.map(Into::into),
            height_from: None,
            start_setback: None,
            end_setback: None,
        };
        let saved = |document: &Document| -> Value { serde_json::from_str(&document.to_json()).unwrap() };
        // A flange that follows the rule saves neither key, and a design of format 23 opens as it was.
        let mut plain = Document::new("A");
        plain.add_feature(flange(None, None), None).unwrap();
        let mut file = saved(&plain);
        let feature = file["features"].as_array().and_then(|f| f.last()).cloned().unwrap();
        assert_eq!(feature["type"], "sheet_flange");
        assert!(feature.get("corner_relief_shape").is_none() && feature.get("corner_relief").is_none(), "{feature}");
        file["format"] = Value::from("solidcraft/23");
        let opened = read(&file.to_string()).unwrap();
        assert_eq!(opened.format, format_string(FORMAT));
        let mut kept = plain.features.clone();
        kept.iter_mut().for_each(|f| f.kernel_piece_order = true);
        assert_eq!(opened.features, kept);
        // One that chooses its relief says so in the file, which is stamped 24 and keeps it.
        let mut own = Document::new("A");
        own.add_feature(flange(Some("round"), Some("6 mm")), None).unwrap();
        let file = saved(&own);
        let feature = file["features"].as_array().and_then(|f| f.last()).cloned().unwrap();
        assert_eq!((&feature["corner_relief_shape"], &feature["corner_relief"]), (&Value::from("round"), &Value::from("6 mm")));
        // A build that reads up to 23 asks for an update on that stamp instead of dropping the relief.
        assert_eq!(version_of(file["format"].as_str().unwrap_or_default()), Some(FORMAT));
        assert_eq!(read(&file.to_string()).unwrap(), own);
    }

    #[test]
    fn version_thirteen_names_keep_their_meaning_under_the_legacy_mark() {
        let file = serde_json::json!({
            "format": "solidcraft/13",
            "features": [
                {"type": "fillet", "edge_names": ["F2:side:l2|F2:top#2~1", ""], "face_names": ["F2:top#1", ""]},
                {"type": "sheet_flange", "edge_names": ["base:l1:top"]},
                {"type": "sketch", "plane": {"type": "face", "plane": {}, "at": [0, 0, 0], "name": "F2:top#1"},
                 "sketch": {"links": [{"id": "l1", "face_name": "F2:top#2"}]}},
                {"type": "construction_axis", "def": {"type": "two_points",
                 "a": {"type": "edge", "at": [0, 0, 0], "name": "F2:start|F2:side:l1"},
                 "b": {"type": "face", "at": [0, 0, 0], "name": "F2:start"}}},
                {"name": "Hole #2", "type": "extrude"},
                {"type": "thread", "face": [1, 2, 3]}
            ],
            "assembly": {
                "joints": [{"a": {"occurrence": 0, "snap": {"type": "face_center", "pick": [0, 0, 0], "face": "F2:top#2"}},
                            "b": {"occurrence": 1, "snap": {"type": "point", "point": [0, 0, 0]}}}],
                "origins": [{"name": "O1", "origin": {"occurrence": 0, "snap": {"type": "circle_center", "pick": [0, 0, 0], "face": "F3:side"}}}]
            },
            "appearances": {"faces": [{"body": "Body1", "name": "F2:top#1"}, {"body": "Body1", "name": "F2:+x", "strict_name": true}]}
        });
        let mut v = file.clone();
        v13_to_v14(&mut v).unwrap();
        assert_eq!(v["features"][0]["edge_names"], serde_json::json!(["?F2:side:l2|F2:top#2~1", ""]));
        assert_eq!(v["features"][0]["face_names"], serde_json::json!(["?F2:top#1", ""]));
        assert_eq!(v["features"][1]["edge_names"][0], "base:l1:top", "sheet metal's edge names are its own");
        assert_eq!(v["features"][2]["plane"]["name"], "?F2:top#1");
        assert_eq!(v["features"][2]["sketch"]["links"][0]["face_name"], "?F2:top#2");
        assert_eq!(v["features"][3]["def"]["a"]["name"], "?F2:start|F2:side:l1");
        assert_eq!(v["features"][3]["def"]["b"]["name"], "?F2:start");
        assert_eq!(v["features"][4]["name"], "Hole #2", "a feature's own name is not a face name");
        assert_eq!(v["features"][5]["face"], serde_json::json!([1, 2, 3]));
        assert_eq!(v["assembly"]["joints"][0]["a"]["snap"]["face"], "?F2:top#2");
        assert_eq!(v["assembly"]["origins"][0]["origin"]["snap"]["face"], "?F3:side");
        assert_eq!(v["appearances"]["faces"][0]["name"], "?F2:top#1");
        assert_eq!(v["appearances"]["faces"][1]["name"], "F2:+x", "strict appearance names are certified lineage");
        // Every feature keeps naming the pieces it splits a body into as the saving build did.
        assert!(v["features"].as_array().unwrap().iter().all(|f| f["shell_piece_order"] == true));
        // The step never marks twice.
        let once = v.clone();
        v13_to_v14(&mut v).unwrap();
        assert_eq!(v, once);
    }

    /// Only the fields that store a face or edge name are marked: other fields with the same key
    /// names, and objects tagged `face` or `edge` that are not references, are left alone.
    #[test]
    fn version_thirteen_marking_targets_name_fields_not_key_names() {
        let file = serde_json::json!({
            "format": "solidcraft/13",
            "features": [
                {"type": "extrude", "options": {"face": "flat", "face_name": "label", "face_names": ["a"]}},
                {"type": "fillet", "settings": {"edge_names": ["kept"]}},
                {"type": "sketch", "sketch": {"style": {"face_name": "kept"}}},
                {"type": "split_body", "tool": {"type": "face", "name": "not a reference"}}
            ],
            "assembly": {"joints": [{"a": {"occurrence": 0, "snap": {"type": "frame", "face": "kept"}}}]},
            "notes": {"face": "F2:top", "face_name": "F2:top", "face_names": ["F2:top"]}
        });
        let mut v = file.clone();
        v13_to_v14(&mut v).unwrap();
        for f in v["features"].as_array_mut().unwrap() {
            f.as_object_mut().unwrap().remove("shell_piece_order");
        }
        assert_eq!(v, file);
    }

    #[test]
    fn version_nine_wire_spaces_preserve_saved_points_and_explicit_markers() {
        use solidcraft_geom::Vec3;
        use solidcraft_sketch::{LinkKind, LinkSource, Sketch, WireSpace};
        let mut sketch = Sketch::new();
        sketch.add_drawn_wire(&[Vec3::new(12.0, 3.0, 4.0)]).unwrap();
        let link = sketch
            .add_link_with_wires(
                LinkKind::Include,
                LinkSource::Edge { body: "Missing".into(), at: Vec3::ZERO },
                &[],
                &[vec![Vec3::new(2.0, 7.0, 9.0), Vec3::new(2.0, 8.0, 9.0)]],
            )
            .unwrap();
        sketch.set_link_lost(&link, true);
        let mut doc = Document::new("legacy wire spaces");
        doc.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let mut json: Value = serde_json::from_str(&doc.to_json()).unwrap();
        json["format"] = Value::from("solidcraft/9");
        for wire in json["features"][0]["sketch"]["wires"].as_array_mut().unwrap() {
            wire.as_object_mut().unwrap().remove("space");
        }
        let old = json["features"][0]["sketch"]["wires"].clone();
        let migrated = read(&json.to_string()).unwrap();
        let crate::FeatureKind::Sketch { sketch, .. } = &migrated.features[0].kind else { panic!("sketch") };
        assert_eq!(sketch.wires[0].space, WireSpace::World);
        assert_eq!(sketch.wires[1].space, WireSpace::Component);
        let saved: Value = serde_json::from_str(&migrated.to_json()).unwrap();
        for (a, b) in old.as_array().unwrap().iter().zip(saved["features"][0]["sketch"]["wires"].as_array().unwrap()) {
            assert_eq!(a["pts"], b["pts"]);
            assert_eq!(a["fit"], b["fit"]);
            assert_eq!(a["link"], b["link"]);
        }
        assert_eq!(read(&migrated.to_json()).unwrap(), migrated);
        json["features"][0]["sketch"]["wires"][1]["space"] = Value::from("world");
        let explicit = read(&json.to_string()).unwrap();
        let crate::FeatureKind::Sketch { sketch, .. } = &explicit.features[0].kind else { panic!("sketch") };
        assert_eq!(sketch.wires[1].space, WireSpace::World);
    }

    #[test]
    fn hostile_wire_and_transfer_shapes_are_refused_before_typed_decode() {
        let base: Value = serde_json::from_str(&Document::new("bounds").to_json()).unwrap();
        let wire = serde_json::json!({"id":"w1", "pts":[[1,2,3],[4,5,6]],"space":"world"});
        for damaged in [
            serde_json::json!({"space":"guess"}),
            serde_json::json!({"pts":[[1,2]]}),
            serde_json::json!({"pts":[[1e9,2,3],[4,5,6]]}),
            serde_json::json!({"fit": vec![[1,2,3];501]}),
        ] {
            let mut changed = wire.clone();
            for (key, value) in damaged.as_object().unwrap() {
                changed[key] = value.clone();
            }
            let mut file = base.clone();
            file["features"] = serde_json::json!([{"type":"sketch","sketch":{"wires":[changed]}}]);
            assert!(validate_wires(&file, FORMAT).is_err());
        }
        let transfer = serde_json::json!({"type":"body_transfer","component":1,"bodies":["Body1"],"from":0,"to":1,"transform":crate::IDENTITY});
        for damaged in [
            serde_json::json!({"bodies":["Body1","Body1"]}),
            serde_json::json!({"from":-1}),
            serde_json::json!({"component":0}),
            serde_json::json!({"transform":[[2,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}),
            serde_json::json!({"bodies":["\n"]}),
        ] {
            let mut changed = transfer.clone();
            for (key, value) in damaged.as_object().unwrap() {
                changed[key] = value.clone();
            }
            let mut file = base.clone();
            file["features"] = serde_json::json!([changed]);
            assert!(validate_body_transfers(&file).is_err());
        }
    }

    #[test]
    fn recorded_transfer_roundtrips_and_unknown_component_is_refused() {
        let mut doc = Document::new("recorded ownership boundary");
        doc.add_feature(crate::FeatureKind::BodyTransfer { bodies: vec!["Body1".into()], from: 0, to: 0, transform: crate::IDENTITY }, None).unwrap();
        assert_eq!(read(&doc.to_json()).unwrap(), doc);
        let mut json: Value = serde_json::from_str(&doc.to_json()).unwrap();
        assert_eq!(json["features"][0]["type"], "body_transfer");
        json["features"][0]["to"] = Value::from(12345);
        json["features"][0]["component"] = Value::from(12345);
        assert!(read(&json.to_string()).unwrap_err().to_string().contains("unknown component"));
        json["features"][0]["to"] = Value::from(0);
        assert!(read(&json.to_string()).unwrap_err().to_string().contains("owner must match"));
        json["features"][0]["component"] = Value::from(0);
        json["features"][0]["transform"][0][0] = Value::from(2);
        assert!(read(&json.to_string()).unwrap_err().to_string().contains("rigid placement"));
    }

    #[test]
    fn version_eight_to_object_offset_keeps_existing_model_parameter_names() {
        use crate::{Direction, Extent, FeatureKind, Operation, PlaneRef, ProfileSel};
        let mut document = Document::new("legacy named extent");
        document.set_param("offset", "2", Some("mm"), None).unwrap();
        let sketch = document
            .add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: solidcraft_sketch::Sketch::new() }, None)
            .unwrap();
        document
            .add_feature(
                FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(sketch),
                    profiles: ProfileSel::All,
                    extent: Extent {
                        convention: crate::ExtrudeConvention::LegacyPerSide,
                        distance: "10".into(),
                        direction: Direction::Negative,
                        distance2: None,
                        start_offset: None,
                        through_all: false,
                        taper: Some("0 deg".into()),
                        to: Some(crate::FacePick(solidcraft_geom::Vec3::new(0.0, 0.0, 10.0))),
                        to_offset: Some("offset + 1 mm".into()),
                        from: Some(crate::FacePick(solidcraft_geom::Vec3::new(0.0, 0.0, 5.0))),
                    },
                    operation: Operation::NewBody,
                    targets: vec![],
                },
                None,
            )
            .unwrap();
        document
            .add_feature(
                FeatureKind::Box {
                    corner: ["0".into(), "0".into(), "0".into()],
                    length: "d1".into(),
                    width: "4".into(),
                    height: "6".into(),
                    operation: Operation::NewBody,
                },
                None,
            )
            .unwrap();
        let mut legacy: Value = serde_json::from_str(&document.to_json()).unwrap();
        legacy["format"] = Value::from("solidcraft/8");
        // Version 8 exposed distance and taper, but had no model slot for To Offset.
        legacy["features"][1]["param_names"] = serde_json::json!(["d1", "d2"]);
        legacy["features"][2]["param_names"] = serde_json::json!(["d3", "d4", "d5"]);
        let old_kind = legacy["features"][1].clone();
        let upgraded = read(&legacy.to_string()).unwrap();
        assert_eq!(&upgraded.features[1].param_names[..2], ["d1", "d2"]);
        assert_eq!(upgraded.features[2].param_names, ["d3", "d4", "d5"]);
        assert_eq!(upgraded.features[1].param_names.len(), 3);
        assert_eq!(upgraded.features[1].param_names[2], "d6");
        let saved: Value = serde_json::from_str(&upgraded.to_json()).unwrap();
        for field in ["extent", "sketch", "operation", "profiles"] {
            assert_eq!(saved["features"][1][field], old_kind[field]);
        }
        let (values, errors) = upgraded.param_values();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(values.get("d1").unwrap().v, 10.0);
        assert_eq!(values.get("d3").unwrap().v, 10.0);
        assert_eq!(values.get("d6").unwrap().v, 3.0);
        assert_eq!(read(&upgraded.to_json()).unwrap(), upgraded);
    }

    #[test]
    fn saved_offset_cannot_alias_and_overwrite_its_source_geometry() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::{CurveKind, Sketch};
        let mut sketch = Sketch::new();
        let center = sketch.add_point(Vec2::ZERO, None).unwrap();
        let major = sketch.add_point(Vec2::new(10.0, 0.0), None).unwrap();
        let source = sketch.add_curve(CurveKind::Ellipse { c: center, m: major, r: 5.0 }, None).unwrap();
        let output = sketch.add_freeform_offset(source, true, -1.0).unwrap();
        assert!(sketch.validate_freeform_offsets().is_ok());
        if let CurveKind::Spline { pts, .. } = &mut sketch.curves[output].kind {
            pts[0] = center;
        } else {
            panic!("offset must produce a fit spline");
        }
        let mut document = Document::new("aliased offset geometry");
        document.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let error = read(&document.to_json()).unwrap_err().to_string();
        assert!(error.contains("damaged design"), "{error}");
    }

    #[test]
    fn structurally_valid_offset_records_must_reference_real_sketch_geometry() {
        let mut document = Document::new("missing offset geometry");
        document
            .add_feature(
                crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch: solidcraft_sketch::Sketch::new() },
                None,
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&document.to_json()).unwrap();
        value["features"][0]["sketch"]["freeform_offsets"] = serde_json::json!([{
            "source":"ellipse", "output":"offset", "forward":true, "distance":2.0,
            "divisions":8, "pieces":4, "closed":true, "tolerance":0.002, "certificate":1
        }]);
        assert!(validate_freeform_offsets(&value).is_ok());
        let error = read(&value.to_string()).unwrap_err().to_string();
        assert!(error.contains("damaged design"), "{error}");
    }

    #[test]
    fn malformed_face_anchors_are_refused_before_typed_decoding() {
        let anchor = serde_json::json!({
            "target":{"body":"Base", "origin_feature":1, "name":"Base:+z", "point":[0,0,10],
                "certificate":{"kind":"box", "axis":2, "high":true}},
            "component":0, "path":[], "author_path":[], "coordinates":[0,0], "x_axis":[1,0]
        });
        let make = |anchor: Value| {
            serde_json::json!({"features":[{"type":"sketch", "plane":{
                "type":"face", "anchor":anchor
            }}]})
        };
        assert!(crate::face_anchor::validate_face_anchors_json(&make(anchor.clone())).is_ok());
        let mut nested = serde_json::json!({"features":[{"type":"sketch", "plane":{
            "type":"offset", "base":{"type":"at_angle", "base":{"type":"face", "anchor":anchor.clone()}}
        }}]});
        assert!(crate::face_anchor::validate_face_anchors_json(&nested).is_ok());
        nested["features"][0]["plane"]["base"]["base"]["anchor"]["coordinates"] = serde_json::json!([0, 0, 0]);
        assert!(crate::face_anchor::validate_face_anchors_json(&nested).is_err());
        let mut malformed = Vec::new();
        for (key, damaged) in [
            ("component", serde_json::json!(-1)),
            ("path", serde_json::json!([0])),
            ("author_path", serde_json::json!((1..=65).collect::<Vec<u64>>())),
            ("coordinates", serde_json::json!([1_000_000_001_u64, 0])),
            ("coordinates", serde_json::json!([0])),
            ("x_axis", serde_json::json!([0, 0])),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut candidate = anchor.clone();
            candidate[key] = damaged;
            malformed.push(candidate);
        }
        let mut missing_certificate = anchor.clone();
        missing_certificate["target"].as_object_mut().unwrap().remove("certificate");
        malformed.push(missing_certificate);
        let mut invalid_role = anchor;
        invalid_role["target"]["certificate"]["axis"] = Value::from(3);
        malformed.push(invalid_role);
        for candidate in malformed {
            let value = make(candidate);
            assert!(crate::face_anchor::validate_face_anchors_json(&value).is_err());
            let error = read(&value.to_string()).unwrap_err().to_string();
            assert!(!error.contains("design file:"), "must refuse before typed decode: {error}");
        }
    }

    #[test]
    fn periodic_markers_are_bounded_before_typed_decoding() {
        let curve = serde_json::json!({"id":"closed", "type":"spline", "pts":[0,1,2,3], "periodic":true});
        let make = |curve: Value| {
            serde_json::json!({"features":[{"type":"sketch", "sketch":{
                "points":[{}, {}, {}, {}], "curves":[curve]
            }}]})
        };
        assert!(validate_versioned_geometry(&make(curve.clone())).is_ok());
        for (key, malformed) in [
            ("periodic", serde_json::json!("true")),
            ("control", serde_json::json!(true)),
            ("degree", serde_json::json!(2)),
            ("pts", serde_json::json!([0, 1, 2])),
            ("pts", serde_json::json!([0, 1, 2, u64::MAX])),
            ("pts", serde_json::json!(vec![0; solidcraft_sketch::MAX_SPLINE_POINTS + 1])),
        ] {
            let mut damaged = curve.clone();
            damaged[key] = malformed;
            assert!(validate_versioned_geometry(&make(damaged)).is_err(), "{key}");
        }
        let mut legacy = curve;
        legacy.as_object_mut().unwrap().remove("periodic");
        legacy["control"] = Value::Bool(true);
        assert!(validate_versioned_geometry(&make(legacy)).is_ok());
        for convention in [serde_json::json!(null), serde_json::json!(false), serde_json::json!("unknown")] {
            assert!(validate_versioned_geometry(&serde_json::json!({"features":[{"type":"revolve", "convention":convention}]})).is_err());
        }
    }

    #[test]
    fn periodic_offset_certificate_requires_closed_periodic_output() {
        let record = serde_json::json!({"source":"ellipse", "output":"offset", "forward":true,
            "distance":2.0, "divisions":8, "pieces":4, "closed":true,
            "tolerance":0.002, "certificate":2});
        let mut value = serde_json::json!({"features":[{"type":"sketch", "sketch":{
            "freeform_offsets":[record], "curves":[{"id":"offset", "type":"spline", "periodic":true}]
        }}]});
        assert!(validate_freeform_offsets(&value).is_ok());
        let duplicate = value["features"][0]["sketch"]["curves"][0].clone();
        value["features"][0]["sketch"]["curves"].as_array_mut().unwrap().push(duplicate);
        assert!(validate_freeform_offsets(&value).is_err(), "periodic output ID must be unique");
        value["features"][0]["sketch"]["curves"].as_array_mut().unwrap().pop();
        value["features"][0]["sketch"]["curves"][0]["periodic"] = Value::Bool(false);
        assert!(validate_freeform_offsets(&value).is_err());
        value["features"][0]["sketch"]["curves"][0]["periodic"] = Value::Bool(true);
        value["features"][0]["sketch"]["freeform_offsets"][0]["closed"] = Value::Bool(false);
        assert!(validate_freeform_offsets(&value).is_err());
        value["features"][0]["sketch"]["freeform_offsets"][0]["certificate"] = Value::from(1);
        assert!(validate_freeform_offsets(&value).is_ok(), "legacy certificate semantics remain valid");
    }

    #[test]
    fn periodic_file_validation_refuses_damaged_closed_geometry() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::{CurveKind, Sketch};
        let mut sketch = Sketch::new();
        let mut pts = Vec::new();
        for point in [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), Vec2::ZERO] {
            pts.push(sketch.add_point(point, None).unwrap());
        }
        let first = pts[0];
        let neighbor = pts[1];
        let closing = pts[3];
        sketch.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: true }, None).unwrap();
        let mut document = Document::new("periodic file validation");
        document.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        assert_eq!(read(&document.to_json()).unwrap(), document);
        let mut value: Value = serde_json::from_str(&document.to_json()).unwrap();
        value["features"][0]["sketch"]["points"][closing]["pos"] = serde_json::json!([1, 0]);
        assert!(read(&value.to_string()).unwrap_err().to_string().contains("periodic"));
        value["features"][0]["sketch"]["points"][closing]["pos"] = value["features"][0]["sketch"]["points"][first]["pos"].clone();
        value["features"][0]["sketch"]["points"][neighbor]["pos"] = value["features"][0]["sketch"]["points"][first]["pos"].clone();
        assert!(read(&value.to_string()).unwrap_err().to_string().contains("periodic"));
    }

    #[test]
    fn freeform_offset_metadata_is_bounded_before_typed_decoding() {
        let valid = serde_json::json!({"source":"ellipse", "output":"offset", "forward":true,
            "distance":2.0, "divisions":8, "pieces":4, "closed":true,
            "tolerance":0.002, "certificate":1});
        let mut malformed = Vec::new();
        for (key, value) in [
            ("divisions", serde_json::json!(u64::MAX)),
            ("pieces", serde_json::json!(u64::MAX)),
            ("divisions", serde_json::json!(3)),
            ("distance", serde_json::json!(0)),
            ("tolerance", serde_json::json!(0.003)),
            ("certificate", serde_json::json!(2)),
            ("output", serde_json::json!("x".repeat(513))),
            ("source", serde_json::json!("offset")),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut record = valid.clone();
            record[key] = value;
            malformed.push(serde_json::json!([record]));
        }
        malformed.push(serde_json::json!(vec![valid.clone(); 129]));
        malformed.push(serde_json::json!([valid.clone(), valid.clone()]));
        let mut cycle = valid.clone();
        cycle["source"] = serde_json::json!("offset");
        cycle["output"] = serde_json::json!("ellipse");
        malformed.push(serde_json::json!([valid, cycle]));
        for records in malformed {
            let mut value: Value = serde_json::from_str(&Document::new("hostile offset").to_json()).unwrap();
            value["features"] = serde_json::json!([{"type":"sketch", "sketch":{"freeform_offsets":records}}]);
            let error = read(&value.to_string()).unwrap_err().to_string();
            assert!(error.contains("free-form offset metadata"), "{error}");
            value["format"] = Value::from(format_string(FORMAT + 1));
            assert!(read(&value.to_string()).unwrap_err().to_string().contains("newer SolidCraft"));
        }
    }

    #[test]
    fn explicit_emboss_target_bounds_precede_typed_decoding() {
        for target in [
            serde_json::json!({"body":"", "at":[0,0,0]}),
            serde_json::json!({"body":"Body", "at":[0,0]}),
            serde_json::json!({"body":"Body", "at":[0,"invalid",0]}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "unknown":true}),
            serde_json::json!({"body":"x".repeat(513), "at":[0,0,0]}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "path":"bad"}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "path":[0]}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "path":[1,1]}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "path":(1..=65).collect::<Vec<u64>>()}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "author_path":[1,1]}),
            serde_json::json!({"body":"Body", "at":[0,0,0], "author_path":(1..=65).collect::<Vec<u64>>()}),
        ] {
            let mut value: Value = serde_json::from_str(&Document::new("hostile target").to_json()).unwrap();
            value["features"] = serde_json::json!([{"type":"emboss", "face":target}]);
            let error = read(&value.to_string()).unwrap_err().to_string();
            assert!(error.contains("emboss face"), "{error}");
            value["format"] = Value::from(format_string(FORMAT + 1));
            let error = read(&value.to_string()).unwrap_err().to_string();
            assert!(error.contains("newer SolidCraft"), "{error}");
        }
    }

    #[test]
    fn version_six_configuration_rows_upgrade_without_changing_values_or_features() {
        use crate::config::{Column, ConfigRow};
        let mut d = Document::new("legacy configurations");
        d.set_param("width", "10 mm", None, None).unwrap();
        let feature = d
            .add_feature(
                crate::FeatureKind::Box {
                    corner: ["0".into(), "0".into(), "0".into()],
                    length: "width".into(),
                    width: "20".into(),
                    height: "5".into(),
                    operation: crate::Operation::NewBody,
                },
                None,
            )
            .unwrap();
        d.configs.columns = vec![Column::Param("width".into()), Column::Suppress(feature)];
        d.configs.rows = vec![
            ConfigRow { name: "Default".into(), cells: vec!["10 mm".into(), "false".into()] },
            ConfigRow { name: "Alternate".into(), cells: vec!["width / 2 + 5 mm".into(), "yes".into()] },
        ];
        d.configs.active = Some("Default".into());
        let mut legacy: Value = serde_json::from_str(&d.to_json()).unwrap();
        legacy["format"] = Value::from("solidcraft/6");
        let original_table = legacy["configs"].clone();
        let upgraded = read(&legacy.to_string()).unwrap();
        d.features.iter_mut().for_each(|f| (f.shell_piece_order, f.kernel_piece_order) = (true, true));
        assert_eq!(upgraded, d, "the identity migration preserves the complete design");
        assert_eq!(serde_json::to_value(&upgraded.configs).unwrap(), original_table);
        assert_eq!(upgraded.format, format_string(FORMAT));
        assert!(upgraded.configuration_matches());
        legacy.as_object_mut().unwrap().remove("configs");
        assert!(read(&legacy.to_string()).unwrap().configs.is_empty());
        legacy["configs"] = serde_json::json!({"columns":[],"rows":[]});
        assert!(read(&legacy.to_string()).unwrap().configs.is_empty());
    }

    #[test]
    fn version_seven_styles_upgrade_and_newer_versions_are_refused_before_columns() {
        use crate::config::{Column, ConfigRow};
        let mut d = Document::new("configured styles");
        d.configs.columns = vec![Column::Material("Body".into()), Column::Appearance("Body".into())];
        d.configs.rows = vec![ConfigRow {
            name: "Blue steel".into(),
            cells: vec!["Steel".into(), r#"{"name":"Custom blue","color":[40,90,186],"opacity":0.4}"#.into()],
        }];
        d.apply_configuration("Blue steel").unwrap();
        let mut value: Value = serde_json::from_str(&d.to_json()).unwrap();
        value["format"] = Value::from("solidcraft/7");
        let original_table = value["configs"].clone();
        let roundtrip = read(&value.to_string()).unwrap();
        assert_eq!(roundtrip, d);
        assert_eq!(serde_json::to_value(&roundtrip.configs).unwrap(), original_table);
        assert_eq!(roundtrip.format, format_string(FORMAT));
        assert!(roundtrip.configuration_matches());
        value["format"] = Value::from(format_string(FORMAT + 1));
        value["configs"]["columns"] = serde_json::json!([{"unknown_future_column":"Body"}]);
        value["configs"]["rules"] = serde_json::json!({"unknown_future_rule": true});
        let error = read(&value.to_string()).unwrap_err().to_string();
        assert!(error.contains("newer SolidCraft") && error.contains("update SolidCraft"), "{error}");
        assert!(!error.contains("unknown variant"), "format refusal precedes decoding new variants");
    }

    #[test]
    fn version_seven_origin_sketch_keeps_geometry_dimension_ids_and_expressions() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::{ConstraintKind, Sketch};
        let mut d = Document::new("legacy origin dimensions");
        d.set_param("reach", "25 mm", Some("mm"), None).unwrap();
        let dim = d.new_model_param("reach", "mm");
        let mut sketch = Sketch::new();
        let end = sketch.add_point(Vec2::new(25.0, 0.0), Some("tip")).unwrap();
        let line = sketch.add_line_pts(0, end, Some("edge")).unwrap();
        sketch.add_constraint(ConstraintKind::Horizontal { l: line }, None).unwrap();
        sketch.add_constraint(ConstraintKind::Length { l: line, value: 25.0 }, Some(dim)).unwrap();
        d.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, Some("Dimensioned")).unwrap();
        let mut legacy: Value = serde_json::from_str(&d.to_json()).unwrap();
        legacy["format"] = Value::from("solidcraft/7");
        let mut features = legacy["features"].clone();
        pieces_in_shell_order(&mut features);
        for f in features.as_array_mut().into_iter().flatten() {
            f["kernel_piece_order"] = Value::Bool(true);
        }
        let params = legacy["params"].clone();
        let upgraded = read(&legacy.to_string()).unwrap();
        d.features.iter_mut().for_each(|f| (f.shell_piece_order, f.kernel_piece_order) = (true, true));
        assert_eq!(upgraded, d);
        let upgraded_json: Value = serde_json::from_str(&upgraded.to_json()).unwrap();
        assert_eq!(upgraded_json["features"], features);
        assert_eq!(upgraded_json["params"], params);
        assert_eq!(read(&upgraded.to_json()).unwrap(), upgraded);
    }

    #[test]
    fn version_eight_persistent_angle_sweep_and_slot_dimensions_roundtrip() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::{ConstraintKind, Sketch};
        let mut d = Document::new("persistent typed dimensions");
        let heading = d.new_model_param("45 deg", "deg");
        let sweep = d.new_model_param("180 deg", "deg");
        let overall = d.new_model_param("10 mm", "mm");
        let mut sketch = Sketch::new();
        let end = sketch.add_point(Vec2::new(2.0, 2.0), None).unwrap();
        let line = sketch.add_line_pts(0, end, None).unwrap();
        let a = sketch.add_arc(Vec2::ZERO, Vec2::new(0.0, 2.0), Vec2::new(0.0, -2.0), [None; 3], None).unwrap();
        let b = sketch.add_arc(Vec2::new(6.0, 0.0), Vec2::new(6.0, -2.0), Vec2::new(6.0, 2.0), [None; 3], None).unwrap();
        sketch.add_constraint(ConstraintKind::LineAngle { l: line, value: std::f64::consts::FRAC_PI_4 }, Some(heading)).unwrap();
        sketch.add_constraint(ConstraintKind::ArcSweep { c: a, value: std::f64::consts::PI }, Some(sweep)).unwrap();
        sketch.add_constraint(ConstraintKind::SlotOverallLength { a, b, value: 10.0 }, Some(overall)).unwrap();
        d.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let mut saved: Value = serde_json::from_str(&d.to_json()).unwrap();
        assert_eq!(saved["format"], format_string(FORMAT));
        assert_eq!(read(&saved.to_string()).unwrap(), d);
        saved["format"] = Value::from(format_string(FORMAT + 1));
        saved["features"][0]["sketch"]["constraints"][0]["type"] = Value::from("future_dimension");
        let error = read(&saved.to_string()).unwrap_err().to_string();
        assert!(error.contains("newer SolidCraft") && !error.contains("unknown variant"), "{error}");
    }

    #[test]
    fn bulk_input_repair_preserves_existing_names_and_reuses_surplus_names() {
        let mut d = Document::new("input repair");
        d.set_param("d2", "1", None, None).unwrap();
        for i in 0..5 {
            d.add_feature(
                crate::FeatureKind::Box {
                    corner: ["0".into(), "0".into(), "0".into()],
                    length: "10".into(),
                    width: "10".into(),
                    height: "10".into(),
                    operation: crate::Operation::NewBody,
                },
                Some(&format!("Box{i}")),
            )
            .unwrap();
        }
        d.features[0].param_names = vec!["".into(), "custom".into(), "".into(), "d1".into(), "d2".into()];
        d.features[1].param_names.clear();
        d.features[2].param_names = vec!["d7".into(), "".into(), "d9".into()];
        d.features[3].param_names = vec!["d7".into(), "d11".into(), "d12".into(), "d1".into()];
        d.features[4].param_names.clear();
        let mut expected = d.clone();
        for id in expected.features.iter().map(|f| f.id).collect::<Vec<_>>() {
            expected.name_feature_inputs(id);
        }
        d.name_all_feature_inputs();
        assert_eq!(d, expected);
        assert_eq!(read(&d.to_json()).unwrap(), d);
    }

    #[test]
    fn large_unnamed_timeline_loads_with_unique_inputs() {
        let mut d = Document::new("large input repair");
        let id = d
            .add_feature(
                crate::FeatureKind::Box {
                    corner: ["0".into(), "0".into(), "0".into()],
                    length: "10".into(),
                    width: "10".into(),
                    height: "10".into(),
                    operation: crate::Operation::NewBody,
                },
                None,
            )
            .unwrap();
        let mut f = d.feature(id).unwrap().clone();
        f.param_names.clear();
        d.features = (1..=5000)
            .map(|id| {
                let mut f = f.clone();
                f.id = id;
                f
            })
            .collect();
        d.next_id = 5001;
        let back = read(&d.to_json()).unwrap();
        let names: Vec<_> = back.features.iter().flat_map(|f| &f.param_names).collect();
        assert_eq!(names.len(), 15000);
        assert_eq!(names.first().unwrap().as_str(), "d1");
        assert_eq!(names.last().unwrap().as_str(), "d15000");
        assert_eq!(names.iter().collect::<std::collections::BTreeSet<_>>().len(), names.len());
    }

    #[test]
    fn versions() {
        assert_eq!(version_of("solidcraft/1"), Some(1));
        assert_eq!(version_of("solidcraft/x"), None);
        // Compatibility shim: files saved before the rename carry the legacy tag.
        assert_eq!(version_of("solvecraft/13"), Some(13));
        assert_eq!(version_of("solvecraftx/13"), None);
        let d = Document::new("A");
        assert_eq!(d.format, format_string(FORMAT));
        let back = read(&d.to_json()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn old_xz_and_nested_offsets_keep_their_frame() {
        let mut doc = Document::new("old XZ");
        let id = doc
            .add_feature(
                crate::FeatureKind::ConstructionPlane {
                    plane: crate::PlaneRef::Offset { base: Box::new(crate::PlaneRef::Origin { name: "XZ".into() }), distance: "7 mm".into() },
                },
                None,
            )
            .unwrap();
        let mut json: Value = serde_json::from_str(&doc.to_json()).unwrap();
        json["format"] = Value::from("solidcraft/5");
        let migrated = read(&json.to_string()).unwrap();
        let crate::FeatureKind::ConstructionPlane { plane } = &migrated.feature(id).unwrap().kind else { panic!("plane") };
        let (values, errors) = migrated.param_values();
        assert!(errors.is_empty());
        let p = migrated.resolve_plane(&values, plane, 0).unwrap();
        assert_eq!(p.y, solidcraft_geom::Vec3::Z);
        assert_eq!(p.normal(), -solidcraft_geom::Vec3::Y);
        assert_eq!(p.origin, solidcraft_geom::Vec3::new(0.0, -7.0, 0.0));
        assert_eq!(read(&migrated.to_json()).unwrap(), migrated);
        let fresh = doc.resolve_plane(&values, &crate::PlaneRef::Origin { name: "XZ".into() }, 0).unwrap();
        assert_eq!(fresh.normal(), solidcraft_geom::Vec3::Y);
    }

    #[test]
    fn older_upgraded_newer_refused_others_rejected() {
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["format"] = Value::from("solidcraft/1");
        assert_eq!(read(&v.to_string()).unwrap().format, format_string(FORMAT));
        v.as_object_mut().unwrap().remove("format");
        assert!(read(&v.to_string()).is_ok(), "a file without a format is format 1");
        v["format"] = Value::from(format_string(FORMAT + 1));
        let e = read(&v.to_string()).unwrap_err().to_string();
        assert!(e.contains("newer SolidCraft"), "{e}");
        for bad in ["solidcraft/0", "acad/3", "solidcraft/-1", "solidcraft/99999999999999999999"] {
            v["format"] = Value::from(bad);
            assert!(read(&v.to_string()).is_err(), "{bad}");
        }
        assert!(read("[]").is_err());
        // Format 2 body colours become body appearances.
        let mut old: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        old["format"] = Value::from("solidcraft/2");
        old["appearances"] = serde_json::json!({"Body1": [200, 10, 20]});
        let d = read(&old.to_string()).unwrap();
        let l = d.appearances.bodies.get("Body1").unwrap();
        // Format 3 joints (aligned unless flipped) keep their placement.
        let mut j: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        j["format"] = Value::from("solidcraft/3");
        j["assembly"] = serde_json::json!({"joints": [{"id": 1, "name": "J", "kind": "rigid", "a": {"occurrence": 0, "snap": {"type": "point", "point": [0, 0, 0]}}, "b": {"occurrence": 0, "snap": {"type": "point", "point": [0, 0, 0]}}}]});
        let dj = read(&j.to_string()).unwrap();
        assert!(dj.assembly.joints[0].flip, "aligned before = not mated now");
        assert_eq!((l.color, l.opacity, l.name.as_str()), ([200, 10, 20], 1.0, "Custom"));
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["huge"] = Value::from(1e300);
        assert!(read(&v.to_string()).is_err(), "out-of-range numbers anywhere");
        assert!(read("{\"format\": 2}").is_err());
    }

    /// Every sketch in the timeline is solved when the design opens, and the solver sizes its
    /// dense matrices by the sketch. The caps interactive editing enforces have to hold for a
    /// file too, before the arrays are deserialised.
    #[test]
    fn oversized_sketches_are_refused_before_typed_decoding() {
        use solidcraft_sketch::{MAX_CONSTRAINTS, MAX_CURVES, MAX_POINTS, Sketch};

        // A sketch right at the point cap, chained by coincident constraints.
        let mut sketch = Sketch::new();
        let mut previous = 0;
        while sketch.points.len() < MAX_POINTS {
            let i = sketch.add_point(solidcraft_geom::Vec2::new(0.001 * sketch.points.len() as f64, 0.0), None).unwrap();
            sketch.add_constraint(solidcraft_sketch::ConstraintKind::Coincident { p: previous, q: i }, None).unwrap();
            previous = i;
        }
        let mut doc = Document::new("big sketch");
        doc.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let at_cap: Value = serde_json::from_str(&doc.to_json()).unwrap();
        assert!(read(&at_cap.to_string()).is_ok(), "a sketch at the caps must still load");

        // One past each cap, grown at the JSON level: no typed array is ever built.
        for (key, cap) in [("points", MAX_POINTS), ("constraints", MAX_CONSTRAINTS), ("curves", MAX_CURVES)] {
            let mut value = at_cap.clone();
            let slot = &mut value["features"][0]["sketch"][key];
            let first = slot.as_array().and_then(|a| a.first()).cloned().unwrap_or(Value::Null);
            *slot = Value::Array(vec![first; cap + 1]);
            let error = read(&value.to_string()).unwrap_err().to_string();
            assert!(error.contains("sketch too large"), "{key}: {error}");
        }
    }

    fn two_components_one_occurrence() -> Document {
        let mut d = Document::new("ids");
        let a = d.add_component(None, 0).unwrap();
        d.add_component(None, 0).unwrap();
        d.add_occurrence(a, 0, crate::IDENTITY).unwrap();
        d
    }

    fn edited(d: &Document, f: impl FnOnce(&mut Value)) -> String {
        let mut v: Value = serde_json::from_str(&d.to_json()).unwrap();
        f(&mut v);
        v.to_string()
    }

    #[test]
    fn missing_or_stale_next_id_does_not_reuse_existing_ids() {
        let d = two_components_one_occurrence();
        let largest = d.components.iter().map(|c| c.id).chain(d.occurrences.iter().map(|o| o.id)).max().unwrap();
        for stale in [None, Some(0), Some(1)] {
            let json = edited(&d, |v| {
                let o = v.as_object_mut().unwrap();
                match stale {
                    Some(n) => o.insert("next_id".into(), n.into()),
                    None => o.remove("next_id"),
                };
            });
            let mut back = read(&json).unwrap();
            assert!(back.next_id > largest, "next_id {} after load with {stale:?}", back.next_id);
            let fresh = back.add_component(None, 0).unwrap();
            assert!(fresh > largest, "new component reused id {fresh}");
        }
    }

    #[test]
    fn older_auxiliary_ids_advance_the_saved_counter_before_deletion() {
        use serde_json::json;

        let origin = json!({"occurrence": 0, "snap": {"type": "point", "point": [0, 0, 0]}});
        let cases = [
            ("browser_groups", json!({"id": 100, "name": "Group", "component": 0, "folder": "bodies", "items": []})),
            ("timeline_groups", json!({"id": 100, "name": "Folder", "features": []})),
            (
                "canvases",
                json!({"id": 100, "name": "Canvas", "format": "png", "data": "", "pixels": [1, 1],
                    "plane": {"type": "origin", "name": "XY"}, "center": [0, 0], "width": 10,
                    "angle": 0, "opacity": 0.5, "flip": false, "visible": true}),
            ),
            ("joints", json!({"id": 100, "name": "Joint", "kind": "rigid", "a": origin, "b": origin})),
            ("contact_sets", json!({"id": 100, "name": "Contact", "occurrences": []})),
            ("studies", json!({"id": 100, "name": "Study", "steps": 10, "keys": []})),
            ("exploded", json!({"id": 100, "name": "View", "moves": []})),
        ];
        for (field, item) in cases {
            let json = edited(&Document::new("Legacy IDs"), |v| {
                v.as_object_mut().unwrap().remove("next_id");
                if matches!(field, "browser_groups" | "timeline_groups" | "canvases") {
                    v[field] = json!([item]);
                } else {
                    v["assembly"] = json!({field: [item]});
                }
            });
            let mut back = read(&json).unwrap_or_else(|e| panic!("{field}: {e}"));
            assert_eq!(back.next_id, 101, "{field}");
            back.browser_groups.clear();
            back.timeline_groups.clear();
            back.canvases.clear();
            back.assembly = Default::default();
            let mut reopened = read(&back.to_json()).unwrap();
            assert_eq!(reopened.allocate_id().unwrap(), 101, "{field}: deleted legacy id was forgotten");
            assert_eq!(reopened.allocate_id().unwrap(), 102, "{field}");
        }
    }

    #[test]
    fn duplicate_component_or_occurrence_ids_are_a_damaged_design() {
        let d = two_components_one_occurrence();
        let dup_component = edited(&d, |v| v["components"][1]["id"] = v["components"][0]["id"].clone());
        let dup_occurrence = edited(&d, |v| {
            let first = v["occurrences"][0].clone();
            v["occurrences"].as_array_mut().unwrap().push(first);
        });
        for json in [dup_component, dup_occurrence] {
            let error = read(&json).unwrap_err().to_string();
            assert!(error.contains("damaged design"), "{error}");
        }
    }

    /// An integer-valued coordinate is range-checked like a float one, while ids and counters
    /// may still be large integers (solidcraft#5021).
    #[test]
    fn integer_valued_coordinates_are_range_checked_and_large_ids_still_load() {
        let mut sketch = solidcraft_sketch::Sketch::new();
        sketch.add_point(solidcraft_geom::Vec2::new(40.0, 0.0), None).unwrap();
        let mut d = Document::new("range");
        d.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let last = |v: &Value| v["features"][0]["sketch"]["points"].as_array().map_or(0, |a| a.len() - 1);
        for huge in ["9223372036854775808", "4294967295", "2000000000", "-2000000000"] {
            let json = edited(&d, |v| {
                let i = last(v);
                v["features"][0]["sketch"]["points"][i]["pos"][1] = serde_json::from_str(huge).unwrap();
            });
            let error = read(&json).unwrap_err().to_string();
            assert!(error.contains("damaged design") && error.contains("out of range"), "{huge}: {error}");
        }
        // In range, an integer-valued coordinate is fine.
        let json = edited(&d, |v| {
            let i = last(v);
            v["features"][0]["sketch"]["points"][i]["pos"][1] = 1_000_000_000u64.into();
        });
        assert!(read(&json).is_ok());
        // Ids and counters are integers and may be past the coordinate limit.
        let json = edited(&d, |v| {
            v["features"][0]["id"] = 5_000_000_000u64.into();
            v["next_id"] = 5_000_000_001u64.into();
        });
        let back = read(&json).unwrap();
        assert_eq!(back.features[0].id, 5_000_000_000);
    }

    #[test]
    fn exhausted_id_space_is_an_error_not_a_panic() {
        let d = two_components_one_occurrence();
        let json = edited(&d, |v| v["next_id"] = u64::MAX.into());
        let mut back = read(&json).unwrap();
        let before = back.components.len();
        assert!(back.add_component(None, 0).is_err());
        assert_eq!(back.components.len(), before);
        let json = edited(&d, |v| v["components"][1]["id"] = u64::MAX.into());
        assert!(read(&json).unwrap_err().to_string().contains("damaged design"));
    }

    /// A sweep saved before its options existed loads as a plain sweep and saves as it was; the
    /// options round-trip.
    #[test]
    fn sweep_options_are_optional_and_round_trip() {
        use crate::{FeatureKind, SweepOrientation};
        let old =
            serde_json::json!({"type": "sweep", "sketch": 1, "profiles": {"type": "all"}, "path_sketch": 2, "path": ["l1"], "operation": "new_body"});
        let k: FeatureKind = serde_json::from_value(old.clone()).unwrap();
        let FeatureKind::Sweep { orientation, twist, scale, .. } = &k else { panic!("{k:?}") };
        assert!(orientation.is_perpendicular() && twist.is_none() && scale.is_none());
        assert_eq!(serde_json::to_value(&k).unwrap(), old);
        let old = serde_json::json!({"type": "surface_sweep", "sketch": 1, "path_sketch": 2, "path": ["l1"]});
        let k: FeatureKind = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(serde_json::to_value(&k).unwrap(), old);
        let new = serde_json::json!({"type": "surface_sweep", "sketch": 1, "path_sketch": 2, "path": ["l1"],
            "orientation": {"direction": [0.0, 1.0, 0.0]}, "twist": "90 deg", "scale": "0.5"});
        let k: FeatureKind = serde_json::from_value(new.clone()).unwrap();
        let FeatureKind::SurfaceSweep { orientation, .. } = &k else { panic!("{k:?}") };
        assert_eq!(*orientation, SweepOrientation::Direction(crate::Dir(solidcraft_geom::Vec3::Y)));
        assert_eq!(serde_json::to_value(&k).unwrap(), new);
        let parallel: SweepOrientation = serde_json::from_value(serde_json::json!("parallel")).unwrap();
        assert_eq!(parallel, SweepOrientation::Parallel);
    }
}
