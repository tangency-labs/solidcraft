# Joint Plate: a plate over a tube joint, laid out by a chosen standard (#21885)

Status: design for review. Nothing is built yet. The draft PR #21887 holds the claim.

A throwaway browser prototype of the dialog, with 20 linked cases, is on #21885. It is the
reference for behaviour described here. Its code is not reused: it is plain JavaScript and
outside the repo.

## 1. What and why

In a welded frame, members often meet at a joint where several tubes lie in one plane: a K or
KT joint in a truss, a T, an L corner or a cross. The joint is strengthened, or the members are
joined, by a flat **joint plate** laid on the coplanar faces of the tubes, on one side or both.
The plate is bolted through the tubes, riveted, screwed to timber, or welded.

Today SolidCraft has Mounting Plate (`frame.plate`, one face, a centred rectangle or round,
never drills the member) and Gusset (`frame.gusset`, a triangle between two faces). Neither
can make a plate whose outline follows several members, nor drill the tubes beneath it, nor keep
the holes and edges within the limits a fabricator has to meet.

**Joint Plate** fills that gap. The user picks the members at a joint. The command then derives:
- the outline;
- corner cut-backs and the edge set-back;
- the hole layout through plate and tubes;
- packers where tube faces aren't flush;
- the edge preparation and weld runs.

A **rule set** keeps every distance and weld size inside the limits of a chosen standard. The
first set is Eurocode (EN 1993-1-8 with EN 1090-2), the standard the owner works to.

**Success:** a fabricator picks the tubes at a K joint, chooses bolt size and rule set, and gets
a plate whose every distance is within the standard. It comes with the holes in the tubes, the
packers, and a checks list citing each clause. Changing the bolt or the members later updates
everything in one timeline step.

## 2. Goals, non-goals, anti-goals

**Goals**
- One timeline feature for the plates at one or more joints, editable as a whole (approach A,
  section 4; several joints, section 12).
- **Joints:** two to six members meeting at one point in one plane, with each member's own
  profile and reach.
- **Outline:** straight or arc edges, corner cut-backs, an edge set-back, one or both sides.
- **Fixings:**
  - bolts through both walls;
  - rivets through the near wall;
  - setscrews into a tapped near wall;
  - plug welds (plate only);
  - wood screws into solid timber;
  - no holes ("welded only").
- **Rule sets:**
  - built-in sets with their edition and a clause plus a confidence mark (✓ checked, ? not
    found in a public source) on every value;
  - Enforce or Warn only;
  - company sets that can only be stricter than the set they copy.
- **Packers:** for members whose faces aren't flush, sized from where the members really are.
  The frame's own anchors decide whether tubes line up on their centrelines or flush on the
  plate side; the plate follows (6.4).
- **Welds:** edge preparation (square, bevel on top, bevel underneath), fillet leg size, flare
  bevel welds under an overhang, and a sealing and galvanizing check.
- **Everything is a command** (AGENTS.md):
  - JSON parameters, so scripts, the CLI and the control channel all reach the same feature;
  - a read-only command that returns the checks, so the dialog, scripts and agents all see the
    same list.

**Non-goals (this design)**
- Strength design: bolt shear, bearing, block tearing, weld resistance or plate buckling. The
  rule sets are detailing and fabrication limits only. Bolt and steel grades are recorded and
  drive only the rules that use them.
- Members whose centrelines don't meet at one point (offset or gap joints). They are refused:
  "not supported yet: members must meet at one point".
- Curved members and round tubes. Square and rectangular hollow sections, and solid timber,
  come first. Round tubes have no flat face to lie on.
