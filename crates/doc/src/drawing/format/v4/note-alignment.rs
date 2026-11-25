//! A note says which end of its text sits at its place (`align`: `left`, `centre` or `right`, the
//! values a title block's cells take; #20122), and each of its lines stands that way too. A note
//! saved without one keeps the layout every note had: the block centred on its place, its lines
//! starting at the block's left edge. So an older drawing reads and draws as it did, and a note
//! with no alignment saves without one: nothing is rewritten as a drawing opens. The last release
//! refuses a drawing saved with one (the note has a field it doesn't know), as a newer format.
