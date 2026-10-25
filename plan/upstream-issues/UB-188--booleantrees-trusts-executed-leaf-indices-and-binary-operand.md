#### UB-188. `BooleanTrees` trusts executed leaf indices and binary operand pointers
- **Severity:** Crash / undefined behaviour
- **Area:**

- **Unit:** `boolean.merge_p3` (PR #245).
- **Location:** `source/SMLib/src/SmMerge.cpp:1850-1888`, `SmMerge::BooleanTrees`.
- **Trigger:** an executed nonnegative leaf outside `rBreps` indexes the array unchecked. An executed binary node dereferences its popped primary operand for `GetContext()` and both operands during the zero-tolerance merge constructor; null input slots can arise from repeated leaf indices. A repeated leaf that is never dereferenced by a later binary node is a defined null output and is accepted by the new guard.
- **How found:** source audit while implementing the postfix evaluator and its FFI guard. No crashing program was executed.
- **Confirmed: no.** No C++ reproduction was run; the source contains unchecked indexing and null dereferences.
- **Guard:** the `boolean_merge_p3` FFI and shim simulate reachable leaf consumption and binary stack/null state before building native objects. They preserve safe underflow errors, unknown-operation errors, repeated leaves with no dereference, and unreachable invalid leaves after a definite early error. The Rust evaluator bounds leaf indexing, keeps null slots, and returns `InvalidInput` at an attempted binary null dereference. No exploit fixture is included.
