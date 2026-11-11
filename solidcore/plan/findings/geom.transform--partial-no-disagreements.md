# geom.transform (partial; no disagreements)

- **`SmTransform::EvaluateBoundingBoxSimple` dereferences a null `pOptParamPseudoBox`.**
  When an output is requested, it calls
  `sThisParamPseudoBox.AreAligned(*pOptParamPseudoBox, alAxisMap)` before checking
  `pOptParamPseudoBox == NULL`, and `AreAligned` reads the other box's basis. So the
  default call (`pOptParamPseudoBox = NULL` plus an output box) crashes. The difftest
  passes a default `SmPseudoBox` for every case that requests an output. Those cases
  are deferred until `geom.pseudo_box` lands. Whoever wires the
  `UNPORTED(geom.pseudo_box)` stub there has to choose what the null case does.
- **The aligned branch of `EvaluateBoundingBoxSimple` scales the parameter intervals
  by `1/|axis|` and shifts them by `origin·axis`.** That is not the image of the box
  under the transform, for example under a pure translation. It is still behind the
  `geom.pseudo_box` stub; whoever ports it should keep it as is.
- **`SmTransform::ReadFromDB` ignores the status of the base-class read**
  (`SmVolume::ReadFromDB`) and always returns `SM_SUCCESS` once the 16 matrix values
  are read.
