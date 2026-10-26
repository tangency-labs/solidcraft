Updates: UB-137

#### UB-137 (update, 2026-10-26). The Rust port now reaches the branch
- **Rust port:** the original record's "branch is not reached yet" no longer holds. `MergeInterval` (`intersect.curve_class_p2`) is wired through the face p4 seam objects, so on all 37 rejected `topology.face_p1` cases Rust takes the merge-both-directions branch (the U merge makes 2 new faces) and completes without deferring.
- **C++ side:** the shim's fork trap rejects these cases because the forked child dies on SIGSEGV. Backtraces on 3 cases (`vase_rev.smb_1_f5_op9`, `guard_m0_c10_00`, `guard_m3_c11_11`) show `MergeInterval` <- `MergeIntervals` <- `SplitAtSeam` <- `SplitAtSeam` <- `MoveSeam`, via the self-recursion at `SmFace.cpp:18130`. No timeouts or exceptions were seen. The rejections are exact and stay: the crash is inside C++, so no Rust-side change makes these inputs comparable.
