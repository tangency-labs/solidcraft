# Original HDR environment inputs

This slice contains **24 authored Radiance RGBE inputs**, each **1024 × 512**.
The files contain linear relative radiance, including values above one where
the recipe calls for bright sources. They are generated from original analytic
functions and are ready for resource-adapter development. Their application
binding and rendered reference validation remain pending (`authored_unbound`,
`runtime_ready: false`).

These are synthetic directional lighting fields. An `ENV` reference identifies
the intended context; it does not assert that the catalog's room, terrain,
geometry or complete scene has been implemented. No captured panorama, commercial
material library, AI beauty image, facility scan or measured photometry is used.

## Authority and regeneration

- [source/recipes.json](source/recipes.json) is the editable parameter authority.
- [../tools/environments.py](../tools/environments.py) is the original authoring
  and verification tool. Its version is `1.0.0`.
- [exports/](exports/) contains the required HDR derivatives at one resolution.
- [manifest.json](manifest.json) records file SHA-256, encoded bytes, dimensions,
  color interpretation, source/generator hashes and relative-energy statistics.
- [../tools/test_environments.py](../tools/test_environments.py) checks analytic
  invariants, the format's published RGBE byte example and invalid inputs.

From the repository root, with Python 3.11+, NumPy and Pillow installed:

```sh
python3 -B assets/render/starter/tools/environments.py build
python3 -B assets/render/starter/tools/environments.py validate --reproducible
python3 -B assets/render/starter/tools/test_environments.py
python3 -B assets/render/starter/tools/environments.py preview
```

If the independent `ffmpeg` executable is installed, also run:

```sh
python3 -B assets/render/starter/tools/environments.py validate --reproducible --independent-decode
```

That optional check invokes the executable to decode each HDR as planar float
RGB, then compares every decoded channel with the local stream check. It does
not link, ship or inspect an external implementation. The authoring tools are
development dependencies only; they are not application runtime dependencies.

The preview command writes its fixed-exposure, Reinhard-mapped contact sheet
only under ignored `target/render-starter-review/environments/`. It is a view of
the authored radiance fields, not a screenshot or render from SolidCraft.

## Interpretation contract

| Property | Declaration |
|---|---|
| Projection | 2:1 equirectangular, pixel-center samples |
| Coordinates | Right-handed; +Z up; azimuth from +X toward +Y |
| Longitude | `phi = 2*pi*(u-0.5)` |
| Colatitude | `theta = pi*v` |
| Direction | `(sin(theta)*cos(phi), sin(theta)*sin(phi), cos(theta))` |
| Cardinal longitudes | `u=0/-X`, `u=0.25/-Y`, `u=0.5/+X`, `u=0.75/+Y` |
| Sampling | Repeat U, clamp V; no duplicated seam column |
| File orientation | Radiance `-Y +X` |
| Channels | Linear RGB; no sRGB transfer curve is baked into the HDR |
| Chromaticities | Explicit sRGB primaries and D65 white in `PRIMARIES` |
| Header corrections | `EXPOSURE=1`, `COLORCORR=1 1 1`, `PIXASPECT=1` |
| Intensity | Authored relative radiance; neither lux nor radiometric SI units |
| Randomness | None; `seed: 0` records an analytic deterministic construction |
| Solar disk | Absent from every file |

The importer must map this coordinate convention explicitly. The format
decoder's `-Y +X` byte ordering does not establish the application's world axes.
The color-space declaration must also be checked before working-space conversion.
See the current [bounded HDR input contract](../../../../docs/render-hdr-input.md).

The upper/lower hemisphere values and smooth rectangular directional patches
describe an authored field at infinity. The patches are not finite-distance
area emitters: they do not supply parallax, distance falloff, hardware dimensions,
occlusion, contact shadows or room interreflection. Their soft borders are part
of the lighting design. Engine-native area lights remain a separate resource.

**One energy owner:** the nonblack maps own their ambient field and bright
patches. A corresponding analytic rig must replace those contributions or use a
separately authored compatible map. Adding equivalent lamps on top would duplicate
illumination. The catalog's original separation of environment and rig remains
unchanged; these are explicitly standalone illumination variants.

