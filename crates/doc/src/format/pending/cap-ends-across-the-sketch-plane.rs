//! Every cap an extrude names by its height answers to the end it closes, `F<extrude>:cap:low` or
//! `F<extrude>:cap:high`, wherever the extrude's start lies. Format 73 gave those names only to
//! the caps of an extrude whose start is offset off its sketch plane with both caps on one side
//! of it. A sketch attached by such a name now still finds its cap after an edit moves the offset
//! across the sketch plane. Nothing in a design is rewritten, and older designs mean what they did.
//! The last release opens a design saved with this with a warning about the newer format. In such a
//! design, a sketch whose cap now lies across the sketch plane fails on that release, saying its
//! face no longer exists, while the rest of the design builds (`crates/doc/src/newer_file.rs`).
