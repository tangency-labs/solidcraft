//! Convert to Sheet Metal can make a foldable 3D print: a `sheet_convert` feature may carry
//! `print_fold: true` with `print_wall`, `print_clearance` and `print_film` (length
//! expressions), `print_joint` ("butt" or "mitre"), `print_splits` (points on edges where the
//! print splits into parts) and `print_filament` (a list of `{at, segments, diameter, tolerance,
//! flip, press?}`: open edges or bends joined by fingers and a channel for a filament, or with
//! `press: {radius, height, tolerance}` by the same fingers and a nub that snaps into a dimple). Such a feature makes the
//! flat pattern as one printable body, with a V-groove at each bend that leaves a thin film to
//! fold on, its walls butted, mitred or fingered where they meet on edges that are no bends, and
//! the faces past each split edge as a part of their own beside the rest. Older designs have none
//! of these fields and convert as before, so no step rewrites them. The last release opens a
//! design that carries them with a warning, keeps what it can't read, and converts that feature
//! to sheet metal.
