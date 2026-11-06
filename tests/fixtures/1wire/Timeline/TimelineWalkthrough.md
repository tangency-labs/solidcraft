# Timeline Walkthrough: 1WireRGBWController

Detailed description of each timeline step, with parameters and a corresponding numbered screenshot.

## Step 01: Base

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 16
  - Profiles: 9
- **Screenshot:** `01_Base.png`

## Step 02: Extrude1

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: New Body
  - Distance: ExternalHeight
  - Symmetric: False
- **Screenshot:** `02_Extrude1.png`

## Step 03: Fillet1

- **Type:** `adsk::fusion::FilletFeature`
- **Health:** 0
- **Description:** Rounds one or more edges.
- **Parameters:**
  - Set 1: radius=caseFilletRadius, edges=12
- **Screenshot:** `03_Fillet1.png`

## Step 04: Shell1

- **Type:** `adsk::fusion::ShellFeature`
- **Health:** 0
- **Description:** Timeline step.
- **Screenshot:** `04_Shell1.png`

## Step 05: Body Split

- **Type:** `adsk::fusion::ConstructionPlane`
- **Health:** 0
- **Description:** Reference plane used as a construction aid for later sketches/features.
- **Screenshot:** `05_Body Split.png`

## Step 06: Split1

- **Type:** `adsk::fusion::SplitBodyFeature`
- **Health:** 0
- **Description:** Splits a body into multiple bodies using a splitting tool.
- **Screenshot:** `06_Split1.png`

## Step 07: Extrude2

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: studHeight
  - Symmetric: False
- **Screenshot:** `07_Extrude2.png`

## Step 08: Extrude3

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: studHeight
  - Symmetric: False
- **Screenshot:** `08_Extrude3.png`

## Step 09: Extrude4

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: studHeight
  - Symmetric: False
- **Screenshot:** `09_Extrude4.png`

## Step 10: Extrude5

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: studHeight
  - Symmetric: False
- **Screenshot:** `10_Extrude5.png`

## Step 11: Extrude7

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: standoffHeight
  - Symmetric: False
- **Screenshot:** `11_Extrude7.png`

## Step 12: Extrude8

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: standoffHeight
  - Symmetric: False
- **Screenshot:** `12_Extrude8.png`

## Step 13: Extrude9

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: standoffHeight
  - Symmetric: False
- **Screenshot:** `13_Extrude9.png`

## Step 14: Extrude10

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: standoffHeight
  - Symmetric: False
- **Screenshot:** `14_Extrude10.png`

## Step 15: Extrude6

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: 0.001 mm
  - Symmetric: False
- **Screenshot:** `15_Extrude6.png`

## Step 16: Sketch3

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 28
  - Profiles: 15
- **Screenshot:** `16_Sketch3.png`

## Step 17: Extrude11

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -4.00 mm
  - Symmetric: False
- **Screenshot:** `17_Extrude11.png`

## Step 18: Extrude12

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -7.00 mm
  - Symmetric: False
- **Screenshot:** `18_Extrude12.png`

## Step 19: Sketch4

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 20
  - Profiles: 11
- **Screenshot:** `19_Sketch4.png`

## Step 20: Extrude13

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -4.00 mm
  - Symmetric: False
- **Screenshot:** `20_Extrude13.png`

## Step 21: Extrude14

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -6.00 mm
  - Symmetric: False
- **Screenshot:** `21_Extrude14.png`

## Step 22: DJ Jack

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 19
  - Profiles: 7
- **Screenshot:** `22_DJ Jack.png`

## Step 23: Extrude15

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -2.00 mm
  - Symmetric: False
- **Screenshot:** `23_Extrude15.png`

## Step 24: Sketch6

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 0
  - Profiles: 0
- **Screenshot:** `24_Sketch6.png`

## Step 25: Extrude16

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `25_Extrude16.png`

## Step 26: Extrude17

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -1.60 mm
  - Symmetric: False
- **Screenshot:** `26_Extrude17.png`

## Step 27: Extrude18

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -1.60 mm
  - Symmetric: False
- **Screenshot:** `27_Extrude18.png`

