# SolidCraft material concept catalog 0.2.0

## Scope and status

This catalog defines **600 original conceptual visual-appearance recipes in 60 families of 10**. It is planning data, not a downloadable shader library, a set of measured optical properties, or a material-selection certificate. MAT-001 through MAT-200 keep the semantic identities of the original catalog. MAT-201 through MAT-600 add 40 focused families for automotive, industrial, electronics, additive manufacturing, woodworking, DIY, LED, aerospace, space and SELENE work.

Every record has an English name, family, domains, description, use cases, optical model, editable controls, scale policy, proposed authoring method, renderer dependencies, content priority, relative cost, proposed validation checks and original-authoring provenance. Names describe intended appearances. They do not certify the underlying alloy, polymer grade, mineral composition, coating process or flight readiness.

Some recipes describe a composite surface, a cut section, a process state or an overlay. They are intentionally more than a single colour or roughness setting. An overlay must preserve the host material; a section may require several material regions; an open mesh, pore network, fibre bundle or optical facet may need real render geometry.

## New families

| ID range | Family | Recipes |
|---|---|---:|
| MAT-201–MAT-210 | Automotive paint stacks and correction states | 10 |
| MAT-211–MAT-220 | Automotive exterior trim and films | 10 |
| MAT-221–MAT-230 | Automotive brakes and contact surfaces | 10 |
| MAT-231–MAT-240 | Automotive tyres and sealing profiles | 10 |
| MAT-241–MAT-250 | Automotive glazing and optical covers | 10 |
| MAT-251–MAT-260 | Automotive cabin and soft trim | 10 |
| MAT-261–MAT-270 | Industrial machining signatures | 10 |
| MAT-271–MAT-280 | Industrial protective treatments | 10 |
| MAT-281–MAT-290 | Industrial wear and service overlays | 10 |
| MAT-291–MAT-300 | Industrial belts meshes and packing | 10 |
| MAT-301–MAT-310 | PCB constructions and fabrication regions | 10 |
| MAT-311–MAT-320 | Electronic bonding and contact finishes | 10 |
| MAT-321–MAT-330 | Electronic packages and encapsulants | 10 |
| MAT-331–MAT-340 | Electrical insulation and cable constructions | 10 |
| MAT-341–MAT-350 | Battery and thermal-management surfaces | 10 |
| MAT-351–MAT-360 | Additive feedstock appearances | 10 |
| MAT-361–MAT-370 | FDM surface pathways and post-processing | 10 |
| MAT-371–MAT-380 | Resin-printing process appearances | 10 |
| MAT-381–MAT-390 | Powder-printing and sintered states | 10 |
| MAT-391–MAT-400 | Printer hotends motion parts and build plates | 10 |
| MAT-401–MAT-410 | Additional timber and bamboo appearances | 10 |
| MAT-411–MAT-420 | Wood cuts joinery and assembled patterns | 10 |
| MAT-421–MAT-430 | Woodworking and DIY finishes | 10 |
| MAT-431–MAT-440 | DIY panels adhesives and joint surfaces | 10 |
| MAT-441–MAT-450 | LED emitters and conversion layers | 10 |
| MAT-451–MAT-460 | LED diffusers and light guides | 10 |
| MAT-461–MAT-470 | LED lenses reflectors and optical hardware | 10 |
| MAT-471–MAT-480 | Aerospace composite constructions | 10 |
| MAT-481–MAT-490 | Aerospace cores and lightweight sections | 10 |
| MAT-491–MAT-500 | Aerospace protective and functional skins | 10 |
| MAT-501–MAT-510 | Space thermal blankets and films | 10 |
| MAT-511–MAT-520 | Space solar-array and optical-device appearances | 10 |
| MAT-521–MAT-530 | Space harness and mechanism interfaces | 10 |
| MAT-531–MAT-540 | Lunar mare-style ground and rock concepts | 10 |
| MAT-541–MAT-550 | Lunar highland-style rocks and ground concepts | 10 |
| MAT-551–MAT-560 | Lunar dust deposition overlays | 10 |
| MAT-561–MAT-570 | SELENE rover mobility and structural appearances | 10 |
| MAT-571–MAT-580 | SELENE arm gripper and sampling-tool appearances | 10 |
| MAT-581–MAT-590 | SELENE transparent sample containment and viewing | 10 |
| MAT-591–MAT-600 | SELENE opaque sample cassettes caps and seals | 10 |