- Replaying the plate's member holes through later frame trims. A trim or member turn placed
  *after* a joint plate that cut member holes is refused by the existing rule ("trim it before
  the features that change it", `crates/doc/src/eval_frame_trim.rs:325–340`), exactly as after
  `solid.hole`. *My call:* accepted, and documented in the command guide.
- Weld *bodies*. Until the weld feature (#15131) and bead operations (#15130, #18500) land,
  welds are recorded as data and drawn as graphics, not modelled as solids.

**Anti-goals (would make it a failure even if it works)**
- **Claiming compliance.** The tool must never say a plate *complies* with a standard. It says
  each value is within the rule set's limits and cites the clause. Amber "?" values stay visibly
  unconfirmed. The docs say a rule set is a fabrication aid, not a design check.
- **Copying standards' text.** Rule sets hold numbers, formulas, clause numbers and at most short
  quotes. Standards are paid documents.
- **Changing existing behaviour.** Mounting Plate and Gusset keep their behaviour and their saved
  designs. No defaults change: the `Changes a default:` line stays "no".
- **Stopping the user.** With the "None" rule set only geometry is checked. Warn only never
  blocks OK.

## 3. Constraints

The repo's rules apply as written: AGENTS.md, plus the local fast-gate rules. The ones that
shape this design are below.

- **Never crash.**
  - No panics. Every input number is checked for being finite and within bounds.
  - Member counts are capped at 6, and hole scans and layouts at fixed maxima.
  - Every kernel call goes through `solidcraft_kernel::guard`.
- **Layering:**
  - outline and hole-layout geometry is pure 2D and kernel-free;
  - the feature and its evaluation live in `crates/doc`;
  - the command lives in `crates/engine`;
  - the dialog lives in `crates/ui-egui`;
  - nothing below L5 touches egui.
- **Design format:**
  - one new file, `crates/doc/src/format/pending/frame-joint-plates.rs`, modelled on
    `pending/frame-gussets.rs`;
  - older designs need no rewrite, since nothing existing changes.
- **Lists:**
  - new UI strings go in `assets/locales/pending/joint-plate.json`;
  - attribution rows, if any, in `attribution/pending/joint-plate.md`;
  - a new command icon follows `assets/AGENTS.md`, `assets/icons/ui/AGENTS.md` and the style guide.
- **WASM:** everything below the apps builds for `wasm32-unknown-unknown`.
- **Clean room:** no commercial CAD product is used for reference values. Expected values in
  tests are analytic.

## 4. Approaches considered

**A. One new command, `frame.joint_plate` (chosen).**
- **What it is:** a new feature, `FeatureKind::FrameJointPlate`. In one timeline step it makes
  the plate body (or two) and the packer bodies, cuts the holes in the plate and in the member
  bodies, and records the weld runs.
- **Why:** editing the plate keeps holes, packers and welds in step. It's built in stages
  (section 9), so each PR stays reviewable.
- **Cost:** one feature that changes bodies it didn't create. This needs care with naming and
  scope; `solid.hole` already does it through `participants`.

**B. Plate command plus ordinary Hole steps.**
- **What it is:** `frame.joint_plate` makes the plate and packers, and OK also adds `solid.hole`
  features targeted at the tubes.
- **Rejected because:** the tube holes would be separate steps that don't follow later edits of
  the plate or the bolt, which is the main thing the user wants kept together.

**C. Extend Mounting Plate.**
- **What it is:** add a "joint" outline mode to `frame.plate`.
- **Rejected because:** `frame.plate` is built around one picked face and a centred rectangle or
  round (`crates/doc/src/frame_plate.rs:90–197`). Multi-member picking, packers, member holes
  and rule sets would make it two commands in one.

The owner chose A.

## 5. What exists to build on

Found by reading the code at `origin/main` 0f7f2396e. XERJ searches confirmed the hole and
preset hits but were stale on line numbers (its index predates this checkout). The file:line
references below are from the checkout.

**Frame members**

| What | Where |
|---|---|
| `FeatureKind::FrameMember` | `crates/doc/src/document.rs:1503` |
| `MemberProfile`, `Profile::RectTube{width,height,wall,outer_radius?,inner_radius?}` | `crates/doc/src/frame.rs:49`, `:383` |
| `Profile::filled` (default outer radius 2 × wall), `Profile::size`, `Profile::region` | `frame.rs:624`, `:670`, `:696` |
| Evaluated members (`ModelState.frame_members`) | `crates/doc/src/eval.rs:1118`, lookup `:1776` |
| Member section (start plane, length, profile, open ends) | `eval_frame::member_section`, `crates/doc/src/eval_frame.rs:2094` |
| Member segments | `eval_frame.rs:2008` |
| Face to member | `end_cap::member`, `crates/doc/src/end_cap.rs:48`; `eval_frame.rs:2395` |
| Member axis | `end_cap.rs:60` |

**Siblings to model on**
- Gusset is the closest pattern: two face picks captured with `sketch::face::capture`, member
  identity kept with the plane, geometry from `solidcore_ops::frame_gusset`, then extruded.
  - Command: `crates/engine/src/cmd/frame_gusset.rs:12–40`
  - Feature: `crates/doc/src/gusset.rs:19,38,117,141`
  - Dialog: `crates/ui-egui/src/dialogs_gusset.rs`
- Mounting Plate is a second example of the same pieces.
  - Command: `crates/engine/src/cmd/frame.rs:79,130`
  - Feature: `crates/doc/src/frame_plate.rs`
  - Dialog: `crates/ui-egui/src/dialogs_frame_plate.rs`

**Holes**
- `FeatureKind::Hole`: `document.rs:1135`
- `HoleKind`: `document.rs:3246`
- `Feature.participants` limits which bodies a feature cuts: `document.rs:4001`. Evaluation is
  in `hole_targets` (`eval.rs:11600`) and the hole scopes (`eval.rs:8007,8018`).
- ISO 273 clearances: `crates/engine/src/cmd/features.rs:4185–4221`
- Tap drills: `crates/doc/src/thread_catalog.rs:210,438`
- The clearance table is in the engine crate. Joint Plate evaluation lives in `doc`, so the table
  moves down (section 7.4).

**Welds**
- No weld feature exists yet.
- Fillet beads: `solidcore/crates/solidcore-ops/src/bead.rs:22,59`. Not yet called from `crates/`.
- Spot welds: `crates/doc/src/weld_spot.rs`.
- Open work: #15131 (weld tool, PR #21664), #18500 (groove welds), #17916 (beads meeting at a
  vertex).

**Dialogs**
- Selection inputs: `SelInput` (`crates/ui-egui/src/selection.rs:94,117`)
- Rows and sections: `crates/ui-kit/src/layout.rs:215,264,284,327,559`
- Live preview by re-running the OK commands on a scratch session: `dialogs.rs:1581,6364`,
  `preview.rs:743`

**Personal rules copied into the design.** Personal Sheet Metal Rules are the exact pattern for
company rule sets.
- They live in preferences: `crates/ui-egui/src/prefs/fields/unpaged.rs:53`,
  `crates/ui-egui/src/personal_sheet_rules.rs`.
- They are copied into the design's `SheetSettings.rules` (`crates/doc/src/sheet.rs:207`), so a
  design always rebuilds without the user's preferences.
- Rule packs load from JSON with validation and bounds: `crates/doc/src/sheet_library.rs`,
  `docs/formats/sheet-rule-packs.md`.

**Profiles:** the SHS, RHS and CHS catalogue is in `crates/doc/src/frame_sizes.rs:304,430,548`.

## 6. Behaviour

### 6.1 Picking the joint

1. **Pick the plate face.** The user picks one planar face of a member: the face the plate lies
   on. It's captured as a plane (`sketch::face::capture`) with the member's identity, as Gusset
   does.
2. **Pick the members.** The user picks the members (2–6). In the dialog, picking the face
   pre-selects every member whose centreline lies in the plane parallel to that face through the
   picked member's centreline. The user can remove members or add more.
3. **Find the joint.** The joint point **J** is where the members' *sketch lines* meet,
   projected into the plate plane: the frame's own idea of the joint, as its corners use it.
   - Everything laid out on the plate (apexes, hole lines, packers) uses each member's *real*
     axis instead: the profile centre after its anchor and offset (`document.rs` ~1515–1525),
     projected into the plate plane. An anchor that shifts a tube sideways moves its holes
     with it. *My call.*
   - Projecting into the plate plane also removes depth differences, so tubes of different
     depths set flush on the plate side still meet at one J. *My call.*
   - Pre-selection in step 2 takes members whose axis is parallel to the plate plane and whose
     plate-side face lies within the deepest picked member's depth of it. The exact tolerances
     are the builder's.
   - If the sketch lines don't meet within 0.5 mm, the command refuses: "not supported yet: members must meet
     at one point".
   - A member whose centreline runs past J on both sides is a **through** member, such as a
     chord. One that ends at J is a **branch**.
4. **Each member brings** its profile (`ProfileSpec::resolve`), axis direction in the plane,
   width across the plane, depth, wall and outer corner radius (`Profile::filled`).
   - Unsupported profiles are refused: round tubes, angles, channels and I-beams.
   - Supported: rectangular hollow sections, and solid timber described as a rectangular bar
     with a timber material.

### 6.2 Outline

**Where each corner goes.** For every member end, there's a corner at that member's reach
L<sub>i</sub> from J, measured along its axis.
- Sort the member ends by angle around J. Neighbours less than 180° apart are **covered**; a
  gap of 180° or more is **open**.
- A corner sits on the member's centreline when the plate covers both sides of it.
- It sits on the member's outside edge (offset by half its width) when the plate stops at that
  member.
- Along an open gap, the outline follows the two members' outside edges to where they meet.

**Arc edges** draw one circle through all the corners: a least-squares (Kasa) fit, refused if any
corner is more than 0.5 mm off it. For an L joint the arc is centred on the outer corner.

**Set-back s** clips the outline by each outside-edge line moved inward by s.

**Corner cut-back c** turns each convex corner into an isosceles cut of height c. It's measured
along the outline at c / cos(θ/2) from the corner, and clamped to 0.45 of the distance to the
next corner, with a warning when clamped.

**Reach** is typed per member (default: one value for all), or fitted from a hole count. When
fitted, each member gets the shortest reach, in 5 mm steps, whose real hole layout gives it its
hole count. Two passes, because one member's reach moves its neighbours' edges.

The outline algorithm is in the prototype (`outline`, `clip`, `cutCorners`). Section 7.1 places
it in the code.

### 6.3 Holes

**Where a hole may go.** On each member line, a hole centre is valid when:
- it is inside the plate, at least e from the plate's edge;
- it is at least the end clearance from every other member's region (a through member ignores
  branches);