## Step 28: Extrude19

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -1.60 mm
  - Symmetric: False
- **Screenshot:** `28_Extrude19.png`

## Step 29: Extrude20

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `29_Extrude20.png`

## Step 30: Extrude21

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: 5 mm
  - Symmetric: False
- **Screenshot:** `30_Extrude21.png`

## Step 31: Extrude22

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: 5 mm
  - Symmetric: False
- **Screenshot:** `31_Extrude22.png`

## Step 32: Extrude23

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `32_Extrude23.png`

## Step 33: Extrude24

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `33_Extrude24.png`

## Step 34: Chamfer1

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=2.5 mm, edges=2
- **Screenshot:** `34_Chamfer1.png`

## Step 35: Extrude25

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Extent: SymmetricExtentDefinition
- **Screenshot:** `35_Extrude25.png`

## Step 36: Fillet2

- **Type:** `adsk::fusion::FilletFeature`
- **Health:** 0
- **Description:** Rounds one or more edges.
- **Parameters:**
  - Set 1: radius=1 mm, edges=1
- **Screenshot:** `36_Fillet2.png`

## Step 37: Sketch8

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 8
  - Profiles: 4
- **Screenshot:** `37_Sketch8.png`

## Step 38: Extrude26

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: 5 mm
  - Symmetric: False
- **Screenshot:** `38_Extrude26.png`

## Step 39: Chamfer2

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=2.5 mm, edges=2
- **Screenshot:** `39_Chamfer2.png`

## Step 40: Fillet3

- **Type:** `adsk::fusion::FilletFeature`
- **Health:** 0
- **Description:** Rounds one or more edges.
- **Parameters:**
  - Set 1: radius=1 mm, edges=1
- **Screenshot:** `40_Fillet3.png`

## Step 41: Extrude27

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `41_Extrude27.png`

## Step 42: Extrude28

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Extent: SymmetricExtentDefinition
- **Screenshot:** `42_Extrude28.png`

## Step 43: Fillet4

- **Type:** `adsk::fusion::FilletFeature`
- **Health:** 0
- **Description:** Rounds one or more edges.
- **Parameters:**
  - Set 1: radius=1 mm, edges=16
- **Screenshot:** `43_Fillet4.png`

## Step 44: Split2

- **Type:** `adsk::fusion::SplitBodyFeature`
- **Health:** 0
- **Description:** Splits a body into multiple bodies using a splitting tool.
- **Screenshot:** `44_Split2.png`

## Step 45: InnerLip

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 54
  - Profiles: 8
- **Screenshot:** `45_InnerLip.png`

## Step 46: Extrude37

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Extent: SymmetricExtentDefinition
- **Screenshot:** `46_Extrude37.png`

## Step 47: ClipesLeft

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 43
  - Profiles: 7
- **Screenshot:** `47_ClipesLeft.png`

## Step 48: ClipsFront

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 27
  - Profiles: 4
- **Screenshot:** `48_ClipsFront.png`

## Step 49: ClipsRight

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 36
  - Profiles: 5
- **Screenshot:** `49_ClipsRight.png`

## Step 50: ClipsBack

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 26
  - Profiles: 3
- **Screenshot:** `50_ClipsBack.png`

## Step 51: Extrude38

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: wallThickness / 3
  - Symmetric: False
- **Screenshot:** `51_Extrude38.png`

## Step 52: Chamfer12

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=clipHeight / 2, edges=6
- **Screenshot:** `52_Chamfer12.png`

## Step 53: Extrude39

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: wallThickness / 3
  - Symmetric: False
- **Screenshot:** `53_Extrude39.png`

## Step 54: Chamfer13

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=clipHeight / 2, edges=8
- **Screenshot:** `54_Chamfer13.png`

## Step 55: Extrude40

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: wallThickness / 3
  - Symmetric: False
- **Screenshot:** `55_Extrude40.png`

## Step 56: Chamfer14

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=clipHeight / 2, edges=4
- **Screenshot:** `56_Chamfer14.png`

## Step 57: Extrude41

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Distance: wallThickness / 3
  - Symmetric: False
- **Screenshot:** `57_Extrude41.png`

