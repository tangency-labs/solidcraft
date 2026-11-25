# craft-plugin

App-independent contracts for one-shot plugins in Craft apps. Commands and schemas are
JSON data supplied by the app. This crate has no SolidCraft or CAD dependency and runs
no plugin code.

## Public API

| Item | Purpose |
|---|---|
| `PluginHost` | App command catalog and execution, transactions, file authorization, logging, progress and cancellation |
| `CommandSchema`, `Effect` | Command input/output schemas and read/edit effects |
| `Manifest`, `RuntimeManifest`, `Entry`, `UiSlot` | Package metadata, executable entries, declared inputs and placement preferences |
| `parse_manifest`, `valid_id` | Bounded parsing and validation; package ids must match folder names |
| `Permissions`, `FilePermission`, `FileGrant` | Requested capabilities and separately approved named folders |
| `ConnectPermission`, `Transport`, `AllowedHost` | Endpoints a `net@0.1` plugin connects to, and the user's allowlist of servers |
| `Limits`, `HostLimits` | Invocation budgets and user ceilings; `HostLimits::check` compares them |
| `HostError`, `Result` | Typed permission, command, limit, manifest, input, trap and cancellation failures |
| `API`, `WIT`, `MAX_*` | Interface identifier, WIT text and size/address-space limits (`MAX_FILE_BYTES`: one file through `files`) |
| `ui` | Typed panel declarations, widgets, events, slash-path data and validation |

The app implements `PluginHost` and defines command policy, approval and transaction history.
`authorize_file_path` applies caller scope to each complete file path, in addition to folder
grants. Its default denies hidden child names unless a more specific approved folder grant
covers them; an app may override it to enforce its explicit scope policy. `names_folder` says
whether `files.locate` may tell the plugin a granted folder's full path: no by default; an app
answers yes only for a folder it keeps for the plugin, never one the user chose.

The [WIT](wit/plugin.wit) declares `craft:plugin@0.2.0`, world `command-plugin`: the
capability `commands@0.2` (`CAPABILITIES`). A manifest lists the capabilities it needs
(`"capabilities": ["commands@0.2"]`); `check_capabilities` refuses an unknown or newer one with
`HostError::UnsupportedCapability`. While a version is 0.x, minors may break each other. 0.2 added
`plugin-error.unsupported`; the frozen [0.1 WIT](wit-0.1/plugin.wit) (`WIT_0_1`,
`OLDER_CAPABILITIES`) is still served, so plugins built against it keep running.
`HostError::OverCeiling` and `HostError::Exhausted` carry a limit's numbers.
The [Wasm runtime](../craft-plugin-wasm/README.md) begins an app transaction and requests
commit or rollback after guest execution. It stages granted file writes until the guest
succeeds and the app commits. File publication after app commit is not atomic across
multiple targets; a publication failure can leave files changed. These contracts do not
promise atomic rollback of all external effects.

## Limits and permissions

Invocation defaults are 10,000,000 fuel, 64 MiB memory and a 5000 ms deadline.
`HostLimits` defaults to 100,000,000,000 fuel and 128 MiB. Callers must check requested
budgets against their ceilings before execution. Fuel must be positive, memory is
1..4096 MiB and the deadline is 1..30000 ms. Manifests are capped at 64 KiB.

Commands are exact ids, intersected with the app catalog. A catalog command may carry a
sensitive `group`; the guest calls it only when `permissions.command_groups` requests the
group and `PluginHost::granted_command_groups` grants it. Folder requests alone grant
nothing: the app supplies matching `FileGrant` values. Clock permission allows wall time;
without it, the runtime returns a fixed epoch. The reserved `net` boolean is refused; a plugin
that connects lists `net@0.1` in its capabilities and each endpoint in `permissions.connect`
(host pattern, optional port, `tcp`, `tls` or `ssh`), and connects only when
`PluginHost::granted_net` says the user allowed it. `HostLimits::allowed_hosts`, when it names
any server, narrows every plugin's endpoints further (`HostLimits::allows_host`).
Parsing a manifest neither approves nor executes its component. These contracts have no
native execution requirement.

## Declarative panels

`ui@0.1` adds the `panel-plugin` world, which includes `command-plugin`, imports `ui`
and exports `ui-provider`. Its records match RFC 0015. `runtime.ui` declares the layouts
shown before approval; `ui-provider.describe` must return those same declarations.
A panel carries only host-drawn widgets and theme colour roles. Unknown widget kinds
remain loadable as placeholders; trees, charts and workspaces are reserved for phase 3.

`ui::Declarations::validate` checks dock zones, command entry references and rooted
layout trees. `ui::PanelData::set_data_for` validates an entire slash-path update before
staging it; `merge_frame` adopts all successful updates once for the renderer. Values
always use millimetres and degrees. A panel has at most 1 MiB of data; layouts have at
most 512 widgets, plugins have at most 8 panels, rows at most 100,000 and charts at most
8 series of 20,000 points. Default `PluginHost::ui_*` methods refuse panels for apps
that do not serve them.

## Validation

From the repository root, `cargo test -p craft-plugin` exercises manifests, permissions,
entries and limits. `cargo xtask shared` checks extraction and tests an isolated workspace.
See [the runtime reference](../../docs/plugin-runtime.md) for the SolidCraft adapter policy.
