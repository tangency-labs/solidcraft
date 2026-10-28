# blend.fillet_standard_solver: variable-radius solvers clone a law that upstream shares (2026-10-28)

**Unit:** `blend.fillet_standard_solver_p1` (`SmVariableRadiusFS`), reached through
`blend.fillet_executive_p2` (`SetFilletParameters`, `SmTemporaryRadiusChange`) and the
`blend.fillet_executive_p3` end-to-end `real_solid` cases. A Rust-vs-C++ divergence on the
default path; the C++ is not wrong.

## Inputs

The `radius_law` real_solid option added in this PR: a 20 mm cube, edges `[0, 1]` (two edges
meeting at a corner), `SetFilletParameters(SM_FS_VARIABLE_RADIUS)` with one
`SmBSplineFilletLaw` for both edges, then `DoFillet` and `StitchAndOrient`:

```json
{"real_solid": {"dimensions": [20, 20, 20], "radius": 4,
  "radius_law": {"degree": 2, "radii": [4, 2, 4], "knots": [0, 1], "multiplicities": [3, 3]},
  "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [0, 1]}}
```

and the same with the degree-1 law `radii [4, 2, 4], knots [0, 0.5, 1], multiplicities [2, 1, 2]`.

## Values

| case | first mismatch | count |
|---|---|---|
| degree 2, 4-2-4 | area C++ 2333.8363973199525, Rust 2333.8364081300915 (4.6e-9); a face's tolerance 8.22e-5 vs 4e-5 and its UV domain (`result_geometry[242..]`, up to 1.73 relative) | 98 |
| degree 1, 4-2-4 | area C++ 2341.8231958803185, Rust 2341.8744739523695 (2.2e-5) | - |

Controls that pass: the same edge pair with a linear law (`radius_end`, or a two-point
degree-1 B-spline law) and with a constant r4; every single-edge law case. `--heap-pads
1,2,3,5,8,13` over 5 pair cases (the two above and the three controls: `radius_end`, the
two-point degree-1 law, constant r4): all padded, `address_dependent=0`, `unstable=0`, so this is
not heap order.

## Cause

A local trace of every `SmBSplineFilletLaw::Evaluate` (a logging subclass in the shim;
`eprintln!` in `fillet_executive::p1::bspline_evaluate`) gives 1514 calls on each side,
identical up to call 12. Both sides set up `SmTemporaryRadiusChange` (the law held at its
end radius: value 4, derivatives 0), revert it, and set it up again (call 12 evaluates the
end radius, 4 / 0.2 / 0.02). At call 13 (parameter 20 on [0, 20]) the C++ returns the
constant law (4, 0, 0) and Rust the variable one (4, 0.2, 0.02); from call 14 both are
constant again.

Upstream `SmVariableRadiusFS::m_rLaw` is a non-owning pointer to the caller's law
(`m_rLaw(&rFilletLaw)`, `SmFilletStandardSolver.cpp:1898` and `:2011`; the destructor
deletes nothing). The same-parameters `SetFilletParameters` overload, used by
`SmApiVariableRadiusFillet` and by this fixture, hands every edge's solver the same law
object. So `SmTemporaryRadiusChange::SetUp` (`SmFilletExecutive.cpp:5008`), which swaps
the law curve for a constant one and drops `m_pExtendedLaw`, changes the law the other
edge's solver evaluates too, and the extension `Evaluate` caches in `m_pExtendedLaw` is
shared as well.

The port clones the law into each solver: `SessionObjects::allocate_variable_radius_solver`
(`crates/solidcore/src/blend/fillet_executive/session.rs:3336`, `.cloned()`), and
`VariableRadiusFs::law` is documented as owned. At a corner the second solver therefore
evaluates its own unswapped copy (call 13 above). A linear law is held the same way (start
and end radius set to the end value), so linear multi-edge variable fillets can diverge as
well; the linear pair above happens to give the same values.

## Status

The two corner-pair cases are left out of `fixtures/blend.fillet_executive_p3/bspline_radius_law.json`
in this PR (no known-mismatch entries). Follow-up: #1406, "fix: blend.fillet_standard_solver —
variable-radius solvers share one law, as upstream": the laws live in the fillet session's
law store, each solver holds a handle, and evaluation, `SmTemporaryRadiusChange` set-up and
revert, and the extended-law cache go through it. That PR adds the corner-pair cases at
parity and a public `variable_radius_fillet` corner test with a non-linear law.

Found while building the #430 groundwork (B-spline radius-law `real_solid` cases).
