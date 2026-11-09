# RFC 0001: Craft bridge, handing work between Craft apps

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-07 |
| Apps | SolidCraft, CamCraft, PCBCraft |
| Discussion | the pull request that adds this file, then an issue per phase |

Nothing here is built yet.

## 1. Goals and non-goals

**Goals**
- One button: "Send to CamCraft" opens the current design in CamCraft, ready to set up.
- Round trips that stay linked: edit the model in SolidCraft, and CamCraft sees the new revision, re-finds its picked faces and regenerates or marks paths stale.
- The same for PCBCraft: board outline, holes and parts go to SolidCraft; the enclosure's outline and mounting points go back.
- Nothing is lost quietly. A reference that no longer resolves is reported by name.

**Non-goals**
- No merged app and no shared process. Each app runs and ships on its own.
- No shared file format. `.sc`, `.camproj` and PCBCraft's project stay as they are. The bridge adds one small exchange format between them.
- No editing another app's data. Each item has one owner app.

## 2. What crosses the bridge

One **exchange file**, `*.craftx`: a ZIP like the `.sc` container, with parts addressed by SHA-256 and a manifest.

| Part | Content | Notes |
|---|---|---|
| `manifest.json` | `format: "craft-exchange/1"`, source app and version, `source_id` (permanent design id), `revision`, content hash, units, the parts it has | Required. Readers ignore parts they don't know. |
| `bodies/<n>.record` | Exact B-rep as a solidcore-record body file | Preferred geometry. CamCraft can read it once its Solidcore milestone (M12.1) lands. |
| `bodies/<n>.step` | STEP of the same body | Fallback any app reads today. |
| `mesh/<n>.bin` | Triangles, plus a per-triangle face index | For apps that cut or display meshes. CamCraft slices meshes today. |
| `faces.json` | Per body: each face's index → persistent name, `exact` flag, surface type, aliases (names it also answers to after a merge). Edges the same way (`<face A>\|<face B>`). | The key to surviving edits. |
| `params.json` | Exposed parameters: name, value, unit, expression (read-only) | So CAM can show "wall = 3 mm", and PCB can drive SolidCraft parameters later. |
| `meta.json` | Material, part number, stock hint (box or offset), colour per face | Taken from design and component metadata. |
| `board.json` | PCB only: outline, cutouts, holes with names, part placements and heights, keep-outs | Defined with PCBCraft. |

**Stable ids.** SolidCraft already names every face and edge from kernel history (`F2:top`, `F4:side:l4`, `F2:top#[…]` for pieces, `F5:blend:<edge>` for rounds). `model.faces` reports them, with `exact: false` where a name was found by geometry. The exchange carries these strings as given. Receivers treat them as opaque strings. CamCraft picks today are facet-geometry hashes; a linked model would also store the face name, which survives an added hole where a facet hash may not.

## 3. Transport

