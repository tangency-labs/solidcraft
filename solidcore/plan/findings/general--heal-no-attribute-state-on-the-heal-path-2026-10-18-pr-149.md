# heal: no attribute state on the heal path (2026-10-18, PR #149)

The Rust heal path carries no `AttributeStore` (and no `SmTopology` records) at any level: no
`HealData` entry point (`heal_targets`, `heal_tgts`, `api_heal_brep`) takes one. So
`SmSAGObject` attribute propagation during healing is not modelled anywhere in heal. This is a
pre-existing design gap, not one #149 introduces. It became visible when
`TestClosureSplitRegion`'s `SmBrep::Notify` was wired: `heal_data_p3`'s `Fix_BadSheets` step
builds a `brep::p2::RegionNotifyState` (an empty attribute store and default region records,
which no heal code changes) for the split, with a comment saying exactly this. No compared
case observes attributes on that path (it defers at `CalculateBoundingBox` right after the
notifications). Fix: thread an env from `heal_targets` once heal gains an `AttributeStore`.
