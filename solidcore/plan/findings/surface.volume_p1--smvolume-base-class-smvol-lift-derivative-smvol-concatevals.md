# surface.volume_p1 (`SmVolume` base class, `smvol_Lift*Derivative`, `smvol_ConcatEvals`)

All difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 2886 default cases,
a 20,886-case run clean); the deferred cases need the simple maps of the twist and unbend
volumes. Upstream behaviour worth knowing:
- **Lift typos (ported as written).** The surface and volume forms of
  `smvol_LiftThirdDerivative` use `crDxxz` where the `Dxzz` term belongs in `rDuuv`,
  `rDuvv`, `rDuuw`, `rDuww`, `rDvvw` and `rDvww` (`SmVolume.cpp:9851`, `:10126`),
  `rDuww` is built from `Fuv`/`Fv` instead of `Fuw`/`Fw`, and `rDvww` from `Fvv`/`Fv`
  instead of `Fvw`/`Fw`. Mixed third derivatives of compound volumes (and of `SmSrfInVolume`)
  are therefore wrong upstream; the port keeps the formulas so results match.
- **Unfilled evaluation slots.** `smvol_ConcatEvals` writes only the partials of total order
  `<= n`; the other slots of the `(n+1)^3` array keep the caller's values (a fresh
  `SmVector3d` array: `SM_UNDEF_DOUBLE`). `Evaluate` / `Map` with an orient or next map
  therefore return `SM_UNDEF_DOUBLE` there, while a plain simple map returns zeros. The port
  returns `Vec3::UNDEF` in those slots.
- **Copies drop maps they cannot copy.** The copy constructor copies an owned next map with
  its virtual `Copy` and ignores the status: a plain `SmVolume` (whose `Copy` fails with
  `SM_ERR`) in the chain truncates the copy there. A shared next map (owner flag 0) is kept.
  `AddNextMap(p, 1)` of a plain `SmVolume` fails and leaves no next map. Without a context
  upstream also silently drops the next map (not modelled: the port always copies).
- **Undefined behaviour.** `RefreshInvOrientMap` without an orient map calls
  `MakeMapSimple` through a null pointer; `IsCurveInParamDomain` dereferences a null copy
  when `Copy` fails (unreachable for the base class, whose curve check is `FALSE`);
  `Evaluate` / `Map` only `SM_ASSERT` the inverse orient map. The port returns
  `InvalidInput`; the ffi never sends `Refresh` without an orient map.
- **`Orient` ignores the orient map's status** (`n > GW_MAX_DERIV` leaves the caller's array
  untouched); without an orient map `n > 5` is `SM_ERR_INVALID_INPUT`.
- **Default-constructed `SmUnbendVolume`.** `IsCurveInInSpaceDomain` of an oriented default
  unbend volume crashes upstream (segfault in the unbend volume's curve check); the ffi
  rejects line and curve domain checks on chains with an unbend map.
- **`SmFindPVIntersectENFO`** is declared only in `SmVolume.cpp`; the shim redeclares it
  identically and links the library's `Evaluate` and vtable.
