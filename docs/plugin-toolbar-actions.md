# Plugin commands in Utilities (proposal)

Status: design for discussion in [#4928](https://github.com/tangency-labs/solidcraft/issues/4928),
related to [#1221](https://github.com/tangency-labs/solidcraft/issues/1221).
This document does not implement a runtime or add buttons to current releases.

## User outcome

A user installs a plugin folder, chooses Refresh in Preferences > Plugins, and
gets that plugin's action in the Design workspace's Utilities tab. Clicking the
action asks for its declared parameters and runs it in the active design.
Installing, enabling, disabling or changing that package does not require a
SolidCraft source change or rebuilding SolidCraft.

There are two distinct requirements:

- **No application rebuild:** a supported plugin package can add its own command
  and toolbar placement at runtime. This also works for a separately built
  WebAssembly component.
- **No plugin compilation:** an author can edit a plain script and run the edited
  source. This additionally requires a scripting runtime; a WebAssembly component
  alone does not promise it. Native Python is an optional design question below.

The installed host must support this API first. A new manifest cannot add an
execution capability to an older metadata-only application.

## Current boundary

`crates/ui-egui/src/plugins.rs` scans installed metadata and saves disabled ids.
It explicitly does not load or execute plugin code. Its manifest accepts only
`id`, `name`, `version` and `description`, with unknown fields refused.
Merged [#2718](https://github.com/tangency-labs/solidcraft/pull/2718) supplies that
page. [#4330](https://github.com/tangency-labs/solidcraft/pull/4330) proposes search
and installation from GitHub; its installer also does not run code.

This proposal covers command contributions and their UI lifecycle. It does not
replace the sandbox/permissions work in [plugins.md](plugins.md), take ownership
of the complete #1221 runtime, or change the Manufacture backend proposals.

## Proposed manifest extension

Keep existing metadata-only manifests valid. Add a versioned extension rather
than silently accepting arbitrary new fields. The following is a candidate v2
shape for discussion, **not a supported manifest today**:

```json
{
  "manifest_version": 2,
  "id": "cube-generator",
  "name": "Cube generator",
  "version": "0.1.0",
  "description": "Creates a cube in the active design.",
  "runtime": {
    "kind": "python-script",
    "entry": "cube.py"
  },
  "commands": [
    {
      "id": "create_cube",
      "label": "Create cube",
      "placement": {
        "workspace": "design",
        "tab": "UTILITIES",
        "panel": "PLUGINS"
      },
      "parameters": [
        {"id": "size", "label": "Edge length", "kind": "length", "unit": "mm", "default": 10, "min": 0.1, "max": 100000}
      ]
    }
  ]
}
```

`python-script` illustrates the uncompiled-script use case and needs explicit
maintainer agreement. The contribution format should also work with the WIT
component runtime planned in #1221, with no second toolbar implementation.

- Qualify each command as `plugin.<package-id>.<local-command-id>`. Reserve the
  namespace, reject collisions with installed or built-in commands, and use
  stable ids for pinned buttons and shortcuts.
- Register contributions in the same host command registry used by the palette,
  toolbar, CLI/control channel and MCP. A toolbar button is a presentation of that
  command, not an independent execution path.
- Initially allow only Design > Utilities > Plugins placement. The host controls
  order, overflow, labels and interaction through its existing toolbar/UI kit.
  Other tabs and workspaces can be added after that boundary is agreed.
- Use an existing host icon id or a host-chosen fallback. Arbitrary images and
  custom widget/window code are not part of this first slice.
- Reuse the future host parameter schema and unit-aware fields. Explicit unit
  conversion must preserve a 10 mm input in an inch document too.
- Set concrete manifest, command-count, string and parameter budgets before
  implementation. Reject invalid paths, unsupported versions and unknown fields
  within each version with an actionable error.

## Discovery, refresh and disable

Discovery and installation read declarative metadata only. They do not import a
Python module, start an interpreter or execute a WebAssembly component.

Refresh validates a complete replacement catalog before publishing it to the
command registry and toolbar. A removed or disabled package loses its active
buttons and execution routes. Existing saved geometry remains native geometry.
Pinned references retain their stable ids and explain a missing plugin instead
of invoking another command with the same label.

A changed script is read for the next explicit invocation; the app is not rebuilt.
If execution approval is recorded, bind it to the relevant package/code revision
so an updated package cannot silently inherit broader trust. Files changing
between validation and invocation must be handled by the chosen runtime's
package snapshot/loading policy.

## Invocation and document safety

The host resolves the selected plugin command, validates parameters and runtime
availability, then dispatches it as a cancellable job. Runtime failures and
missing dependencies are shown as errors; the UI continues repainting.

All model changes use the existing engine command surface. For the initial
one-shot slice, collect or stage edits and commit them as one undoable,
journaled transaction. A failure or cancellation discards partial edits. Record
the resulting native features so reopening the design does not execute plugin
code or require that plugin to be installed.

Capture the target document and active component, not just the current tab at
completion. If that target changes while the job runs, reject or explicitly
reconfirm the result. Selection, camera and unrelated documents remain unchanged
unless an approved command explicitly requests otherwise.

## Runtime decision: uncompiled Python

The default direction in #1221 is a sandboxed WIT component with bounded compute,
memory and granted capabilities. Toolbar contributions should preserve it.

A plain OS Python subprocess is a different trust model. Without an OS sandbox,
it has the user's file and process access; a command allowlist or timeout does
not remove those capabilities. If supported, make it an explicit, optional
desktop-only trusted-script mode, with no automatic execution on discovery,
installation or document open. Ordinary SolidCraft modelling must not require
Python to be installed. The host remains Rust.

Before implementing that mode, agree on interpreter discovery, explicit trust,
the request/result protocol, bounded output, cancellation and process-tree
cleanup. If the project chooses WIT only, document that users avoid rebuilding
SolidCraft but may still need to build their plugin component. Do not advertise
plain `.py` installation in that case.

## Acceptance criteria for implementation

1. Install one small package and Refresh. Its named action appears in Utilities
   without changing or rebuilding the installed app.
2. Enable/disable/remove/refresh updates the toolbar and shared command routes;
   metadata-only packages continue to list as before and execute nothing.
3. Run the cube action with 10 mm. Measure a 1000 mm³ native body. Undo removes
   exactly that action; Redo restores it. Save and reopen without the plugin.
4. If trusted scripts are accepted, edit the script to emit different geometry
   and invoke it again. The new behavior runs without compiling the script or app.
5. Test malformed manifests, unsupported schema/runtime, path escapes, duplicate
   ids, missing interpreters, invalid units/ranges, runtime errors and cancellation.
6. A document/component switch during a job cannot apply its result to another
   target. A failed multi-command action leaves the model and history unchanged.
7. Demonstrate the toolbar and parameter flow in the actual app, using the shared
   UI kit and command surface, with public-safe screenshots and regression tests.

## Decisions requested

- Is a declarative Utilities command contribution the right first UI surface?
- Should contributions be declared in the install-time manifest, the WIT
  `manifest()` export, or a shared schema exposed by both? Discovery must stay inert.
- Is an optional trusted Python mode acceptable, or should source scripting use
  a different sandboxed language/runtime? This determines whether plain scripts
  can meet the no-compilation workflow.
- Which invocation/context and parameter types should be shared with #1221
  before implementing registration and toolbar rendering?
