//! A cap of an extrude also answers to the end it closes, `F<extrude>:cap:low` or
//! `F<extrude>:cap:high`, and a sketch on a cap whose own name can't keep its attachment attaches
//! by that name, saved as the attachment's face name. That happens with an extrude whose start is
//! offset off its sketch plane: both caps lie on one side of it and share one name (`top`, or
//! `bottom`, numbered by where they lie), and once a later feature split one, a sketch on a piece
//! of it could only take a fixed plane before. The faces keep their names, and older designs need
//! no change. The last release opens a design with such an attachment with a warning about the
//! newer format; it doesn't know that name, so that sketch fails, saying its face no longer
//! exists, while the rest of the design builds (`crates/doc/src/newer_file.rs`).
