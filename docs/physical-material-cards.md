# Physical material data cards

`material.properties.validate {card}` checks a caller-supplied candidate record,
without assigning it, saving it in the design, reading files, fetching sources or
changing history, selection, camera or live geometry. `material.create/edit` can
also keep a validated card in a design-owned physical material. Saved cards use
the same schema and validator; density used for mass and visual appearance remain
separate. This implements the persistence slice of #1060 under #923/#922/#785.

Validation means numerical and structural consistency. It does not authenticate
sources, approve redistribution, certify design values, provide a solver or enable
advanced rendering. The result reports `analysis_solver_available: false` and
`rendering_properties_available: false`. Missing properties remain unknown.

## Design-owned cards

| Command | Engineering-card behavior |
|---|---|
| `material.create` | Accepts optional `physical_card`; omitted or `null` leaves engineering properties unspecified. `density_g_cm3` remains required. |
| `material.edit` | An omitted `physical_card` retains the complete saved card; an object replaces it atomically; `null` clears it. |
| `material.inspect` | Takes `material: name` and returns one built-in or design-owned material, its density/default look and any saved card, validation and density comparison. Read-only. |
| `material.list` | Retains the 14 built-in density rows and includes engineering details for design-owned materials that have a card. Read-only. |
| `material.properties.validate` | Reviews a supplied candidate without saving or assigning it. |

The `physical_card` value is the schema-1 card object described below. Its typed
wrapper revalidates input during commands and deserialization. Invalid data refuses
before changes to the material name, body/configuration references, assigned
density, appearance or undo history. Renaming, optical edits and density-only
edits retain an omitted card. Undo/redo, native save and cold reopening preserve
source claims, per-property conditions and SI values.

Mass continues to use the material's `density_g_cm3`, including the existing
generic built-ins. A card density in `kg_m3` never changes assigned density
implicitly. When both exist, inspection reports their values in kg/m³, their
absolute relative difference divided by the larger value, and whether that
difference is at most `1e-9`. This is a consistency report, not a decision about
which density applies. Appearance/PBR changes do not change a card, and a card
does not supply optical properties or mechanical axes for a body.

New files declare the `physical-material-cards` pending format change. Older
designs without a card need no data migration and retain unspecified engineering
properties. The last released reader refuses the newer format; a pending build
without this descriptor refuses the unknown change. No historical format number
or previous migration is repurposed.

Engineering constants such as elastic moduli can exceed the geometry coordinate
bound. The separate SI admission path applies only to a root
`user_materials[].physical_card`; similarly named fields elsewhere do not bypass
geometry checks. Normal loading and commands both bound user-defined materials
to 1,000, in addition to the card limits below. Existing document metadata keeps
its separate handling.

The portable `solidcraft-library/2` catalog preserves physical cards, their sources
and conditions, and independent PBR default appearances. The scalar v1 reader remains
supported; explicit v1 export refuses any card or PBR loss before writing, including
empty cards whose designation/source information matters. See [local material
libraries](book/src/dev/local-libraries.md) for copy, collision, file and size contracts.

## Contract (standalone card schema 1)

### Candidate review window

Search for **Validate Physical Material Data** in the command palette (or use
`ui.start` with `material.properties.validate`) to open the candidate reviewer.
Paste a schema-1 card object and choose **Review card**. The existing engine
validator checks it; the window shows known properties in SI units, unknown
properties, per-property conditions and source claims as inert text. Material-local
axes do not assign a frame to a body. Isotropic G/K come from the engine report.

Editing the input clears the previous review. Invalid input shows its refusal
without retaining a stale valid result. Input is capped at 262,144 characters and
256 KiB before JSON parsing; the validator's tighter structural and text budgets
still apply. The window does not read files, fetch references, assign properties,
replace density, modify history or save the candidate. Close discards it. Calling
the engine's JSON command still returns its report without opening a dialog.

Saved cards can be reviewed and edited in the existing **Physical Material**
assignment panel. Select a design-owned material and expand **Properties** to
review it, or choose **Edit** to edit its fields. **New** starts a blank material;
**Duplicate** copies the selected built-in or design-owned material into an
independent user material. Built-ins remain immutable. Assigned density is shown
separately in g/cm³ and controls mass; card density is in kg/m³ and does not change
that assigned density.

The graphical draft has optional engineering properties, all nine property types,
isotropic and orthotropic elasticity, material-local axes, source metadata and
per-property temperature, moisture, strain rate and state. Numeric fields show
SI units and accept scientific notation. Missing properties and empty optional
conditions remain unknown. Adding a property leaves its required values blank;
Save refuses incomplete or invalid constants rather than creating measured data.
Source IDs remain stable in the form; source titles, type, license and reference
are editable. Sources still in use cannot be removed. Changing elasticity model
clears its numeric draft fields and requires explicit values for the new model.

**Save** runs one `material.create` or `material.edit` command and keeps that change
with normal undo/redo, without assigning the material to any selected body.
**Cancel** closes the panel and discards only an unsaved draft; it never rolls back
a previous Save. **Discard** leaves the panel open and drops the current draft.
While a draft is open, footer OK and assignment through Enter are blocked. After
saving or discarding, OK retains the existing explicit body-assignment behavior.

