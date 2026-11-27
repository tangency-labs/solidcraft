//! A sheet metal rule can take its bends' K-factor from a press brake chart (#6794):
//! `unfold_method` is `k_factor` (the default: the rule's `k_factor`, as before), `deduction` or
//! `allowance`, and `unfold_value` is the 90° bend deduction or allowance the chart gives for the
//! rule's thickness and bend radius, a length expression. The rule works the K-factor out from
//! that value (K = (2·BA90/π − R)/T, with BA90 = 2·(R + T) − BD90 for a deduction), so every bend
//! still unfolds from one K-factor and the sheet bodies saved in the design's model are unchanged.
//! Both are saved only for a rule that unfolds by deduction or allowance. Older designs need no
//! step: a rule without them unfolds from its K-factor, as before. The last release warns about
//! the newer format number and opens the design, keeping what it can't read; it unfolds such a
//! rule's bends from the rule's `k_factor` expression instead of the chart value.
