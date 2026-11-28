//! A linked component's link keeps `inherited_metadata`: the linked design's properties (its
//! `metadata`) as the component last took them, `{"part_number": "P-1", "revision": "B"}`, left
//! out when the design has none. Update Links and Relink Source then give the component the
//! design's new values, except for a property changed in this design. Older designs have no
//! `inherited_metadata`: a property their component already has stays, and one it lacks is added
//! at the next Update Links, so nothing in them is rewritten. The last release opens a design that
//! has it with a warning about the newer format and keeps the key as it came; there Update Links
//! leaves the component's properties as they are, as before.
