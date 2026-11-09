//! The title bar's regions, for native hit testing.
//!
//! The UI lays the title bar out each frame; the operating system asks the window procedure what
//! is under the pointer (`WM_NCHITTEST` on Windows) at any time, on the window's thread, possibly
//! while the UI is in the middle of a frame. [`CaptionMap`] is how the answer crosses that gap
//! without a lock: the UI [`publish`](CaptionMap::publish)es the regions when they change, the
//! window procedure [`hit`](CaptionMap::hit)s them. It is a sequence lock over plain atomics, so a
//! reader never blocks and never sees half of one layout and half of another.
//!
//! Coordinates are client-area physical pixels (the window's top-left is 0,0), the space
//! `WM_NCHITTEST`'s screen point lands in after subtracting the window's origin.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

/// Most interactive rectangles one layout can publish (tabs, their close buttons, menus, search).
pub const MAX_INTERACTIVE: usize = 32;
/// Most drag rectangles (the bare title bar, the empty part of the document tab strip).
pub const MAX_DRAG: usize = 4;

/// A rectangle in client-area physical pixels, `min` inclusive, `max` exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PxRect {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl PxRect {
    pub const fn new(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.min_x && x < self.max_x && y >= self.min_y && y < self.max_y
    }

    pub fn is_empty(&self) -> bool {
        self.max_x <= self.min_x || self.max_y <= self.min_y
    }
}

/// What the title bar has at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionHit {
    /// Bare title bar: the operating system drags the window from here (native move loop,
    /// double-click to maximise, the system menu, snapping).
    Drag,
    /// A control the UI handles itself (tabs, menus, search, minimise, close).
    Client,
    /// The maximise button: the operating system shows its snap layouts here (Windows 11).
    MaximizeButton,
    /// Not part of the title bar.
    Outside,
}

/// One layout of the title bar, as the UI computes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptionLayout {
    /// The whole title bar strip; points outside it are [`CaptionHit::Outside`].
    pub bar: PxRect,
    /// Where a press drags the window. Checked after `interactive` and the buttons.
    pub drag: Vec<PxRect>,
    /// Controls inside the bar the UI handles (they win over `drag`).
    pub interactive: Vec<PxRect>,
    /// The maximise button, when the window has one.
    pub maximize: Option<PxRect>,
}

const RECTS: usize = 2 + MAX_DRAG + MAX_INTERACTIVE; // bar, maximize, drag.., interactive..
const BAR: usize = 0;
const MAX_BTN: usize = 1;
const DRAG0: usize = 2;
const INTER0: usize = DRAG0 + MAX_DRAG;

/// The published title-bar layout, readable from any thread without blocking.
pub struct CaptionMap {
    /// Even: stable. Odd: a write is in progress. Bumped twice per publish.
    seq: AtomicU64,
    coords: [[AtomicI32; 4]; RECTS],
    drag_len: AtomicU32,
    interactive_len: AtomicU32,
    /// 0: nothing published yet (every point is [`CaptionHit::Outside`]).
    published: AtomicU32,
}

