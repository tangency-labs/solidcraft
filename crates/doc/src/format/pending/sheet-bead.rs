//! Formed beads: a `sheet_bead` feature (Bead: `sketch`, `curves`, `depth`, `width`, optional
//! `shape`, `angle` or `crown_width`, `base_radius`, `crown_radius`, `end_length`, `margin`, `flip`
//! and `body`), and a sheet made from surfaces' beads in its panels' charts (`beads`). Older
//! designs need no transformation: they have no beads. The last release refuses a design saved
//! with them (the newer format number) instead of meeting a feature it can't read.
