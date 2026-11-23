---
name: solidcraft-plugins
description: Use when authoring, packaging or testing a SolidCraft command plugin or importer, including its manifest, permissions and execution budgets.
---

# Author a SolidCraft plugin

Read [Writing a plugin](../../docs/book/src/dev/plugins.md) for the tested Rust walkthrough,
[Plugins](../../docs/plugins.md) for packaging and importers, and
[the runtime reference](../../docs/plugin-runtime.md) for approval, permissions and limits.
Use the current [WIT](../../crates/craft-plugin/wit/plugin.wit) and generated command schemas
from `solidcraft-cli commands` or `engine.describe`. Keep plugin algorithms and packages outside
the SolidCraft host repository.

## Rust quickstart

Create a `cdylib` crate with `wit-bindgen` and copy the current WIT into its `wit` folder.
Generate the `command-plugin` world, implement the exported guest and export it:

```rust
wit_bindgen::generate!({ path: "wit", world: "command-plugin" });
struct Plugin;
impl exports::craft::plugin::entry::Guest for Plugin {
    fn execute(_: String, _: String) -> Result<String, craft::plugin::types::PluginError> {
        craft::plugin::commands::run("solid.box", r#"{"length":20,"width":10,"height":3}"#)?;
        Ok("{}".into())
    }
}
export!(Plugin);
```

Build for `wasm32-unknown-unknown`, then wrap the module with `wasm-tools component new`.
Use the book's manifest and build steps; build no WASI dependency.

## TypeScript quickstart

Generate bindings with `jco types` from the same WIT. Compile this entry module to JavaScript:

```typescript
import { run } from 'craft:plugin/commands@0.2.0';
export const entry = {
  execute(_entry: string, _params: string): string {
    run('solid.box', JSON.stringify({ length: 20, width: 10, height: 3 }));
    return '{}';
  }
};
```

Use `jco componentize` with world `command-plugin` and the current WIT; disable its `stdio`,
`random`, `clocks`, `http` and `fetch-event` WASI features. Run the host's package check to
verify the resulting imports. The component contains its JavaScript engine; the host permits
no network imports.

## Manifest, budgets and consent

The package folder matches its `id` and holds `plugin.json` plus the named component.
Declare `commands@0.2`, exact command ids, entry schemas and only required permissions.
Folder names and command groups request access; only the user's grants authorize it.

For a large command list, declare `host_calls` in `runtime.limits`; omission keeps 256.
The guest compute deadline defaults to 5000 ms and excludes native host work. Raising the
request needs the user's `max_host_calls` or `max_deadline_ms` ceiling raised in Preferences
or the `--plugin-limits` file. Changing a request changes the content hash: inspect again
and obtain approval. Never treat an inspected hash as consent or retry automatically.
One successful run is one undo step; any refusal discards the transaction, even if caught.

## Check, test and package

Run `solidcraft-cli plugin check <package>` before execution. Put synthetic JSON cases in
the package's tests folder, then run `solidcraft-cli plugin test <package> --json --require-cases`.
Assert model measurements and failures, including default-ceiling refusal and an approved
raised ceiling for larger requests. An `over_ceiling` names host policy; `exhausted` names
the manifest budget. Follow each error's named limit instead of raising unrelated budgets.
Ship a zip containing the manifest and component, without source or tests. Review its
dry-run installation result before authorized installation or execution.
