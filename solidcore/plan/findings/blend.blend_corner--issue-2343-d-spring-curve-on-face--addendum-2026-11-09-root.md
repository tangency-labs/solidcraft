# Addendum: a Setback's spring curve leaving its trimmed face, the named size as the limit

Addendum to `blend.blend_corner--issue-2343-d-spring-curve-on-face.md`.

**The named size.** The refusal no longer names the last point of the grid of thousandths. After
the bisection on that grid, `setback::springs_refusal` halves the last grid cell (`REFINE`, at most
30 times) until it is shorter than the corner's tolerance along every edge. Each halving keeps the
end the check found clear, so the size named is still one the check found clear.

**Measured** (`issue_2343_curved_setback::bored_d_prism_spring_curve_leaving_the_top_names_the_setback_that_keeps_it`,
`--profile goldens`, values printed by a temporary `eprintln!`):

| | before (the record) | now |
|---|---|---|
| named for asked 8.5 | 7.803 (0.918 x 8.5) | 7.806221 (0.918379 x 8.5) |
| below the limit 7.806805 | 3.8e-3 | 5.8e-4 |
| spring's least rho - 6.9 | 3.3e-4 | 5.1e-5 |

The corner's tolerance is 4e-5 and the longest asked setback 8.5, so the cell of 0.0085 is halved
8 times (to 3.3e-5): the refusal asks the check 19 times in all. What remains below the limit is
the check's gap: a spring curve counts as clear only beyond the tolerances of its face's other
edges. The named size builds, graded as the other
curved Setbacks. In the record's figure the spring curve labelled 7.803 stands for the one now
named; at the figure's scale the two do not differ.

**The rails' directions are tested.** The one-off comparison in the record is replaced by committed
tests. `setback::tests::a_rails_direction_in_closed_form_is_its_derivative` checks `rail_direction`
against the rail's derivative by central differences on a convex cylinder, a cone, a sphere and a
concave corner beside a bore. The other face is a tilted plane, so `normal_turn` turns the
direction there. In the corner tests on a cylinder, a concave cylinder, a cone and a sphere, the
spring curves the check made (`SetbackCorner::springs`) are the fill's spring edges within 1e-11
(measured at most 9.2e-13). Their rail-end tangents agree within 1e-9 rad (at most 5.5e-14).