- it is within the flat part of the member's face: half the flat width less half the hole. For
  timber it's half the width less the side distance.

**Hole lines.**
- **Branch members:** holes run from the plate-edge end towards J at pitch p.
- **Through members:** holes are centred in each valid run.

**Two lines** at gauge g:
- **Opposite:** both lines start from the nearer of their starting points.
- **Staggered:** the shorter line is filled first, then the longer one at half-pitch offsets.
  The diagonal distance between lines is checked.

**Depth, by fixing:**

| Fixing | Plate | Near wall | Far wall | Far plate |
|---|---|---|---|---|
| Bolt | clearance | clearance | clearance | clearance, with both sides |
| Rivet | rivet hole | rivet hole | — | — |
| Setscrew | clearance | tapped: tap drill, plus thread data or a modelled thread | — | — |
| Plug weld | Ø of plug | — | — | — |
| Wood screw | clearance | pilot hole, about 70% of d and to a set depth | — | — |

Holes in the plate and through the member walls are modelled as cuts. The member cuts are
limited to the joint's members through the feature's participants.

**Clearances come from the rule set:**

| Rule set | Clearance |
|---|---|
| EN 1090-2 Table 11 | +1 / +2 / +3 mm |
| AISC | Table J3.3M |
| AS 4100 | +2 / +3 mm |
| Screws | ISO 273 medium series |
| Rivets | ISO 15977 Table 2 |

### 6.4 Members of different sizes: packers

- **Taken from the model, not set.** *My call* (builder check, Q1). In the prototype, alignment
  and profiles are inputs because it has no model. In SolidCraft each member's position is
  already fixed by its anchor, so the command reads it and never stores its own alignment or
  profile override. A setting that disagreed with the model would size a packer wrong, so the
  plate would float above a tube or cut into it.
- **Plate faces:** each side's plate sits on the outermost member face on that side.
- **Packer thickness:** for each member, the gap between its own face and that plane, measured
  from its real body. Centreline-aligned tubes get (deepest − own depth) ÷ 2 a side; flush ones
  get none on the flush side and the full difference on the other. Both cases follow without a
  setting.
