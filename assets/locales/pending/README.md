# Pending UI strings

A pull request that adds UI strings adds **one new file here** instead of editing
`assets/locales/<locale>.json`. Two pull requests then never touch the same file, so they never
conflict.

- Name it after the change, lowercase with dashes: `sketch-trim-hints.json`.
- Contents: a map from locale code to `{"message id": "text"}`. Give English; add other locales
  you can translate (a locale you leave out falls back to English, as before):

  ```json
  {
    "en": { "sketch.trim.hint": "Click a segment to trim it" },
    "de": { "sketch.trim.hint": "Segment zum Kürzen anklicken" }
  }
  ```

- The id must not already be in that locale's catalog, nor in another pending file; the build and
  `cargo xtask lists` fail otherwise. To change an existing message, edit the catalog itself.
- Don't sort or fold anything. The app, `cargo xtask i18n` and the tests read these files as part of
  the catalogs, so your strings work in your own pull request.
- After a merge, the Generated files workflow runs `cargo xtask fold-pending` on `main`: it adds the
  messages to the catalogs (sorted, one per line) and deletes the files. Never run it in a pull
  request.