## SELENE component mapping

These are candidate appearance assignments for design discussion, not final engineering material selections. Apply them to distinct CAD components or face regions. Keep sample identities, engineering materials and the bill of materials independent of appearance changes.

| SELENE area | Candidate material IDs | Intended distinction |
|---|---|---|
| Mare-style terrain and collected fragments | MAT-531–MAT-540 | Packing, disturbance, basalt-like fractures, glass-like shards and breccia-like mixtures; no site-specific identification. |
| Highland-style terrain and samples | MAT-541–MAT-550 | Fracture, cut surface, clast structure and granular assemblies; no mineral percentages or measured reflectance. |
| Dust on hardware and containment | MAT-551–MAT-560 | Sparse, dense, recessed, transferred, cleaned and internally settled deposits; all preserve their host. |
| Wheel rim, shell and grousers | MAT-561, MAT-562, MAT-563 | Machining, forming and localized contact finishes remain separate. |
| Candidate compliant wheel constructions | MAT-564, MAT-565, MAT-566 | Spring wire, woven mesh and sheet flexure geometry; no validated mobility performance. |
| Chassis, hub covers and joints | MAT-567, MAT-568, MAT-569, MAT-570 | Anodized faces, sealing lands, articulated components and sensing-cover regions. |
| Arm tube and wrist | MAT-571, MAT-572 | Composite orientation versus machined-metal operation marks. |
| Gripper jaws and contact pads | MAT-573, MAT-574 | Geometric contact teeth versus separate dielectric pad inserts. |
| Scoop, abrasive tip and transfer funnel | MAT-575, MAT-576, MAT-577 | Formed surfaces, mixed abrasive regions and polished internal transfer faces. |
| Arm routing, capture liner and identification | MAT-578, MAT-579, MAT-580 | Cable-guide body, removable liner and original tool markings. |
| Rigid transparent vessels and windows | MAT-581, MAT-582, MAT-583, MAT-587, MAT-588 | Ampoule, vial, crystalline-looking window, capillary and double-wall interfaces. |
| Transparent liner, bag and inspection mount | MAT-584, MAT-585, MAT-586 | Independent inner liner, folded film and embedding-medium concepts. |
| Inspection witness and coated cassette lid | MAT-589, MAT-590 | Original etched markings versus an explicitly modelled antireflection stack. |
| Metal sample bodies | MAT-591, MAT-592, MAT-593 | Aluminium-like machining, titanium-like brushing and stainless-like polished interiors. |
| Sample insert and opaque cassette | MAT-594, MAT-595, MAT-600 | Separate insert material, polymer-body candidate and individually identified machined cells. |
| Cap and sealing candidates | MAT-596, MAT-597, MAT-598, MAT-599 | Threaded cap, plated metal gasket, fluoropolymer-like ring and alternative elastomer ring. |
| Thermal blankets and films | MAT-501–MAT-510 | Layer order, folds, spacer mesh, seams and exposed multilayer edges. |
| Solar arrays and optical devices | MAT-511–MAT-520 | Optical cover, cell-like regions, interconnects and reference-device concepts. |
| Harnesses and mechanism interfaces | MAT-521–MAT-530 | Braids, films, insulators, bellows, joining regions and original labels. |

### Lunar visual constraints

- Use a vacuum scene. Regolith and dust are particles, packed ground or surface deposits; they are not terrestrial atmospheric haze, smoke or fog.
- Track relief, scoop disturbance and transferred dust are authored scene states unless a separate validated granular simulation produces them.
- Do not bake illumination or a bright edge into a dust texture. Preserve direct-light shadow structure and the selected environment's actual illumination.
- Lunar-inspired names do not establish sample provenance, mineralogy, maturity, albedo or a measured bidirectional reflectance function. Any later measured dataset needs its own source and conditions.
- Dust-covered solar cells, optics and seals are visual comparisons only. They do not provide power loss, optical throughput, contamination, friction or sealing measurements.

## Renderer dependencies and representation choices

