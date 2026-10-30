# Surface texture indications (W30a, #7258)

`solidcraft_geom::surface_texture` holds manually specified requirements without
reading model surfaces. It is below the UI and uses only the geometry crate's
existing serde/thiserror dependencies. It does not register commands, save an
annotation in a Drawing, place it on a sheet, make leaders, attach it to a view or
dimension, or implement ISO 13715 edges.

`Indication` contains an explicit standard, symbol kind (any process, removal
required, removal prohibited), ordered parameter lines, optional lay and process,
and an all-round intent. The standard belongs to the indication; changing the
sheet profile does not convert it. ISO 1302 is deliberately unsupported: converting
its implicit 16% acceptance rule to ISO 21920's maximum rule would change meaning.

The ISO 21920-1:2021 subset accepts Ra, Rz, Rp, Rv, Rq, Rmax, Rt and Pt with explicit
upper/lower limits in micrometres and explicit maximum, 16% or median acceptance.
These are the parameters with defined defaults identified by the study, §11.31.
Parameters requiring additional operators are rejected by the enum rather than
accepted without their mandatory settings. The first line remains first: it is
significant for default settings. Opposite limits for one parameter must agree on
acceptance and cannot form an inverted interval.

The ASME Y14.36-2018 subset holds Ra, Rq, Rz, Rt, Rp and Rv amplitude requirements,
each with a maximum or a minimum–maximum interval. Its amplitude unit is explicit:
micrometres or microinches. Values are never converted silently. It does not borrow
ISO acceptance-rule fields. Waviness domains, filters, sampling/evaluation lengths,
reference information and the standard's full textual-presentation grammar remain
unsupported. Material-removal allowance is not introduced as a current field.

`validate`, `parameter_texts`, `text` and `preview_geometry` validate every time. There must
be 1–16 lines, numeric amplitudes must be finite in `(0, 1e6]`, and a process has
1–256 UTF-8 bytes with no controls, bidi overrides, invisible formatting or
surrounding whitespace. Duplicate requirements and inverted intervals fail. Process text is not parsed to infer or certify a manufacturing method. These
are implementation resource/domain limits, not standard limits. Derived serde operations
do not run semantic validation; validate before serialization too. Deserialization
is structural only; future transports must also limit bytes, allocation and
nesting before decoding.

`text()` supplies an explicit diagnostic description with standard, units,
removal condition, parameter order, lay, quoted process and all-round intent.
It is **not** the normative ASME textual notation or a parser input. ISO parameter
fragments use single spaces, `L` for lower tolerance and `T16%`/`Tmed` when selected;
maximum acceptance is explicit in data even when omitted from the fragment.

`preview_geometry(h, reference_width_mm, format_number)` returns original `paper::Item` strokes and
separate text runs in local paper millimetres (x right, y up, tip at zero), with no
view-scale input. There are no copied glyphs, fonts, artwork or new assets. ISO
accepts only nominal text heights **2.5, 3.5, 5, 7, 10, 14 and 20 mm**. ASME accepts
any finite height in **2.5..=20 mm**. Intermediate ISO sizes return an error; the
rounded nominal dimensions are not interpolated or replaced by one multiplier.

The caller formats every amplitude using the drawing's number policy, returning both text
and its rounded numeric value. Map the drawing formatter's errors into `Invalid` and return
its `Shown.text` and `Shown.value`; do not convert the explicitly stored amplitude unit.
A positive value rounded to zero is refused: increase permitted precision or report that
it cannot be represented under the selected policy. Formatted numeric fragments are capped
at 128 UTF-8 bytes as a resource limit, not a standards limit. The record and diagnostic
`text()` retain the original value. Scientific notation is not asserted to be standards
compliant; its use on a drawing requires an independently established presentation policy.
The existing drawing formatter is on a separate development line and its integration into
sheet placement remains future work.

The shared `plain_text::is_plain_line` checks characters; each annotation retains its own
length and whitespace policy. The weld branch must adopt this helper separately.

`formatted_parameter_texts(format_number)` returns the same validated fragments before
geometry construction, so the caller can measure them and choose the reference width.
The reference width comes from a future font measurer and must span the supplied
text; character counts are not font metrics. Its finite bound is `[h, 1000]` mm.
The bar reserves margins and room for a lay mark. Parameter lines retain their
order and grow the symbol within the collection bound. Actual text widths remain
the caller's responsibility.

The all-round mark stays in its separate output field, using the same coordinates
as the other strokes. Its centre is at the long-leg/reference-line corner. The
removal-prohibited circle is a different mark, tangent to the two legs. The ASME
corner anchor is supported by its construction convention; the current ISO
all-round interpretation and both circle diameters remain provisional.

## Evidence and limits

Construction uses dimensional facts from ISO surface-texture standards and ASME
surface-texture conventions. ISO's nominal sizes and distinguishing-bar dimensions
are based on draft requirements; **published-edition verification remains pending**.
The short-leg height, minimum reference-line height, distinguishing-bar gap and
bar length use their own nominal dimensions. For example, a 5 mm ISO text height
has a 7 mm short leg, not the 7.5 mm used by the ASME construction. ISO's separate
bar distinguishes it from legacy ISO 1302. ISO lay follows the process above the
reference line; ASME uses its separate lower lay area.

All dimensions and sizing choices are collected in the feature-specific
`surface_texture/proportions.rs` module, with separate ISO and ASME profiles.
Unchecked values are explicitly identified there: circle diameters, exact text
margins/baselines, extra-line growth, lay sizes and original letter-shaped marks.
The unit contours are original line/arc constructions, not copied font glyphs.
Corrections to sizing belong in these profiles, without changing the record format.

The constructions remain **previews**, not verified ISO/ASME production geometry.
A standards reviewer must check the profiles and the ISO all-round convention
before conforming drawing export. No conformance claim, legacy migration, full
ISO operator validation or normative ASME textual serializer is made here. A
future exporter must not treat this preview API as certified output.

Tests cover validation and text, all nominal sizes and lay/kind/family combinations,
rejected intermediate ISO sizes, continuous ASME sizing, nominal rounding, 60-degree
legs, tangent circles, all-round corner placement, distinct ISO/removal bars,
parameter and lay bounds, scaling where applicable, and paper-scene validity.
These check the implemented contract and do not replace standards review.
