# heal.heal_data_p3: jack_in_the_box body 52's healed volume is 1.9e-6 off C++ from the seam edges' loose boxes (2026-11-02)

After heal op 22 (`Fix_SplitFaceAtSeams`), `ap203/jack_in_the_box.stp` body 52 (plain recipe) has the same topology on both sides, and the same vertex points, edge lengths and face areas at 1e-9. The healed volume differs: C++ -10094.169290704225, Rust -10094.18834643477.

The two seam edges op 22 inserts are curves on surface with equal intervals and end points, but their loose boxes (`SmCrvOnSurf::CalculateBoundingBox` over the edge interval) differ in max x: C++ 22.331390917705704, Rust 22.331407369104006. That moves the B-rep box's mid by 3.2e-6 in x. The body is not one closed solid (4 regions, 6 shells), so `SmApiBrepComputeVolume`'s integration about the box mid moves by 1.9e-6 relative.

Batches 54m6, 54m85, 54m96, 54n2 and 54n8. Not pursued after the 2026-11-02 owner directive that C++ parity is no longer the gate.
