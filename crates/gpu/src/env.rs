//! SolidCraft's environment for its GPU work. The app reads these variables and passes what they
//! say to the viewport's GPU code, which reads no environment of an app's:
//!
//! - `SOLIDCRAFT_GPU` (`auto`, `hardware`, `software`, `cpu`): the [`Policy`] that picks the
//!   adapter headless pictures, the CLI and the path tracer draw with; `auto` when unset.
//! - `SOLIDCRAFT_CPU_TESS`: set, bodies are built on the CPU on any device (the `force_cpu` of
//!   [`solidcraft_viewport_gpu::view::tessellation`]).
//!
//! `WGPU_BACKEND` and `WGPU_DX12_COMPILER` are wgpu's own and are read where wgpu reads them.

use solidcraft_viewport_gpu::capture::Headless;
use solidcraft_viewport_gpu::select::Policy;
use solidcraft_viewport_gpu::{GpuError, TessGpu};

/// The variable naming the adapter policy.
pub const POLICY: &str = "SOLIDCRAFT_GPU";
/// The variable that builds bodies on the CPU on any device.
pub const CPU_TESS: &str = "SOLIDCRAFT_CPU_TESS";

/// What to tell someone whose machine has no adapter under the policy.
const NO_ADAPTER_HINT: &str = "set SOLIDCRAFT_GPU to choose another policy; `solidcraft-cli gpu` lists the adapters this machine offers";

/// The policy `value` (the variable's value, `None` when unset) names: [`Policy::Auto`] when unset,
/// an error naming the variable when it names none.
pub fn policy_from(value: Option<&str>) -> Result<Policy, String> {
    match value {
        Some(v) => Policy::parse(v).ok_or_else(|| format!("{POLICY} must be auto, hardware, software or cpu, not `{v}`")),
        None => Ok(Policy::Auto),
    }
}

/// The policy `SOLIDCRAFT_GPU` names ([`policy_from`]).
pub fn policy() -> Result<Policy, String> {
    policy_from(std::env::var(POLICY).ok().as_deref())
}

/// Whether `value` (the variable's value, `None` when unset) asks for bodies built on the CPU:
/// any value does.
pub fn force_cpu_from(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some()
}

/// Whether `SOLIDCRAFT_CPU_TESS` asks for bodies built on the CPU ([`force_cpu_from`]).
pub fn force_cpu_tessellation() -> bool {
    force_cpu_from(std::env::var_os(CPU_TESS).as_deref())
}

/// `e` as SolidCraft reports it: with how to choose another policy when no adapter qualified.
pub fn explain(e: &GpuError) -> String {
    match e {
        GpuError::NoAdapter(_) => format!("{e}; {NO_ADAPTER_HINT}"),
        _ => e.to_string(),
    }
}

/// What draws this process's headless pictures under `SOLIDCRAFT_GPU` ([`solidcraft_viewport_gpu::capture::headless`]).
pub fn headless() -> Result<Headless, String> {
    solidcraft_viewport_gpu::capture::headless(policy()?).map_err(explain)
}

/// A picture of `scene` drawn as [`headless`] draws it ([`Headless::rgba`]).
pub fn headless_rgba(
    scene: &solidcraft_viewport::Scene,
    cam: &solidcraft_viewport::Camera,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, &'static str), String> {
    headless()?.rgba(scene, cam, width, height)
}

/// A tessellation device of its own under `SOLIDCRAFT_GPU` ([`TessGpu::headless`]).
pub fn headless_tessellation() -> Result<TessGpu, String> {
    TessGpu::headless(policy()?).map_err(|e| explain(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's variable still picks the policy, and still refuses a value that names none.
    #[test]
    fn the_app_reads_its_policy_variable() {
        assert_eq!(policy_from(None), Ok(Policy::Auto));
        for (v, p) in [("auto", Policy::Auto), ("Hardware", Policy::Hardware), (" software ", Policy::Software), ("cpu", Policy::Cpu)] {
            assert_eq!(policy_from(Some(v)), Ok(p), "{v}");
        }
        let e = policy_from(Some("fast")).unwrap_err();
        assert!(e.contains("SOLIDCRAFT_GPU") && e.contains("`fast`"), "{e}");
    }

    /// Any value of the app's CPU-tessellation variable builds bodies on the CPU; unset, none.
    #[test]
    fn the_app_reads_its_cpu_tessellation_variable() {
        assert!(!force_cpu_from(None));
        assert!(force_cpu_from(Some(std::ffi::OsStr::new("1"))));
        assert!(force_cpu_from(Some(std::ffi::OsStr::new(""))));
    }

    /// The app adds how to choose another adapter to a missing adapter, and nothing to others.
    #[test]
    fn a_missing_adapter_is_explained_with_the_apps_variable() {
        let e = explain(&GpuError::NoAdapter("hardware"));
        assert!(
            e.starts_with(&GpuError::NoAdapter("hardware").to_string()) && e.contains("SOLIDCRAFT_GPU") && e.contains("solidcraft-cli gpu"),
            "{e}"
        );
        assert_eq!(explain(&GpuError::DeviceLost), GpuError::DeviceLost.to_string());
    }

    /// The shared viewport crates read no variable of SolidCraft's and name neither SolidCraft nor
    /// its CLI, in their code or their tests: what the app wants, it passes in.
    #[test]
    fn the_viewport_crate_reads_no_variable_of_the_apps() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut dirs = ["viewport", "viewport-gpu", "viewport-egui"].map(|c| crates.join(c)).to_vec();
        let mut scanned = 0;
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if !path.extension().is_some_and(|e| e == "rs" || e == "wgsl" || e == "toml" || e == "md") {
                    continue;
                }
                scanned += 1;
                for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    let lower = line
                        .to_ascii_lowercase()
                        .replace("solidcraft_color", "")
                        .replace("solidcraft-color", "")
                        .replace("solidcraft_viewport", "")
                        .replace("solidcraft-viewport", "");
                    assert!(!lower.contains("solidcraft"), "{}:{}: {line}", path.display(), n + 1);
                }
            }
        }
        assert!(scanned > 20, "{scanned} files");
    }
}
