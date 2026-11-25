# craft-plugin-wasm

App-independent Wasmtime component execution for
[`craft-plugin`](../craft-plugin/README.md). The app supplies commands through `PluginHost`;
this crate has no SolidCraft or CAD dependency.

## Public API and lifecycle

- `run(host, manifest, component, entry, params)` uses default runtime options.
- `run_with_options(...)` also accepts `RuntimeOptions { stack_bytes }`.
- `describe_ui(host, manifest, component)` reads and validates static panel declarations without command or file authority.
- `run_ui_event(host, manifest, component, entry, params, event)` delivers a declared panel action and runs its ordinary entry in the same transaction.
- Entry calls return `(host, Result<serde_json::Value>)`; `describe_ui` returns
  `(host, Result<craft_plugin::ui::Declarations>)`. Both retain the host on failure.
  The owned host must implement `PluginHost + 'static` because Wasmtime stores own their host data.

Each invocation compiles a component and creates a fresh store. After preparation, the
runtime begins the app transaction, instantiates the component and calls its requested
entry. Preparation failures return the host without starting a transaction. Guest failures
request rollback; a successful JSON result requests `commit_transaction`.

Granted file writes are staged, and reads in that invocation can see staged content.
A guest failure or failed app commit discards staged writes without replacing targets.
After app commit, the runtime publishes writes in call order. Publication across several
files is not atomic: an error can leave the app committed and some files changed.
The returned error reports that possibility; this is not an unconditional rollback guarantee.

Panel plugins use the additive `panel-plugin` world. Their typed `ui-provider.describe`
records must match `runtime.ui` exactly before panel host calls are enabled. The runtime
validates slash paths and typed values, then forwards `set-data`, `clear-data` and
`show-panel` to `PluginHost`. The app stages these changes with its command transaction
and merges successful data updates once per frame. Existing command and importer
worlds, including the frozen 0.1 interfaces, retain their original behavior.

The caller owns approval and must enforce `HostLimits::check` before calling `run`.
The runtime validates the runtime declaration and applies its requested budgets; it does
not obtain user consent or install packages.

## Sandbox and targets

Native execution is provided on x86-64 and AArch64. On other architectures, including
browser `wasm32`, functions return the supplied host and `HostError::Trap` explaining that
execution is unsupported. There is no browser execution backend.

The [WIT](../craft-plugin/wit/plugin.wit) imports app commands, logging, progress,
cancellation, named files and a clock. No WASI is linked. Commands are intersected with
the host catalog; read entries cannot invoke commands marked edit, and a command in a
sensitive group needs that group requested by the manifest and granted by the host. A
component importing another version of the `craft:plugin` interfaces is refused before
instantiation with `HostError::UnsupportedCapability`. File access requires
matching approved grants and `PluginHost::authorize_file_path` approval for each complete
path. A symbolic link (or Windows junction) anywhere in a requested path inside a granted
folder is refused, not followed. There is no ambient filesystem access. Wall time requires
clock permission.
Requests for `net` and reserved socket, HTTP and fetch imports are refused.

The additive `user-storage` interface requires `user-storage@0.1` in a command plugin's
manifest. It delegates JSON get/set/remove/list calls to the host for the calling plugin
alone. A `read` entry cannot set or remove settings, even when the capability is present.
Keys and JSON inputs (at most 1 MiB before parsing) are checked before delegation; the host applies the
shared map budgets (128 keys and 64 KiB), persistence and commit/rollback policy. Existing
0.1 and 0.2 components continue linking without importing the new interface.

Fuel, epoch interruption and store limits bound guest execution. Defaults are
10,000,000 fuel, 64 MiB memory and 5000 ms. One linear memory is allowed; memory64 is
disabled. Components are capped at 16 MiB and JSON parameters/results at 1 MiB each.
`RuntimeOptions` defaults to a 512 KiB stack and accepts 64 KiB..64 MiB.
Compilation and app-native command work are outside guest fuel accounting. The deadline
cannot forcibly interrupt an arbitrary native host call.

## Validation

From the repository root, `cargo test -p craft-plugin-wasm` exercises original synthetic
components, transactions, traps, limits, file scope and staged writes on supported native
targets. `cargo xtask shared` also checks isolated shared-crate builds and tests.
See [the runtime reference](../../docs/plugin-runtime.md) for full limits and app policy.
