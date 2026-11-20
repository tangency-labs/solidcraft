//! A note says which end of its text sits at its place (`align`: `left`, `centre` or `right`; #20122),
//! and its lines stand that way too. A note saved without one is centred, as every note was before, so
//! an older drawing reads and draws as it did, and a centred note saves without one: nothing is
//! rewritten as a drawing opens. The last release refuses a drawing saved with one (the note has a
//! field it doesn't know), as a newer format.