Edits omit unchanged fields, preserving every saved source, condition, anisotropic
constant and visual appearance, including PBR. An unchanged Save creates no undo
entry. A changed material is refused if another command altered it while the
draft was open. Duplicates copy optical scalar/PBR values; their owned appearance
name follows the new material name. No new window, JSON editor, persistent format,
solver or measured material dataset is introduced by the graphical form.

Numerical validation is visible before Save and the engine validates again at
commit. Source/license claims remain unverified. Runtime checks and real UI
captures are required in the integration gate; this documentation does not imply
that a numerical card is engineering certification.

Local file import remains a separate operation. No dataset or rendering capability
is supplied by reviewing or saving a card.

A card has `schema_version: 1`, `designation`, `family` (`wood`, `polymer`, `glass`,
`metal`, `other`), `sources` and `properties`. Source IDs are exact, case-sensitive
local references. Each source has `id`, `title`, `kind` (`user_supplied`,
`measurement`, `public_reference`, `illustrative`), optional `license` and
`reference` strings. These strings are metadata supplied by the caller; even a
declared open license remains in `source_review_required`. Nothing is downloaded.

Every property has `data`, `source_id` and optional `conditions`. Conditions are
`temperature_k`, `moisture_dry_basis_kg_kg`, `strain_rate_per_s` and a free-text
`state` (grade, temper, processing or specimen condition). Conditions belong to
the property, not an assumed global room temperature. An elastic block represents
constants measured under the same conditions and source; mixed-condition curves
and independently sourced constants need a later representation.

| `data.kind` | Explicit SI fields |
|---|---|
| `density` | `kg_m3` |
| `elasticity` | `model`, described below |
| `tensile_strength`, `compressive_strength`, `yield_strength`, `shear_strength` | `pa` |
| `thermal_conductivity` | `w_m_k: [k1,k2,k3]`, `axes` |
| `thermal_expansion` | `per_k: [a1,a2,a3]`, `axes` |
| `specific_heat` | `j_kg_k` |

Directional values use `axes: material123` (material-local 1/2/3) or `wood_lrt`
(longitudinal/radial/tangential). These labels do not assign a frame to a body or
infer mechanical orientation from texture rotation. Equal directional values can
describe an isotropic thermal property. Thermal expansion may be zero or negative.

An isotropic elastic model has `symmetry: isotropic`, `young_modulus_pa` and
`poisson_ratio`. Stability requires E > 0 and -1 < nu < 0.5. Valid auxetic values
are accepted. The result derives G = E/[2(1+nu)] and K = E/[3(1-2nu)], in Pa.

An orthotropic model has `symmetry: orthotropic`, `axes`,
`young_moduli_pa: [E1,E2,E3]`, `shear_moduli_pa: [G12,G23,G31]` and
`poisson_ratios: [nu12,nu23,nu31]`. Here nu_ij = -eps_j/eps_i under stress along i;
reciprocity supplies nu_ij/Ei = nu_ji/Ej. The normal compliance is symmetric with
diagonal 1/Ei and off-diagonal -nu_ij/Ei. Diagonal scaling produces unit diagonal;
Sylvester's criterion checks its positive definiteness. All three shear moduli
must be positive. General orthotropic Poisson ratios are not capped at 0.5.
Moduli whose reciprocals cannot be represented finitely are rejected; the
isotropic derived response and its reciprocals must also remain finite/positive.

Moisture is kg water / kg dry material and can exceed one. It is not a wet-mass
fraction. Unspecified temperature/state, wood moisture or polymer strain rate are
reported in `unspecified_conditions`; they are not guessed or replaced by zero.
The validation report also returns `missing_condition_fields`, a map from each
incomplete property kind to its exact absent field names. Temperature and state
apply to every family; wood additionally checks dry-basis moisture and polymers
strain rate, following the same reporting policy as `unspecified_conditions`.
For example, a wood density with temperature and state but no moisture reports
`{"density":["moisture_dry_basis_kg_kg"]}`. Complete properties are omitted;
an empty card returns an empty map, without inventing properties or conditions.
This detail is read-only and does not change admission or the saved card schema.
Older serialized validation reports without the map deserialize with an empty
map; regenerate validation from the card to obtain the missing-field detail.
The family label does not certify that a chosen constitutive model fits a material.
Scalar strengths are individual measurements, not a three-dimensional failure
envelope. Typed strength directions and test methods remain a separate extension.

## Admission limits

Before cloning/deserializing JSON: at most 2,048 nodes, depth 12, 32 array entries
or object fields and a 256 KiB conservative accounting budget (32 bytes per node
plus string/key bytes). The typed record allows 32 sources and 16 properties;
duplicate source IDs or property kinds and unknown fields are rejected.

