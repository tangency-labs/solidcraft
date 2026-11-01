//! Floating controls reserve drawing space without changing rendering or picking projection.
use egui::{Rect, pos2, vec2};
use solidcraft_engine::geom::{Aabb3, Vec3};
use solidcraft_engine::render::Camera;

#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub viewport: Rect,
    pub drawable: Rect,
    /// The view without the panels (Browser, Assistant, workspace panels), but still counting
    /// the floating controls as drawn over it: where a fit puts the middle of the model. The
    /// compass and the navigation bar take room out of `drawable` without moving this.
    pub middle: Rect,
    pub content: Rect,
    pub axis_view: Rect,
    pub navigation: Rect,
    /// OK and Cancel in the view's corner (#6709), or nothing while they are not shown.
    pub confirm: Rect,
}

/// How far the OK and Cancel pair sits in from the content's top right corner, and the space
/// between it and the axis view moved down below it.
const CORNER_INSET: f32 = 8.0;
const CORNER_GAP: f32 = 4.0;

/// Enumerate rectangles whose edges meet the viewport or a bounded number of overlays.
pub(crate) fn free_rect(view: Rect, obstacles: &[Rect]) -> Rect {
    if !view.is_finite() || !view.is_positive() {
        return Rect::NOTHING;
    }
    let mut xs = vec![view.left(), view.right()];
    let mut ys = vec![view.top(), view.bottom()];
    let obstacles: Vec<_> = obstacles.iter().take(16).map(|r| r.intersect(view)).filter(|r| r.is_positive() && r.is_finite()).collect();
    for r in &obstacles {
        xs.extend([r.left(), r.right()]);
        ys.extend([r.top(), r.bottom()]);
    }
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);
    let mut best = Rect::from_min_size(view.min, vec2(0.0, 0.0));
    for (i, &left) in xs.iter().enumerate() {
        for &right in xs.iter().skip(i + 1) {
            for (j, &top) in ys.iter().enumerate() {
                for &bottom in ys.iter().skip(j + 1) {
                    let r = Rect::from_min_max(pos2(left, top), pos2(right, bottom));
                    if r.area() > best.area().max(0.0) && !obstacles.iter().any(|o| r.intersect(*o).is_positive()) {
                        best = r;
                    }
                }
            }
        }
    }
    best
}

impl Layout {
    /// `corner` is the size of the OK and Cancel pair (#6709; zero while it is not shown): it is
    /// held in the top right corner of the content, and the axis view moves down below it.
    /// `navigation` is the measured rectangle of the navigation bar (nothing while it is hidden).
    pub fn with_content(
        viewport: Rect,
        content: Rect,
        docks: &[Rect],
        scale: f32,
        style: crate::prefs::NavigatorStyle,
        corner: egui::Vec2,
        navigation: Rect,
    ) -> Self {
        Self::build(viewport, content, docks, scale, style, corner, navigation, None)
    }

    /// Render fits stay between the docked panels, including beneath a short Browser card.
    pub fn product(
        viewport: Rect,
        content: Rect,
        docks: &[Rect],
        scale: f32,
        style: crate::prefs::NavigatorStyle,
        corner: egui::Vec2,
        navigation: Rect,
    ) -> Self {
        Self::build(viewport, content, docks, scale, style, corner, navigation, Some(crate::frame::Side::Right))
    }