- **Too thin to make:** a gap under 0.5 mm gets no packer; it's reported as info.
- **Packer shape:** each packer is a separate body, the member's width across, under the plate,
  stopping where a deeper member is.
- **Holes:** they go through the packers.

### 6.5 Edges and welds

**Edge preparation:** square (fillet), bevel on top, or bevel underneath. It takes bevel angle α,
land and top chamfer. Each rule set gives its limits:

| Rule set | Bevel joint |
|---|---|
| ISO 9692-1 ref 1.9.1 | β 35–60°, land 1–2 mm, plate 3–10 mm |
| AWS D1.1 BTC-P4 | α 45–55°, root face ≥ 3 mm |
| AS/NZS 1554.1 | limits still "?" |

**Fillet leg z.** The throat is a = 0.7 z.
- **EN 1993-1-8:2005:** a ≥ 3 mm (4.5.2).
- **AISC 360-22:** minimum by Table J2.4; maximum along an edge per J2.2b. A leg out of range is
  moved under Enforce, with a note.
- **Every set:** the geometric maximum is the plate thickness.

**Weld runs.**
- **Edge runs:** the parts of the plate's edge that lie on a tube face, sampled every 1 mm and
  joined across corners into continuous runs.
- **Overhang runs:** where the plate is wider than a member, the member's side edges under the
  plate get **flare-bevel** runs, or nothing with "flats only".
  - **On packed members** the run is a fillet to the packer's square edge.
  - **Flare throat:** AISC 360-22 Table J2.2 gives 5/8 R (GMAW, FCAW-G) or 5/16 R (SMAW, FCAW-S,
    SAW), with R = 2t allowed. EN 1993-1-8 Figure 7.5 defines the throat as the weld's depth
    into the groove, with no formula, so under EN the AISC value is shown as a guide only.
- **Checks:**
  - minimum run length: EN max(30 mm, 6a), AISC 4 × leg;
  - a fillet's toe must not reach the tube's corner radius (EN 10219-2 Table 3, largest radius);
  - painted laps left open (ISO 12944-3 5.3);
  - galvanized sealed overlaps over 10,000 mm² need vents or a 2.5 mm gap (the galvanizers'
    guidance, quoting ISO 14713-2).

Welds are stored as data in the feature: run polylines in plate coordinates, with type, size,
process and side. They're drawn as graphics in the 3D view, solid along edges and dashed under
overhangs. Section 9 stage 6 makes them bodies once the weld feature exists.

### 6.6 Rule sets

**What a rule set is.** A named, versioned table of limits, each limit carrying:
- a value or formula;
- the clause;
- the edition;
- a confidence mark: ✓ for checked against the standard's text or a public source quoting it,
  ? for not found.

**Built-in sets**, in build order:

| # | Set | Contents |
|---|---|---|
| 1 | None | geometry only |
| 2 | **Eurocode**, EN 1993-1-8:2005 + EN 1090-2:2018 | the owner's standard; built and tested first |
| 3 | AISC 360-22 + AWS D1.1:2025 | |
| 4 | AS 4100:2020 + AS/NZS 5131 | |
| 5 | Timber, EN 1995-1-1:2004 + A1 + A2 | screws |

The values, clauses and confidence marks are listed in **Appendix A**, copied from the standards
comparison page made during the design. Appendix A is the authority; where the prototype
disagrees, Appendix A wins. A value marked ? ships marked ? and is never shown as checked.

**Inputs the rule sets use**, beyond the fixing (all in the prototype, so in scope):
- execution class EXC1–EXC4 (Eurocode; recorded, and shown with the punching rule);
- edge preparation grade P1–P3 (ISO 8501-3; P3 sets the top-edge radius);
- edge cutting: sheared or hand flame-cut, machine-cut, rolled (AS 4100 9.5.2 edge factor);
- holes drilled or punched (EN 1090-2 6.6.3 limit);
- plate material: steel, or plywood on timber (EN 1995-1-1 8.3.1.3 spacings × 0.85).

Eurocode set, in full:
- **Spacing and edge distance (EN 1993-1-8 Table 3.3):**
  - e₁, e₂ ≥ 1.2 d₀;
  - e ≤ 4t + 40 when exposed;
  - p₁ ≥ 2.2 d₀, p₂ ≥ 2.4 d₀;
  - p ≤ min(14t, 200);
  - staggered rows: gauge 1.2 d₀ with every pair ≥ 2.4 d₀ apart (note 5).
- **Distance to a tube end:** e₁ is measured "to the adjacent end of any part" (1.5).
- **Holes (EN 1090-2:2018):** clearances from Table 11; punching allowed when t ≤ 1.4 d₀
  (6.6.3), with its note on fatigue, slip-resistant and lap joints above 8.8.
- **Setscrew engagement (EN 1993-1-8:2024 Table 5.10):** by bolt class and steel grade, M12–M36.
- **Edge finish (ISO 8501-3):** grades P1–P3, with P3 needing a radius of at least 2 mm.
- **Welds:** the fillet and flare rules in 6.5.

**Enforce or Warn only.**
- **Enforce:** values outside the limits are refused, and OK is disabled while any check fails.
  When an upstream choice changes, such as the bolt size, dependent values move up to the new
  minimums, with a note per value.
- **Warn only:** out-of-range values are kept and marked.
- **None:** checks geometry only, such as "hole must stay inside the plate".

