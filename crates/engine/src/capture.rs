//! How headless pictures of the model are drawn: a renderer the host installs. The engine
//! describes what a picture shows ([`crate::view::scene`]); drawing it is a GPU job the engine
//! cannot do itself (the GPU crate sits above it), so the application, the CLI and the MCP
//! server install one ([`install`]) that draws with the viewport's own pipelines (or, as a last
//! resort the host chooses, the CPU rasterizer). Every picture names the renderer that drew it.
//! Without a renderer, a picture is an error and a save simply has no thumbnail.

use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use solidcraft_render::{Camera, Scene};

/// A picture: straight RGBA, rows top to bottom, and the renderer that drew it (`gpu`,
/// `gpu-software` or `cpu`).
pub struct Picture {
    pub rgba: Vec<u8>,
    pub renderer: &'static str,
}

/// A picture as PNG bytes, and the renderer that drew it.
pub struct Png {
    pub bytes: Vec<u8>,
    pub renderer: &'static str,
}

/// What a reply says when the CPU rasterizer drew the picture.
pub const CPU_WARNING: &str = "drawn by the CPU rasterizer (no GPU adapter qualified, or SOLIDCRAFT_GPU=cpu): surface analysis colours, highlights and legends are not drawn; `solidcraft-cli gpu` lists the adapters";

/// The warning that goes with a picture drawn by `renderer`, if any.
pub fn warning(renderer: &str) -> Option<&'static str> {
    (renderer == "cpu").then_some(CPU_WARNING)
}

/// Draws `scene` seen by `cam` at `width` × `height` pixels (`width * height * 4` bytes of RGBA),
/// or says why it could not.
pub type Renderer = dyn Fn(&Scene, &Camera, u32, u32) -> Result<Picture, String> + Send + Sync;

static RENDERER: RwLock<Option<Arc<Renderer>>> = RwLock::new(None);

/// Make `renderer` draw this process's headless pictures (replacing any installed before).
pub fn install(renderer: Arc<Renderer>) {
    if let Ok(mut r) = RENDERER.write() {
        *r = Some(renderer);
    }
}

/// Pictures being drawn, and whether the process has stopped drawing them: a picture still on
/// the GPU when the process exits can crash inside the graphics driver as it is torn down.
struct Gate {
    state: Mutex<(usize, bool)>,
    done: Condvar,
}

/// A picture being drawn, counted until it is dropped (also when the renderer panics).
struct Drawing<'a>(&'a Gate);

impl Drop for Drawing<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.0 = s.0.saturating_sub(1);
        }
        self.0.done.notify_all();
    }
}

impl Gate {
    const fn new() -> Gate {
        Gate { state: Mutex::new((0, false)), done: Condvar::new() }
    }

    /// One more picture being drawn; none once closed.
    fn enter(&self) -> Result<Drawing<'_>, String> {
        let mut s = self.state.lock().map_err(|_| "the picture gate is poisoned".to_string())?;
        if s.1 {
            return Err("the app is closing: no more pictures are drawn".into());
        }
        s.0 += 1;
        Ok(Drawing(self))
    }

    /// No new pictures; waits up to `within` for those being drawn. → whether they all finished.
    fn close(&self, within: Duration) -> bool {
        let end = Instant::now() + within;
        let Ok(mut s) = self.state.lock() else { return false };
        s.1 = true;
        while s.0 > 0 {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            match self.done.wait_timeout(s, left) {
                Ok((next, _)) => s = next,
                Err(_) => return false,
            }
        }
        true
    }
}

static GATE: Gate = Gate::new();

/// For a process about to exit: no more pictures are drawn, and those being drawn are waited
/// for, up to `within`. → whether they all finished.
pub fn close(within: Duration) -> bool {
    GATE.close(within)
}

/// The installed renderer, if any.
pub fn installed() -> Option<Arc<Renderer>> {
    RENDERER.read().ok().and_then(|r| r.clone())
}

