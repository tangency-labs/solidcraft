# Original box plugin

This package is built separately from SolidCraft. It exercises the implemented command-plugin
world with one native `solid.box` operation. It is a developer example; no executable is bundled.

From this directory, with Rust's `wasm32-unknown-unknown` target and `wasm-tools` installed:

```sh
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new target/wasm32-unknown-unknown/release/solidcraft_example_box.wasm -o plugin.wasm
```

No WASI adapter is needed. Keep `plugin.json` and `plugin.wasm` together in this `example-box`
folder, or copy both to the per-user plugins directory's `example-box` folder. Rebuild the component
when the WIT changes; API 0.x is experimental.

Call `plugin.inspect {"path":"/absolute/path/to/example-box"}`, review its manifest, then use the
returned `approval` in `plugin.run {"path":"/absolute/path/to/example-box","approval":"...",
"entry":"make","params":{"length":20,"width":10,"height":3}}`. These are engine commands:
through the CLI, put each call in a JSON script and use `solidcraft-cli run`; for the running app
use standalone `engine.execute` control requests. A script cannot silently substitute the hash
of a changed component. The expected box volume is 600 mm³ and Undo removes the complete result.

The example has no geometry read permission and no file/network/process access. Its dependency
sources are ordinary permissively licensed Rust crates. Build outputs and generated component bytes
are ignored; the source, manifest and WIT are the reviewable deliverables.
