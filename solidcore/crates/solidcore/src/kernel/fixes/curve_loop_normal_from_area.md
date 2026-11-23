`SmCurve::ComputeCrvsNormal` uses the sampled polygon's area vector, so a
concave planar loop's normal follows its orientation and its taper drafts
in the requested direction (WR-587, #1776). An open chain closes by its chord.
As the fork's change (tangency-labs/smlib@be45e8d, unchanged to eaa998a), a
failed point evaluation is the call's status and a zero area is
`SM_ERR_INVALID_INPUT`, where upstream's turns give `(0, 0, 1)`.