**Presets:** Minimum, Typical (about 1.2–1.5 × the minimum) and Custom, for e, p, g and end
clearance. The preset lives in the dialog only. The feature stores the resulting numbers plus the
preset's name for display. A rebuild never re-applies a preset, so it never moves geometry the
user didn't touch. *My call.*

**On rebuild** (an upstream edit, such as a thinner wall, pushes a stored plate past its rules).
*Decided by the owner:*
- **Enforce:** the feature fails with the first error's text and the fix, for example "edge
  distance 22 mm is under 1.2 d₀ = 26.4 mm (EN 1993-1-8 Table 3.3): edit the plate, or switch
  to Warn only". This is the existing failed-feature path (`crates/doc/src/eval.rs:4730`): the
  feature is marked failed, and the rest of the design still builds. It's consistent with
  creation, where Enforce refuses too.
- **Warn only:** builds, and the checks list the breaks.
- **None:** builds unless the geometry is impossible.

**Company sets.**
- **How they're made:** copied from a built-in set. They can only tighten it:
  - edge-distance and pitch factors of at least 1;
  - restricted bolt sizes and plate thicknesses;
  - punching off.
- **Where they live:** in preferences, as Personal Sheet Metal Rules do.
- **In the design:** using a set copies its resolved values into the feature, with the base set's
  id and edition. The design rebuilds identically on another machine, and a later edition of a
  built-in set doesn't silently change an existing plate.
- **Sharing:** export and import as a small JSON file, validated and bounded as rule packs are.

**Bolt grade and tube steel.** Recorded per set, because Eurocode rules use them:
- EN: 4.6–10.9 with S235–S460, for Table 5.10 and the punching note;
- AISC: A307, A325, A490 with A500C, A1085, A847, where A847 bare tightens the maximum pitch to
  14t ≤ 180;
- AS: 4.6/S to 8.8/TF with C250L0–C450L0.

### 6.7 Checks

`frame.joint_plate.check` is a read-only command. It takes either the dialog's current parameters
or an existing feature, and returns the checks list.

Each check gives:
- a level: ok, info, warn or error;
- its text;
- the rule's clause, edition and confidence, where a rule applies.

The dialog's Checks section, scripts and the control channel all use this one list. Under
Enforce, `frame.joint_plate` itself fails with the first error's text when any check is an
error, so a script can't create a plate that breaks its own rule set.

## 7. Where the code goes

### 7.1 Layout geometry

Outline, set-back, cut-backs, arc fit, hole layout, reach fitting, weld runs, overhang runs,
packer extents, contact areas.
- **Crate:** a new kernel-free module, next to `frame_gusset` in solidcore-ops
  (`solidcore/crates/solidcore-ops/src/joint_plate.rs`) if that crate's rules allow plain 2D
  code. Otherwise in `crates/geom`. *Builder's call*, after reading
  `skills/solidcraft-layers/SKILL.md` and the existing primitives there (polygon clip, offset,
  point-in-polygon).
- **Inputs and outputs:** it takes plain numbers (member directions, widths, flat widths, reaches,
  e, p, g, clearances) and returns plain 2D results. It knows nothing about rule sets.

### 7.2 Feature and evaluation (`crates/doc`)

`FeatureKind::FrameJointPlate` in `document.rs`, with its own module
`crates/doc/src/joint_plate.rs`.

**What it stores:**
- a list of joints (1 to a fixed maximum), each the plate plane (`PlaneRef`) and its member
  identities, as Gusset stores them (section 12);
- per-member reach or hole count (no profile override or alignment: both come from the model,
  6.4);
- outline settings: edges, t, sides, c, s;
- fixing, size, grade, lines, pattern, e, p, g, end clearance (as numbers, plus the preset name
  for display), pilot depth, hole chamfer;
- execution class, edge preparation grade, edge cutting, holes drilled or punched, and plate
  material;
- the edge preparation, fillet leg, overhang welds, process and finish;
- the **resolved rule-set snapshot**: id, name, base id, edition, values;
- the mode (Enforce or Warn only).

**What evaluation does:**
1. Resolves the members from `ModelState.frame_members`.
2. Calls the layout geometry.
3. Builds the plate bodies by extruding the region with holes, adding the bevels and chamfers on
   the edges.
4. Builds the packer bodies.
5. Cuts the member holes, limited to the members through participants.
6. Stores the weld runs for display.

All kernel calls go through `guard`.

**Body names:** `Joint plate`, `Joint plate (2)`…, with packers named `Joint plate packer 1`….
`body_name` is supported as on Gusset.

### 7.3 Rule sets (`crates/doc`)

`crates/doc/src/joint_rules.rs` holds:
- the built-in sets as data;
- the resolved-snapshot type stored in the feature;
- the limit functions, such as e_min(d₀) and clearance(d);
- validation of company sets: only stricter, finite, bounded.

The JSON exchange format is documented in `docs/formats/joint-rule-sets.md`.

### 7.4 Shared fastener data

ISO 273 clearances move from `crates/engine/src/cmd/features.rs:4185` into `crates/doc`, re-exported
for `solid.hole`, so both use one table with unchanged values. Tap drills are already in
`thread_catalog.rs`.

### 7.5 Command (`crates/engine`)

`crates/engine/src/cmd/frame_joint_plate.rs`:
- `frame.joint_plate`, label "Joint Plate", WELDMENTS → CREATE beside Gusset;
- `frame.joint_plate.check`, the read-only checks.