impl Default for CaptionMap {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptionMap {
    pub fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            coords: std::array::from_fn(|_| std::array::from_fn(|_| AtomicI32::new(0))),
            drag_len: AtomicU32::new(0),
            interactive_len: AtomicU32::new(0),
            published: AtomicU32::new(0),
        }
    }

    /// Publish a layout. Only one thread (the UI) publishes. Excess rectangles beyond
    /// [`MAX_DRAG`] / [`MAX_INTERACTIVE`] are dropped; the caller is expected to stay within
    /// them (an interactive rectangle that is dropped would turn into a drag area, so callers
    /// merge small controls into larger rectangles instead of overflowing).
    pub fn publish(&self, layout: &CaptionLayout) {
        let before = self.seq.fetch_add(1, Ordering::AcqRel);
        debug_assert!(before.is_multiple_of(2), "CaptionMap has a single publisher");
        let put = |i: usize, r: PxRect| {
            let c = &self.coords[i];
            c[0].store(r.min_x, Ordering::Relaxed);
            c[1].store(r.min_y, Ordering::Relaxed);
            c[2].store(r.max_x, Ordering::Relaxed);
            c[3].store(r.max_y, Ordering::Relaxed);
        };
        put(BAR, layout.bar);
        put(MAX_BTN, layout.maximize.unwrap_or_default());
        let drag = layout.drag.len().min(MAX_DRAG);
        for (i, r) in layout.drag.iter().take(drag).enumerate() {
            put(DRAG0 + i, *r);
        }
        let inter = layout.interactive.len().min(MAX_INTERACTIVE);
        for (i, r) in layout.interactive.iter().take(inter).enumerate() {
            put(INTER0 + i, *r);
        }
        self.drag_len.store(drag as u32, Ordering::Relaxed);
        self.interactive_len.store(inter as u32, Ordering::Relaxed);
        self.published.store(1, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Forget the layout (the custom title bar is gone, e.g. fullscreen): every point is
    /// [`CaptionHit::Outside`] until the next publish.
    pub fn clear(&self) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.published.store(0, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    fn rect(&self, i: usize) -> PxRect {
        let c = &self.coords[i];
        PxRect {
            min_x: c[0].load(Ordering::Relaxed),
            min_y: c[1].load(Ordering::Relaxed),
            max_x: c[2].load(Ordering::Relaxed),
            max_y: c[3].load(Ordering::Relaxed),
        }
    }

    fn hit_once(&self, x: i32, y: i32) -> CaptionHit {
        if self.published.load(Ordering::Relaxed) == 0 || !self.rect(BAR).contains(x, y) {
            return CaptionHit::Outside;
        }
        let inter = (self.interactive_len.load(Ordering::Relaxed) as usize).min(MAX_INTERACTIVE);
        if (0..inter).any(|i| self.rect(INTER0 + i).contains(x, y)) {
            return CaptionHit::Client;
        }
        let max = self.rect(MAX_BTN);
        if !max.is_empty() && max.contains(x, y) {
            return CaptionHit::MaximizeButton;
        }
        let drag = (self.drag_len.load(Ordering::Relaxed) as usize).min(MAX_DRAG);
        if (0..drag).any(|i| self.rect(DRAG0 + i).contains(x, y)) {
            return CaptionHit::Drag;
        }
        CaptionHit::Client
    }

    /// What the title bar has at client pixel (`x`, `y`). Never blocks; retries while a publish
    /// is in flight and, after a bounded number of tries, answers [`CaptionHit::Client`] (the
    /// conservative answer: the UI sees the press).
    pub fn hit(&self, x: i32, y: i32) -> CaptionHit {
        for _ in 0..64 {
            let a = self.seq.load(Ordering::Acquire);
            if !a.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }
            let hit = self.hit_once(x, y);
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == a {
                return hit;
            }
        }
        CaptionHit::Client
    }

    /// The published bar and maximize button (`None` while nothing is published), read as one
    /// consistent snapshot. The window procedure reports them to the system as the title bar's
    /// geometry (`WM_GETTITLEBARINFOEX` on Windows), which decides where the system's own
    /// maximize-button behaviour (snap layouts) applies.
    pub fn bar_and_maximize(&self) -> Option<(PxRect, Option<PxRect>)> {
        for _ in 0..64 {
            let a = self.seq.load(Ordering::Acquire);
            if !a.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }
            let published = self.published.load(Ordering::Relaxed) != 0;
            let bar = self.rect(BAR);
            let max = self.rect(MAX_BTN);
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == a {
                return published.then_some((bar, (!max.is_empty()).then_some(max)));
            }
        }
        None
    }

    /// Whether a layout is currently published.
    pub fn is_published(&self) -> bool {
        self.published.load(Ordering::Acquire) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> CaptionLayout {
        CaptionLayout {
            bar: PxRect::new(0, 0, 1000, 40),
            drag: vec![PxRect::new(0, 0, 1000, 40)],
            interactive: vec![PxRect::new(10, 5, 120, 35), PxRect::new(862, 0, 908, 40), PxRect::new(954, 0, 1000, 40)],
            maximize: Some(PxRect::new(908, 0, 954, 40)),
        }
    }

    #[test]
    fn nothing_published_is_outside() {
        let m = CaptionMap::new();
        assert_eq!(m.hit(500, 20), CaptionHit::Outside);
        assert!(!m.is_published());
    }

    #[test]
    fn regions_resolve_in_priority_order() {
        let m = CaptionMap::new();
        m.publish(&layout());
        assert_eq!(m.hit(500, 20), CaptionHit::Drag, "bare bar drags");
        assert_eq!(m.hit(50, 20), CaptionHit::Client, "a tab is the UI's");
        assert_eq!(m.hit(930, 20), CaptionHit::MaximizeButton);
        assert_eq!(m.hit(980, 20), CaptionHit::Client, "close is the UI's");
        assert_eq!(m.hit(500, 40), CaptionHit::Outside, "max is exclusive: below the bar");
        assert_eq!(m.hit(-1, 20), CaptionHit::Outside);
    }

    #[test]
    fn the_bar_and_maximize_button_read_back_as_published() {
        let m = CaptionMap::new();
        assert_eq!(m.bar_and_maximize(), None);
        m.publish(&layout());
        assert_eq!(m.bar_and_maximize(), Some((PxRect::new(0, 0, 1000, 40), Some(PxRect::new(908, 0, 954, 40)))));
        m.publish(&CaptionLayout { bar: PxRect::new(0, 0, 100, 30), ..Default::default() });
        assert_eq!(m.bar_and_maximize(), Some((PxRect::new(0, 0, 100, 30), None)), "no maximize button");
        m.clear();
        assert_eq!(m.bar_and_maximize(), None);
    }

    #[test]
    fn bar_without_drag_rect_is_client() {
        let m = CaptionMap::new();
        m.publish(&CaptionLayout { bar: PxRect::new(0, 0, 100, 30), ..Default::default() });
        assert_eq!(m.hit(50, 10), CaptionHit::Client);
    }

    #[test]
    fn clear_and_republish() {
        let m = CaptionMap::new();
        m.publish(&layout());
        m.clear();
        assert_eq!(m.hit(500, 20), CaptionHit::Outside);
        m.publish(&layout());
        assert_eq!(m.hit(500, 20), CaptionHit::Drag);
    }

    #[test]
    fn overflow_is_truncated_not_panicking() {
        let m = CaptionMap::new();
        let mut l = layout();
        l.interactive = (0..100).map(|i| PxRect::new(i * 10, 0, i * 10 + 5, 40)).collect();
        m.publish(&l);
        assert_eq!(m.hit(2, 20), CaptionHit::Client);
    }

    #[test]
    fn concurrent_readers_never_see_torn_layouts() {
        // Two layouts that disagree everywhere: a reader must see one or the other.
        let a = CaptionLayout { bar: PxRect::new(0, 0, 100, 10), drag: vec![PxRect::new(0, 0, 100, 10)], ..Default::default() };
        let b = CaptionLayout { bar: PxRect::new(0, 0, 100, 10), interactive: vec![PxRect::new(0, 0, 100, 10)], ..Default::default() };
        let m = std::sync::Arc::new(CaptionMap::new());
        m.publish(&a);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let (m, stop) = (m.clone(), stop.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let h = m.hit(50, 5);
                        assert!(matches!(h, CaptionHit::Drag | CaptionHit::Client), "{h:?}");
                    }
                })
            })
            .collect();
        for i in 0..20_000 {
            m.publish(if i % 2 == 0 { &b } else { &a });
        }
        stop.store(true, Ordering::Relaxed);
        for r in readers {
            r.join().unwrap();
        }
    }
}
