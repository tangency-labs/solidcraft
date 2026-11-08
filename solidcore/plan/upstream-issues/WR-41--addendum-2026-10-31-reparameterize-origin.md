Updates: WR-41

#### WR-41 addendum (2026-10-31): opt-in origin shift for plane reparameterization

- **Severity:** Wrong result (release-only)
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmPlane.cpp:3080-3116` `SmPlane::Reparameterize`.
- **Reference issue:** `plan/upstream-issues/WR-41--smplane-reparameterize-does-not-move-the-origin-in-release.md`.
- **What happens:** In upstream release builds, `sOriginMax` was computed but only referenced under `#ifdef SM_DEBUG_CODE`. `m_vPosition.SetOrigin(sOriginMax)` was omitted before setting the updated domain and UV scales (`:3116-3117`). As a result, the analytic plane's origin remained fixed in global coordinates while its underlying NURB representation was reparameterized, causing the analytic plane and the NURB representation to spatially diverge when evaluating points over any reparameterized domain that does not start at the same corner.
- **Port fix:** An opt-in fix `Kernel::fixes.plane_reparameterize_moves_origin` (`crates/solidcore/src/kernel.rs`) restores origin adjustment. Off by default for 100% bit-level parity with pinned SMLib v1.0.1.
- **Implementation:**
  - `Axis2Placement::set_origin`: updates `self.origin = origin`.
  - `Plane::reparameterize_opt_in`: when `move_origin` is true, computes `point_max` and `origin_max` per upstream formulas:
    ```rust
    let scale_x = (self.uv_scale.x * self.anal_uv_domain.x_length()) / trim_domain.x_length();
    let scale_y = (self.uv_scale.y * self.anal_uv_domain.y_length()) / trim_domain.y_length();
    let point_max = self.position.origin()
        + self.position.x_axis() * (self.uv_scale.x * self.anal_uv_domain.u_max())
        + self.position.y_axis() * (self.uv_scale.y * self.anal_uv_domain.v_max());
    let origin_max = point_max
        - self.position.x_axis() * (scale_x * trim_domain.u_max())
        - self.position.y_axis() * (scale_y * trim_domain.v_max());
    self.position.set_origin(origin_max);
    ```
  - `Plane::reparameterize_with_kernel`: passes `k.fixes.plane_reparameterize_moves_origin` to `reparameterize_opt_in`.
  - `Surface::reparameterize`: delegates `SurfaceKind::Plane(s) => s.reparameterize_with_kernel(k, new_domain)`.
- **Verification:**
  - Pinned parity: `Plane::reparameterize` keeps release origin behavior; `surface.plane_p1` difftest passes 100% parity with zero regressions.
  - Opt-in unit tests: `crates/solidcore/src/surface/plane/p1.rs::tests::wr41_*` verify that enabling `plane_reparameterize_moves_origin` maintains complete consistency (< 1e-12 distance) between analytic plane evaluations and base NURB evaluations across translated and rescaled UV domains.
