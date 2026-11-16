#### UB-24. `ST_curxcrr` in place frees an uninitialised curve
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_CrvAdv.cpp:4179`-`4182` (`EXIT: if( curP EQ curQ ) N_FreeCrv( &curA, SP );`); `curA` is assigned only at `:3724`. Function `ST_curxcrr` (def. `:3540`).
- **What happens:** An in-place `NL_G2` call that recurses, or fails before `:3724`, reaches `EXIT` and frees stack garbage.
- **Trigger:** In-place `N_CrvExtendByParamDist` with `NL_G2` and a parameter more than `0.9213` ranges outside the curve.
- **How found:** crash in the C++ reference run (SIGSEGV).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Has no such free and completes the extension.
