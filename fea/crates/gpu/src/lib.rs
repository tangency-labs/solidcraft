//! GPU backend dispatch for the SolidCraft FEA scaffold.
//!
//! Phase 1 will add the `wgpu`/WGSL matrix-free PCG kernels behind
//! [`Backend::Gpu`]. Until then, everything runs on [`Backend::Cpu`],
//! which is also the permanent fallback when no GPU is available
//! (CI, headless, unsupported driver). Physics code must not branch
//! on the backend — see `docs/fea/ARCHITECTURE.md`.

/// Compute backend selected at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `wgpu`/WGSL GPU-resident kernels (Phase 1+, not yet implemented).
    Gpu,
    /// CPU fallback (`faer` / `nalgebra-sparse` / `rayon`, Phase 0+).
    Cpu,
}

impl Backend {
    /// Backend available in this scaffold build.
    ///
    /// Always [`Backend::Cpu`] for now: no `wgpu` device is requested
    /// yet, and probing for one would be a Phase 1 change.
    pub fn detect() -> Self {
        Backend::Cpu
    }

    /// Whether the hot loop can stay GPU-resident with this backend.
    pub fn is_gpu_resident(&self) -> bool {
        matches!(self, Backend::Gpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_detects_cpu_backend() {
        assert_eq!(Backend::detect(), Backend::Cpu);
        assert!(!Backend::detect().is_gpu_resident());
    }
}
