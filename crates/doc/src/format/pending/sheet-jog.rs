//! A design can hold sheet metal jogs (#6046): a `sheet_jog` feature with its bend line (a
//! sketch line by `sketch` and `curve`, else the points `a` and `b`), the `offset` of the step
//! and what it is measured between (`measure`: outside, inside or faces), the `angle` and
//! `radius` of its two bends, where the line sits in the jog (`position`: start, middle or
//! end), `flip`, the `fixed` side and the sheet's `body`. Older designs need no transformation:
//! they have no such feature. The last release refuses a design saved with this (the newer
//! format number).
