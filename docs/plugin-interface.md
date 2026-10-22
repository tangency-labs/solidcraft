# Plugin interface (WIT)

Proposal for [#1221](https://github.com/tangency-labs/solidcraft/issues/1221): the WIT interface,
the host behaviour around it and the first two plugins. Design only; nothing here is implemented
yet. Companions: [plugins.md](plugins.md) (parametric plugin features, phase 2) and
[web-runtime.md](web-runtime.md) (browser hosting). #4933 proposes how plugin commands appear in
Design > Utilities; section 13 says how the two fit.

The interface is shaped by two pilot plugins, so that `1.0.0` is frozen only after a real plugin has
used it:

- **Spur gear** (`command-plugin` world): the minimal transaction path from the issue's first milestone.
- **Review notes** (`panel-plugin` world): team review notes pinned to the model and synced through a
  server, which forces the interface to answer the questions a real plugin raises (UI, network,
  secrets, local data, the design's own data). CAM stays in view (section 13) as a later, heavier
  pilot.

## 1. Goals and non-goals

Goals (from #1221, unchanged): sandboxed by default; any language that compiles to a WebAssembly
component, Rust first; the engine's own JSON command set is the only model API; a versioned WIT world;
desktop first.

Added by this proposal:

- G1. One small WIT package, `solidcraft:plugin@0.1.0`, with two worlds (`command-plugin`,
  `panel-plugin`). `0.x` may change; `1.0.0` is frozen only after both pilots ship (section 10, S6).
- G2. Two kinds of calls with different rights (section 3): a **transaction** changes the design and
  is one undo step but has no network; a **handler** drives a panel and may use the network but never
  edits the design.
- G3. Permissions are known before any plugin code runs: they come from the static `plugin.json`, not
  from a `manifest()` export.
- G4. A plugin cannot crash the app or damage a design through its code or its host calls: traps,
  timeouts and limit breaches end the call with an error, and a failed transaction leaves the design
  unchanged. Two costs stay in the app's own process and are bounded by caps, not by the sandbox:
  compiling a component (its file size, 6.2) and opening an importer's output (the normal checks for
  an untrusted design file, 9a).

Non-goals for `0.1` (each has a place in section 13): parametric plugin features
(`FeatureKind::Plugin`, docs/plugins.md phase 2), file exporters (importers are designed in section
9a behind the unstable feature `file-formats`), plugin-defined icons or
artwork, the browser build, long-running background jobs (CAM, FEA), real-time co-editing, signing and
a plugin index.

## 2. What exists today (checked in the code)

| Area | Fact | Where |
|---|---|---|
| Plugin folders | `{data}/plugins/<id>/plugin.json`, fields `id`, `name`, `version`, `description`, `#[serde(deny_unknown_fields)]`, 16 KiB cap; nothing is loaded or run | `crates/ui-egui/src/plugins.rs:1-96` |
| Turning off | `preferences.disabled_plugins` (max 256); unknown ids are on | `plugins.rs:127-130`, `prefs.rs:549` |
| Install from GitHub | open PR #4330 adds search and a GitHub zip install into the same folder | PR #4330 (touches `plugins.rs`, `prefs.rs`) |
| Commands | static `CommandSpec` lists, no run-time registration; `Run = fn(&mut Session, &Value) -> Result<Value>` | `crates/engine/src/cmd/mod.rs:97-132,258-314` |
| `Session::execute` | snapshot, `catch_unwind`, restore on error; every undoable command that changes the doc pushes its own undo step, nested ones too (no grouping API); `MAX_UNDO = 200` | `crates/engine/src/lib.rs:774-860,371` |
| Batches | `engine.batch` uses checkpoints and leaves one undo step per command | `crates/ui-egui/src/control.rs:148-188` |
| Crash journal | records outermost commands only; a panic's replay is that one command | `crates/engine/src/crash.rs:486-540` |
| Design data for other tools | `document.metadata.set` with reverse-DNS keys (`is_namespaced`), 64 KiB per namespaced value, 256 KiB total, undoable | `crates/doc/src/metadata.rs:34-62`, `crates/engine/src/cmd/metadata.rs:14-28` |
| Selection and camera | `document.inspect` returns `selection`; `select.set`, `view.apply {camera:{target,yaw,pitch,distance,fov?}}` are `noundo`; the engine cannot read the live viewport camera | `cmd/inspect.rs:274`, `cmd/edit.rs:71`, `cmd/browser.rs:36,428-479`, `lib.rs:284-287` |
| Entity names | face names `F<feature>:...`, edge names `<faceA>|<faceB>`; `model.faces` / `model.edges` give names, points and normals | `crates/doc/src/naming.rs`, `cmd/inspect.rs:19-25` |
| File access by commands | `vfs::PathScope` per thread, `vfs::check` refuses paths outside it | `crates/io/src/vfs.rs:129,410,434` |
| Agent command gate | refuses `diagnostics.*`, `*crash*`, `app.*`, `prefs.*`; asks before `file.*`, `doc.*` | `crates/ui-egui/src/agent_chat/gate.rs:113-134` |
| HTTP and secrets | `ureq` + rustls and `keyring` live in the app crate and reach the UI through `Services` (`http_get`, `agent_http`, `secrets`) | `apps/solidcraft/Cargo.toml:38-42`, `crates/ui-egui/src/lib.rs:335-358` |
| Change detection | no events; `Session.revision` increments on every document change (and on `select.set`) and the UI polls it | `lib.rs:248-249`, `cmd/edit.rs:1039` |
| Identity | no user name anywhere; only the design's `metadata.author` | `metadata.rs:25-26` |
| Layers | L4 engine, L5 ui-egui/gpu/mcp; every layered crate must build for `wasm32-unknown-unknown`; native-only deps go under `cfg(not(target_arch = "wasm32"))` | `xtask/src/layers.rs:37-80`, `xtask/src/main.rs:345-366` |
| Wasm runtimes | none in `Cargo.lock`; `tokio` 1.53 is already there | `Cargo.lock` |
| Unsafe | workspace `unsafe_code = "forbid"`, listed exceptions only (AGENTS.md) | `AGENTS.md` |
| Design docs | `docs/plugins.md` (milestone 1 `manifest()` + `execute`, phase 2 parametric shell), `docs/web-runtime.md` (browser broker) | `docs/` |

## 3. Design overview

```text
Preferences > Plugins        plugin.json (static)          Session (engine, L4)
  approve permissions  --->  id, entries, commands,  --->  plugin.run / plugin.act / plugin.panel / plugin.list
  store secrets              permissions, limits             |  calls through
                                                              v
                                    PluginHost trait (engine, wasm-safe)  <--- app installs it
                                                              |
                          crates/plugin-host (L5, native-only, wasmtime)
                          one worker thread + one Store per running plugin
                                                              |
                                  component (wasm32-wasip2)  imports engine/log/store/net/host
                                                             exports command (+ panel)
```

- **The seam.** `solidcraft_engine::plugin::PluginHost` is a trait object on `Session`, installed by
  the desktop app and the CLI, absent in the browser and in tests that do not install one (like the
  render job host, it survives new/open/recover). The engine's four static commands call through it,
  so plugins add no `CommandSpec`s and nothing below L5 links wasmtime.
- **One worker per running plugin.** Its thread owns the plugin's `wasmtime::Store` and a
  single-threaded tokio runtime; every export call runs there, one at a time, as an async call
  (`call_async`) under `tokio::time::timeout` at the call's deadline. Host functions are async too
  (`engine.run` awaits the session thread's answer, `net.fetch` awaits a blocking request run on
  tokio's blocking pool). At the deadline the call's future is dropped, which ends the call wherever it
  is waiting: in guest code (epoch interruption makes long guest code yield so the timeout can fire),
  in a WASI poll, or in one of our host functions. A plugin whose call timed out or trapped has its
  Store dropped and is restarted fresh on its next call (its in-memory state is lost; its `store`
  data is not). Panel events queued for the old instance are dropped; a fresh instance whose panel is
  open first gets `opened`, so it rebuilds the panel from its `store` and the design.
  Instantiation itself (`instantiate_async`, which runs the start functions of the component's core
  modules) is the first call on a fresh Store and runs under the same timeout, with the deadline of
  the call that needed the instance; a start function that loops ends there with `limit`, and
  `set_policy` can drop it like any running call.
- **Transaction** = `command.execute(entry, params)`, reached only through the engine command
  `plugin.run {plugin, entry, params}`. The session thread sends the call to the worker and, until it
  returns or the deadline passes, serves the plugin's `engine.run` calls as nested `Session::execute`
  calls. Rights: any command plugin.json lists (minus the host's deny list); `store`; `log`. The
  network returns `denied`. Result: one undo step; on any error, trap or timeout the design is
  restored (the outer `Session::execute` already does this for a failing command).
- **Handler** = `panel.handle(event)`, `panel.render()`, `panel.markers()`. The UI never waits for
  them: it posts the event and draws the last panel and pins it received. Rights: `net` (if granted),
  `store`, `log`, `host.request-execute`, and `engine.run` for the short list of handler-safe
  commands (inspect, select, view; section 6.4). To change the design, a handler calls `host.request-execute(entry, params)`; the host
  runs that transaction after the handler returns and reports the outcome as an `executed` event.
- **A handler belongs to one design.** The host records which design was active when the handler
  call started, as (tab, design generation). The generation is a counter the host keeps per tab and
  bumps whenever the tab's design is replaced rather than edited: new, open, recover, revert, and
  anything else that replaces the `Session` or its document wholesale (edits and undo do not bump
  it). Its `engine.run` calls and its `request-execute` apply to that design only: if the active pair
  differs when one of them is served, it returns `denied` ("the design changed") and
  a queued `request-execute` is not run (its `executed` event carries that error). So Connect started
  in design A can never write A's connection into design B, also when B was opened into A's tab.
- **Entries have a kind.** A `transaction` entry (the default) runs through `plugin.run` with
  transaction rights. An `action` entry runs through `plugin.act {plugin, entry, params}`, a
  `noundo` engine command, with handler rights (network, handler-safe commands, `request-execute`),
  bound to the active design like a handler. Transactions an action requests run right after it,
  on the same design, and `plugin.act` returns `{result, executed: [{entry, outcome}]}`. Both call the same export,
  `command.execute`; the host picks the rights from plugin.json. This is how scripts, the CLI and
  MCP agents reach what a panel button does, so every user-visible plugin behaviour is a command.
  `plugin.act` never holds the window thread on the network. In the desktop app, an `engine.execute`
  of `plugin.act` from the control channel, MCP or the agent chat hands the action to the plugin's
  worker and the window goes on drawing; the reply is sent when the worker finishes, through the
  deferred-reply path the control channel already has for off-thread work (`control_job.rs`,
  `control.pending`, #1989). Its handler-safe requests are served each frame like a panel handler's,
  and its deadline is the handler one (30 s). Only a top-level `engine.execute` of `plugin.act` in the
  app is deferred this way. In the app, `plugin.act` as a step of `engine.batch` or `engine.script`,
  or run by another command, is refused with "run plugin.act as its own request": a batch holds the
  window for its whole run, so an action inside it would either wait on a frame the batch holds or
  freeze the window on the network. Headless hosts (the CLI and its scripts, MCP's headless backend)
  have no window: there `plugin.act` runs inline, and the calling thread serves the action's
  handler-safe requests itself, as `plugin.run` does, within the same 30 s. A batch or script that
  contains `plugin.run` is never computed ahead (`Session::ahead`, 6.3). The panel itself never uses
  `plugin.act`.

Why this split: a transaction must be atomic and quick (the design is locked while it runs), and a
network round trip inside it would freeze the window. A handler may wait seconds for a server, so it
must not hold the design. The pair also makes every design change replayable from `plugin.run`
parameters, which are plain JSON chosen before the transaction starts.

## 4. The WIT package

File `wit/solidcraft-plugin.wit` (validated with `wasm-tools component wit` 1.262 and by building a
dummy `panel-plugin` component). Doc comments are part of the contract. A host spike with
`wasmtime::component::bindgen!` over this file builds under `#![forbid(unsafe_code)]` with wasmtime
50 (no new unsafe exception is needed).

```wit
package solidcraft:plugin@0.1.0;

/// Values the other interfaces share.
interface types {
    /// JSON text. Whoever receives it parses it with size and depth caps.
    type json = string;

    /// Why a call failed.
    variant plugin-error {
        /// The command, request or export failed; the text says why.
        failed(string),
        /// plugin.json does not ask for this, the user did not allow it, or it is not
        /// available in this kind of call (the network inside a transaction).
        denied(string),
        /// A size, count or time limit was reached.
        limit(string),
        /// Malformed input: bad JSON, an unknown id, a value out of range.
        invalid(string),
    }
}

/// The model API: SolidCraft's engine commands, as MCP and the control channel run them.
interface engine {
    use types.{json, plugin-error};

    /// Run one command (its dialog-free JSON form) on the active design.
    ///
    /// In a transaction (`command.execute`) any command plugin.json lists may run, and the
    /// whole transaction is one undo step. In a handler (`panel.*`) only the host's short
    /// list of handler-safe commands runs (inspect, select, view).
    run: func(command: string, args: json) -> result<json, plugin-error>;
}

/// Messages for the user and for crash reports.
interface log {
    enum level { debug, info, warn, error }

    log: func(level: level, message: string);
}

/// This plugin's own data on this computer, not in the design. Needs `store`.
interface store {
    use types.{plugin-error};

    get: func(key: string) -> result<option<list<u8>>, plugin-error>;
    set: func(key: string, value: list<u8>) -> result<_, plugin-error>;
    remove: func(key: string) -> result<_, plugin-error>;
    keys: func() -> result<list<string>, plugin-error>;
}

/// HTTPS to the origins plugin.json names. Handlers only; a transaction gets `denied`.
interface net {
    use types.{plugin-error};

    enum method { get, post, put, patch, delete }

    record request {
        method: method,
        /// An absolute `https://` URL on an origin plugin.json lists.
        url: string,
        headers: list<tuple<string, string>>,
        body: list<u8>,
    }

    record response {
        status: u16,
        headers: list<tuple<string, string>>,
        body: list<u8>,
    }

    /// Send one request and wait for its answer. When the user stored the origin's secret,
    /// the host adds it as the header plugin.json names; the plugin never sees the secret.
    fetch: func(req: request) -> result<response, plugin-error>;

    /// Whether the user stored the secret named `name` in plugin.json.
    has-secret: func(name: string) -> bool;
}

/// Work the host does once the current handler returns.
interface host {
    use types.{json, plugin-error};

    /// Run `entry` as a transaction after this handler returns. The panel gets `executed`.
    request-execute: func(entry: string, params: json) -> result<_, plugin-error>;
}

/// Entries: what scripts, the CLI and agents can call (`plugin.run`, `plugin.act`).
interface command {
    use types.{json, plugin-error};

    /// Run one entry plugin.json lists. A `transaction` entry may edit the design and is one
    /// undo step; an error or a trap leaves the design as it was. An `action` entry gets the
    /// rights of a panel handler (network, no design edits).
    execute: func(entry: string, params: json) -> result<json, plugin-error>;
}

/// A panel the host draws with the app's widgets, and pins in the 3D view.
interface panel {
    use types.{json, plugin-error};

    enum tone { normal, dim, accent, warning, error, done }

    record button { id: string, label: string, enabled: bool }
    record checkbox { id: string, label: string, checked: bool }
    record text-input { id: string, label: string, value: string, multiline: bool, max-chars: u32 }
    record number-input { id: string, label: string, value: f64, min: f64, max: f64, step: f64, unit: string }
    record choice { id: string, label: string, options: list<string>, selected: u32 }
    record styled { text: string, tone: tone }

    /// One thing on a row.
    variant cell {
        text(styled),
        button(button),
        checkbox(checkbox),
    }

    /// One line of the panel, top to bottom.
    variant item {
        heading(string),
        text(styled),
        row(list<cell>),
        button(button),
        checkbox(checkbox),
        text-input(text-input),
        number-input(number-input),
        choice(choice),
        separator,
    }

    record vec3 { x: f64, y: f64, z: f64 }

    /// The viewport camera, in the form `view.apply` takes (mm, radians).
    record camera { target: vec3, yaw: f64, pitch: f64, distance: f64 }

    /// A pin in the 3D view, in design coordinates (mm).
    record marker { id: string, at: vec3, label: string, tone: tone }

    /// An input's value when an action happened.
    variant value { text(string), number(f64), checked(bool), selected(u32) }

    /// `camera` is the viewport camera at the click (none in a headless host).
    record action { id: string, inputs: list<tuple<string, value>>, camera: option<camera> }
    record executed { entry: string, outcome: result<json, string> }

    variant event {
        /// The panel was opened.
        opened,
        /// A button was clicked, or a checkbox or choice changed; `inputs` holds every input.
        action(action),
        /// A pin was clicked.
        marker(string),
        /// The active design changed: an edit, undo, another tab. Coalesced.
        design-changed,
        /// What a `host.request-execute` did.
        executed(executed),
    }

    handle: func(e: event) -> result<_, plugin-error>;
    render: func() -> list<item>;
    markers: func() -> list<marker>;
}

/// A plugin whose entries change the design (a part generator).
world command-plugin {
    import engine;
    import log;
    import store;
    export command;
}

/// A plugin with a panel and pins, which may also use the network.
world panel-plugin {
    import engine;
    import log;
    import store;
    import net;
    import host;
    export command;
    export panel;
}

/// The file an importer converts, and the files next to it. Unstable: feature `file-formats`.
@unstable(feature = file-formats)
interface input {
    @unstable(feature = file-formats)
    use types.{plugin-error};

    /// The file being imported: its name without folders, and its size in bytes.
    @unstable(feature = file-formats)
    record file-info { name: string, size: u64 }

    /// Read at most 8 MiB at `offset`. `path` is "" for the file being imported, or a path
    /// relative to its folder (only with `reads_input_folder`; no `..`, no absolute paths, no
    /// hidden files).
    @unstable(feature = file-formats)
    read: func(path: string, offset: u64, len: u32) -> result<list<u8>, plugin-error>;

    /// The size of such a file, or none when it does not exist or may not be read.
    @unstable(feature = file-formats)
    size: func(path: string) -> result<option<u64>, plugin-error>;
}

/// Where an importer writes the converted design. Unstable: feature `file-formats`.
@unstable(feature = file-formats)
interface output {
    @unstable(feature = file-formats)
    use types.{plugin-error};

    /// Append bytes of the `.solidcraft` design: at most 8 MiB per call, 256 MiB in all.
    @unstable(feature = file-formats)
    write: func(bytes: list<u8>) -> result<_, plugin-error>;
}

/// File importers. Unstable: feature `file-formats`.
@unstable(feature = file-formats)
interface importer {
    @unstable(feature = file-formats)
    use types.{plugin-error};
    @unstable(feature = file-formats)
    use input.{file-info};

    /// Convert `main`, writing the design through `output`, and return the loss report (plain
    /// text, at most 64 KiB). An error or a trap fails this one import.
    @unstable(feature = file-formats)
    import-design: func(main: file-info) -> result<string, plugin-error>;
}

/// A plugin that converts files from other formats into SolidCraft designs.
@unstable(feature = file-formats)
world importer-plugin {
    import log;
    @unstable(feature = file-formats)
    import input;
    @unstable(feature = file-formats)
    import output;
    @unstable(feature = file-formats)
    export importer;
}
```

Notes on the shape:

- **JSON where the engine speaks JSON, typed records where the host draws.** Command arguments and
  results stay JSON (`types.json`) so the WIT never grows with the command set. The panel, pins, HTTP
  and errors are typed so every language gets real bindings and the host validates structure, not
  strings.
- **No recursive types** (WIT has none): the panel is a flat list of items, with `row` holding cells.
- **Versioning.** From `1.0.0` on, a minor version may add functions to the interfaces plugins
  import (the host provides them) and may add new interfaces. It never adds or changes functions in
  an interface plugins export, because an older plugin would not have them; a new export capability
  is a new interface that the host looks for in the component and treats as optional. A plugin built
  against `1.0.0` links against a `1.3.0` host because wasmtime resolves semver-compatible names, for
  imports in the `Linker` ("a component imports `a:b/c@0.2.0` then that import will resolve to the
  `0.2.1` version", wasmtime 50 `src/runtime/component/linker.rs:26-50`) and for export lookups
  (`src/runtime/component/component.rs:1170`, export `@0.1.0` found by a lookup for `@0.1.1`). A
  breaking change is a new major: `@1.x` and `@2.x` names are different names, a `Linker` may define
  several versions at once (same doc comment), and the host carries one generated binding module per
  supported major. While `0.x`, every minor may break (semver treats `0.1` and `0.2` as incompatible).
- **Command arguments are not versioned by the WIT.** Scripts, MCP agents and plugins share one
  risk: a command whose parameters change. The load-time check (every id in `commands` exists)
  catches removed commands before any code runs; changed parameters surface as command errors with
  the engine's message. Command contracts (`crates/engine/src/schema`, #2829) are how this becomes
  checkable; the author guide points plugin authors at `engine.describe` for commands that have one.
- **Unstable parts.** Items marked `@unstable(feature = file-formats)` (the importer, section 9a) are
  visible to authors and tools (`wasm-tools component wit --features file-formats`) but are not part
  of the `0.1` promise: a host links them only once it implements that feature, and they may change
  until the mark is removed. Every item inside a gated interface carries the mark, as WIT requires.
  wasmtime 50's `bindgen!` has no feature switch and generates the gated items too (checked: the
  `importer-plugin` world binds under `#![forbid(unsafe_code)]`), so gating on the host is a matter
  of not linking them before S3b.
- **Imports are linked for every plugin of a world;** a call without the permission returns `denied`.
  A component that imports an interface its world does not have fails to load (wasmtime link error,
  reported on the Plugins page).
- **WASI.** Toolchains other than bare Rust (`wasm32-wasip2` std, componentize-py, jco) import WASI
  0.2 even for printing. The host links `wasmtime-wasi` with a closed context: no preopened folders,
  no environment, no arguments, no sockets (address check denies all, name lookup off), stdin empty,
  stdout/stderr into the plugin log (capped), `wasi:random` real, monotonic clock real, wall clock
  fixed at the Unix epoch unless the plugin has the `clock` permission. The clock and randomness are
  WASI's, not ours, so every language's standard library reaches them.

## 5. plugin.json, version 1

The listing fields stay as they are. A folder whose plugin.json has no `component` is listing-only,
exactly as today. New optional fields (all checked with the existing caps style; unknown fields are
still refused):

```json
{
  "id": "review-notes",
  "name": "Review notes",
  "version": "0.1.0",
  "description": "Team notes pinned to the model, synced through GitHub issues.",
  "api": "0.1",
  "component": "review_notes.wasm",
  "world": "panel-plugin",
  "entries": [
    { "name": "bind", "label": "Record the repository in the design" },
    { "name": "connect", "label": "Connect design to a repository", "kind": "action" },
    { "name": "list", "label": "List notes", "kind": "action" },
    { "name": "add", "label": "Add note", "kind": "action" },
    { "name": "reply", "label": "Reply to a note", "kind": "action" },
    { "name": "resolve", "label": "Resolve a note", "kind": "action" },
    { "name": "reopen", "label": "Reopen a note", "kind": "action" }
  ],
  "panel": { "label": "Review notes" },
  "commands": ["document.inspect", "document.metadata", "document.metadata.set", "model.faces", "model.edges", "select.set", "view.apply"],
  "permissions": {
    "store": true,
    "clock": true,
    "net": [
      { "origin": "https://api.github.com",
        "secret": { "name": "token", "label": "GitHub token", "header": "Authorization", "prefix": "Bearer " } }
    ]
  },
  "limits": { "memory_mib": 64, "transaction_seconds": 10 }
}
```

| Field | Rule |
|---|---|
| `api` | `"0.1"`; the host refuses an unknown major (and, while `0.x`, an unknown minor) |
| `component` | a file name (no separators) inside the folder, at most 16 MiB (6.2) |
| `world` | `command-plugin`, `panel-plugin`, or `importer-plugin` (only on a host with the `file-formats` feature, 9a); checked against the component's exports at load |
| `entries` | 0-64; `name` `[a-z0-9_-]{1,32}`, unique; `label` 1-80 chars; `kind` `transaction` (default) or `action`; a `plugin.run` or `host.request-execute` of an unlisted or `action` entry, or a `plugin.act` of a `transaction` entry, is `invalid` |
| `panel` | required for `panel-plugin`, refused otherwise; `label` 1-40 chars |
| `commands` | 0-256 engine command ids, each byte-equal to a `CommandSpec::id` in this build (`find_command` also matches case-insensitively, so the host compares the found spec's `id` with the listed one); otherwise the plugin does not load and the page says which id is missing. `engine.run` compares the same way |
| `permissions.store` | private storage (section 6.6) |
| `permissions.clock` | real wall clock in WASI |
| `permissions.net` | 0-8 origins, each exactly `https://host[:port]` (no path, no wildcard, no IP literals, no `localhost`); optional `secret` with `name` `[a-z0-9_-]{1,32}`, `label` 1-40 chars, `header` from a fixed set (`Authorization`, `X-Api-Key`, `Private-Token`), `prefix` at most 16 chars |
| `limits.memory_mib` | 16-256, default 64 |
| `limits.stack_kib` | 512-16384, default 512 (wasmtime's default); the plugin's wasm stack (6.2) |
| `limits.component_mib` | 1-16 in 0.1, default 16; the maximum rises only after S2's measurement (6.2) |
| `formats` | `importer-plugin` only: 1-16 file extensions (`[a-z0-9]{1,16}`, without the dot), each with a `label`; an extension that another enabled importer already lists makes the later plugin refuse to load, with a message naming both |
| `reads_input_folder` | `importer-plugin` only, default false: lets `input.read` open files next to the imported one (assemblies, 9a); shown on the approval screen |
| `limits.transaction_seconds` | 1-120, default 10 |

No `icon` in 0.1: plugin artwork would bypass the asset rules. Panels use a generic plugin icon from
the app's registry (or none) until the asset process covers plugin icons.

Compatibility with #4330: it installs a plugin folder from GitHub without running anything; this
change only extends the manifest it reads, so whichever lands second rebases. An app older than this
change refuses the new fields (`deny_unknown_fields`) and lists the plugin under "skipped", which is
the right outcome for a plugin it cannot run.

## 6. Host behaviour

### 6.1 Approval and loading

- A plugin with a `component` runs only after the user turned it on **and** approved its exact
  permission set on the Plugins page. The page shows, in plain words, what was asked: "can read and
  change your open designs with these commands: ...", "can send data to https://api.github.com",
  "keeps up to 4 MiB on this computer", "can read the clock", "may keep the window busy for up to
  N seconds per change" (`limits.transaction_seconds`), "may use up to N MiB of memory"
  (`limits.memory_mib`); for an importer, "opens files with these extensions: ..." and, with
  `reads_input_folder`, "may read other files in the same folder as a file you import". The approval stores a hash of
  (`commands`, `permissions`, `limits`, `world`, and each entry's `name` and `kind`) in preferences
  (`approved_plugins: {id: hash}`, max 256 entries). A plugin update that changes any of these needs a
  new approval; a code-only update with the same permissions does not (the sandbox bounds it to what was
  approved). `disabled_plugins` keeps its meaning.
- Who tells the host: the host never reads preferences itself. Whoever installs it hands it the policy
  through `PluginHost::set_policy(approved: id -> hash, disabled: ids)`. The desktop app passes
  `approved_plugins` and `disabled_plugins` at start and again whenever the Plugins page changes them;
  `set_policy` never waits: a plugin that becomes disabled or loses its approval is marked stopped at
  once (no new calls start, its queued requests are dropped, its panel and pins disappear), and its
  worker is told to drop the running call's future and its Store. A blocking network request still in
  flight on tokio's blocking pool finishes within its own 20 s limit and its result is discarded. The CLI has no preferences: it approves nothing unless the command line names
  the plugin (`solidcraft-cli run --plugin <id>`, and the same flag for `solidcraft-cli mcp`, whose
  headless backend then runs `plugin.run` and `plugin.act` for agents), and that flag approves the
  manifest as it is on disk for that process only. A `plugin.run` or `plugin.act` of a plugin the current policy does not approve
  is `Disabled`.
- Existing behaviour change: today an unknown plugin id is "on" because nothing runs. With code, a new
  plugin with a `component` starts unapproved; listing-only folders keep today's behaviour.
- A component is compiled (`Component::new`) only after its plugin is approved and turned on;
  before that, the Plugins page checks plugin.json and the file's size only, so a plugin merely
  dropped into the folder costs nothing. Compiles run on one background thread, one at a time, never
  on the UI or session thread, once per change of the file (keyed by its size, modification time and
  SHA-256). The page keeps reading only plugin.json each frame, as today, and shows "checking..."
  until the compile reports. `Component::new` cannot be stopped from outside, so a compile that takes
  longer than 60 s per started 16 MiB of component is reported as failed and its result dropped when it ends; because compiles are
  serial and only for approved plugins, at most one such compile runs at a time. A `plugin.run` or `plugin.act` that arrives
  while its plugin is still compiling fails at once with "the plugin is still loading". No
  compiled-code cache: `Component::deserialize` is `unsafe` and the workspace forbids `unsafe`.
  Measured compile times go in the S2 PR description.
- Load-time checks, each a readable message on the Plugins page: manifest valid, file exists and is
  within the cap, component validates, its world matches `world`, every `commands` id exists.

### 6.2 Limits (frozen for 0.1)

| Limit | Value |
|---|---|
| Component file | `limits.component_mib`, at most 16 MiB in 0.1. Compiling runs in the app's process, and the compiler's memory is not covered by any `Store` limit, so the file cap is what bounds it. S2 measures peak compile memory and time on a real large component (an all-formats importer of 28-42 MB was reported in #1221) and only then raises the maximum, with 64 MiB as the target and the expected compile time on the approval screen; Winch or Pulley are the fallbacks if Cranelift costs too much. With no compiled-code cache (6.1), every app start compiles again, in the background after approval |
| Wasm stack | `limits.stack_kib` (512 KiB-16 MiB, default 512 KiB) as `Config::max_wasm_stack`. Stack limits belong to a wasmtime `Engine`, so every plugin gets its own `Engine`. With async calls the fiber stack (`Config::async_stack_size`, which must not be smaller) is the wasm stack plus 2 MiB, because that difference is what host functions run in and overflowing it aborts the process (wasmtime 50 docs of both settings). Deeper recursion traps the call |
| Linear memory | `limits.memory_mib` (16-256 MiB, default 64) for the sum of all the component's linear memories. `StoreLimits::memory_size` applies to each memory separately (wasmtime 50 `limits.rs:232-235`), so the host uses its own `ResourceLimiter` whose `memory_growing` refuses growth that would take the total over the cap; at most 4 memories, 10 000 table elements, 16 instances |
| Transaction wall time | `limits.transaction_seconds` (default 10 s, max 120 s), counted from the call, engine work included; checked between commands (a started engine command finishes, 6.3) |
| Handler wall time | 30 s per call, network waits included |
| Ending a call at its deadline | the call's future is dropped (`tokio::time::timeout` around `call_async`); epoch interruption, one tick per 10 ms from one ticker thread, with `epoch_deadline_async_yield_and_update` so guest loops yield to the timeout; blocking WASI calls (`wasi:io/poll`, clock subscriptions) and our host functions are awaited futures, so they end with the call; the timed-out call is `limit` |
| `engine.run` calls | 10 000 per transaction, 1 000 per handler call |
| JSON in either direction | 4 MiB per value, depth 64 |
| Copy into the host per call | wasmtime copies a host call's arguments out of the guest before our own checks run, up to 128 MiB by default; the host sets `Store::set_hostcall_fuel` to 8 MiB (above the largest per-call input, 4 MiB of JSON), so a bigger argument traps before it is copied. An export's result (`render`, `markers`, `execute`) lives in the guest's memory, which `memory_mib` bounds; S2 checks whether hostcall fuel also covers lifting results and records the answer in the S2 PR |
| `net.fetch` | 64 per handler call; request body 1 MiB; response body 8 MiB; 64 headers, 8 KiB each; 20 s per request; `https` only; no redirects followed (a 3xx is returned as is) |
| `store` | 4 MiB total per plugin, 1 024 keys, key `[a-z0-9][a-z0-9._-]{0,127}`, value 1 MiB; one file per plugin (6.6) |
| Panel | 2 000 items, 32 cells per row, 4 KiB per text, 64 options per choice, ids at most 64 bytes; every `f64` finite, `min <= value <= max` (value clamped), `step > 0`, `selected` within `options`; a panel that breaks a rule is refused whole (`limit` or `invalid`) and the previous one stays |
| Markers | 500, label 80 chars, finite coordinates within 1e6 mm |
| Log | 200 lines per call, 1 KiB per line, prefixed `[plugin <id>]` |
| `host.request-execute` | 4 per handler call |

Hitting a limit ends that one call with `plugin-error.limit` (for host calls) or a trap (for guest
limits); the app keeps running. Fuel is not used: the timeout plus epoch yields cover runaway loops
at lower cost, and they also end calls that wait inside the host, which fuel would not.

### 6.3 Transactions (`plugin.run`)

`plugin.run {plugin, entry, params?}`: undoable, `enabled` only when a plugin host is installed. Its
`run` function:

1. Looks up the approved, enabled plugin and the entry (`invalid` / `Disabled` otherwise).
2. Moves the session's undo and redo stacks aside, so nested commands push into (and clear) empty
   stacks, and saves the session state that commands change without an undo step: `active_sketch`,
   `active_component`, `active_occurrence_path`, `pending_moves`, `visibility`, `selection`,
   `camera`, `section`, `analysis`, `continuity`, `clipboard`, `sketch_clipboard`, `auto_project`.
   An S2 test
   classifies every `Session` field as saved here, document, history, cache or host-owned (`path`,
   `log`, `param_check`, ...), so a field added later cannot be missed silently.
3. Sends `execute(entry, params)` to the plugin's worker and serves its `engine.run` calls until the
   worker answers or the deadline passes. Each served call goes through the command policy (6.4) and
   then a nested `Session::execute`.
   - Frames: `Session::execute` uses one `outermost` flag (`frames::Nested`, `lib.rs:794-798`) both
     to map world picks into the active component's frame and to write the crash journal. The two
     are split: commands a plugin runs inside `plugin.run` are mapped as if they were outermost, so
     their points mean what they mean over MCP, while the journal records them
     under the transaction's own entry (see Crash reports below), not as separate outermost commands.
   - Deadline: it is checked before each served call, and on the worker it drops the call's future
     (section 3). A single engine command that has started runs to its end, as it would from the
     toolbar or MCP; the deadline then stops the transaction before the next one.
   - Not computed ahead: the control channel can evaluate an agent's design change ahead on a copy
     of the session and then run it again for real (`control_job.rs`, `Session::ahead`, preference
     `agent_off_thread`). `plugin.run` (alone or inside a batch or script) is excluded
     from that and always runs once, for real, on the session that owns the design: a plugin may draw random ids, so two runs would differ, and the
     copy has no plugin host.
4. Restores both stacks (whatever the outcome). On failure it also puts back the saved session state
   before returning the error (`Session::execute` restores the document, but on its own would leave,
   for example, an `active_sketch` that a nested `sketch.edit` changed). The outer
   `Session::execute` then pushes the one snapshot from before `plugin.run` and clears redo, or
   restores the design if step 3 failed, in which case undo, redo and the saved session state are
   exactly as before.
5. Returns the plugin's JSON result, plus `"commands": n` (how many commands ran).

`plugin.run` never waits for a busy worker. If the plugin's worker is in a handler or an action when
`plugin.run` arrives (for example over MCP while the panel is syncing), `plugin.run` fails at once with
"the plugin is busy, try again"; the window is not held and no handler request can interleave with
the transaction. Transactions a handler or an action requested with `host.request-execute` are queued
by the host and start only when the worker is free, so they never meet this. Requests from other
plugins stay queued until a transaction ends; a handler of another plugin waiting that long may reach
its own 30 s deadline and end with `limit` (its panel says "timed out, try again"). Transactions
default to 10 s, so this needs a plugin that asked for a long `transaction_seconds`, which its approval
screen shows.

Every host-side wait (a worker waiting for the session thread, the session thread waiting for a
worker) is bounded by the deadline of the call it belongs to; a wait that runs out returns `limit` to
the guest or an error to `plugin.run`. On the worker, the same deadline drops the call's future
(section 3), whatever the guest is doing at that moment. Every message between a
worker and the session thread carries the id of the call it belongs to. Once a call has ended (done,
failed or timed out), anything still arriving for its id is dropped and never served: a late answer,
and also a late `engine.run` request, which therefore cannot edit the design after `plugin.run`
reported its error. The worker's Store is dropped and the next call starts a fresh instance.

State that is not an undo step: commands that are not undoable when the user runs them stay that way
inside a transaction. `browser.visibility` (saved with the design, "not an undo step") run by a
plugin is not undone by undoing `plugin.run`, exactly as when the user runs it from the browser; the
author guide lists such commands, and the Plugins page shows the commands a plugin may run before it
is approved.

Crash reports: a plugin may draw random values, read its `store` or depend on the network, so
neither `plugin.run {plugin, entry, params}` nor `plugin.act` is a reproducible record. The journal
therefore records what a transaction actually did: the nested commands it ran, in order, with their
exact params, under one entry marked with the plugin's id and version. `plugin.act` itself is never
journaled; the transactions it ran (`connect` running `bind`) are recorded the same way. A replay
runs those commands, so it needs neither the plugin nor its randomness, store or network, and a panic
inside a nested command is reproduced from the design before the transaction plus the commands up to
it. S2 adds this journal hook and a test that replays a crash report of a transaction that drew
random values, with the plugin removed, and documents it in docs/crash-reports.md.

### 6.4 Command policy

Every `engine.run` from a plugin passes these checks in order:

1. The id is in the plugin's `commands` list.
2. The id is not on the host deny list: `plugin.*`, `edit.undo`, `edit.redo`, `edit.checkpoint`,
   `edit.restore_checkpoint`, `diagnostics.*`, any id containing `crash`, `app.*`, `prefs.*`,
   `file.*`, `doc.*`, `render.export`, `render.start`, and `engine.*` except the read-only
   `engine.describe` (the same families the agent chat
   gate refuses or asks about, `gate.rs:113-134`).
3. The call runs under an empty `vfs::PathScope`, so a command that names a file is refused by the
   same check MCP uses.
4. `document.metadata.set` and `document.metadata.remove` may only touch keys equal to
   `plugin.<id>` or starting with `plugin.<id>.` (`plugin.review-notes` passes `is_namespaced`). Every
   key in a `values` object is checked. Reading other keys is allowed.
5. In a **handler**, the command must also be on the engine's fixed handler-safe list
   (`solidcraft_engine::plugin::HANDLER_SAFE`): `document.inspect`, `document.metadata`,
   `model.faces`, `model.edges`, `engine.describe`, `select.set`, `select.clear`, `view.apply`.
   "Not undoable" is not enough: some `noundo` commands change state that is saved with the design
   or that the user would lose (`browser.visibility` is "saved with the design; not an undo step",
   `component.revert_position` drops pending moves, `component.activate` and `sketch.edit` change the
   active context). A test runs every listed command on a sample design and asserts that the
   document and every field of the saved session state of 6.3 step 2 are unchanged, except
   `selection` (`select.*`) and `camera` (`view.apply`); adding a command to the list needs that
   test to pass.

Handler calls are queued to the UI thread and served in the next frame (the worker asks the UI to
repaint). A command refused by the policy returns `denied` with the rule that refused it.

### 6.5 Network and secrets

- `net.fetch` runs on the worker thread with `ureq` (already a dependency of the app; the host crate
  takes it under `cfg(not(target_arch = "wasm32"))`). The URL must parse, be `https`, and its origin
  must equal one listed origin. Certificates are always verified against the listed host name
  (rustls, system roots). The boundary is therefore the approved names, not network ranges: a plugin
  reaches only servers that hold a valid certificate for an origin the user approved. That may be a
  server on the user's own network (a self-hosted backend is exactly that), and it is shown by name on
  the approval screen; other LAN devices, which have no certificate for that name, are out of reach. No
  IP literals and no `localhost` are accepted as origins.
- Secrets: the Plugins page shows one masked field per declared `secret`. The value goes to the system
  keychain (service `SolidCraft plugins`, account `<plugin id>/<secret name>`). `plugin-host` defines
  its own small get/set/delete trait for this, and the desktop app passes an adapter over the same
  `keyring` code its agent chat `Keychain` uses (`apps/solidcraft/src/main.rs`); `plugin-host` does
  not depend on ui-egui. `fetch` adds `header: prefix + value` to requests for that
  origin only, after removing any header of the same name the plugin set. The plugin can ask only
  `has-secret`: the host never hands it the value. What the approved origin does with the token is
  that origin's business (it receives the token by design); if it echoes the token back, the host
  replaces every exact occurrence of the value in the response headers and body with `[secret]`
  before the plugin sees them. That stops the plain echo; a server that encodes the token
  differently is a server the user chose to trust with it.
- Requests carry `User-Agent: SolidCraft/<version> plugin/<id>/<plugin version>`; no cookies are kept.

### 6.6 Store

One file per plugin, `{data}/plugin-data/<id>.json` (outside `plugins/`, so the folder scan never
sees it): a JSON object from key to base64 value, rewritten atomically (temp file + rename) on every
`set` and `remove`, at most 4 MiB of values (6.2). Keys never become file names, so `.`, `..` or a
Windows device name (`con`, `nul`) are just keys; a key must still start with a letter or digit
(`[a-z0-9][a-z0-9._-]{0,127}`), anything else is `invalid`. Removing a plugin from the
Plugins page offers to delete its data.

### 6.7 Panels and pins (ui-egui)

- `plugin.panel {plugin}` is a static command: `ui.start` (toolbar, Plugins page "Open" button,
  control channel `ui.start` with `params`) opens the plugin's panel window. The control channel
  today refuses `ui.start` params for every command except `joint.drive` and `motion.study`
  (`crates/ui-egui/src/control.rs:427-430`); S3 adds `plugin.panel {plugin}` to that list; `engine.execute` returns
  an error saying the panel opens in the app window. `plugin.list` (read-only) returns installed
  plugins, approval state, entries with their kinds and whether a panel is open, so MCP and agents
  can find entries and call `plugin.run` or `plugin.act`. Opening the panel of a plugin that is
  not approved, turned off, still compiling or failed to load shows the panel window with that state
  and a button to the Plugins page, instead of plugin content; nothing of the plugin runs.
- The panel window is an `ui_kit::Window` (the universal container `cargo xtask ui-containers`
  enforces) and draws the last `render()` result with `ui_kit` widgets and `theme::Tokens` colours
  (`tone` maps to tokens). Inputs keep their edited values locally; an action event carries all of them
  and the viewport camera.
- After every `handle` call (and when the panel opens) the host calls `render()` and then `markers()`
  in the same worker turn, and the window draws the new result from the next frame. If either call
  fails or breaks a limit, the previous panel and pins stay and the panel shows the error line.
- Panel content and pins belong to the design (tab and generation) their handler call started in,
  like its other calls (section 3). Results for a design that is no longer active are dropped. When the active tab
  changes, the host hides the pins and greys the panel at once, sends `design-changed`, and shows the
  new design's panel and pins only when that call's `render()` and `markers()` arrive.
- While the panel is open the host draws the pins from the last `markers()` result in the
  viewport (projected, with the label on hover, colours from tokens). A click on a pin sends
  `marker(id)` and is not a pick in the model.
- `design-changed` is sent at most every 500 ms while the panel is open, when the active tab or the
  document itself changes (a different `Arc<Document>`: an edit, undo, redo, open). It does not follow
  `Session.revision`, because `select.set` also bumps that, and a plugin that selects something in
  its `design-changed` handler would otherwise wake itself every 500 ms.
- While a handler runs, the panel shows a small busy indicator and keeps the last content.

### 6.8 Errors

Guest traps, `limit` and `denied` become `EngineError::Other("plugin <id>: <message>")` for
`plugin.run`, and a status-line message plus a line in the plugin log for handlers. A trap restarts
the plugin; three traps within a minute turn its panel into an error notice until the user reopens it.
No error path panics; every wasmtime call returns a `Result` that the host maps.

## 7. Crates and layering

| Piece | Where | Layer | wasm32 |
|---|---|---|---|
| `PluginHost` trait, manifest types, policy checks, panel/pin data types | `crates/engine/src/plugin/` | L4 | builds (no wasmtime) |
| `plugin.run`, `plugin.act`, `plugin.panel`, `plugin.list` | `crates/engine/src/cmd/plugin.rs` | L4 | disabled without a host |
| wasmtime host (one `Engine` per plugin, 6.2), workers, WASI context, net, store | new `crates/plugin-host` | L5 | wasmtime (with its `async` feature), wasmtime-wasi, tokio (already in the workspace) and ureq under `cfg(not(target_arch = "wasm32"))`; the crate is an empty stub on wasm32 |
| Plugins page (approve, secrets, open), panel window, pins | `crates/ui-egui/src/plugins.rs` + `plugin_panel.rs` | L5 | panel code builds; the page already shows a desktop-only note |
| Installing the host | `apps/solidcraft` (desktop), `apps/solidcraft-cli` (transactions and actions; no panels) | exempt | - |

`plugin-host` depends on engine only; ui-egui reaches plugins through `Session`'s trait object, so the
two L5 crates do not depend on each other. `cargo xtask layers` gets one new row.

Dependency cost: wasmtime 50 with Cranelift and `wasmtime-wasi` add about 130 crates to the build
graph (tokio is already in it). Binary size growth is measured in the S2 PR; if it is unacceptable,
the fallback is wasmtime's Pulley interpreter (smaller, slower) behind the same API.

## 8. Pilot A: spur gear (`command-plugin`)

`examples/plugins/spur-gear/`, its own Cargo workspace (not a member of the app workspace), built for
`wasm32-wasip2` with `wit-bindgen`. One entry `spur`, params `{teeth, module, width, pressure_angle?}`
(validated: teeth 6-200, module 0.1-50 mm, width 0.1-1000 mm). It sketches the profile with
`sketch.create`, `sketch.spline.fit_point` (involute flanks), `sketch.arc.center_point` (root and tip),
`sketch.finish` and `solid.extrude`, all listed in `commands`. No panel and no form in 0.1: it is reached through `plugin.run` from MCP, the control
channel and CLI scripts (`solidcraft-cli run`). A generic entry form built from declared parameters is
later work (section 13). Purpose: the smallest end-to-end transaction test and the code sample in
`docs/plugin-author-guide.md`.

## 9. Pilot B: review notes (`panel-plugin`)

`examples/plugins/review-notes/` (same workspace as the gear). Team members open the same design
(through PDM, git, a shared folder or a zip) and see the same notes, synced through a repository's
issues. It changes the design only once, when a design is connected.

### 9.0 Conventions it follows

From public documentation of review tools and open standards (no product's text or UI is copied):

- **Topic, flat comments, status.** A note is a topic with a title, text, author and an open or
  resolved status; replies are one flat, ordered list; resolving is a change of the topic's status,
  not of a reply. This is the split of buildingSMART's BCF 3.0 (topic, comment, viewpoint), the one
  open standard for model review threads in wide use, and it maps one to one onto issues.
- **Layered anchor, strongest first:** an entity reference (our face or edge name, or a body), the
  picked world point and normal, and the camera at the time (BCF's viewpoint). When the entity no
  longer resolves after an edit, the note is shown as detached at its point and camera; a note is never
  hidden or deleted because the model changed.
- **Ids made by the client** (random), so notes can be created offline; the server's number (the issue
  number) is for people, never chosen by the client.
- **Server is the source of truth,** with a local outbox and cache; last writer wins on status.
- **Not in the first version:** live co-editing of the model, check-out locks (the PDM work in #3711
  covers versions and locks), drawing markup on snapshots, release workflows.

### 9.1 What the user does

1. Preferences > Plugins: turn on Review notes, approve its permissions, paste a GitHub token into its
   "GitHub token" field: a fine-grained token with Issues read and write on the team's repository,
   or, for outside collaborators on an organisation's repository (who cannot use fine-grained tokens
   there, per GitHub), a classic token with the `repo` scope.
2. Open the Review notes panel. A design that is not connected shows a repository field
   (`owner/name`) and Connect. Connect checks access (`GET /repos/{owner}/{repo}`): the repository
   must have issues turned on (`has_issues`), and the account needs the Write role
   (`permissions.push`), because creating a label does; otherwise it says which one is missing (a
   teammate with write access can connect the design instead). It then creates the design's label and calls
   `host.request-execute("bind", {repo, key})`, bound to this design (section 3); the `bind` transaction
   writes the design metadata (9.2). The panel says before Connect that notes are posted as issues in
   that repository. The design file must be saved and shared as usual for teammates to see the
   connection.
3. Add note: select a face, edge or body (or nothing), type the text, click Add note. The handler reads
   the selection (`document.inspect`), the face or edge name and a point (`model.faces` /
   `model.edges`), takes the camera from the action, and creates the issue.
   The panel's Connect, Add, Reply, Resolve and Reopen buttons run the same code as the plugin's
   `action` entries (`connect`, `add`, `reply`, `resolve`, `reopen`, `list`), so an agent or a script
   can do each of them through `plugin.act` too; `connect` requests the `bind` transaction, which
   `plugin.act` runs before it returns.
4. The list shows open notes first (a choice switches to All): first line, author, age, what it is on,
   and a row of buttons: Show, Reply, Resolve (or Reopen). Show looks the stored face or edge name up
   in `model.faces {body}` / `model.edges {body}`, which give each entity's current index and a point,
   and selects it with `select.set {items: [{type: face|edge, body, index, point}]}` (the shape
   `Sel::Face` / `Sel::Edge` take, `crates/engine/src/lib.rs:98-108`), or `{type: body, name}` for a body;
   when the name no longer resolves it selects nothing. It then moves the view with `view.apply` to
   the note's camera;
   the pin stays at the recorded point either way, and the panel says when the entity is gone.
5. Reply opens a text field under the note; Send posts a comment. Replies load when a note is shown.
6. Refresh fetches the list again; the panel also refreshes when opened and when the design's
   connection changes. There is no background polling in 0.1.
7. Pins: one per note that has a point; accent tone for open, done tone for resolved; resolved pins
   are hidden while the list shows open notes only. Clicking a pin moves its note to the top of the
   list, expanded.

`@login` in a note or reply is plain text to us, and GitHub notifies that person itself, so teams get
mentions and e-mail without SolidCraft sending anything.

### 9.2 Data

- Design metadata `plugin.review-notes` (written by `bind`, at most 1 KiB):
  `{"v": 1, "backend": "github", "repo": "owner/name", "key": "<16 lowercase hex>"}`. The key is drawn
  from WASI random when connecting; it ties issues to this design, so two designs in one repository
  keep separate notes. Copies of the design (Save As) keep the same key and the same notes, which is
  the expected behaviour for revisions of one part.
- Local store (per user): `cache.<key>` (last fetched list, at most 1 MiB), `outbox.<key>` (unsent
  operations, at most 256), `synced.<key>` (time of the last good refresh). When the list does not fit
  in 1 MiB, the cache keeps open notes first, newest first, then resolved ones while they fit, and the
  offline view says "showing N of M notes". A full outbox refuses new operations with a message
  instead of dropping old ones.
- A note (9.0): a client id (16 lowercase hex, random), the issue number, title, text, author login,
  created and updated times, state (open or resolved), and an anchor `{kind: face|edge|body|none,
  body?, name?, point?: [x,y,z], normal?: [x,y,z], camera?: {target, yaw, pitch, distance}}` in mm and
  radians. A note whose `name` no longer resolves in the open design is shown as detached.

### 9.3 GitHub mapping

| Note concept | GitHub |
|---|---|
| Note | issue labelled `solidcraft-note:<key>` (label created at Connect; `422 already_exists` is fine) |
| Title | the note's first non-empty line, at most 72 characters; a note whose text is empty after trimming is refused before any request (`invalid`) |
| Text and anchor | issue body: the text, a blank line, then `<!-- solidcraft-note v1 {...} -->` holding the anchor and the operation id (at most 2 KiB) |
| Reply | issue comment; its body ends with `<!-- solidcraft-reply v1 {"op": "..."} -->` |
| Resolve / reopen | `PATCH` state `closed` (`state_reason: completed`) / `open` |
| List | `GET /repos/{o}/{r}/issues?labels=<label>&state=all&sort=created&direction=desc&per_page=100`, at most 5 pages (the newest 500 notes); entries with `pull_request` are skipped |
| Replies | `GET /repos/{o}/{r}/issues/{n}/comments?per_page=100`, which GitHub returns oldest first: the plugin reads the page the `Link` header names `last` and goes back at most 3 pages (the newest 300 replies), when the note is shown |
| Duplicate check | the operation id in the fetched newest 500 notes or 300 replies; otherwise GitHub search for it, `GET /search/issues?q=repo:{o}/{r}+label:"<label>"+"<op id>"+in:body` for a resent add and `...+in:comments` for a resent reply, which has no page limit to fall out of; details in 9.4 |

Roles, from the repository's `permissions` field: Connect and Add need Write (`push`), because
creating a label and labelling a new issue need it; Resolve and Reopen need Triage or Write, or being
the note's author (the plugin learns the token's login once per session with `GET /user` and compares
it with the issue's `user.login`); Reply works for anyone who can read. The panel shows only the buttons the account
can use. GitHub silently drops the labels of a new issue when the account lacks write access, so the
plugin also checks the label in the create response; when it is missing, it closes that issue
(`state_reason: not_planned`, which its author may do) and says the account needs write access.

Moved repositories: GitHub answers requests for a renamed or transferred repository with a 301 and
a `Location` on the same origin. `net.fetch` follows no redirects, so the plugin follows 301, 302,
307 and 308 itself, only to `https://api.github.com` and at most 3 hops. When the repository itself
moved, the panel shows "the repository moved to owner/new-name" with an Update button, which runs
`bind` with the new name (one undo step; the design must then be saved and shared again), and until
then the plugin keeps using the new location for the session. `plugin.act` calls report the move in
their result.

Every request sends `Accept: application/vnd.github+json` and `X-GitHub-Api-Version: 2022-11-28`.
401/403 say the token is missing or lacks Issues access; 404 says the repository is not found or not
visible to the token; a 403 or 429 with rate-limit headers says when to try again.

### 9.4 Sync, offline, duplicates

- GitHub is the source of truth. The cache lets a design opened offline show its notes, marked
  "offline, last synced <time>". Note text is edited or deleted on GitHub, not in 0.1.
- Write-ahead: every add, reply, resolve and reopen is written to the outbox (with its operation id)
  before its request is sent, and removed only when GitHub's answer has been read. A call that is
  cancelled midway (handler deadline, `set_policy`, a crash) therefore leaves the operation in the
  outbox, and the panel shows it as "pending" rather than failed, so the user has no reason to retry it
  under a new id. Pending operations are looked at again on Refresh and on panel open.
- Each add and reply carries a random operation id and its time in its hidden block (time needs the
  `clock` permission). Before each attempt the outbox entry is rewritten as "in flight" (clearing the
  previous attempt's outcome), and after it the entry records how that attempt ended, so a crash
  during any attempt, the first or a retry, leaves "in flight" behind. A pending add or reply is in
  one of two cases:
  - **Certainly not sent**: the recorded outcome is that the connection failed before the request
    was written (DNS, connect or TLS errors). It is resent automatically, also after the app was
    restarted, since the outcome is stored with the entry.
  - **Maybe sent**: a timeout or a reset after the request was written, a lost answer, or an entry
    still marked "in flight" (the call was cancelled or the app stopped during an attempt). It is
    never resent automatically.
    The plugin looks for its id, first in the newest 500 notes or the note's newest 300 replies (which
    a refresh fetches anyway), then, once the first attempt is at least 5 minutes old (search indexes
    new text with a delay), through GitHub search (9.3), which covers any number of notes and replies.
    Found: the operation is dropped. A complete search answer (`incomplete_results: false`) with no
    hit: the panel shows it as "not confirmed" with Send again and Discard, because a teammate may
    have edited the note on GitHub and removed the hidden block that carries the id. An incomplete
    answer, a 403 or a 422 is never read as "not there": the operation stays pending, and after three
    such checks at least an hour apart it is shown as "not confirmed" too. The user decides; the
    plugin never guesses.
- Two people resolving and reopening at once: the last request wins, as on GitHub. For each
  queued resolve or reopen, save the issue's last fetched `updated_at` in the outbox; if none
  is known, fetch the issue before queuing or refuse while offline. Before retrying, `GET`
  the issue: if it already has the target state, drop the operation; if its `updated_at`
  differs from the saved value, drop it and tell the user that the issue changed; otherwise
  resend it. A change racing after the `GET` still follows GitHub's last-request-wins rule.

### 9.5 Untrusted remote content

- Only issues carrying the design's label are notes. On GitHub only people with triage or write access
  to the repository can label issues, so outsiders on a public repository cannot add notes to the
  list; their comments on a note show with their login.
- Remote text is plain text: control characters removed, 4 KiB per note and reply on screen, never
  read as markup or commands.
- The hidden block is parsed with caps (2 KiB, depth 8). Names are used only as `select.set` items
  (body at most 128 characters, face or edge name at most 256), points and cameras only when finite
  and within 1e6 mm. A hostile block can at most select something, move the view or put a pin in the
  wrong place; a malformed one gives a note without a pin.
- The token stays in the keychain; the host never hands it to the plugin, and GitHub does not echo
  it, so it does not reach the plugin's store, logs, the design or crash reports (host guarantee, 6.5).

### 9.6 Acceptance

- N1 Two app instances with two tokens on one test repository: a note added in A appears in B after
  Refresh, with its pin at the same point; Resolve in B shows as resolved in A after Refresh.
- N2 Add while offline (connection refused) is pending and sent once when back online; an add whose
  answer was lost (mock server drops it) is found by its id and not sent again, and when its hidden
  block was removed meanwhile it shows "not confirmed" instead of being resent.
- N3 A design without the metadata opens and the panel makes no network request.
- N4 A labelled issue whose hidden block is malformed shows without a pin; an unlabelled issue never
  shows.
- N5 The token appears in no file under the plugin's store, the log, the saved design or a crash report.
- N6 `plugin.act {plugin: "review-notes", entry: "add", params: {text, anchor}}` from MCP creates a
  note that the panel shows after Refresh; `entry: "list"` returns the notes as JSON.
- N8 A mocked search answering `incomplete_results: true` never triggers a resend; after three such
  answers an hour apart the operation shows Send again and Discard.
- N7 An account with only Read sees the list and Reply, no Connect or Add, and Resolve only on notes
  it wrote; a resent reply
  on a note with 350 replies, or a resent add after 600 newer notes, is found by the search check and
  not posted twice; an operation younger than 5 minutes is not resent.
- Tests: the plugin's GitHub client sits behind a small `Fetch` trait, so its logic runs as native
  unit tests against a mock of the six endpoints above; the wasm build wires it to `net.fetch`. N1 is
  also checked once by hand against a real repository, with screenshots in the S5 PR.

### 9.7 Later

- Other backends behind the same plugin code: a BCF-API 3.0 server (an open REST standard with
  existing open-source servers), GitHub Enterprise, Gitea or Forgejo, the PDM server of #3711 once it
  lands. Self-hosted servers need user-added origins, which needs a host rule for approving an origin
  at run time (`net` in 0.1 only knows the origins plugin.json lists), and BCF-API needs OAuth 2 with
  PKCE in the host.
- A hosted backend on a fixed origin (for example one the project runs) needs nothing new from the
  WIT: it is one more origin in plugin.json, and a token the user pastes works as in 0.1. A "Sign in"
  button instead of a pasted token is a host feature for secrets (OAuth 2 device authorization,
  RFC 8628), not a WIT change: the host gets the token and the plugin still only sees `has-secret`.
- Export and import of notes as a BCF 3.0 file: entity reference as the component's authoring-tool
  id, point as a zero-length line, mm to m and radians to degrees. Written from the published spec;
  its schema files are CC BY-ND, so they are not copied or modified in the repository.
- Re-pinning a detached note; editing note text; a "created on an older revision" badge once designs
  have a persistent revision id (PDM).

## 9a. Importer plugins (`importer-plugin`, unstable)

Asked for in #1221 by the author of an importer that converts other CAD formats into `.solidcraft`
designs and already builds unchanged as a `wasm32-wasip2` component. Designed now so authors can build
against it; implemented after S3 (step S3b) and marked `@unstable(feature = file-formats)` in the WIT
until then.

- **Calls.** `importer.import-design(main)` gets the imported file's name (no folders) and size. The
  plugin reads the file in pieces with `input.read("", offset, len)` and writes the converted design
  with `output.write`, so neither the input nor the output passes through one host call, and the
  8 MiB per-call copy limit of 6.2 holds. It returns the loss report as plain text.
- **Assemblies.** With `reads_input_folder`, `input.read` and `input.size` also open files by a path
  relative to the imported file's folder, read-only and only during the call. The host refuses
  absolute paths, `..`, hidden files and anything that is not a regular file, and opens only files
  that are still inside that folder once symlinks are resolved, checked on the opened handle rather
  than only on the name. A single-file format does not ask for this.
- **Rights.** The world imports only `log`, `input` and `output` (plus the closed WASI context of
  section 4): no `engine`, `store` or `net`. An importer never sees or changes an open design.
- **Where it runs.** On the plugin's worker, like an action, never on the window thread. Two ways in,
  both commands: opening a file whose extension a plugin lists in `formats` (`doc.open`, the Open
  dialog, drag and drop, MCP `open`, `solidcraft-cli run` and `eval`), and
  `plugin.import {plugin, path}`. The input path first passes the caller's path scope (MCP folders,
  the CLI's), as for a built-in format; the agent chat gate asks before `plugin.import`, as it does
  for `doc.*`.
- **Result.** The written bytes open as a new, unsaved design through the normal open path, with all
  of its checks for an untrusted file, and the host names the design after the input's stem. The
  report is shown to the user (plain text, at most 64 KiB) and returned to MCP and CLI callers.
- **Limits.** 120 s per import; guest memory up to `limits.memory_mib`; reads of at most 8 MiB per
  call, 1 GiB and 4 096 files per import; output 256 MiB; report 64 KiB. A trap, a timeout or a
  limit fails that import with "the importer failed on this file", and the next import gets a fresh
  instance (section 3).
- **Clock.** Decoders get WASI's real monotonic clock; the wall clock reads the Unix epoch unless the
  plugin asks for `clock`, so conversions do not depend on the date.
- **Cost.** A large all-formats importer pays its compile on every app start (no compiled-code cache,
  6.1); one plugin per format keeps that small.

## 10. Steps (one PR each)

| Step | Content | Done when |
|---|---|---|
| S1 | this document and `wit/solidcraft-plugin.wit` (this PR); docs/plugins.md's milestone 1 then points here | maintainer agrees on the shape in #1221 |
| S2 | engine seam + `plugin.run` + `crates/plugin-host` (load, limits, WASI context, transactions, policy, store) + plugin.json v1 parsing and approval on the Plugins page | tests T1-T9 pass; size and compile time measured |
| S3 | handlers, `net` + secrets, `host.request-execute`, action entries and `plugin.act`, panel window, pins, `plugin.panel`, `plugin.list` | tests T10-T15; screenshots of a test panel |
| S3b | importers (9a): `input`, `output`, `plugin.import`, routing of `formats` in `doc.open`, `reads_input_folder`; the `@unstable` mark goes once a real importer runs through it | tests T17-T20 |
| S4 | spur-gear example + author guide + CI job that builds `examples/plugins/*` for `wasm32-wasip2` and runs the integration test | T16 |
| S5 | review-notes plugin | section 9 acceptance |
| S6 | rename the package to `solidcraft:plugin@1.0.0` after S4 and S5 shipped in a release and at least one outside author built against 0.x | API freeze note in docs |

## 11. Tests

Host tests use small hand-written components in the WebAssembly text format (wasmtime's `wat`
feature), so they need no guest toolchain.

- T1 a component without `store` permission gets `denied` from `store.set`; with it, data survives a
  restart of the app.
- T2 a guest `loop {}` in `execute` ends with a `limit` error within the deadline + 1 s; the design
  and undo depth are unchanged; the app keeps running; the next call starts a fresh instance.
- T3 a guest trap (`unreachable`) in the middle of a transaction that already ran two edits and a
  `sketch.edit` leaves the design exactly as before (`document.inspect` equal), undo and redo depth
  unchanged, and the saved session state of 6.3 step 2 equal (no sketch left active).
- T4 a transaction that runs 5 edits is one undo step: `edit.undo` once returns the design to before.
- T5 commands not in `commands`, and deny-list ids listed in `commands`, return `denied`; nothing runs.
  `engine.describe`, when listed, works in a transaction and in a handler.
- T6 `motion.export_animation {path}` listed in `commands` is refused by the empty PathScope, in a
  transaction and in a handler (it is `noundo`, so only the PathScope stops it in a handler).
- T7 `document.metadata.set` on `plugin.other` or on `author` is `denied`; on `plugin.<id>` it works
  and is undoable.
- T8 memory growth past `memory_mib` traps; the app keeps running.
- T9a an unapproved plugin's component is never compiled; once approved, a 15 MiB component compiles
  on the background thread: the Plugins page keeps drawing frames while it shows "checking...", and a
  `plugin.run` during the compile fails at once with "still loading".
- T9 plugin.json v1: every rule in section 5 has a refusal case; a listing-only folder still lists.
- T10 a handler calling an undoable command, or a `noundo` command off the handler-safe list
  (`browser.visibility`, `component.activate`), gets `denied` even when plugin.json lists it;
  `select.set` works. The handler-safe list test of 6.4 rule 5 passes.
- T10a a handler started in design A whose `engine.run` or `request-execute` is served after the user
  switched to design B gets `denied`; nothing is written to B (Connect scenario of 9.1).
- T10c a guest blocked in a WASI clock subscription or `poll` for an hour ends at its deadline with
  `limit`; a guest waiting in `net.fetch` on a server that never answers ends the same way.
- T10d `set_policy` that disables a plugin in the middle of a slow `net.fetch` returns at once (under
  50 ms in the test), its panel disappears, and the late response is discarded.
- T10e an `action` entry through `plugin.act` may fetch but gets `denied` for an undoable command;
  `plugin.run` of an `action` entry and `plugin.act` of a `transaction` entry are `invalid`.
- T10f in the app, `plugin.act` over the control channel against a server that never answers: the
  window keeps drawing frames (a `ui.screenshot` succeeds meanwhile) and the request's reply comes
  through `control.pending` with `limit` after 30 s.
- T10g `plugin.run` while the same plugin's handler waits on a slow `net.fetch` fails at once with
  "busy" (under 50 ms), and a `request-execute` from that handler runs after it returns.
- T10h in the app, `engine.batch` with a `plugin.act` step is refused at once with "run plugin.act
  as its own request"; in a CLI script the same step runs and returns the action's result; a
  component with two memories cannot together exceed `memory_mib`.
- T10i Connect started in a new tab, then a design opened into that same tab before GitHub answers:
  `bind` is `denied` and the opened design has no `plugin.review-notes` metadata.
- T10b a transaction whose plugin answers late: an `engine.run` request that arrives after
  `plugin.run` timed out is dropped, and the design and undo depth stay as `plugin.run` left them.
- T11 `net.fetch` to an unlisted origin, to `http://`, or to a URL whose origin differs only in port
  or case of scheme is `denied`; a certificate that does not match the host name fails the request
  (tested against a local HTTPS test server, no internet).
- T12 the secret header is added for its origin only, a plugin-set header of the same name is
  replaced, and the plugin cannot read the value.
- T13 a transaction calling `net.fetch` gets `denied`.
- T14 `request-execute` from a handler runs after it returns and delivers `executed`.
- T15 panel and marker caps: an over-long list ends the call with `limit`; the previous panel stays.
- T16 (S4) the spur gear built from source runs through `plugin.run` and produces one body whose
  volume matches the analytic gear volume within 0.5 %.
- T17 (S3b) an importer reads a test file in 1 MiB pieces and its result opens as a new unsaved design
  named after the input; a malformed `.solidcraft` result is refused by the normal open checks and
  leaves no tab behind.
- T18 with `reads_input_folder`, a part file next to the input is readable; `..`, an absolute path, a
  hidden file and a symlink that leads out of the folder are `denied`; without the flag, every path
  other than "" is `denied`.
- T19 a decoder trap fails that import with "the importer failed on this file" and the next import
  succeeds on a fresh instance; recursion deeper than `stack_kib` traps the call and the app keeps
  running.
- T20 two importers listing the same extension: the second does not load, and the Plugins page names
  both.

## 12. Threat model

Trusted: the app, the user, the user's approval. Untrusted: plugin code and its plugin.json; design
files (their `plugin.*` metadata is input to plugins and to the host); every network response; any
text from other team members. The sandbox goal is: a plugin can do only what its approved
permissions say, and cannot crash the app or corrupt a design.

Frozen depth for 0.1 (findings beyond this are out of scope, not defects):

- Out: the run time of one engine command the user approved in `commands` (it runs to its end as it
  would from the toolbar or MCP; the deadline only stops the plugin from starting more); the CPU time
  of compiling an approved component (one compile at a time, 6.1);
  side channels and speculative-execution attacks inside wasmtime; bugs in wasmtime or Cranelift
  themselves (handled by keeping wasmtime current); a plugin wasting its own approved budgets (it is
  stopped by the limits, nothing more); what a plugin does with data on an origin the user approved;
  social engineering on the approval screen beyond showing the permissions plainly.
- In: escaping the permissions; reading files other than an imported one (and its folder with
  `reads_input_folder`); writing files; reaching servers other than those that hold
  a certificate for an approved origin (section 6.5);
  reading secrets; editing the design from a handler; leaving a partial edit; growing app memory or
  time without bound; crashing through any host function input.

## 13. Alternatives and later work

- `manifest()` export (issue sketch): needs code to run before consent; replaced by static plugin.json.
- Raw C ABI on wasmi: narrow and hand-marshalled; wasmi runs no components.
- Rhai scripts (#1221 comment): good for macros (#4267 covers recording), but Rust-only syntax, no
  other languages, and no isolation story for network secrets. Not excluded later as a scripting layer
  over the same commands.
- `wasi:http` instead of `net`: standard bindings, but streams and pollables add host code and
  hyper/tokio surface for one request-response use; revisit with WASI 0.3.
- Dynamic `CommandSpec` registration: the registry is static by design; `plugin.run` with entries
  keeps one registry.
- #4933 (plugin commands in Design > Utilities) is complementary: it is about where plugin actions
  appear and how their parameters are asked for, this document about the runtime under them. One
  point to settle between the two: #4933 gives each contribution its own namespaced id in the host
  command registry, while here every entry is reached through the static `plugin.run` /
  `plugin.act` commands. Both can hold if a Utilities button or palette item for an entry runs
  `plugin.run {plugin, entry, params}` with the parameters its host-rendered form collected, and an
  id such as `plugin.<id>.<entry>` is a display and search name for it rather than a new
  `CommandSpec`.
- Events by subscription: the engine has none; the coalesced `design-changed` over `Session.revision`
  covers the pilots.
- Later interfaces (each as a new interface in `1.x` with `@since`): an exporter world (importers
  are in 9a), `feature` world (`evaluate` for parametric features per docs/plugins.md),
  background jobs with progress (CAM, FEA, slicer), viewport overlays beyond pins (toolpaths), plugin
  workspaces, the browser host (docs/web-runtime.md), signing and an index.

Open questions for the maintainer:

- Q1 Is the wasmtime + WASI dependency cost acceptable (measured in S2), or Pulley/no-WASI first?
- Q2 Default for a newly installed plugin with code: unapproved (proposed) vs today's "on".
- Q3 GitHub issues as the first notes backend (section 9). Proposed because its origin is fixed
  (fits `net` in 0.1), teams already have it, and it brings mentions and e-mail for free. The cost: a
  forge-specific store, so the plugin keeps its anchors in its own hidden block and a BCF export path
  (9.7). Alternatives: a BCF-API server first (needs run-time origins and OAuth in the host), or the
  PDM server (#3711) once it lands. A hosted backend would be one more origin and changes nothing in
  this interface; the free GitHub and self-hosted paths stay.
- Q4 Examples in this repository (`examples/plugins/`) or a separate tangency-labs repository.