/// `scene` drawn by the installed renderer.
pub fn rgba(scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Picture, String> {
    rgba_with(installed().as_deref(), scene, cam, width, height)
}

/// `scene` drawn by the installed renderer, as PNG.
pub fn png(scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Png, String> {
    png_with(installed().as_deref(), scene, cam, width, height)
}

/// [`rgba`] with `renderer` instead of the installed one.
pub fn rgba_with(renderer: Option<&Renderer>, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Picture, String> {
    let renderer = renderer.ok_or("no renderer is installed to draw pictures in this process")?;
    let drawing = GATE.enter()?;
    let picture = renderer(scene, cam, width, height)?;
    drop(drawing);
    if picture.rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(format!("the renderer returned {} bytes for {width} × {height} pixels", picture.rgba.len()));
    }
    if let Some(w) = warning(picture.renderer) {
        log::warn!("{w}");
    }
    Ok(picture)
}

/// [`png`] with `renderer` instead of the installed one.
pub fn png_with(renderer: Option<&Renderer>, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Png, String> {
    let Picture { rgba, renderer } = rgba_with(renderer, scene, cam, width, height)?;
    let bytes = solidcraft_render::png(width as usize, height as usize, rgba).ok_or_else(|| "PNG encoding failed".to_string())?;
    Ok(Png { bytes, renderer })
}

/// For tests: a stand-in for the renderer the hosts install, a flat picture whose first byte
/// counts the scene's meshes.
#[cfg(test)]
pub(crate) fn install_test_renderer() {
    install(Arc::new(|scene: &Scene, _: &Camera, w: u32, h: u32| {
        let mut rgba = vec![200; (w * h * 4) as usize];
        if let Some(first) = rgba.first_mut() {
            *first = scene.meshes.len() as u8;
        }
        Ok(Picture { rgba, renderer: "gpu" })
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_renderer_a_picture_is_an_error_not_a_crash() {
        let e = png_with(None, &Scene::default(), &Camera::default(), 16, 16).err().unwrap();
        assert!(e.contains("no renderer"), "{e}");
    }

    #[test]
    fn a_renderer_that_returns_the_wrong_size_is_an_error() {
        let short: &Renderer = &|_, _, _, _| Ok(Picture { rgba: vec![0; 3], renderer: "gpu" });
        assert!(rgba_with(Some(short), &Scene::default(), &Camera::default(), 2, 2).is_err());
        let plain: &Renderer = &|_, _, w, h| Ok(Picture { rgba: vec![255; (w * h * 4) as usize], renderer: "cpu" });
        let png = png_with(Some(plain), &Scene::default(), &Camera::default(), 2, 2).unwrap();
        assert!(png.bytes.starts_with(b"\x89PNG"));
        assert_eq!(png.renderer, "cpu");
        assert_eq!((warning("cpu"), warning("gpu"), warning("gpu-software")), (Some(CPU_WARNING), None, None));
    }

    #[test]
    fn closing_waits_for_pictures_being_drawn_and_refuses_new_ones() {
        let gate = Arc::new(Gate::new());
        let (started, go) = std::sync::mpsc::channel();
        let drawer = Arc::clone(&gate);
        let thread = std::thread::spawn(move || {
            let drawing = drawer.enter();
            let _ = started.send(());
            std::thread::sleep(Duration::from_millis(200));
            drop(drawing);
        });
        go.recv().unwrap();
        let t = Instant::now();
        assert!(gate.close(Duration::from_secs(10)));
        assert!(t.elapsed() >= Duration::from_millis(100), "it waited for the picture");
        assert!(gate.enter().is_err());
        thread.join().unwrap();
        // A picture that does not finish in time: closing gives up and says so.
        let stuck = Gate::new();
        let drawing = stuck.enter().unwrap();
        assert!(!stuck.close(Duration::from_millis(20)));
        drop(drawing);
        assert!(stuck.close(Duration::ZERO));
    }
}