The JSON parameters mirror the stored fields. Picks are `{face, body, index, path?}` as on Gusset,
plus `members`: picks on the members (each resolved to its Frame Member feature and line, as `TrimMemberRef` does, never kept as a body name). Unknown keys are refused, as `frame_gusset.rs:22–26` does. A hostile
parameters test joins `hostile_params_never_panic`.

### 7.6 Dialog (`crates/ui-egui`)

`dialogs_joint_plate.rs`, laid out like the prototype.

**Sections:**

| Section | Contents |
|---|---|
| Joint | face and member picks, members table |
| Rules | set, Manage…, set-specific options, Enforce / Warn |
| Outline | edges, reach mode, reach, t, sides, c, s |
| Holes and fasteners | holes: none / one / two lines, fixing, sizes, spacing |
| Edges and weld | preparation, bevel, fillet leg, overhang welds, process, finish |
| Checks | the list from `frame.joint_plate.check` |

**How it behaves:**
- Values apply when a field is left, and each value's limits show under it with a clause chip.
- **Live preview:** the existing mechanism, which re-runs the OK command on a scratch session.
  It shows the plate, the packers and the hole cuts in the 3D view.
- **Fabrication Rules dialog** (Manage…): lists every set's rules and clauses, and copies a
  built-in set into a company set.

**Not in the first dialog:** the prototype's separate plan view and Section A–A inset. The 3D
preview with a section view covers it. A dedicated section inset is follow-up work if reviewers
miss it.

## 8. Testing

**Geometry: analytic cases, with exact expected coordinates.**
- **Outline:** the K-joint trapezoid; an L triangle and its arc centred on the outer corner;
  the T and cross arcs.
- **Hole positions:** for the K joint with e = 30, p = 60. Expected values are worked out by
  hand and match the prototype.
- **Staggered fill order.**
- **Reach fitting:** for hole counts 2, 3 and 4.
- **Packer thicknesses:** both alignments.
- **Weld run lengths:** on a square plate over a square tube.

**Rule sets**
- Every built-in limit function at two sizes.
- Company-set validation refuses anything looser.
- A changed bolt size moves dependent values under Enforce.

**Command**
- JSON round trip.
- Unknown keys refused.
- An offset joint refused.
- Round tubes refused.
- `hostile_params_never_panic` additions.
- Enforce refusing a script value that breaks the rules.

**Evaluation**
- Plate and packer bodies have the expected volume (analytic).
- The member holes cut only the joint's members (participants).
- A design saved and reloaded rebuilds identically.

**Format:** the pending file and its round-trip test.

**Evidence for PRs:** screenshots from the running app (AGENTS.md), on the cases from #21885.

## 9. Build stages

Each stage is its own PR. They reference #21885 or a sub-issue for each, as the owner prefers.
#21887 carries stage 1.

| # | Stage | Contents |
|---|---|---|
| 1 | Layout geometry | 7.1 with its tests. No feature. |
| 2 | Feature, command, plate | Plate, packers and plate holes with the None rule set; format file storing a list of joints; `frame.joint_plate` and `.check`; no member cuts. |
| 3 | Dialog | Picks, members table, sections, live preview, checks list. |
| 4 | Rule sets, Eurocode first | The rule-set machinery, Enforce / Warn, presets, company sets in preferences, the Fabrication Rules dialog, export and import. |
| 5 | Holes through the members | All fixings: bolt, rivet, tapped setscrew, plug weld, wood screw. |
| 6 | Welds | Edge preparation geometry on the plate. Weld runs and flare runs as data and graphics. Weld bodies once #15131 / #15130 land, coordinated with them. |
| 7 | More rule sets | AISC + AWS, AS + AS/NZS, timber EN 1995, each with its own tests. |

Docs grow with each stage: a Joint Plate section in `docs/frames.md`, a command guide, and the
rule-set format page.

## 10. Decided by the owner

- **Several joints:** one feature holds a list of joints with shared settings; only the ticked
  joints get plates; both faces stay in one feature (section 12).
- **Rebuild under Enforce:** a plate that an upstream edit pushes past its rules fails, with
  the reason and the fix; the rest of the design still builds (6.6).
- **Approach A:** one feature that makes the plate, packers, holes through plate and tubes, and
  weld record.
- **First rule set:** Eurocode, EN 1993-1-8 + EN 1090-2.
- **Behaviour as in the prototype:**
  - outline rule, isosceles cut-backs, parallel set-back;
  - hole rules: start at e from the edge, never cut a tube end, opposite or staggered, shorter
    line first;
  - bevel either way;
  - fixings and hole depths: both walls, near wall, tapped, timber screws;
  - per-member profile and reach;
  - packers, with the flush option (flush now comes from the frame's anchors, not a setting: 6.4);
  - welded-only plates;
  - fillet leg;
  - flare welds;
  - bolt grade and steel grade fields.
- **No pirated standards:** values come only from legitimate public sources, and unconfirmed
  ones stay marked "?".

## 11. Left to the builder

**Calls I made after the builder check** (marked *My call* where they appear):
- Profile, alignment and packer thickness come from the model, not from settings (6.4, 6.1).
- J comes from the sketch lines; the layout uses each tube's real axis (6.1).
- Presets are dialog-only; the feature stores numbers (6.6).
- Trims after a joint plate are refused, as after `solid.hole` (2).
- Hole and end-distance checks use the real, trimmed member ends.
- Members are identified by feature and line (`TrimMemberRef`), never by body name.
- A timber member is one whose material family is `Family::Wood`.
- A new plate starts on the None rule set.
- t in EN Table 3.3 is the thinner of the plate and the tube wall.
- One format file per stage that saves new fields.
- Stage PRs reference #21885 without "closes" until the last one.

**Also left to the builder:**

- The exact crate for the layout geometry (7.1), after reading the layer skill.
- Internal type names and how the stored fields are grouped.
- How the weld graphics are drawn in the 3D view, using the existing overlay mechanisms.
- The icon design, within the asset rules.
- Splitting a stage further if a PR gets too large.

## 12. Plates at several joints

The design above makes one plate (or pair) at one joint. A truss has many joints that want the
same plate. What the code already offers:

- **A frame already knows its joints.** `frame_corners::find_corners` (two ends meet),
  `find_crowded` (three or more ends meet) and the layout's `Fit` (a brace end fitted against a
  chord that runs through, the usual K or N joint) (`crates/doc/src/frame_corners.rs:169,186`,
  `frame_layout.rs:448`). A joint plate's joints could be found from these rather than from
  loose picks.
