# Building and running

Use Rust and Cargo 1.95 or newer, as declared in the workspace manifest. Check
which tools your shell will run with `rustc --version` and `cargo --version`.
If Cargo reports that resolver `3` is invalid, it is too old to read the
workspace manifest. When using rustup on Linux or macOS, check that
`$HOME/.cargo/bin` comes before the system Cargo directory in `PATH`.

For a headless build, run `cargo build --locked --release -p solidcraft-cli`
from the repository root.

```sh
cargo run --release -p solidcraft -- --sample          # desktop app with a sample part
cargo run --release -p solidcraft-cli -- commands      # the command registry
cargo run --release -p solidcraft-cli -- run examples/bracket.json --out bracket.step
cargo run --release -p solidcraft-cli -- snapshot examples/bracket.json --out bracket.png
cargo run --release -p solidcraft-cli -- snapshot part.sc --out preview.png --tool Pin --section z=12
cargo run --release -p solidcraft-cli -- render examples/render-studio.json --out product.png
cargo run --release -p solidcraft -- part.step          # open a STEP file as a new design
cargo run --release -p solidcraft-cli -- eval part.step # measure an imported STEP file
cargo xtask ci                                          # fmt, clippy, tests, icons, assets, layers, wasm
```

UI artwork changes follow the [icon specification](../crates/look/icons/STYLE_GUIDE.md).
The [included review tool](../crates/look/icons/README.md) discovers new icons and keeps
unaccepted artwork Open; run `cargo xtask icons` to check the collection.

Windows builds: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.


In the browser: `cd apps/solidcraft-web && trunk build --release --public-url ./` (WebGPU, or
WebGL2 with `?webgl`; `?sample` opens the sample part).

To build the documentation, put `mdbook` on `PATH` and run `cargo xtask book`
from the repository root. This command generates the command reference and
screenshots before building the book into `target/book`. A fresh checkout needs
this generation step before `mdbook build docs/book` can read every chapter.

Agents can control the document open in a browser tab through `window.solidcraft.execute(...)`
and `window.solidcraft.request(...)`; see the [browser command API](browser-api.md).

See [CONTRIBUTING](../CONTRIBUTING.md) for the test and review workflow.
