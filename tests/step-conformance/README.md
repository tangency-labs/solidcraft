# step-conformance parts

STEP files of parts whose exact shape is known, generated from formulas by
[step-conformance](https://github.com/dan335/step-conformance) (`corpus/generated`, commit
96f30bb). The corpus is dedicated to the public domain under CC0 1.0.

`step_in::tests::step_conformance_parts_read_back_their_exact_shape` in `crates/kernel` reads
each one and checks its volume against the generator's profile, its box and its face count.
