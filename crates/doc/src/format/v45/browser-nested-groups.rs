//! Browser groups can nest and be ordered. A group saves its `parent` group (omitted at the top
//! level); a group's items and a folder's `browser_order` list may name groups as numbers beside
//! item keys as strings; and `browser_order` has a `"<component>/components"` list ordering a
//! component's sub-components by placement id. Older designs read unchanged: they have no
//! parents and no group entries. The last release refuses the newer format and asks for an
//! update instead of dropping the nesting and the order.