The `capability_requirements` entries refer to the shared CAP-001 through CAP-104 registry. Content priority describes authoring priority, not a guarantee that the required renderer feature already exists. A P1 content item can depend on a P2 engine feature. Compatibility checks must inspect the full dependency list.

| Requirement | Relevant capabilities | Consequence for the catalog |
|---|---|---|
| Base material, Fresnel, physical texture scale and colour handling | CAP-017, CAP-018, CAP-033, CAP-040, CAP-057 | Maintain a common calibrated-or-declared baseline; keep data maps out of colour transforms. |
| Directional manufacturing and fibre finishes | CAP-020, CAP-034 | Brushing, turning, extrusion, winding and grain need stable local coordinates. |
| Coatings and non-destructive surface states | CAP-021, CAP-038, CAP-039 | Keep substrate, film, printing, wear and deposit masks independently editable. |
| Real refractive thickness and nested media | CAP-025, CAP-026, CAP-027 | Containers, films, lenses, bubbles and liquid/sample boundaries need correct interfaces. |
| Subsurface and bulk scattering | CAP-028 | Diffusers, filled resins, translucent polymers and liquids require scale-aware scattering. |
| Metal flakes and specialized optical structures | CAP-023, CAP-031, CAP-098 | Sparkle, thin-film interference and multilayer interference are distinct mechanisms. |
| Diffraction, retroreflection and spectral conversion | CAP-099, CAP-100, CAP-101 | These are not ordinary roughness, a camera bloom filter, or constant RGB emission. |
| Excitation history and saved temporal emission state | CAP-102 | Phosphorescence requires a state; static renders must record the state they depict. |
| Lenslets and optical microgeometry | CAP-103 | Facets and micro-lenses must actually redirect rays when physical behaviour is claimed. |
| Fibre, mesh, pore and particulate geometry | CAP-036, CAP-044, CAP-104 | A normal map cannot create real openings, silhouette changes or internal interfaces. |
| Robust CAD assignment and precision | CAP-090, CAP-097 | Keep appearance attached to stable component/face references; do not change the engineering B-rep. |

Emission from an LED or display can be an authored steady source without simulating semiconductor physics. A physical phosphor-conversion model is a different capability: it must respond to excitation through CAP-101. Likewise, RGB spectral-looking colour, wavelength-dependent transport, polarization, interference and diffraction are not interchangeable. A sapphire-like window using a scalar approximation must not claim birefringent accuracy; that requires specialized optical support such as CAP-032 within its documented scope.

## Units, data and clean-room boundaries

- Use mm for scene lengths, radians for stored angles and Z-up coordinates. If an optical control is entered in another unit, convert it explicitly; never silently mix optical-film units and model dimensions.
- No numerical IOR, conductivity, friction, thermal, structural or emissivity values are fabricated in these recipes. Add measured values only with source, grade, conditions, spectral range where relevant and uncertainty or applicability information.
- Engineering material selection remains separate from appearance. No recipe certifies flight readiness, vacuum compatibility, outgassing, sample cleanliness, sealing, electrical performance or a manufacturing result.
- Descriptions and proposed procedural recipes are original authoring under `ORIGINAL-2026-10-CATALOG`. No commercial material presets, proprietary CAD extraction, GPL/LGPL/AGPL source code or copied artwork were used to create this catalog.
- Optional later reference capture, measured data, photographs or external assets require their own provenance and redistribution review. AI-assisted image creation would remain authored imagery, not automatically a measured BSDF or validated physical texture set.
- The authoring script is an offline data/document-generation aid, not application runtime code. It introduces no Python dependency into SolidCraft's Rust application.

## Validation and publication gate

The script verifies counts, ID sequence, name uniqueness, complete English-field structure, accepted priority/cost values, capability-ID bounds and original provenance. Per-recipe physical and visual checks remain proposed, not completed.

Before publishing an executable preset, implement its recipe, resolve required capabilities, render a neutral coupon/curved-part/edge set, check physical scale and filtered detail, validate CPU/GPU behaviour within declared tolerances, record any approximations, and preserve source and resource versions. Show one of three states in the UI: **available with supported model**, **available with an identified approximation**, or **not yet implemented**. Never replace an unsupported model silently.

Treat `materials.json` as the canonical planning data. Future importers must validate the schema, required capabilities and provenance before applying a recipe.