    /// Render compass at the free canvas edge beside a left-hand Inspector.
    pub fn product_left(
        viewport: Rect,
        content: Rect,
        docks: &[Rect],
        scale: f32,
        style: crate::prefs::NavigatorStyle,
        corner: egui::Vec2,
        navigation: Rect,
    ) -> Self {
        Self::build(viewport, content, docks, scale, style, corner, navigation, Some(crate::frame::Side::Left))
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        viewport: Rect,
        content: Rect,
        docks: &[Rect],
        scale: f32,
        style: crate::prefs::NavigatorStyle,
        corner: egui::Vec2,
        navigation: Rect,
        inspector_side: Option<crate::frame::Side>,
    ) -> Self {
        let fit_in_content = inspector_side.is_some();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let mut obstacles: Vec<_> = docks.iter().take(14).copied().filter(|r| r.is_finite()).map(|r| r.expand(4.0)).collect();
        // Navigation is anchored to the viewport, never to transient dialog space.
        let available = content;
        let confirm = if corner.is_finite()
            && corner.x > 0.0
            && corner.y > 0.0
            && available.width() >= corner.x + CORNER_INSET
            && available.height() >= corner.y + CORNER_INSET
        {
            Rect::from_min_size(pos2(available.right() - CORNER_INSET - corner.x, available.top() + CORNER_INSET), corner)
        } else {
            Rect::NOTHING
        };
        // A navigation bar on the view's top edge (#8183) keeps the pair and the axis view below it.
        let bar_on_top = navigation.is_positive() && navigation.center().y < available.center().y;
        let below_bar = |rect: Rect| {
            if bar_on_top && rect.intersect(navigation).is_positive() {
                rect.translate(vec2(0.0, navigation.bottom() + CORNER_GAP - rect.top()))
            } else {
                rect
            }
        };
        let confirm = below_bar(confirm);
        // The axis view starts below the pair while it is shown.
        let top = if confirm.is_positive() { confirm.bottom() + CORNER_GAP } else { available.top() };
        let cube_size = if fit_in_content { 110.0 } else { super::viewport::axis_view_size(style) };
        let axis_left = |size| {
            if inspector_side == Some(crate::frame::Side::Left) { available.left() } else { available.right() - size }
        };
        let top = below_bar(Rect::from_min_size(pos2(axis_left(cube_size), top), vec2(cube_size, cube_size))).top();
        let mut axis_view = if available.width() >= cube_size && available.bottom() - top >= cube_size {
            Rect::from_min_size(pos2(axis_left(cube_size), top), vec2(cube_size, cube_size))
        } else {
            Rect::NOTHING
        };
        if fit_in_content {
            // A focus-exit or folded-dock chip may occupy the compass's corner.
            for _ in 0..obstacles.len() {
                let bottom = obstacles.iter().filter(|dock| axis_view.intersect(**dock).is_positive()).map(Rect::bottom).max_by(f32::total_cmp);
                let Some(bottom) = bottom else { break };
                axis_view = if available.bottom() - bottom >= cube_size {
                    Rect::from_min_size(pos2(axis_view.left(), bottom), vec2(cube_size, cube_size))
                } else {
                    Rect::NOTHING
                };
            }
        }
        // On tiny viewports keep the bottom controls usable rather than overlapping them.
        // This decision depends only on viewport size, never on an overlay.
        if axis_view.intersect(navigation).is_positive() {
            let compact = (navigation.top() - top - 4.0).min(cube_size);
            axis_view = if style == crate::prefs::NavigatorStyle::Cube && compact >= 96.0 {
                Rect::from_min_size(pos2(axis_left(compact), top), vec2(compact, compact))
            } else {
                Rect::NOTHING
            };
        }
        let within = if fit_in_content { content } else { viewport };
        // Only the panels decide where the middle of the view is; the controls below are drawn
        // over the view and would otherwise push the model into a corner of it.
        let middle = free_rect(within, &obstacles);
        let middle = if middle.is_positive() && middle.is_finite() { middle } else { within };
        for control in [axis_view, navigation, confirm] {
            if control.is_positive() {
                obstacles.push(control);
            }
        }
        let drawable = free_rect(within, &obstacles).shrink(16.0 / scale);
        Self { viewport, drawable, middle, content, axis_view, navigation, confirm }
    }
}

/// Product cameras keep the authored lens while framing the free canvas.
pub(crate) fn fit_product_bounds(cam: Camera, bounds: &Aabb3, layout: &Layout) -> Option<Camera> {
    fit_bounds(cam, bounds, layout, cam.fov)
}

pub(crate) fn fit_bounds(cam: Camera, bounds: &Aabb3, layout: &Layout, fov: f64) -> Option<Camera> {
    fit_bounds_sized(cam, bounds, layout, fov, 1.0)
}