- **A frame already names a joint stably.** `CornerOverride` (`frame_corners.rs:73`) names a
  corner by the line ids of the members that meet there, so it survives renames and edits.
- **One feature with many places has a precedent.** `FrameEndCap` keeps `ends: Vec<End>`, up to
  `MAX_FACES` = 256, and makes one body per end (`crates/doc/src/end_cap.rs:14`).

Options:

1. **One feature per joint (the design as written).** Repeat the command for each joint. It's
   the simplest, but changing the bolt size on a 20-joint truss means 20 edits, unless the user
   ties the values to parameters.
2. **One feature, many joints, shared settings.** Like End Cap: one rule set, thickness, bolt
   and edge settings, and a list of joints, each stored as the plate plane plus its members'
   line ids. The dialog adds "All joints in this plane": pick a face, and every joint of the
   frame lying in that plane with 2–6 members is listed, each one with a tick box. Joints of
   different shapes each get their own outline from the same rules. Per-joint overrides (for
   example a different reach on one member) can come later in the `CornerOverride` style.
3. **A frame-wide setting on Frame Member.** "Plate every joint", with per-joint overrides,
   like corner types. It's the most automatic, but it pulls plate, holes and packers into the
   frame build (`eval_frame.rs`). Rejected for the same reason as approach C: it makes one
   command into two.

**Decided by the owner: option 2.**
- **Stored as a list from the start.** Stage 2 stores a list of joints, even while the first
  dialog fills in only one. Moving from one joint to many later would need a design-format step.
- **Shared settings first.** Every joint in one feature uses the same rule set, thickness,
  fixing and edge settings. Each joint still gets its own outline, reach fitting and packers
  from its own members. Per-joint overrides (for example a different reach on one member) can
  come later in the `CornerOverride` style, as a new optional field.
- **Both faces in one feature.** As before, one side or both. The sides apply to every joint in
  the list.
- **Only the ticked joints.** "All joints in this plane" fills the list once, when picking. A
  later frame edit that adds a joint in the plane doesn't add a plate there. The user re-opens
  the feature and ticks it. This is predictable, and it follows the rule that a rebuild never
  changes what the user didn't touch.
- **A joint that disappears.** A frame edit can remove a joint, for example when a member is
  deleted. The feature then fails with the joint's member lines named ("joint of b1, b2, c1 no
  longer meets"), through the usual failed-feature path. The user removes the joint from the
  list or reselects its members.
- **Bodies:** one plate body per joint and side, named `Joint plate`, `Joint plate (2)`… in
  list order, with each joint's packers after its plates.
- **Bounds:** at most 64 joints per feature, checked on input and on load, as End Cap does
  with its `MAX_FACES`. *My call.*

**Dialog** (stage 3): the Joint section lists the joints, each with a tick box and its member
names. Picking a face adds every joint of that frame lying in the plane with 2–6 members.
Picking members directly adds one joint.
## Appendix A. Rule-set values

d₀ is the hole diameter, d the bolt diameter, t the thinnest plate in the joint. ✓ means checked
against the standard's text or a public source quoting it; ? means no public source was found, so
the value must be checked against a bought copy. These are numbers and clause references only,
not the standards' text. Every value must be signed off against a bought copy of the current
edition before a set ships as anything other than "draft".

### Steel