## Step 58: Chamfer15

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=clipHeight / 2, edges=6
- **Screenshot:** `58_Chamfer15.png`

## Step 59: Combine1

- **Type:** `adsk::fusion::CombineFeature`
- **Health:** 0
- **Description:** Performs a boolean operation between bodies.
- **Parameters:**
  - Operation: Cut (removes material)
- **Screenshot:** `59_Combine1.png`

## Step 60: Chamfer16

- **Type:** `adsk::fusion::ChamferFeature`
- **Health:** 0
- **Description:** Bevels one or more edges.
- **Parameters:**
  - Set 1: distance=wallThickness, edges=15
- **Screenshot:** `60_Chamfer16.png`

## Step 61: LegClampProfile

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 14
  - Profiles: 2
- **Screenshot:** `61_LegClampProfile.png`

## Step 62: LegClampRight

- **Type:** `adsk::fusion::ConstructionPlane`
- **Health:** 0
- **Description:** Reference plane used as a construction aid for later sketches/features.
- **Screenshot:** `62_LegClampRight.png`

## Step 63: LegClampExtrusion

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 4
  - Profiles: 0
- **Screenshot:** `63_LegClampExtrusion.png`

## Step 64: Sweep1

- **Type:** `adsk::fusion::SweepFeature`
- **Health:** 0
- **Description:** Sweeps a profile along a path to create geometry.
- **Screenshot:** `64_Sweep1.png`

## Step 65: Floor

- **Type:** `adsk::fusion::ConstructionPlane`
- **Health:** 0
- **Description:** Reference plane used as a construction aid for later sketches/features.
- **Screenshot:** `65_Floor.png`

## Step 66: Split3

- **Type:** `adsk::fusion::SplitBodyFeature`
- **Health:** 0
- **Description:** Splits a body into multiple bodies using a splitting tool.
- **Screenshot:** `66_Split3.png`

## Step 67: DeleteFace1

- **Type:** `adsk::fusion::DeleteFaceFeature`
- **Health:** 0
- **Description:** Deletes a face; adjacent faces extend to close the gap.
- **Screenshot:** `67_DeleteFace1.png`

## Step 68: RemoveBody-Top (1)

- **Type:** `adsk::fusion::RemoveFeature`
- **Health:** 0
- **Description:** Removes a body or component from the design.
- **Screenshot:** `68_RemoveBody-Top (1).png`

## Step 69: Fillet6

- **Type:** `adsk::fusion::FilletFeature`
- **Health:** 0
- **Description:** Rounds one or more edges.
- **Parameters:**
  - Set 1: radius=1 mm, edges=4
- **Screenshot:** `69_Fillet6.png`

## Step 70: LegClampHole

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 12
  - Profiles: 6
- **Screenshot:** `70_LegClampHole.png`

## Step 71: Extrude44

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Join (adds material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `71_Extrude44.png`

## Step 72: Extrude45

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Distance: -3 mm
  - Symmetric: False
- **Screenshot:** `72_Extrude45.png`

## Step 73: Extrude46

- **Type:** `adsk::fusion::ExtrudeFeature`
- **Health:** 0
- **Description:** Extrudes a profile into solid/surface geometry.
- **Parameters:**
  - Operation: Cut (removes material)
  - Extent: ToEntityExtentDefinition
- **Screenshot:** `73_Extrude46.png`

## Step 74: Sketch27

- **Type:** `adsk::fusion::Sketch`
- **Health:** 0
- **Description:** Defines 2D profile geometry used by later features.
- **Parameters:**
  - Curves: 5
  - Profiles: 1
- **Screenshot:** `74_Sketch27.png`

## Step 75: LeftRightMirror

- **Type:** `adsk::fusion::ConstructionPlane`
- **Health:** 0
- **Description:** Reference plane used as a construction aid for later sketches/features.
- **Screenshot:** `75_LeftRightMirror.png`

## Step 76: Mirror1

- **Type:** `adsk::fusion::MirrorFeature`
- **Health:** 0
- **Description:** Mirrors input features/bodies across a plane.
- **Screenshot:** `76_Mirror1.png`
