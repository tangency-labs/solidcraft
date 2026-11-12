# Electronics library, parts and formats

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

## 1. Reuse the library pack system

The repo already proposes one offline pack envelope for materials, appearances and component families
(`docs/book/src/dev/libraries.md`, #785–#787):

- immutable revisions and stable namespaced ids;
- typed payload plus required resources, each with a content digest;
- author, source, licence and included licence text;
- explicit dependencies, bounded and acyclic;
- bundled, project and user scopes, with deterministic discovery;
- designs embed the definitions they use, so a saved design opens without the pack.

Electronics adds an **electronic part** entry kind to that system. It does not add a second library
format, a second installer or a second provenance model.

## 2. The part record

```text
Part {
  id, revision, kind,
  manufacturer, mpn, description, category, package,
  pins: [{ number, name, electrical_type, alternate_functions, unit }],
  symbol: { units[], graphics, pins, body },
  footprint: { pads[], courtyard, silkscreen, assembly, keepouts, model_transform },
  model3d: { hash, units, transform, fidelity, source, licence },
  spice: { kind: subcircuit|model, text, pin_map },
  ibis: optional,
  datasheet: { url or local path },        // linked, not bundled by default
  supply: { distributor, sku, price, stock } // optional, user-entered or imported
  provenance: { author, source, retrieved, checksum },
  licence: { name, text or url, redistribution }
}
```

Rules:

- **Immutable revisions.** Editing a part creates a revision; existing designs keep the old one.
- **Embedded snapshots.** Symbol, footprint and model bytes are copied into the design. The source
  identity stays for inspection and an explicit update.
- **Explicit updates.** `library.part.update` shows pin/pad/net changes and refuses to apply a change
  that would silently disconnect a net.
- **Validation is a command.** Pin/pad numbering, duplicate pads, courtyard, pad-to-pin mapping,
  3D transform and model units are checked; failures are typed and per part.
- **Parametric fallbacks.** Packages without a vendor model get original parametric geometry:
  chip packages, SOT/SOD, SOIC/TSSOP, QFN/DFN, QFP, BGA, headers, common connectors. This is what
  makes renders complete without shipping third-party CAD.

## 3. Where part data comes from

| Source | Path | Bundled with SolidCraft? |
|---|---|---|
| User's KiCad libraries | `library.import_kicad` reads symbol/footprint/3D files from the user's disk | No |
| User STEP/GLB models | Imported into a part | No |
| Parametric package generators | Original code and data | Yes |
| Original demo library | Authored for SolidCraft | Yes, with attribution rows |
| SPICE models supplied by the user | Imported into the part or a project library | No |
| Datasheets | Link only | No |
| Future provider adapters | Optional, user-initiated, user key, local cache | No |

No scraping and no background network. The core app remains offline; a future provider integration
is an explicit library action with per-provider terms, and is not required for any feature.

## 4. Compatibility contract

Adoption depends on opening what people already have. The contract is a versioned subset, tested
with real files supplied by users, not a vague "supports KiCad".

| Format | Direction | First gate | Notes |
|---|---|---|---|
| KiCad schematic `.kicad_sch` | import + export | E1 | S-expression; hierarchy, symbols, wires, buses, labels |
| KiCad board `.kicad_pcb` | import + export | E2 | Footprints, pads, tracks, arcs, vias, zones, stackup, rules |
| KiCad symbols `.kicad_sym` | import | E0 | Libraries from the user's installation |
| KiCad footprints `.kicad_mod` | import | E0 | Including 3D model paths |
| KiCad project `.kicad_pro` | import/export | E6 | Net classes, rules, settings where meaningful |
| Eagle XML `.sch` / `.brd` | import | E1/E2 | Legacy but common |
| IPC-2581 | import + export | E3 | Open exchange for assembly and review |
| ODB++ | export | E3 | Assembly exchange; import later |
| Gerber X2 | write + read | E3 | Read for review and round-trip checks |
| Excellon | write + read | E3 | Plated/non-plated, slots, tool table |
| SPICE netlist | import + export | E4 | Documented subset and dialect notes |
| STEP | import for 3D | E0 | Existing importer; component models |
| DXF/STEP board outline | export | E0 | Mechanical and CAM hand-off |

Loss reports are mandatory in both directions: preserved, approximated, dropped, with locations.

Altium and Allegro binary formats are not direct targets. Their users reach SolidCraft through
IPC-2581, ODB++ or STEP export from those tools.

## 5. Fabrication formats and profiles

- **Gerber X2** for copper, mask, paste, silkscreen, mechanical and drill maps, with file attributes
  (`TF.FileFunction`, `TF.Part`) and aperture attributes.
- **Excellon** for plated and non-plated drills, slots and tool tables.
- **IPC-2581** for a single complete manufacturing description.
- **BOM/CPL** column layouts per fab profile, with DNP, variants, side and rotation conventions.
- **Panel** output: rails, fiducials, mouse-bites, V-grooves, array origin and tooling holes.
- **Fab profile** is data, not code: minimum track/clearance/drill/annular/soldermask/silkscreen,
  board size, layer count, material and surface finish, plus the assembly column mapping. Profiles
  are versioned, editable and attributable.
- **DFM** runs the profile rules and reports violations with layer and coordinates; it is not a
  guarantee of acceptance.

A package command writes a zip with a manifest, readme, layer mapping and SHA-256 checksums. The app
can re-open its own Gerber/IPC output to review it.

## 6. File safety

Every imported file is hostile input:

- caps on file size, entry count, layer count, object count and text length;
- no filesystem extraction from archives; blob hashes verified;
- no execution of scripts, models or macros from a file;
- path traversal, ZIP64, multi-disk archives and duplicate entries refused, consistent with the
  container reader;
- malformed or unsupported constructs produce a named error or a loss report, never a panic;
- fuzzing for every importer is part of the milestone, not a follow-up.

## 7. Licence rules

The ground rules are the same as the rest of the repo: no GPL/LGPL/AGPL code, not even reading it.
That shapes the implementation:

| Item | Licence reality | Consequence |
|---|---|---|
| KiCad application code | GPLv3 | No reading, no porting. Formats are implemented from published specifications and user files |
| KiCad official libraries | CC-BY-SA 4.0 with a design exception | Import from the user's disk; do not bundle or redistribute |
| ngspice / Xyce / Qucs-S | GPL | No linking, no reading; SPICE is clean-room from textbooks and public device equations |
| Vendor 3D models and datasheets | Per-vendor terms, usually no redistribution | User-supplied; link datasheets; record provenance |
| Gerber X2 specification | Published by Ucamco | Implement from the specification |
| IPC-2581 | Open IPC standard | Verify the current terms before shipping test data |
| ODB++ | Published format | Verify terms; export first |
| Parametric package geometry | Original | Authored in-tree with attribution rows |

When in doubt: implement from the specification, test with files the user supplies, and do not copy
code or data.

## 8. Size and caching

- A design embeds used parts once, by content hash; duplicate parts and shared models are stored
  once in the container.
- Default: embed symbol, footprint and parametric 3D. Vendor STEP models are embedded when they fit
  the design budget; otherwise they are linked with a warning and the render reports the lower
  fidelity.
- Datasheets are links, not blobs.
- The library cache is outside the design, keyed by revision and hash, and can be cleared without
  touching a design.
- Preflight reports embedded-part bytes, board bytes and estimated save/open cost.
