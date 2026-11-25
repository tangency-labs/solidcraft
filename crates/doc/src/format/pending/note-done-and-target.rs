//! A note may save that it is done (`"done": true`) and the element it refers to
//! (`"target": {"kind": "face", "body": "Plate", "index": 15}`), as the Notes panel records them
//! when a note is made on a selection or ticked off. Both are saved only when set, so a design
//! whose notes have neither saves as before, and older designs open with every note open and
//! without a target. The last release opens a design that has them with a warning about the
//! newer format number, and keeps the fields it doesn't read.
