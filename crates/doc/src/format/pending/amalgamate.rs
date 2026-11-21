//! Amalgamate features (#18799): `{"type": "amalgamate", "tool": {source, tag, records, roles, faces?},
//! "seeds": [frame..], "turns"?, "flip"?, "remove_from"?, "add_to"?}` place the bodies an Amalgam Tag
//! labels in another design (kept here as B-rep records, in the tag's seat frame) at each seed.
//! Older designs have none and are unchanged. The last release warns about the newer format number,
//! opens what it can read and keeps the rest.