| Option | Desktop | Browser (wasm) | Good for | Weak at |
|---|---|---|---|---|
| **A. Exchange file + file watching** | Yes. Write beside the design, watch for changes. | Same-origin apps can share OPFS. No watching, so poll or use the `BroadcastChannel` below. | Durable, works with either app closed, easy to debug | Update waits for a save |
| **B. Control channel session** (SolidCraft's existing loopback socket, token file, `control.subscribe` → `design.changed`) | Yes, already shipped | No sockets. The same messages can go over `postMessage` or `BroadcastChannel` between same-origin tabs. | Instant change notice, handshake | Both apps must run |
| **C. MCP** | Yes (stdio) | No | Agents driving both apps | Made for agent to app, not app to app. No events between apps. |
| **D. Launcher / "open in"** (start the other app with a path; later a registered file type or `camcraft://open?path=`) | Yes | `window.open` the other app's URL with the file in shared OPFS, or hand it over with `postMessage` | The button | Carries no updates by itself |

**Recommendation: A + D now, B as a speed-up.** The exchange file is the contract and the only thing an app must read. The button writes it and launches the other app with its path; live linking watches that file. A running app may also take a control-channel notice for faster updates. MCP stays for agents.

## 4. Linking and change propagation

- **A link** in the receiving project is `{source_id, path hint, revision, content hash}`. `source_id` is the permanent id. The path is only a hint, as in PCBCraft's file rules and SolidCraft's Insert Design links.
- **Change detection**: the exchange file's hash differs from the linked one → there's a new revision. Taking it can be automatic or one click; the receiver decides.
- **Re-resolving references** (each receiver does this, with the same outcomes SolidCraft uses for linked designs):

| Outcome | Meaning | Receiver does |
|---|---|---|
| exact | the name is there | keeps the pick |
| merged | another face answers to it as an alias | keeps it, notes the merge |
| split | several pieces carry the name | uses all pieces, or asks the user if that changes the meaning |
| by geometry | the name only matched on an `exact: false` face | keeps it, flags "check this" |
| missing | gone | **marks the operation broken, naming the face and where it was. It keeps the old path for viewing, refuses to post it, and asks the user to re-pick. Never dropped quietly.** |

- **Who owns what** (one owner per item; the other side holds a read-only linked copy, and Break Link turns it into its own editable copy):

| Item | Owner | Others get |
|---|---|---|
| Part geometry, parameters, face names | SolidCraft | CamCraft, PCBCraft (read) |
| Setups, stock, WCS, tools, toolpaths, posts | CamCraft | Nothing yet (maybe stock and as-machined bodies later) |
| Board outline, holes, part placement and heights | PCBCraft | SolidCraft (a linked body and sketch, like an inserted design) |
| Enclosure-driven outline and mount points | SolidCraft | PCBCraft (read, shown as a constraint it can accept) |

To change something you don't own, "Edit in SolidCraft" (or the owner app) opens the source.

## 5. The SolidCraft side

Every step is a command with input and output schemas. Programmatic calls never open dialogs.

| Command | Does |
|---|---|
| `bridge.export {path?, bodies?, parts?: [record, step, mesh], mesh_tolerance?}` | Writes a `.craftx` and returns `{path, revision, hash, faces, warnings}`. Refuses with `BadParams` on a design with feature errors or no bodies, saying why. |
| `bridge.send {app, bodies?}` | Exports, then launches `app` with the path. If the app isn't found, it returns an error that names the setting to fix. No file picker. |
| `bridge.links` | Lists the targets this design exports to and the exchanges it has inserted, each with revision, `changed` and missing references. |
| `bridge.auto {target, on}` | Rewrites the exchange on every save. |
| `file.insert_exchange {path}` | Brings in a PCBCraft board (or any `.craftx`) as a linked component. It reuses Insert Design's link, Update Links and Break Link machinery and face matching. |
| `component.update_links` | Also covers inserted exchanges. Reports joint and reference errors as it does today. |

Face names, `model.faces`, linked-design face matching, design metadata and `control.subscribe` already exist.

**Format additions** (one file per change in `crates/doc/src/format/pending/`):
- `design-source-id.rs`: a permanent random id for each design, made on first save. Older designs get one when next saved. A copy made with Save As gets a new one.
- `bridge-targets.rs`: the export targets (`app`, path hint, auto on or off).
Inserted exchanges ride the existing Import feature link fields. An older release warns, opens what it can and keeps the rest. Another app's private data, if ever stored in a design, goes under its own reverse-DNS metadata key, which SolidCraft already preserves.

**UI**: a "Send to CamCraft" button, and later "Send to PCBCraft". It changes the default ribbon, so the owner decides where it goes before it merges.

## 6. A contract any Craft app can implement

- **Read**: `craft-exchange/1`. A reader reads the parts it knows, warns about the rest, and never refuses a file for a newer minor version.
- **Versioning**: `craft-exchange/<major>`. Minor additions are new optional parts or fields. Only a major bump may change meaning. The manifest lists `requires` (the parts a reader must understand to use the file safely) separately from `parts`.
- **Capabilities**: strings with versions, e.g. `brep.solidcore-record/1`, `step/ap214`, `mesh/1`, `faces.names/1`, `params/1`, `meta/1`, `board/1`. A writer includes what it can. A reader takes the best it supports, preferring record, then STEP, then mesh.
- **Live session (optional)**: `bridge.hello {app, version, protocol: [1], capabilities}` gets the same answer back. Both use the highest common protocol and the capabilities they share. After that come `exchange.changed {source_id, revision, hash, path}` notices and `exchange.request {source_id}` asking for a fresh export. On desktop these go over the control channel. In the browser they go over `BroadcastChannel`, with the same JSON.
- **Launch**: each app accepts `--open <file.craftx>` (or a file path argument). A running instance should take the file over instead of starting a second window.
- **Units and frames**: millimetres, in the source design's world frame; the manifest carries the display unit. The receiver places bodies (WCS, component placement).

## 7. Phases

| Phase | Delivers | Who |
|---|---|---|
| 0 | Agree on this contract and the manifest | Both authors |
| 1 | **Send to CamCraft**: `bridge.export` (STEP, mesh, face table) plus launching CamCraft with the path. CamCraft imports the STEP as it does today. | SolidCraft. CamCraft only needs a path argument. |
| 2 | CamCraft reads `faces.json`: picks store face names, and re-sending updates the same model (its existing UpdateModel and stale marking), with the outcome table above | CamCraft |
| 3 | Live: `bridge.auto` + file watching. CamCraft either marks paths stale or regenerates; it never posts on its own. Plus a control-channel notice for running apps. | Both |
| 4 | Exact B-rep through solidcore-record, once CamCraft moves to Solidcore | Both |
| 5 | PCBCraft: board → SolidCraft via `file.insert_exchange`, and enclosure outline → PCBCraft, once PCBCraft has boards | PCBCraft, SolidCraft |
| 6 | Browser: shared OPFS, `BroadcastChannel`, `window.open` hand-off between same-origin web builds | All |

## 8. Open questions for CamCraft's author

1. **Would CamCraft store our face and edge names on its picks, next to its facet-hash ids, and re-resolve by name first?** That one choice decides whether CAM setups survive model edits.
2. Does `camcraft-ui` accept a file path at launch, and can a running window take a second file instead of opening a new one?
3. Should a linked model update automatically and mark paths stale, or regenerate them too, and with what confirmation?
4. Should stock and material come from SolidCraft (a stock hint in `meta.json`), or stay CamCraft-only?
5. Where should the exchange file live: beside the `.sc`, beside the `.camproj`, or copied into the project, as CamCraft keeps facets today?
6. Is there anything CamCraft wants to send back, such as the as-machined body for drawings, or fixture bodies for assemblies?
7. What's the timing for Solidcore (M12.1), so we know when to put the B-rep record first?
8. Would CamCraft add the optional live session over a socket, or is watching the file enough?
