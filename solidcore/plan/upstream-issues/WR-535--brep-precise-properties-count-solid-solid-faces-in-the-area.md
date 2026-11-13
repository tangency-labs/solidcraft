#### WR-535. `SmBrep::ComputePreciseProperties` counts a face between two solid regions in the area, once per side
- **Severity:** Wrong result (area; and under area behaviour 2, the void side's volume)
- **Area:** Mass properties
- **Location:** `source/SMLib/src/SmBrep.cpp`, `SmBrep::ComputePreciseProperties` (def. `:1627`), the volume-separating branch: a faceuse's area, volume, mass and all moments are added whenever `bDoFUArea || bDoFUVolume`.
- **What happens:** A volume-separating faceuse adds both its area and its volume when either behaviour selects it. A face between two solid regions bounds both volumes, so it is selected for volume on each side and its area is added once per side. This goes against `SmMassPropertyBehavior`'s default (`SmTopoTypes.h`: area "only for those faces which lie between a solid region and a void region"). Under area behaviour 2, the void side also adds its volume.
- **Trigger:** a merge of two overlapping 10 mm cubes (three solid regions) measures 1200 mm2 instead of 800.
- **How found:** SolidCraft change; the fork's fix is tangency-labs/smlib@d2d9586 ("a brep's area counts the faces between solid and void only"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port's `brep_properties` follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.brep_area_between_solid_and_void`, `brep_properties` changes what a volume-separating faceuse adds (`add_selected`): its volume, mass and volume moments only when it is selected for volume, and its area and area moments only when it is selected for area. `merged_cubes_area_counts_solid_void_faces_only` in `mass/integrator/properties.rs` checks area 800 and volume 1500 for the merged cubes.
