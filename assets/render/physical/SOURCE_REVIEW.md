# Source review and interpretation limits

Research date: 2026-10-22. Exact URLs and source locators are stored in
[sources.json](sources.json); each numerical record in [profiles.json](profiles.json)
points to one of those references. Selection and normalization are original work.
Supplier publications retain their own rights and are not bundled or relicensed.

## Evidence levels

The profiles distinguish typical values, published intervals, lower/upper bounds
and explicitly calculated values. A manufacturer datasheet is evidence for the
named grade and stated test, not identification of an object from its rendering.
No record claims certified design allowables. A public URL and retrieval date are
traceability metadata, not a signed source archive or proof that the URL can never
change. Verify the relevant current datasheet and certificate before engineering use.

Missing temperature, strain rate, specimen orientation or test method is marked
unknown. Published crosshead speed is retained as speed, not guessed strain rate.
A source's plus/minus notation is preserved without inventing its confidence level.

## Conflicts and exclusions retained for review

| Source or material | Treatment |
| --- | --- |
| Sandmeyer 304/304L | Use the identified main physical/plate tables; do not average different FAQ values. Typical plate strengths are not minimum-grade specifications. |
| Elgiloy Nickel 200 | Keep the cited metric expansion value; a conflicting imperial entry is not used to create a second inconsistent constant. |
| SAC305 solder | A field with resistivity units but a conductivity label is excluded rather than silently reinterpreted. Solid alloy data does not identify flux-bearing paste. |
| PolyLite ABS | Product-page and wiki densities disagree, so density remains unresolved. The suspect modulus unit is not repaired by intuition. The retained XY/Z strengths remain directional. |
| Isola 370HR | The modulus row cites a flexural test; retain it as flexural modulus, not a complete orthotropic elastic tensor. Dielectric-strength unit entries are not silently reconciled. |
| Fused silica | A softening value is not used as a melting point; uncertain or inconsistent auxiliary optical entries were excluded. |
| Amorim TPS | Use the cited aerospace table rather than mix differing values from another brand page. Room-condition reference properties do not prove ablation performance. |

Every exclusion stays explicit in the relevant profile. Reference candidates that
would imply wrong chemistry were removed during association review: HDPE is not a
vinyl-flooring default; a glass profile is not a polymer interlayer; MDF is not an
unspecified insulation core. Wrought 6061 may only be an explicitly labelled
comparison for an unidentified casting, never a cast-alloy strength certificate.

## Materials requiring special treatment

### Wood and engineered panels

USDA clear-wood results identify species, moisture and test orientation. Clear
specimen means are not graded structural-lumber design values. Their bending
modulus is not renamed tensile modulus or combined with an invented Poisson ratio.
The specific-gravity basis is retained. For the cited 12% moisture rows, wet mass
at the stated specimen volume is derived as `SG * 1000 * (1 + 0.12)` kg/m3, with
1000 kg/m3 explicitly identified as the water-reference convention. That formula
is recorded, checked and not extrapolated to another moisture content.

Birch specimen properties are a veneer/constituent reference, not effective plywood
properties. Panel layup, glue, moisture, thickness class and density must be known.
MEDITE PREMIER >12 to 19 mm values are bounded panel specifications; the missing
board density is not invented. Cork's MDFACADE product name is not confused with
medium-density fibreboard.

### Printing and composites

Printed PLA/ABS coupon directions and reported spreads remain distinct. Feedstock
solid density is not an infilled-part envelope density. SLS PA12 is not automatically
a valid MJF material model. TPU density intervals remain intervals.

AS4 fibre data describes a constituent. AS4/8552 unidirectional laminate data
requires the stated fibre volume, cure and orientation. Neither describes a woven
twill part simply because its texture resembles carbon fibre. The woven aramid
surface remains unresolved until a particular fibre/resin/layup is selected.

### Electronics, elastomers and coatings

FR-4 dielectric values retain frequency, and expansion retains axis and thermal
regime. A solder-mask appearance is not a single FR-4 property card for the whole
board. Copper thickness, laminate construction, mask, components and interfaces
need separate treatment.

Dragon Skin 20's stress at 100% extension is not Young modulus. A reference casting
silicone is not automatically qualified for cable insulation, optical lenses or
space seals. Filled potting and IC mould compounds need their actual filler/cure
recipe; unfilled casting epoxy is only a labelled comparison.

AZ-93 thermal emittance and solar absorptance retain their coating context and
thickness condition. They are not visible RGB, material emission strength or a
bulk heat capacity. A coating's nominal application thickness is not an automatic
thickness assignment to a CAD model.

### Lunar-like resources

The four Exolith datasets are terrestrial laboratory analogues. Grain density,
packing density, void ratio, porosity and apparatus-dependent repose angle remain
separate. They do not identify a geological site, vacuum condition, internal
friction law or reduced-gravity mechanical response. Rock concepts are not assigned
loose-soil density merely to increase numeric coverage.

## Review backlog

The 294 concepts without an applicable named reference need a selected recipe,
grade or multi-material construction. The 306 with candidates still require review
of their stated relation and conditions; some references are constituents or
explicit comparisons, not effective-property defaults. No numerical completeness
percentage is derived from these association counts.

Further evidence is especially needed for actual paint/cure stacks, woven aramid
laminates, named glass/polymer optical grades, flexible-cable formulations, filled
thermal interfaces, resin systems, foams, friction materials and aerospace-qualified
processes. Temperature-dependent curves, fatigue, creep, damping, outgassing and
failure envelopes should be added as documented quantities/models, not filled with
uncited family averages.


## Added source review — 2026-10-22

Eight additional primary supplier/manufacturer references add 43 selected records,
bringing the dataset to 59 profiles, 50 source records and 416 numerical records.
All 48 authored surfaces now have an explicit reference-candidate relationship.
This does not make the 600 concepts numerically complete or identify their actual
manufacturing recipe.

PMMA sheet preserves published intervals for stiffness, Poisson ratio, heat
capacity, thermal expansion/conductivity and electrical resistivity; no midpoint
is selected for elasticity. Rigid PVC stock data does not identify plasticized
cable insulation. Kevlar 49 axial fibre data does not establish cured laminate or
meta-aramid paper/honeycomb properties; those mappings remain separate.

Sikafloor 161 HC compressive and flexural data retain the 28-day cure at 23 C;
its Shore D datum retains the 7-day cure. The manufacturer's mixed-liquid density
is not assigned to the cured resin. UV22 retains two different glass-transition
ranges, distinguishing UV-only cure from thermal post-cure. Its refractive index
is not a spectral model or certified optical transmission curve.

C71 Speedline, Stoving Enamel Hammer and Acrythane SC601 remain named coating
candidates with only the published temperature and/or coverage-basis dry-film
thickness recorded. Temperature statements without duration are upper-bound
manufacturer references, not continuous-use allowables. No cured density, modulus,
strength or film mass is inferred from paint package weights, liquid densities,
nominal coverage, visual roughness or colour. Those unknowns remain visible.

The inspected Kuhn PVC page contains blank mechanical cells and a hardness row
with inconsistent units. Neither is imported. Public reference metadata preserves
the source relationship without claiming that an openly accessible supplier page
has a permissive dataset licence. No PDF, table layout, prose, logo or image from
those publishers is included in this repository.