`HDR-024` contributes exactly zero energy. Any Sun, work light, reflected light,
stars or Earth contribution must come from separately owned scene resources.
Automatic sky fill, fake stars and exposure compensation are not part of this
vacuum input. A scene with no other source will correctly remain dark.

## Included resources

| ID | Resource | Context |
|---|---|---|
| HDR-001 | Neutral side softbox | ENV-001 |
| HDR-002 | White enveloping studio | ENV-002 |
| HDR-003 | Black metal strips | ENV-003 |
| HDR-004 | Hangar skylight pattern | ENV-007 |
| HDR-005 | Neutral comparison field | ENV-008 |
| HDR-006 | Warm table window | ENV-021 |
| HDR-007 | Showroom ribbons | ENV-034 |
| HDR-008 | Mechanical bay windows | ENV-071 |
| HDR-009 | CNC overhead lanes | ENV-072 |
| HDR-010 | Woodshop broad window | ENV-073 |
| HDR-011 | Garage doorway field | ENV-075 |
| HDR-012 | Makerspace worklights | ENV-079 |
| HDR-013 | Home workbench window | ENV-080 |
| HDR-014 | Electronics bench neutral | ENV-082 |
| HDR-015 | Sun-free diffuse overcast | ENV-182 |
| HDR-016 | Automotive turntable ribbons | ENV-201 |
| HDR-017 | Paint inspection bands | ENV-206 |
| HDR-018 | Assembly bench soft | ENV-211 |
| HDR-019 | Rework offset strips | ENV-212 |
| HDR-020 | FDM neutral inspection | ENV-214 |
| HDR-021 | Printed-part photography | ENV-217 |
| HDR-022 | Robot LED editorial | ENV-218 |
| HDR-023 | Spacecraft clean-bay field | ENV-231 |
| HDR-024 | Vacuum zero-ambient field | ENV-244 |

The colored accents in HDR-022 are explicitly editorial RGB illumination. The
recipe does not claim a spectral LED model, color temperature or measured emitter.
HDR-015 is a generic sun-free sky-like gradient, not a weather observation or
physical sky model. HDR-005 is an exactly uniform unit-radiance analytic reference.

## Numeric and visual checks

The validator checks strict recipe fields, duplicate JSON keys and IDs, concept
references, finite/channel bounds, path-safe identifiers, bounded dimensions,
RGBE framing and runs, metadata corrections, hashes, file inventory and the
zero-energy vacuum invariant. Reproducibility mode regenerates each HDR in
memory and compares the bytes. The independent decoder mode checks all pixels.

Quantization checks bound each channel's error by one RGBE step relative to that
pixel's largest channel. Exact latitude-cell solid angles are used for mean
radiance; the six hemisphere cosine integrals use the pixel-center direction and
are explicitly numeric estimates. Tests compare an independent uniform-field
expectation, `E = pi * L`, against those integrals. They also check cardinal source
orientation, unit direction vectors and invalid metadata/stream rejection.

The source is continuous across longitude wrap. Opposite-edge pixel centers
are neighboring samples, so they are not incorrectly required to have equal
pixel values. The export has no baked tone mapping or clipped [0,1] radiance.

The first authored set was inspected in the generated contact sheet for source
shape, orientation, continuity and unintended bands. This is authoring review;
native CPU/GPU consumption, diffuse/conductor/glass reference scenes, UI previews
and platform parity still need the resource integration work. Repository gates
are recorded in the parent pack's validation record.

## Provenance

Original SolidCraft contribution, `MIT OR Apache-2.0`, authored 2026-10-21 for
the personal rendering fork. No external binary asset is bundled. Exact rows for
all shipped files and development tools are maintained in root `ATTRIBUTION.md`.

Public format references consulted for byte/header meaning:

- [Radiance File Formats](https://radsite.lbl.gov/radiance/refer/filefmts.pdf)
- [The Radiance Picture File Format](https://radsite.lbl.gov/radiance/refer/Notes/picture_format.html)

The encoder is original authoring code. No routines from those pages were
adapted. These references do not convert relative authored radiance into a
calibrated light probe.
