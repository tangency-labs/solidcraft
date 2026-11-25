//! An Import feature's stored product tree (`components`) says which STEP solid each body is:
//! each node may carry `items`, the solid entity of each of its `bodies`, and `left_out`, the
//! product's solids the reader could not read, at their place among its solids in file order.
//! When opening finds a solid the stored tree has but the reader leaves out, the solid keeps its
//! slot: its body index and item stay in the tree and its name stays in `body_names`, after the
//! file's bodies (#16857). Older designs have neither field, so nothing in them is rewritten; a
//! body of theirs is matched by its place among its product's solids in file order, or by name.
//! The last release warns about the newer format number, opens the design, matches its bodies as
//! it did before, and keeps the fields it can't read when the design is saved.
