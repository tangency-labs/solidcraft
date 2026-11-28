//! A sketch on a cap of an extrude whose start is offset off its sketch plane (both caps on one
//! side of it, both named `top`, or both `bottom`, numbered by where they lie) can attach by the
//! end the cap closes: `F<extrude>:cap:low` or `F<extrude>:cap:high`, saved as the attachment's
//! face name. Before, once a later feature split the cap, such a sketch could only take a fixed
//! plane. The faces keep their names, and older designs need no change. The last release opens a
//! design with such an attachment with a warning about the newer format; it doesn't know that
//! name, so that sketch fails, saying its face no longer exists, while the rest of the design
//! builds (`crates/doc/src/newer_file.rs`).
