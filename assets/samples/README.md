# Sample previews

The editable authority is the built-in command scripts in
`crates/engine/src/sample.rs`. The six 240 × 150 PNGs in `exports/` are generated
by SolidCraft's thumbnail renderer from those original designs, using the same
isometric camera as Recent. They are embedded by `home.rs`, so displaying Home
does not evaluate sample geometry or read pixels back from the GPU. Selecting a
sample builds its editable design through the normal commands.

Regenerate after changing a sample script:

```sh
cargo test --profile ci -p solidcraft-ui-egui --lib home::tests::regenerate_sample_thumbnails -- --ignored --exact --nocapture
```

`exports/scripts.json` records SHA-256 hashes of the scripts. The regular tests
check the hashes and decode all six pictures with the same bounds as Recent.
The generated PNGs and hash file are necessary shipped exports; benchmark
screenshots and other review output stay outside the asset tree.