/// [`fit_bounds`] with the model filling `share` (0.1 to 1) of the room between the panels, the
/// compass and the navigation bar instead of all of it: the Navigation preference "Home and Fit
/// size" (#10091). The model stays clear of what covers the view at every size.
pub(crate) fn fit_bounds_sized(mut cam: Camera, bounds: &Aabb3, layout: &Layout, fov: f64, share: f64) -> Option<Camera> {
    let perspective = fov > 0.0;
    if !fov.is_finite() || !(0.0..std::f64::consts::PI).contains(&fov) {
        return None;
    }
    let share = if share.is_finite() { share.clamp(0.1, 1.0) } else { 1.0 };
    if bounds.is_empty()
        || !bounds.min.is_finite()
        || !bounds.max.is_finite()
        || !cam.is_valid()
        || !layout.viewport.is_positive()
        || !layout.drawable.is_positive()
        || !layout.drawable.is_finite()
    {
        return None;
    }
    let original_fov = cam.fov;
    cam.fov = fov;
    let (r, u, b) = cam.basis();
    let centre = bounds.center();
    let mut hx: f64 = 0.0;
    let mut hy: f64 = 0.0;
    let mut depth: f64 = 0.0;
    let corners: Vec<_> = [bounds.min.x, bounds.max.x]
        .into_iter()
        .flat_map(|x| {
            [bounds.min.y, bounds.max.y].into_iter().flat_map(move |y| [bounds.min.z, bounds.max.z].into_iter().map(move |z| Vec3::new(x, y, z)))
        })
        .collect();
    for p in &corners {
        let d = *p - centre;
        hx = hx.max(d.dot(r).abs());
        hy = hy.max(d.dot(u).abs());
        depth = depth.max(d.dot(b).abs());
    }
    let h = f64::from(layout.viewport.height());
    let drawable = layout.drawable;
    let hh = (hx * h / f64::from(drawable.width())).max(hy * h / f64::from(drawable.height())).max(0.5);
    // Home and Fit put the model in the middle of the view. `drawable` decides its size and keeps
    // it clear of every control; the room it leaves over in either direction moves the model
    // towards `middle`, the middle of the view, instead of the middle of the gap between the
    // compass and the navigation bar, which left it low and to one side of an otherwise empty view.
    let size = vec2((hx * h / hh) as f32, (hy * h / hh) as f32).min(drawable.size());
    let toward = |want: f32, lo: f32, hi: f32| if lo <= hi { want.clamp(lo, hi) } else { (lo + hi) * 0.5 };
    let at = pos2(
        toward(layout.middle.center().x, drawable.left() + size.x * 0.5, drawable.right() - size.x * 0.5),
        toward(layout.middle.center().y, drawable.top() + size.y * 0.5, drawable.bottom() - size.y * 0.5),
    );
    let half = vec2((at.x - drawable.left()).min(drawable.right() - at.x), (at.y - drawable.top()).min(drawable.bottom() - at.y));
    let room = Rect::from_center_size(at, half * 2.0);
    if !room.is_positive() || !room.is_finite() {
        return None;
    }
    let hh = (hx * h / f64::from(room.width())).max(hy * h / f64::from(room.height())).max(0.5) / share;
    let offset = room.center() - layout.viewport.center();
    let tangent = if perspective { (fov * 0.5).tan() } else { 0.4 };
    cam.distance = hh / tangent + if perspective { depth } else { 0.0 };
    // Offset the target so the model projects into the free rectangle in the original viewport.
    // Perspective needs a bounded correction because corners have different depths.
    for _ in 0..16 {
        let k = 2.0 * cam.half_height() / h;
        cam.target = centre - r * (f64::from(offset.x) * k) + u * (f64::from(offset.y) * k);
        let radius = (bounds.diagonal() * 0.5 + centre.dist(cam.target)).max(cam.half_height() * 2.0).max(10.0);
        let fits = corners.iter().all(|p| {
            cam.to_screen(*p, f64::from(layout.viewport.width()), h, radius).is_some_and(|(x, y, z)| {
                x.is_finite()
                    && y.is_finite()
                    && (0.0..=1.0).contains(&z)
                    && room.contains(pos2(layout.viewport.left() + x as f32, layout.viewport.top() + y as f32))
            })
        });
        if fits {
            cam.fov = original_fov;
            return cam.is_valid().then_some(cam);
        }
        cam.distance *= 1.15;
    }
    None
}

#[cfg(test)]
#[path = "viewport_layout_tests.rs"]
mod tests;
