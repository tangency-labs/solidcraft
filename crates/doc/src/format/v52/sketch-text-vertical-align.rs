//! A sketch text's style can say where its block of lines sits up and down against its anchor
//! (`at`, or the curve it follows): `vertical_align` is `top`, `middle` or `bottom`, and is left
//! out for the first line's baseline, where every text sat before. Older designs need no rewrite:
//! without the key a text stays on its baseline. The last release opens a design that carries the
//! key with a warning, keeps what it can't read, and draws such a text on its baseline.
