Updates: WR-545

#### WR-545 (update, 2026-11-24). The exact-edge pass keeps only its find-or-check failures as marched edges

`make_edges_on_analytics_exact` no longer catches every error of an edge's exact-curve search. An edge keeps
its marched curve only where its exact curve cannot be found or checked: a grown surface copy is refused, the
intersection or the edge's trimmed curve fails, no line or conic runs through its three points, or the piece
fails its validity check (`exact_edge`). Every other error propagates with its cause and drops the shell: a
missing callee, a cancellation, a failed topology read or evaluation, or a failure changing the body.
`exact_piece`'s `NotConverged` is its turn-count guard. Its parameters come from the conic's own inversion, so
only a broken conic reaches it, and that error propagates too. This supersedes the 2026-11-23 addendum's sentence
that any error but a missing callee or a cancellation keeps the marched curve. The `offset.*` and
`blend.fillet_executive_p3` goldens are unchanged (changed=0).
