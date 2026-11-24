//! An occurrence can count in the bill of materials as another type than its component
//! (`bom_type` on the occurrence: `normal`, `phantom`, `reference`, `purchased` or `inseparable`),
//! for that occurrence only; `occurrence.bom_type` sets it. Older designs need no transformation:
//! an occurrence without it counts as its component. The last release asks for an update instead
//! of dropping it on save.
