//! A joint can say which assembly constraint it was made as (`"constraint": "mate"`, `flush`,
//! `distance`, `concentric`, `insert`, `parallel`, `perpendicular`, `angle`, `tangent` or
//! `point_on_axis`), written only for a joint made with `assembly.constraint.create`. The joint
//! itself is an ordinary joint of the kind the rule needs (a mate is a planar joint), so the solver
//! never reads the field; it names the rule in the browser and the Constrain dialog, and makes the
//! joint start where its parts are when it is changed. Older designs need no transformation: their
//! joints have no constraint and are written as before. The last release opens a design with such
//! a joint with a warning and keeps the field it can't read; there the joint still solves as its
//! kind.
