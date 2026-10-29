//! A design can hold a Lighten feature (`"type": "Lighten"`): lightening pockets cut into a
//! planar face, with its face pick, body, wall, depth, draft, corner radius and the optional
//! edge fillet and chamfer (`fillet`/`fillet_at`, `chamfer`/`chamfer_at`). Older designs need no
//! transformation: they have no Lighten feature. The last release refuses a design saved with
//! this (the newer format number).
