# GD&T records and original symbols (W28a, #7257)

> **Provisional drafting geometry — standards compliance outstanding.** Missing
> construction details use educated original approximations. Characteristic shapes,
> circled modifiers, lettering, ASME diameter and ASME frame/label conventions must
> be checked and changed where necessary against the applicable current standards
> before this output is described as compliant or used as compliant manufacturing
> annotation. Passing tests establishes the supported behavior and geometry
> invariants, not normative conformity.

`solidcraft_geom::gdt` supplies single-row feature-control frames and boxed datum
identifiers as data, text transcriptions and local vector geometry. It depends
only on the existing geometry, serde and error types, with no UI, operating-system
API, font or new dependency. It can be used on wasm.

A caller explicitly selects `iso1101_2017_iso5459_2024` or `asme_y14_5_2018`.
These select the ISO 1101:2017 / ISO 5459:2024 pair or ASME Y14.5-2018 (R2024).
The family and edition travel with each record; conversion is never automatic.

## Supported grammar

- ISO's fourteen characteristics; ASME's twelve, rejecting concentricity and
  symmetry under the supported 2018 edition.
- One tolerance compartment: width, diameter or spherical diameter, a value in
  millimetres, optional maximum/least material modifier and optional projected
  height. No arbitrary symbol strings or mutually conflicting material modifiers.
- A surface or derived feature of size is explicitly recorded. Roundness,
  cylindricity, profiles and runout control surfaces in this subset. Position,
  concentricity/coaxiality and symmetry control derived size features here.
- Width is supported for surfaces; diameter for derived straightness, orientation,
  position and ISO concentricity/coaxiality; spherical diameter only for position.
  Flatness and symmetry use width. The supported ISO concentricity/coaxiality
  indication uses diameter. These are subset restrictions, not a complete account
  of every standard's toleranced-feature and zone combination.
- Maximum/least material on the tolerance requires a derived size feature and
  straightness, flatness, orientation or position. ISO means material requirement;
  ASME means material condition. On datum references, ASME means material boundary;
  ISO retains material-requirement semantics (whose evaluation is ISO 2692 work).
  An omitted modifier retains the selected family's default. No circled S is used.
- Positive tolerance/height values range from 1e-9 to 10000 mm. Zero tolerance is
  supported at maximum material only. These bounds limit resources and text size;
  they are not normative limits. Negative zero transcribes as zero.
- An in-frame projected height is supported for ASME position with a width or
  diameter zone. ISO's external projected-height dimension is not treated as an
  ASME in-frame height: this field is rejected under ISO. ISO projected zones
  await the dimension/attachment work.
- Zero to three ordered, single-datum references. Form controls take no datums;
  orientation, runout, concentricity and symmetry require one to three. Position
  and profiles can be datum-free. Datum-free position is accepted and returns
  `DatumFreePositionNeedsPatternContext` for the later drawing checker, following
  the plan's instruction not to reject all datum-free position specifications.
- Datum identifiers are one or two uppercase ASCII letters excluding I, O and Q.
  This is a conservative supported alphabet, not a claim that ISO excludes all
  three letters. Ordered references cannot repeat an
  identifier. Common/multiple-feature datums
  such as A-B require a future structured grammar and are rejected here.

Every text/layout boundary revalidates the record, including deserialized input.
Unknown serde fields and enum variants are errors. Encoded-input callers must
still bound bytes and nesting before deserialization. Validation does not establish
that cited datums exist, are unique within the part, are size features when a
material modifier requires that, or constrain the intended degrees of freedom.
Those checks need a document/model context absent from this foundation slice.

## Geometry and text

`Frame::compartments` returns reading-order cells. `Frame::text` joins them with
` | `, for example `⌖ | ⌀0.1Ⓜ | A | B | C`. This is a transcription, not a
parseable interchange format. Values retain their shortest round-trip decimal
form in millimetres, without precision rounding or locale substitution.
`DatumIdentifier::text` returns `[A]` for A.

`layout` returns paper `Item` strokes/fills, size and compartment bounds, in
millimetres from the frame's local lower-left corner. It does not create a paper
scene group or choose a layer. Existing writers can consume these primitives once
the sheet annotation layer groups and places them. No view scale enters the API.
A datum layout is its boxed label only; its connecting line and triangle belong
to the deferred leader/attachment layer.

All geometry is original code: lines, analytic circular arcs and filled runout
arrowheads, plus a small original single-stroke alphabet for values and datum
letters. Characteristic and modifier Unicode characters are transcriptions only;
no font glyph is used to draw a symbol. No standard's figures, fonts, text or
commercial CAD assets/code were copied.

