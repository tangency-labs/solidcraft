# Sourced physical material references

Expanded data checkpoint: **2026-10-22**. Physical engineering data is maintained
separately from the authored render appearance. The original source files below
are authoritative; exported native cards are reproducible projections, not a
second independently edited dataset.

## Delivered coverage

| Collection | Current coverage |
| --- | --- |
| Named reference profiles | **59**, with **416 numerical records** covering **46 quantity types** |
| Public primary source records | **50**, with locators, retrieval date and review limitations |
| Material concept specifications | **600/600** have an explicit physical role, identification requirements and unknowns |
| Concepts with reference candidates | **306**; these can be substrate, constituent, comparison or named-grade candidates |
| Concepts still requiring a specific formulation | **294**; no unrelated numerical material is substituted |
| Authored PBR surface associations | **48/48** linked to reviewed reference candidates and explicit physical-role rules |
| Schema-1 native projections | **59 envelopes**: **48 nonempty**, **11 extended-only**, **113 native property blocks** |
| References eligible for material creation | **46** have a supported point density; other references do not acquire an invented mass density |

A surface reference association is **not identification of a manufactured part**.
A metallic-looking coating is not a metal substrate; a fibre is not a laminate;
feedstock is not an isotropic printed part; loose granular density is not grain
density. References remain explicitly named and require grade, processing and
conditions to match the intended application. No physical material is silently
assigned to a body, and no appearance edit changes its engineering data.

## Data and relationships

- [profiles.json](profiles.json): the 59 exact material identities, numerical
  values/ranges, original units, SI conversion, measurement conditions and limits.
- [sources.json](sources.json): primary-source URLs, selected locators and review
  records. Public availability is not a redistribution licence or authentication.
- [quantities.json](quantities.json): units, dimensions and bounds. Hardness scales,
  thermal transitions, frequencies and directional measurements remain distinct.
- [bindings.json](bindings.json): all 600 concept specifications and 48 authored
  surface relationships, including substrate/stack/recipe dependencies.
- [exports/native-cards.json](exports/native-cards.json): conservative native
  schema-1 projections, with every excluded property identified and retained in
  the extended profiles. This is not itself a native library import file.
- [exports/coverage.json](exports/coverage.json): generated coverage and exclusions.

The references cover metals and solders, engineering polymers, silicones and
resins, additive-manufacturing feedstock and specimens, woods, cork/MDF, glass and
ceramics, PCB and composite constituents, coatings and terrestrial lunar
simulants. New named references include PMMA sheet, PVC stock, Kevlar 49 fibre,
cured epoxy, UV epoxy and specific alkyd, stoving-enamel and acrylic coatings.

## Properties preserved

Mechanical records include density, Young/shear/Poisson measurements, tensile,
yield, compression, shear, flexural properties, elongation, fracture/tear data
and explicitly identified hardness scales where published. Thermal records
include conductivity, expansion, heat capacity, glass transition, melting,
decomposition and qualified service-temperature statements. Electrical records
retain frequency-specific permittivity/loss, resistivity and dielectric strength.
Optical, moisture, granular and application-thickness records retain their own
units and meaning; they are not reinterpreted as native elastic constants.

Each numerical record preserves a source, original value/unit, conversion,
qualifier, condition fields and limitations. A range is not reduced to its
midpoint; a lower-bound strength is not a typical value. Missing conditions and
uncertainties remain null/unknown. Per-property cure schedules survive native
projection. An isotropic elastic block refuses mismatched source, temperature,
moisture, rate, state or direction rather than combining unrelated constants.

Some new coating profiles contain only published temperature or film-thickness
information. Their cured density, modulus and strength remain unknown. In
particular, liquid/mixed-resin density and package weight are not cured-film
material properties. See [source review](SOURCE_REVIEW.md).

## Validate and inspect

All tools use Python 3.11+ standard-library code. From the repository root:

```sh
python -B assets/render/physical/tools/physical.py validate
python -B -m unittest discover -s assets/render/physical/tools -p 'test_*.py'
python -B assets/render/physical/tools/check_physical.py --cli /trusted/solidcraft-cli
python -B assets/render/physical/tools/physical.py resolve SURF-001
python -B assets/render/physical/tools/physical.py prepare steel-c45 --acknowledge-reference
```

The native check calls the read-only `material.properties.validate` command. It
checks numerical admission and property preservation, **not** source authenticity,
material assignment, solver correctness, persistence or library import. Evidence
and executable hashes stay in ignored `target/physical-material-review/`.

## Native library export without data loss

Generate a non-executing plan for all point-density references:

```sh
python -B assets/render/physical/tools/export_library.py --acknowledge-reference
```

With a trusted runtime that actually supports physical-card persistence and
library v2, create and verify a separate reference library:

```sh
python -B assets/render/physical/tools/export_library.py \
  --acknowledge-reference --cli /trusted/card-capable/solidcraft-cli
```

Optional repeated `--profile ID` arguments restrict the selection. The exporter
first verifies the command contract, then uses a fresh CLI design with no
geometry, writes a native v2 library and design into a new `target/` directory,
checks every card/source/condition/density, asks the native library inspector to
verify content revisions, and cold-reopens/re-exports the design. It does not
manually approximate Rust's canonical revision serialization. Existing files,
live GUI sessions, user models and body assignments are untouched.

**The baseline Pegasus CLI validates cards but cannot persist them in materials;
the exporter therefore refuses it before creating anything.** Unit tests and that
refusal are not a successful end-to-end v2 export. A successful modern-runtime run
writes its own receipt; no such runtime receipt is implied by the authored data.
The complete extended dataset must accompany the narrower native library.
Electrical, hardness, ranges, directional measurements and unsupported quantities
must not be claimed to have been embedded in schema 1 merely because a profile
has a native projection.

## Integration and publication

Use [INTEGRATION.md](INTEGRATION.md) to connect the records to the separate Render
branch. `runtime_assignment` remains false until actual target selection and
application acceptance are performed. No native document migration, material-card
schema expansion, rendering shader, solver, model, appearance texture or optical
preset is changed here. Full workspace CI remains a separate integration gate.

All new curation, validators and tooling are original contributor work under
MIT OR Apache-2.0. Publisher data remains attributed to its sources; supplier
brand/grade names do not imply affiliation or certification. Selected numerical
facts are documented without bundling supplier tables, PDFs, artwork or code.
