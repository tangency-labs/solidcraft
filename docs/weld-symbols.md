# Welding-symbol records (W30b, #7259)

`solidcraft_geom::weld` provides manual welding requirements, semantic text and original local
symbol geometry. It needs no weldment, bead, model or active document. This foundation stays
below the UI: no commands, sheet placement, leaders, view attachment or saved drawing schema.
It does not implement the model-side weldment work in #4003 or symbol families #7257/#7258.

## Contract

`Weld` has independent optional `arrow_side` and `other_side` records, all-round and field
marks, and an optional single-line tail. Each `WeldSide` has a kind, an optional typed size,
nominal length, optional intermittent centre-to-centre pitch and element count, and a contour.
Supported elementary shapes are fillet, square butt and single-V butt.

Fillet sizes distinguish leg, nominal throat and deep-penetration throat. A deep-penetration
size stores **both** `total_mm` and `nominal_mm`, with total greater than nominal. Butt
penetration is a different size kind. Values are never converted between size meanings.
The straight-face contour is `Contour::Flush`: ISO calls it flush, while AWS distinguishes
flat fillet faces from flush groove faces. Convex and concave faces are separate choices.
No finishing operation, grinding allowance or surface-roughness requirement is inferred.

The tail is author-supplied text, including any WPS/process/quality references; it is not a
validated ISO 4063 or ISO 5817 catalogue. A tail-only note supports details elsewhere without
inventing an elementary weld requirement. Missing dimensions are not autofilled. Callers
must account for standard interpretations of omitted dimensions: for example, a butt symbol
without a penetration dimension normally specifies full penetration unless other information
governs it; a missing length normally means the joint length. This record does not resolve
external notes or WPS references.

Dimensions are finite mm in `1e-6..=100000`. Intermittent fillets require element length and
a centre pitch exceeding it by at least `1e-6 mm`. An optional count is 2 to 65535 and never
allocates repeated geometry. Two-sided chain arrangements require matching length, pitch
and optional count. Staggered patterns, all-round intermittent notes and intermittent butt
welds are unsupported. These bounds are implementation limits, not universal manufacturing
rules. Tails contain nonblank text of at most 512 UTF-8 bytes; controls, line separators and
bidi overrides/isolates are rejected.

Call `validate()` after serde deserialization or editing. Encoded-input boundaries must also
cap bytes and nesting before parsing; validation cannot undo parsing allocations. Unknown
fields and variants are refused, including within the paired deep-throat size. Every text
and geometry entry point validates again, including records constructed directly in Rust.

`text()` gives a semantic summary with sides, size meanings, both deep-throat values, full
stored numeric precision and units, explicitly named pitch, contours, marks and quoted tail
text. One-sided elements are intermittent; matching elements on both sides form a chain.
The summary is not a standards callout, parser format, localized UI label or compliance claim.

## Drawing and convention choices

`draw(convention, style, measure)` requires an explicit edition/system. Changing it never
modifies the semantic record. No drawing-wide default or silent standard conversion is added.

| Convention | Arrow side | Other side | Supported dimension labels |
|---|---|---|---|
| `Iso2553_2019A` | Continuous line | Dashed identification line, explicitly above or below | Fillet `z` or `a`; paired deep throat `s8 a5`; numeric butt penetration; length or intermittent `n×length (gap)` / `length (gap)` |
| `Iso2553_2019B` | Below the single line | Above the single line | Same ISO size and spacing meanings |
| `AwsA2_4_2020` | Below the single line | Above the single line | Fillet leg without prefix; length or uncounted intermittent `length-pitch` |

ISO A's two reference lines have equal extents. Identical double-sided welds omit the dashed
line and put the symbols opposite one another on the continuous line. Tail-only ISO A notes
also omit the dashed line. Asymmetric side meaning follows line identity even when the dashed
line moves below. Fillet vertical strokes stay left; text is never mirrored or turned sideways.

ISO spacing in parentheses is the **clear gap**, calculated as stored centre pitch minus
length. For three 50 mm elements at 100 mm centre pitch the label is `3×50 (50)`; without a
count it is `50 (50)`, applying along the joint length. The AWS label is `50-100`.
Counted AWS fillet callouts and AWS throat/deep-throat/butt-penetration callouts explicitly
reject; they are not silently converted. An unsupported rendering still has semantic text.