| Rule | Eurocode: EN 1993-1-8:2005 + EN 1090-2:2018 | US: AISC 360-22 + AWS D1.1 | Australia: AS 4100:2020 + AS/NZS 5131 |
|---|---|---|---|
| Clearance hole | +1 mm M12–M14, +2 mm M16–M24, +3 mm M27 up · 1090-2 6.6.1 T11 ✓ | M12→14, M16→18, M20→22, M22→24, M24→27, M27→30, M30→33, d+3 from M36 · T J3.3M ✓ | d+2 to M24, d+3 above · AS 4100:1998 14.3.5.2 ✓; since 2020 in AS/NZS 5131 6.7 ? |
| Min edge distance | 1.2 d₀ · T3.3 ✓ | M12 18, M16 22, M20 26, M22 28, M24 30, M27 34, M30 38, M36 46, then 1.25 d · T J3.4M ✓ | 1.75 d sheared/hand flame-cut, 1.5 d machine-cut/sawn/planed, 1.25 d rolled · 9.5.2 ✓ |
| Distance to a member's end | e₁ to "the adjacent end of any part" · 1.5 ✓ | edge rule applied to the tube end ? | edge rule applied to the tube end ? |
| Max edge distance | 4t + 40 exposed; none otherwise · T3.3 ✓ | 12t ≤ 150 · J3.6 ✓ | 12t ≤ 150 · 1998 9.6.4 ✓ |
| Min pitch | 2.2 d₀ along, 2.4 d₀ across · T3.3 ✓ | 2⅔ d (3 d preferred) · J3.4 ✓ | 2.5 d · 9.5.1 ✓ |
| Max pitch | 14t ≤ 200 · T3.3 ✓ | 24t ≤ 300 painted; 14t ≤ 180 bare weathering · J3.6 ✓ | 15t ≤ 200 · 1998 9.6.3 ✓ |
| Staggered rows | gauge 1.2 d₀ if every pair ≥ 2.4 d₀ · T3.3 note 5 ✓ | 2⅔ d between centres ✓ | 2.5 d between centres ✓ |
| Punched holes | t ≤ 1.4 d₀; unreamed unsuited to fatigue, slip-resistant, lap joints above 8.8 · 1090-2:2018 6.6.3 ✓ | no thickness limit · M2.5 → RCSC 3.3 ✓ | AS/NZS 5131 6.7 ? |
| Weld preparation | single bevel β 35–60°, root face 1–2, gap 2–4, t 3–10 · ISO 9692-1:2013 ref 1.9.1 ✓; plate flat on a tube face ? | BTC-P4: 45° (+10/−0), root face ≥ 3, no gap · D1.1:2025 Fig 5.2 ✓ | AS/NZS 1554.1 4.5, App E ? |
| Fillet weld size | a ≥ 3 · 4.5.2 ✓; length ≥ max(30, 6a) · 4.5.1 ✓; no max | min leg 3/5/6/8 by thinner part · T J2.4 ✓; max t under 6, t − 2 from 6 · J2.2b ✓; length ≥ 4 × leg ✓ | min by table, ≤ thinner part · 9.6.3.2 ? |
| Flare-bevel weld | a = depth into the groove, face line to root, no reinforcement · 4.3.6, 7.3.1(7), Fig 7.5 ✓; weld all round · 7.3.1(2) ✓ | 5/8 R (GMAW, FCAW-G), 5/16 R (SMAW, FCAW-S, SAW), R = 2t for HSS; R < 10: reinforcing fillet only · J2.1a, T J2.2 ✓; BTC-P10 ✓ | none found ? |
| Sealing the lap | painted: seal by continuous welds · ISO 12944-3:2017 5.3 ✓. Galvanized: ≤ 10,000 mm² fully welded; ≤ 100,000 mm² 2 vents Ø12+ (or 2 × 25 mm gaps); ≤ 250,000 mm² 4; beyond, Ø20 corners plus more; or a 2.5 mm gap · GAA Design Manual T1, UK galvanizers on ISO 14713-2 ✓ (secondary) | (same) | (same) |
| Edges for coating | P1–P3; P3 radius ≥ 2 · ISO 8501-3 T1 ✓ | by specification | P1–P3 · AS/NZS 5131 9.8.4 ✓; radius ? |
| Setscrew engagement | ≥ 1.0–1.58 d by class and grade (8.8 in S355: 1.11 d), M12–M36 · EN 1993-1-8:2024 5.9.3 T5.10 ✓ | none found ? | none found ? |
| Tube corner radius | SHS outside 1.6–2.4 T (T ≤ 6), 2.0–3.0 T (to 10), 2.4–3.6 T (over 10) · EN 10219-2 T3 ✓ | (same) | (same) |

### Timber: EN 1995-1-1:2004 + A1 + A2, screws into solid wood

| Rule | Value |
|---|---|
| Small and large screws | d_ef ≤ 6: nail rules (8.3.1); larger: bolt rules (8.5.1); spacings use outer thread d · 8.7.1 + A2 ✓ |
| Nail rules, pre-drilled, along the grain | a₁ 5 d, a₂ 3 d, loaded end 12 d, unloaded end 7 d, edge 3 d · T8.2 ✓ |
| Bolt rules | a₁ 5 d, a₂ 4 d, loaded end max(7 d, 80), edge 3 d · T8.4 ✓ |
| Steel plate on timber | nail spacings × 0.7, end and edge unchanged · 8.3.1.4 ✓ |
| Plywood plate on timber | spacings × 0.85; plywood edges 3 d unloaded, (3 + 4 sin α) d loaded · 8.3.1.3 ✓ |
| Pilot hole | above 6 mm and all hardwoods; threaded part about 70 % of shank · 10.4.5 ✓ |
| Clearance in the plate | 5→5.5, 6→6.6, 8→9, 10→11 · ISO 273 medium ✓ |

### Newer editions

- EN 1993-1-8:2024 moves hole positions to 5.8; its values are in the paid text only, so the set
  stays on 2005 with the 2024 Table 5.10 added.
- EN 1090-2:2018+A1:2024 keeps 6.6.1–6.6.3; whether A1 changed the values isn't public.
- AWS D1.1:2025 redrew the prequalified joints with dimensions unchanged.
- EN 1995-1-1:2025 moves screw spacings to 11.4 and pre-drilling to EN 1995-3:2025; the 2004
  edition stays valid until national withdrawal (by early 2028).
- AS 4100:2020 renumbered bolt detailing from 9.6 to 9.5; the max pitch and max edge values
  above are the 1998 ones.
