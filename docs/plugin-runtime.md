# Experimental command plugin runtime

This is the implemented one-shot native runtime. It does not implement every part of
[issue #1221](https://github.com/tangency-labs/solidcraft/issues/1221), the broader
[interface proposal #4980](https://github.com/tangency-labs/solidcraft/pull/4980), or
[Utilities placement proposal #4933](https://github.com/tangency-labs/solidcraft/pull/4933).
The [current plugin RFC](rfcs/0002-plugins.md), especially its Decisions section, takes
precedence over these earlier proposals. The shared command world is defined in
[`craft-plugin`'s WIT](../crates/craft-plugin/wit/plugin.wit). The package
manifest uses an optional `runtime` block so old metadata-only packages still work. Its manifest
layout and explicit per-invocation approval flow are deliberately documented as experimental,
not as a claim that the outstanding design proposals have been accepted.

## Boundary and lifecycle

`craft-plugin` owns the manifest, typed errors, permissions and `PluginHost` trait.
`craft-plugin-wasm` runs components against that trait without app dependencies.
SolidCraft's engine implements the trait with its command schemas and transaction surface.
The runtime takes ownership of a host and returns `(host, result)` on success and failure.
This satisfies Wasmtime's requirement for store data without borrowed references. SolidCraft
supplies an owned scratch session and adopts its edits only after a successful call.

1. Install a plugin zip through **Preferences > Plugins** or `plugin.install`. Its `dry_run: true`
   mode describes the package without writing files or grants. The single installation confirmation
   screen collects command-group and named-folder approvals. `plugin.install` stores only the supplied
   `approve` grants; omitting `approve` grants nothing. A manually installed package also receives no
   stored grants. See [Installing](plugins.md#installing).
2. `plugin.inspect` reads bounded bytes through the caller's existing file scope and returns
   the manifest and content approval hash. It neither instantiates the component nor invokes
   a manifest function. Metadata can therefore be reviewed before code runs.
3. The user reviews the package and explicitly calls `plugin.run` with that exact hash, an
   exported entry and JSON parameters. The package is read once for this call; the bytes whose
   hash was checked are the bytes compiled and executed. Changing the files afterward cannot
   substitute different code for that invocation.
4. The host creates a fresh Wasmtime engine/store, with fuel, epoch interruption and a resource
   limiter, and links only the published component interfaces. No WASI is linked. Reserved
   sockets, HTTP and fetch imports are refused even if unused. Instantiation and the entry
   share the same execution budget, including component start functions.
5. `commands.run` reaches `PluginHost::run_command`, which SolidCraft implements with
   `Session::execute` on a scratch session, under an empty file scope (a granted `file` or
   `doc` command sees only the granted folders; see below).
   Explicit paths and implicit reads during feature evaluation cannot inherit the caller's broader
   file access. Results are bounded JSON. Refused calls poison the transaction, even if guest code
   catches the error and returns success.
6. The result must be valid JSON, every newly changed feature must evaluate, and the call must
   remain within its budget. Only then are the native results accepted as one undo step.
7. The desktop control channel performs the whole computation on a worker, warms the resulting
   display and adopts the existing computed-commands transaction exactly once. A changed design,
   authoring context, selection, disabled-plugin policy, or Stop prevents adoption. It does not
   re-run the plugin on the window thread. Headless CLI execution is synchronous.

No discovery, preference toggle, document open or saved feature executes a component. Every
invocation gets a fresh instance, so guest memory is not persistent state. A design is data, never
an instruction to install, approve, download or execute a package.

## Manifest

The metadata fields remain `id`, `name`, `version`, and optional `description`. IDs are 1..64
lowercase letters, digits and hyphens, without a leading/trailing hyphen, and must equal the folder
name. Names are at most 80 characters; versions at most 32; descriptions at most 240. Control
characters and unknown fields are refused. The manifest has a 16 KiB byte cap.

`capabilities` lists the host capabilities the plugin needs, as `name@major.minor`. A package with
a `runtime` needs `"commands@0.1"`. The host checks the list when it reads the manifest
(inspection, listing, running) and refuses an unknown or newer capability with a typed
`unsupported_capability` error in plain words, for example `needs commands 0.2; update SolidCraft
or the plugin`. Nothing crashes and no code runs. A manifest from the earlier preview, which named
`runtime.api: "solidcraft:plugin@1.0.0"`, is told what it was built for. A component importing
another version of the `craft:plugin` interfaces is refused the same way before it is instantiated.

`runtime` is optional for metadata-only packages. Executable packages declare:

| Field | Meaning |
|---|---|
| `module` | One lowercase `.wasm` file name within the package; no directories |
| `entries` | 1..32 distinct entries with `name`, `effect: read \| edit`, `input_schema` and optional `slots`; legacy string ids default to edit |
| `commands` | At most 128 exact command ids from the app's MCP catalog; a sensitive command also needs its group in `permissions.command_groups` |
| `limits` | Optional object with all of `fuel`, `memory_mib`, `deadline_ms`; omitted object uses defaults |
| `permissions` | Optional object: `files` requests named read/write folder grants, `clock` requests wall clock, `command_groups` requests sensitive command groups; `net: true` is refused |

Entry inputs are object schemas; selection and target fields are arrays. Named slots use
`{"kind":"menu","name":"…"}` or `{"kind":"ribbon","name":"…"}`; the app decides which
slots exist. SolidCraft matches File and Help menu names ignoring case; other menu names draw no
host menu items. The app bar's Plugins menu lists every installed entry regardless of its slots.
Ribbon names match the existing Design tabs ignoring case; other names use the Plugins tab.
Only enabled plugins contribute host slots. Each plugin's ribbon group carries its name, and every
slot item has a plugin badge. Slot clicks resolve the namespaced entry against the Plugins menu's
shared list and open the same input-schema dialog through `plugin_menu::open_entry`. The dialog
uses the same enabled state and grant checks from every placement. Opening it runs no plugin code.
See [menu and ribbon placement](plugins.md#menu-and-ribbon-entries).
Declaring a slot never supplies UI code. `plugin.list` and `engine.describe`
expose the entry declarations. A read entry can call only commands marked read in the app catalog.

`files.read-file` and `files.write-file` resolve paths only within matching app-approved folder
grants with the requested access. The caller's file scope is checked against each complete
path on every read and write; hidden paths require explicit authorization. Declaring a folder
name does not itself grant access. Writes use temporary sibling files, and reads in the same call
see their staged content. Targets are replaced only after successful guest execution and app
transaction commit. A trap, refusal, cancellation or failed app commit removes staged files
without replacing targets. If publication fails after app commit, the error reports that files
may have changed; publication of several files is not atomic. `clock.now`
returns a fixed epoch without the clock permission. Log, progress and cancellation checks are
always available. There are no socket, HTTP, fetch or WASI imports.

The component must be a WebAssembly **component**, not an ordinary core `.wasm` module, DLL,
executable or script. Unsupported imports fail instantiation; the host never falls back to starting
another process. A dependency is not installed when a component fails to load.

## Command and data surface

Every command in the MCP catalog is reachable: the same ids, input schemas, results and read/edit
effects an MCP client gets (`engine.commands`, `engine.describe`). A package lists the exact ids it
calls; there are no wildcards or aliases. Commands that reach past the open design are grouped,
and each group is refused unless the manifest asks for it in `permissions.command_groups` **and**
the user grants it, as folders are:

| Group | Commands |
|---|---|
| `file` | `file.*`, `library.*`, `sketch.insert_*`, and other commands that read or write files: `canvas.insert`, `component.insert_design`, `parts.insert`, `pointcloud.insert`, `pointcloud.reload`, `parameters.import`, `parameters.export`, `timeline.export_script`, `sketch.export_dxf`, `sheet.export_dxf`, `sheet.export_bend_table`, `render.export`, `motion.export`, `motion.export_animation` |
| `doc` | `doc.*`: opening, recovering or switching designs |
| `app` | `settings.*`, `ui.*`, `diagnostics.*`: app state outside any design |
| `plugin` | `plugin.*`: inspecting or running plugins |

A manifest that lists a grouped command without its group is refused when it is read. A refused
call is a typed `permission_denied` error naming the group, and it poisons the transaction like
any other refusal. Ungrouped commands run under an empty file scope, so even a feature that names
an external font or linked design gets no file access. A granted `file` or `doc` command runs with
a scope of exactly the approved folders that the manifest requests for both read and write.
`plugin.install` validates its optional `approve` object against the manifest and stores grants
by plugin id after successful publication. Each folder approval has `name`, absolute local `path`,
`read` and `write`. Omitting `approve` leaves no grants, including after replacement. Explicit approvals
replace the stored set, so stale grants cannot survive an update. Desktop preferences persist grants
and pass them to the session and its worker copies. `plugin.run` intersects
that id's stored grants with the manifest's requests; another plugin's grants never apply.
A hand-installed package with no grant record receives no groups or folders. Removing a package
through `plugin.remove` deletes its grant record. These are host preferences, never design data.

## Versions and capabilities

The WIT package is `craft:plugin@0.1.0`. Its interfaces (`types`, `commands`, `log`, `control`,
`files`, `clock` and the exported `entry`) make up the capability `commands@0.1`. Later
capabilities arrive as their own interfaces, each named and versioned on its own in the manifest:
`ui`, `viewport`, `formats` and `storage`. While a version is 0.x, a minor version may break the
previous one, so a host for `commands@0.1` runs only `commands@0.1` plugins.

## Per-design plugin data

A design container reserves `plugins/<plugin-id>/` for files a plugin keeps with that design
(`plugin_data` in the design, base64 by `<plugin-id>/<path>` in a plain JSON design). SolidCraft
keeps those files and saves them back byte for byte whether or not the plugin is installed;
opening a design never needs, loads or runs a plugin. Plugins cannot read or write this data yet:
the `storage` capability adds that.

`model.mesh {body, tolerance?}` is a general read-only input boundary. Tolerance is a finite positive
number in millimetres (default 0.05). It returns `body`, its owning `component`,
`frame: "component-local-mm"`, `positions: [[x,y,z], ...]` and `triangles: [[i,j,k], ...]`. Indices are
zero-based. The mesh is of the evaluated body, rather than only the original import's vertices.
The command refuses more than 8192 vertices or 16384 triangles. A plugin's host-call result must
also fit the JSON byte limit; a smaller mesh can still exceed that limit and be refused.
No reconstruction, fitting, sewing, mesh replacement or conversion algorithm is part of this API.

A plugin computes its specialised behaviour externally, then emits ordinary native commands or
validated native features through `timeline.add`. New algorithms do not acquire private kernel
entry points. This initial boundary does not promise arbitrary B-rep construction, a stable binary
kernel serialization, persistent custom feature types, or automatically replaying plugin code on
parameter changes. A later specialised plugin must prove its own geometry, failure, lineage,
attachment, undo and reopen contracts in its own change.

## Consent and identity

`plugin.inspect` returns SHA-256 of a domain tag, the manifest byte length, exact manifest bytes,
and exact component bytes. `plugin.run` compares it before instantiation. Whitespace changes to a
manifest also require reinspection. A matching digest is an identity check, not a signature;
reviewing permissions does not establish whether a publisher is trustworthy.

Stored installation grants authorize only the listed groups and folders; they do not approve
running external code. `plugin.install` with `dry_run: true` validates and describes the archive,
including supplied approvals, without writing anything. Installation validates the zip again before
publication and compares any supplied `approval` hash against the exact manifest and component.
The install dialog sends the dry-run hash, so replacing the archive after review refuses installation
before files or grants are written. Each execution still requires the
reviewed manifest-and-component identity
from `plugin.inspect`. Agent chat separately asks before running external code;
its model cannot turn an inspected hash into user consent. A host or MCP caller is responsible
for its existing user-consent policy and package-folder scope. Desktop disable preferences apply
to the running desktop session and all plugin worker copies; a separate CLI process has its own
policy, not the GUI's preferences.

## Budgets and limits

| Resource | Default / limit |
|---|---|
| Component input | 16 MiB |
| Guest fuel per invocation | 10,000,000; user host ceiling defaults to 100,000,000,000 |
| Guest memory | 64 MiB; user host ceiling defaults to 128 MiB; one memory total |
| Entry deadline | 5 s / 30 s |
| Core instances / tables | 8 / 2 (guest plus canonical ABI shim) |
| Table elements / guest stack | 10,000 / 512 KiB |
| Host engine calls | 256 |
| Params, one host result, final result | 1 MiB each, bounded JSON parser depth |
| Log messages | 1 KiB each, 64 KiB per call; control characters stripped |

Fuel counts metered Wasm operations rather than milliseconds. A substantial separately compiled
CAD routine can use billions of fuel units. The default invocation budgets and default host
ceilings above are unchanged. The initial 128 MiB host ceiling is a conservative policy choice,
not a measured minimum or a guarantee that every plugin fits.

### Change the host ceilings without rebuilding

The native GUI persists `plugin_max_fuel` and `plugin_max_memory_mib` in user preferences.
**Preferences > Plugins** presents exact decimal text fields (**Max plugin fuel** and
**Max plugin memory (MiB)**). Enter, Apply or OK validates and accepts both integers together;
an incomplete or invalid field cannot silently save a numeric prefix. Cancel restores the last
Apply/open snapshot. Restore defaults resets this page to the original host ceilings. The
`ui.prefs` control method uses the same strict validation. Agent chat cannot change preferences;
guests have no preference or host-policy command.

A standalone CLI or `mcp --headless` process can read the same two ceiling concepts from an
explicit startup file:

```json
{"max_fuel": 200000000000, "max_memory_mib": 512}
```

```text
solidcraft-cli --plugin-limits plugin-limits.json run inspect-plugin.json --results
solidcraft-cli --plugin-limits plugin-limits.json mcp --headless --allow-path /path/to/plugins
```

This UTF-8 JSON file is at most 16 KiB; a UTF-8 BOM is accepted for Windows editors. Its keys are
`max_fuel` and `max_memory_mib`; missing keys use the original host defaults, while unknown keys,
zero, fractional and out-of-range values are refused. The CLI reads it once before creating
local sessions. Omit the flag to use the original ceilings. `--connect` and non-headless `mcp`
refuse this flag because the connected desktop owns its preferences. GUI preferences and CLI
startup files are separate user settings; a package or saved design cannot supply either.

Both host and manifest accept positive integer fuel through 18446744073709551615 (`u64::MAX`),
without conversion through floating-point controls. Memory accepts 1..4096 MiB: the remaining
4096 MiB maximum is the technical Wasm32 address-space boundary (65536 pages of 64 KiB), not
the old policy ceiling. Memory64 remains disabled and only one linear memory is allowed. A
higher ceiling does not reserve memory or guarantee allocation; select a value your machine
can sustain. The 30-second maximum deadline and the other fixed limits in the table remain.

### Approve the package's actual request

`runtime.limits` still determines the invocation's fuel, memory and deadline. Omitting that
object still requests 10 million fuel, 64 MiB and 5000 ms, even under much larger host ceilings.
`plugin.inspect` accepts a structurally valid larger request without instantiating it. Its reply
contains the exact requested manifest limits, current `host_limits`, `within_host_limits`, and
an optional `limits_error`. Inspect and review the code, grants and budgets first. If needed,
explicitly raise the host ceilings, then pass the inspected content hash to `plugin.run`.
The content hash covers the declared budgets: editing them requires fresh inspection and approval.
Raising a host ceiling alone never approves content, increases a request, executes or retries it.
The host rechecks the current ceilings before compilation/execution. Lowering them can refuse a
previously approved package. Desktop results prepared under changed ceilings, grants or disable settings
are discarded rather than adopted into the current design. Settings survive new/open design
operations and scratch copies, but are never serialized as design data.

Fuel and epoch checks contain guest execution, not arbitrary native work. Component compilation
happens before the guest deadline; its input size is bounded, but compiler memory/time are not
covered by guest fuel. Each approved native command runs under its existing engine/kernel limits;
the guest deadline prevents more commands rather than forcibly interrupting a native call.
The desktop worker keeps this work off the window thread. Stop drops adoption; an already running
worker finishes unseen. It does not synchronously kill a native compiler or kernel call.

Host failures are typed data: missing permission, failed command and reason, exhausted limit,
invalid manifest or input field and reason, trap, or cancellation. The WIT and Rust `HostError`
describe the same cases. A failure never retries the component, raises its limits or commits
a partial transaction.

After an entry returns valid JSON, the runtime writes consumed fuel, remaining fuel and the
configured fuel limit to the `craft_plugin_wasm` debug log target. These counters describe guest
execution before the application commits changes or publishes staged files. In the CLI, pass `--debug` to see these counters on stderr.
Normal command output and plugin result JSON are unchanged. The counter message contains no
model data or package paths.

Plugins install from a `.zip` with `plugin.install` (see [Plugins](plugins.md#installing)).
There is no compiled-code cache, unsafe deserialization, WASI filesystem/clock/socket/process API,
network integration, persistent guest store, signed index, custom panels or plugin-supplied icons.
The WIT has no persistent store interface yet (`storage` arrives later; designs already keep
`plugins/<plugin-id>/` data untouched). `manifest()` is not used.

## Author an original plugin

Plugin packages, examples and converters live outside this repository, as specified in
[the plugin RFC](rfcs/0002-plugins.md). This repository contains the host, WIT and synthetic test
fixtures. The host compiles no plugin algorithm into SolidCraft. Rust first builds a core module with WIT metadata; `wasm-tools component new`
wraps it as a component without a WASI adapter. Other languages may use the same published
[WIT](../crates/craft-plugin/wit/plugin.wit) if they produce compatible components without unsupported imports;
JavaScript works with `jco componentize` with its WASI features turned off (`stdio`, `random`,
`clocks`, `http`, `fetch-event`). A Rust guest calls `craft::plugin::commands::run` and implements
`exports::craft::plugin::entry::Guest`; a JavaScript guest imports `run` from
`craft:plugin/commands@0.1.0` and exports `entry`. Its manifest says `"capabilities": ["commands@0.1"]`.

## Platforms and validation

Native execution on x86-64 or AArch64 requires the engine's `plugins` Cargo feature.
Other native architectures retain the commands but report that no execution host is available. The browser build publishes the
same command descriptions but refuses execution without a native host. Turning off this feature
keeps metadata, inspection and the general mesh query available while excluding Wasmtime.
No custom `FeatureKind::Plugin` is introduced. The one design-format change is the reserved
`plugins/<plugin-id>/` data namespace.

The runtime tests use original generated components, not mocks of a successful plugin: host calls,
two edits grouped into one undo step, measured volume, undo/redo and cold reopen after removing
the package; guest traps, fuel/memory exhaustion, forbidden calls, stale approval, disabled plugins,
and caller path scopes. The desktop tests cover worker scheduling and rejection of stale results.
The PR records actual validation results; this guide is not a claim that a particular build passed.