`Drawing` contains `paper::Item` strokes/fills and semantic `Text` runs with measured widths,
lettering heights and left-aligned baselines. Coordinates are local paper mm, x right/y up.
The elbow is `(0,0)` and the tail junction is `(reference_length_mm,0)`. No arrow is drawn.
The all-round circle and filled field flag are at the elbow; the fork and tail are opposite.
A later leader must use these junctions, terminating at the all-round circle boundary to keep
its interior hollow. A fragment alone is not a complete welding symbol on a sheet.
Later sheet composition must declare the system and engineering units (mm).

Use the same font and cap height for `measure(text, height_mm)` and for the returned text.
The caller must detect unsupported glyphs and escape text in its output format. No lettering
font, glyph substitution or guessed character-width table is supplied. Widths must be finite,
positive and at most 1000 mm. Lettering height is `1..=20 mm`, line width `0.01..=height/4`.
These caps bound work and coordinates even with hostile metrics; view scale never alters them.
Dashes are explicit strokes with balanced endpoints because paper has no pattern primitive.

Geometry is constructed independently from lines, circular arcs and a triangular region.
Fillet outlines are closed independently of gaps in ISO A's dashed identification line.
At the default 3.5 mm lettering height, the AWS SI reference profile uses a 5×5 mm fillet,
6 mm square-groove strokes spaced 3 mm apart, and a V 10 mm wide by 5 mm high. Its all-round
circle is 4.5 mm in diameter, field flag has a 6 mm staff and 5×3 mm triangle, tail arms have
5 mm horizontal/vertical components, and curved contours have 7.5 mm radius and chord.
All scale together with lettering height. ISO uses consistent original local ratios, including
a 60-degree V glyph; AWS's reference V glyph is 90 degrees. Neither stores a preparation angle.
Contours follow the fillet face or span the butt opening and keep ink clear at maximum width.
Text clearance and reference length grow from actual metrics instead of fixed character counts.
No source artwork, fonts, paths, text or file assets are copied or redistributed; no dependency
is added.

## Standards references and limits

The issue and linked drawings research/W30 plan establish this backend scope. The following
edition-specific rules were reviewed for the supported subset:

- ISO 2553:2019, 4.3 and 4.4/Table 1: systems and elementary shapes; 4.5/Table 3: contours,
  all-round and field marks; 4.7.1A and 4.7.2.1: reference lines and side placement; 4.8: tail;
  5.2, 5.4 and 5.5/Table 6: numeric butt sizes and paired deep/nominal fillet throats;
  5.3.2: clear spacing, optional counts and chain arrangements. Annex C's alternative
  intermittent methods are not selected automatically by choosing system B.
- AWS A2.4:2020, 4.1–4.5/Figures 4.1–4.2: basic and supplementary symbols; 6.1: sides;
  6.9–6.13: field/all-round marks, tail and contours; 6.16: drawing symbols; Clause 8: fillet
  dimensions. Annex C supplies the SI reference profile used here.

AWS Annex C is **informative reference guidance**, not a claim of mandatory fixed dimensions.
Default AWS geometry follows its reference proportions; explicit height changes preserve
those ratios. ISO tables establish shapes and orientation without dimensioning this module's
local height/spacing profile. ISO glyph sizes, identification-line offset/pattern and layout
clearance remain explicit original choices; no unverified normative ratios are asserted.
Font qualification, complete sheet composition and application-specific drafting acceptance
remain caller/reviewer responsibilities. This is not certification to every ISO/AWS clause.

Further work includes additional weld kinds, compound/multiple-reference symbols, unequal
legs, joint-preparation/root dimensions, staggered patterns, finish methods, validated process
catalogues and alternative intermittent notations. Edition identifiers do not imply those
features are implemented. Unsupported cases return errors rather than partial drawings.

Tests use analytic coordinates, actual-width layout, semantic assertions, dimension/serde
boundaries, hostile text/style/metrics, line identity, equal reference extents, symmetric ISO A,
side swapping, paired throat dimensions, contour direction/ink clearance, field-flag area,
AWS reference proportions and paper-scene validation. They establish implemented behavior,
not standards certification.
