# Nested reads (all polymorphic readers)

Upstream `SmCurve::ReadFromDB` dispatches to `SmProjectedCurve::ReadFromDB` (and offset,
iso, ... curves), which reads its base curve through `SmCurve::ReadFromDB` again, with no
depth limit: a crafted file of a few bytes per level overflows the stack. The Rust
dispatcher bounds nesting with `DatabaseReader::enter_nested` (limit
`MAX_READ_NESTING` = 32, lowered from 64 for the wasm32 stack); deeper input is `InvalidInput`. Deviation from upstream only
for inputs that would crash it. Found by the commit security review of
`curve.projected_curve`.