Text limits in UTF-8 bytes: designation/title 256, source ID 128, license 512,
reference 2,048 and state 1,024; controls and blank text are rejected. All numeric
values must be finite. Positive-value upper admission limits are density 1e6
kg/m³, moduli/strengths 1e15 Pa, conductivity 1e9 W/(m K), heat capacity 1e9
J/(kg K), temperature 20,000 K and strain rate 1e12 /s. Expansion is within ±1 /K,
dry-basis moisture within [0,1e6] (also accommodating highly hydrated polymers),
orthotropic Poisson ratios within ±1e6 in addition
to the compliance stability test. These are generous technical admission limits,
not measured applicability ranges or a conditioning guarantee for a future solver.

## Original synthetic example

These deliberately invented constants demonstrate validation, not a real grade:

```json
{
  "card": {
    "schema_version": 1,
    "designation": "Synthetic elastic example — not design data",
    "family": "other",
    "sources": [{
      "id": "example",
      "title": "Original synthetic example",
      "kind": "illustrative",
      "license": "MIT OR Apache-2.0"
    }],
    "properties": [{
      "data": {
        "kind": "elasticity",
        "model": {"symmetry": "isotropic", "young_modulus_pa": 12000000000, "poisson_ratio": 0.25}
      },
      "source_id": "example"
    }]
  }
}
```

The result has G = 4.8e9 Pa, K = 8e9 Pa, unspecified conditions and source review
still required. No assigned mass, saved material or appearance changes.

## Next phases and references

Portable engineering-card catalog v2 copies the validated records above. Scalar v1
remains a strict compatibility format. Curves,
uncertainty, hardness scales, viscoelasticity and failure criteria remain later work.
Visual PBR/texture/environment consumption uses the renderer contract requested
in #813, with #909/#837 input utilities; accepting physical data advertises none
of those capabilities.

The [USDA Wood Handbook](https://research.fs.usda.gov/fpl/wood-handbook), including
[chapter 4's dry-mass moisture definition](https://research.fs.usda.gov/download/treesearch/62243.pdf), is a public
scientific reference for moisture, mechanical behavior and the distinction between
specimen properties and graded design values. No handbook data/table is bundled.
The [Khronos PBR material model](https://www.khronos.org/gltf/pbr) is a reference for
the separate optical track. No third-party code, engineering dataset or asset is
added by this implementation.

## Directional elasticity query

`material.properties.directional {card, direction: [x,y,z]}` reports Young's modulus
in Pa for a uniaxial, small-strain constitutive test along a material-local direction.
It returns the safely normalized direction, declared axes (`null` for isotropy), and
the full validation/source/condition report. It neither assigns axes to a body nor
predicts a part's displacement, failure, strength or safety factor. No solver or
render capability is enabled. Missing elasticity refuses instead of inventing data.

For orthotropy, let n=(x,y,z) be the unit direction. With engineering shear
compliance 1/G and the reciprocal Poisson convention above:

```
1/E(n) = x^4/E1 + y^4/E2 + z^4/E3
       + x²y²(1/G12 - 2 nu12/E1)
       + y²z²(1/G23 - 2 nu23/E2)
       + z²x²(1/G31 - 2 nu31/E3).
```

This follows by applying stress sigma = n tensor n, computing strain with the
symmetric compliance, then contracting n with strain and n. The implementation
uses a Cholesky sum of squares for the normal contribution, avoiding cancellation
between the positive diagonal and negative coupling terms. Shear contributions
are also nonnegative. Isotropy returns E directly, independent of orientation.

Input directions must contain exactly three finite numbers and be nonzero; arbitrary
magnitude is allowed. Max-component scaling avoids overflow or underflow while
normalizing very large or tiny vectors. Unrepresentable compliance/modulus refuses;
this finite-precision check does not establish a condition-number certificate.
The card's test conditions still apply; changing direction does not extrapolate
temperature, moisture, process, rate, damage or nonlinear behavior. Synthetic analytic
tests cover principal axes, the isotropic limit and off-axis shear/coupling terms.
No physical dataset or external implementation is included.

## Design-owned persistence

`material.create` accepts an optional `physical_card`; omission or null means
unknown. `material.edit` retains an omitted card, validates and replaces a supplied
card, and clears it only with explicit null. Density-only, appearance-only and
rename edits retain the card. Assignment, configurations, undo/redo and Paste New
keep using the same design-owned material definition.

`material.list` reports the optional card, its validation report and a density
comparison. Assigned density in g/cm3 and recorded density in kg/m3 stay separate:
card data never silently changes the density used for mass. Source and condition
claims are preserved, not certified, and this does not add an analysis solver.

The single pending descriptor is `material-engineering-cards.rs`. The build
assigns the pending format automatically; no released number, history or migration
is edited. Older designs keep absent cards as unknown. Older readers refuse the
newer saved format rather than silently losing engineering records.

Only `user_materials[].physical_card` receives the typed SI admission needed for
elastic moduli above coordinate limits. Other numeric/depth limits, including the
second range check after integer-valued coordinates become typed floats, remain
unchanged. Normal loading admits at most 1000 design-owned materials.

Scalar catalog v1 refuses cards before creating or replacing an export file.
Rich catalog transport is the separate #1086 contribution. The synthetic example
`examples/material-card.json` uses an assigned density of 0.8 g/cm3, a recorded
700 kg/m3 and invented isotropic constants; it is not a real material dataset.