Lettering height h is 1.8–20 mm, stroke width h/10, frame height 2h, first cell
square, following cells content-sized with horizontal ink clearance of at least
max(2h/10,0.7 mm). Padding additionally allows for the frame's half stroke width
and the lettering's worst permitted miter extension, so the clearance applies to
visible ink rather than centre lines.
A single-letter datum label is square. Bounds describe stroke centre lines;
painted ink extends h/20 outside the outer frame. The frame/nominal-height/line-width
ratios, square first compartment and horizontal clearance were checked in part
against ISO 7083:2021. This does not verify the painted height or construction of
the original alphabet. The custom 1.8 mm height is a supported layout policy,
outside the recommended sizes used for this frame layout.

The ISO diameter prefix was also checked in part against ISO 7083:2021. With
d=h/10, the circle's centre-line diameter is 6d and the diagonal spans 3d
horizontally and 9d vertically. Analytic filled circles of radius d/2 give its
round ends without changing paper's butt-cap stroke contract. Its painted width
is 7d and height is 10d. These are mathematical proportions used in original code;
no standard artwork is extracted or shipped. The ASME diameter construction stays
separate and provisional pending review, rather than inheriting ISO proportions.
At h=3.5 mm, the example `⌀0.1Ⓜ` compartment is 17.5 mm wide under ISO and 18.2 mm
in the provisional ASME layout; the five-cell ISO example is 40.25 × 7 mm.

## Provisional proportions and replacement points

Unverified details are intentional temporary approximations, not completed
standards work. They retain recognizable characteristic forms, consistent stroke
weight and bounded geometry. They are original code; no standard artwork, fonts
or CAD assets were copied.

| Detail requiring review/correction | Adjustment point | Verification still needed |
| --- | --- | --- |
| All fourteen characteristic constructions (twelve under ASME) | `crates/geom/src/gdt/geometry/provisional.rs`, `Pen::characteristic`, one named match arm per characteristic | Current ISO 7083 characteristic grids; ASME Y14.5 construction requirements |
| Circled M/L/P dimensions and placement | Same module, `MODIFIER_*` constants | Family-specific circle, internal-letter and spacing proportions |
| ASME diameter prefix | Same module, `ASME_DIAMETER_*` constants | ASME Y14.5/Y14.2 construction requirements |
| Original alphabet, decimal mark, widths and inter-character advance | Same module, `Pen::letter`, `LETTER_WIDTH_H`, `DECIMAL_WIDTH_H`, `CHARACTER_GAP_H` | ISO 3098 lettering construction/spacing and ASME Y14.2 requirements |
| ASME frame/boxed-label proportions; current ISO boxed-label conventions | `crates/geom/src/gdt/geometry.rs`, `frame`, `datum`, `inset`, `Pen::stroke` | ASME Y14.5/Y14.2 and ISO 5459:2024 label dimensions |

All construction coordinates and metrics in the provisional module are normalized
to h. Change coordinates and their width/advance metrics together. Characteristic
and lettering guesses currently share a construction between the families; branch
on `self.standard` there when verified ISO and ASME rules differ. No persisted
record, semantic validation, paper schema or UI change is needed to replace these
proportions. Keep unsupported semantic combinations rejected rather than guessing
new grammar.

`geometry.rs` keeps the independently checked ISO diameter grid separate; do not
replace it with the ASME approximation. Its frame ratios and clearance have the
ISO 7083 checks described above, while reuse for ASME remains provisional.
The current ISO 5459 datum indicator's connecting line and triangle remain deferred;
the boxed label alone is not a complete datum feature indicator.

The original alphabet has not been verified against ISO 3098. In particular,
the current alphabet uses h as its centre-line height and a 0.2h
centre-line gap. ISO 3098-1 distinguishes painted height/spacing from centre-line
height/spacing (h1=h-d, a1=a+d). Alphabet geometry and advances therefore need
correction or verification together; the current preview must not be presented as
verified Type B lettering. The 1.8 mm height is included among ISO 3098-1's Type B
sizes, despite being outside the recommended heights used for this frame layout.

ISO 2692:2021 supports material requirements on derived linear features of
size, including coaxiality/symmetry cases beyond this implementation's conservative
subset. Existing restrictions deliberately remain narrower; actual datum/feature
eligibility, material evaluation and degrees of freedom need model context.

After replacing provisional details, rerun geometry/clearance/scaling and hostile
input tests, inspect output from actual Rust primitives, and run the required full
CI. A green test suite by itself cannot certify the new proportions. Drafting
review remains outstanding before claiming graphical standards conformity or
manufacturing completeness.

## Boundaries

Issue #7257 defines this below-UI foundation. ISO 1101:2017, ISO 5459:2024,
ISO 7083:2021 and ASME Y14.5-2018 (R2024) remain distinct; the provisional
details above are not a complete verification against any of them.

No sheet placement, leader, view/dimension attachment, datum target, composite or
stacked frame, model evaluation, document persistence or UI/engine command is
introduced. Tangent-plane/free-state modifiers, unequal profiles, common datums,
auxiliary plane indicators and ISO 5459:2024's advanced datum operators also await
separate validated grammar. Unknown variants fail rather than being approximated.
