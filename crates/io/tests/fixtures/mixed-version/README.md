# Mixed-version fixtures: a design opened and saved by the last release

`saved-by-main.sc` is a small design saved by main with every kind of binary data a design
keeps as container parts: plugin files, a canvas image, a point cloud with an edge, a face and a
property, an embedded HDR and a texture, and a drawing with a placed image and a title-block logo.

`saved-again-by-0.45.0.sc` is that file opened and saved by the SolidCraft 0.45.0 release CLI
(`solidcraft-cli run --in saved-by-main.sc --save saved-again-by-0.45.0.sc`).

What 0.45.0 does with it, checked by hand with the release CLI:

- It warns that a newer SolidCraft saved the file and opens it. It names what it doesn't read and
  keeps it: the `drawing_parts` field, the point cloud section with its `complex_blob`, and the
  drawing's `image_files`, `placed_images` and `logo_cells`. The drawing opens without its images,
  and the logo cell shows its text.
- Its save keeps every binary part byte for byte. It keeps a part only when `document.json` or a
  feature names it, so the design names the drawings' image parts in `drawing_parts`.

The tests in `crates/io/src/design/drawing_tests.rs` check that main reads the file 0.45.0 saved
back exactly as it saved it, with every part. They also check that what main writes keeps the
layout 0.45.0 needs. To repeat the check by hand, download the release CLI, open and save a design
saved by main with it as above, and reopen the result in main.
