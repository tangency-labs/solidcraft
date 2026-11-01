//! A scene's colours come from its [`Palette`] (#974): the app draws `ui.render` in its theme,
//! while `solidcraft-cli snapshot`, MCP screenshots and thumbnails keep [`Palette::DEFAULT`].

use super::*;
use serde_json::json;
use solidcraft_geom::Vec2;
use solidcraft_sketch::{LinkGeom, LinkKind, LinkSource};

/// #4214: the grid's fade is at full strength in the middle of the view, and gone before the
/// grid's own edge and before the picture's far plane cuts the ground, so that neither of them
/// shows as a cut.
#[test]
fn the_grid_fades_out_before_its_edge_and_before_the_far_plane() {
    let fade = GridFade::new(40.0, 1.0, 120.0, 90.0);
    assert!(fade.start < fade.end, "the fade has a width");
    assert!(fade.end <= 90.0, "the grid is gone before the far plane cuts the ground at 90");
    assert!(fade.end <= 120.0, "the grid is gone before its own edge at 120");
    assert_eq!(fade.alpha(0.0), 1.0, "the middle of the grid is at full strength");
    assert_eq!(fade.alpha(fade.start), 1.0, "the grid is at full strength up to {}", fade.start);
    assert_eq!(fade.alpha(fade.end), 0.0, "the grid is gone at {}", fade.end);
    assert_eq!(fade.alpha(fade.end * 2.0), 0.0, "the grid does not come back");
    assert_eq!(fade.alpha(f64::NAN), 0.0, "a distance that is not a number fades the grid away");
    // The fade falls evenly.
    let mid = (fade.start + fade.end) * 0.5;
    assert!((fade.alpha(mid) - 0.5).abs() < 0.01, "the fade falls evenly: {} at {mid}", fade.alpha(mid));
}

/// #4214: the headless grid fades out with the distance from the middle of the view, as the
/// viewport's does: the lines near the middle are at full strength, the ones further out are
/// dimmer, and the grid is gone before the distance where it used to stop.
#[test]
fn the_headless_grid_fades_out_with_the_distance() {
    let s = Session::default();
    let mut cam = home_camera(&s);
    // A top view: the ground is the XY plane and the grid is centred on the target.
    cam.pitch = std::f64::consts::FRAC_PI_2;
    cam.target = Vec3::ZERO;
    let scene = scene(&s, &cam);
    assert!(!scene.grid.is_empty(), "the grid is drawn");
    // How far each piece of the grid is from the middle of the view, and the alpha it is drawn
    // with.
    let mut pieces: Vec<(f64, f32)> = scene
        .grid
        .iter()
        .map(|l| {
            let mid = l.points[0] + (l.points[1] - l.points[0]) * 0.5;
            ((mid.x * mid.x + mid.y * mid.y).sqrt(), l.alpha)
        })
        .collect();
    pieces.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // The grid in the middle of the view is at full strength.
    let strongest = pieces.iter().filter(|(d, _)| *d < 1.0).map(|(_, a)| *a).fold(0.0f32, f32::max);
    assert_eq!(strongest, 1.0, "the middle of the grid is at full strength");
    // The further from the middle, the dimmer: the outer half of the grid is fainter than the
    // inner half.
    let (near, far) = pieces.split_at(pieces.len() / 2);
    let mean = |v: &[(f64, f32)]| v.iter().map(|(_, a)| f64::from(*a)).sum::<f64>() / v.len().max(1) as f64;
    assert!(mean(near) > mean(far) + 0.1, "the grid fades: {} near the middle, {} far from it", mean(near), mean(far));
    // The grid is gone before the distance where it used to stop, so its edge never shows.
    let (_, major) = grid_step(cam.half_height());
    let cut = (cam.half_height() * 3.0 / major).ceil() * major;
    let farthest = pieces.last().map(|(d, _)| *d).unwrap_or(0.0);
    assert!(farthest < cut, "the grid fades out at {farthest}, short of the cut at {cut}");
}

/// #4214: the grid's half size, the one the picture and the window share: the grid reaches as far
/// on its plane as the view is wide, and further as the plane is seen at a slant.
#[test]
fn the_grid_reaches_further_as_its_plane_is_seen_at_a_slant() {
    let major = grid_step(40.0).1;
    let flat = grid_ext(40.0, major, 1.0);
    assert_eq!(flat, (40.0 * 3.0 / major).ceil() * major, "seen face on, the grid reaches as far as the view is wide");
    assert!(grid_ext(40.0, major, 0.5) > flat, "seen at a slant, the grid reaches further on its plane");
    // A plane seen edge-on keeps a usable size.
    assert!(grid_ext(40.0, major, 0.0).is_finite());
}

/// #9570: zooming through a switch of the grid's spacing used to swap the whole grid at once, so
/// scrolling made it pop over and over. The drawn grid now cross-fades: zoomed in small steps
/// through a switch, no line's strength jumps from one step to the next, while away from a
/// switch only [`grid_step`]'s level is drawn, at full strength, as before.
#[test]
fn the_grid_cross_fades_between_levels_as_the_view_zooms() {
    // Every 1, 2, 5 switch, zoomed by 1% a step from a half height of 8 to 1200.
    let strengths = |hh: f64| {
        let mut m = std::collections::BTreeMap::new();
        // The grid's centre off the origin, as when the view has panned: each level's lines
        // stay on the plane's own places.
        let at = 37.0;
        for l in grid_lines(hh, at, 60.0 * hh, 4000) {
            let world = l.offset + at;
            // Weight and major share, keyed by the line's place on the plane (to 1e-6 mm).
            m.insert((world * 1e6).round() as i64, (l.weight, l.major));
        }
        m
    };
    let mut hh = 8.0;
    let mut prev = strengths(hh);
    let mut faded = 0;
    while hh < 1200.0 {
        let next_hh = hh * 1.01;
        let next = strengths(next_hh);
        // Only lines both steps draw: compare within the smaller reach, short of its ends, where
        // each level stops at a whole number of its steps.
        let reach = ((60.0 * hh - hh / 4.0) * 1e6) as i64;
        for key in prev.keys().chain(next.keys()).filter(|k| (**k - 37_000_000).abs() < reach) {
            let (a, ma) = prev.get(key).copied().unwrap_or((0.0, 0.0));
            let (b, mb) = next.get(key).copied().unwrap_or((0.0, 0.0));
            assert!((a - b).abs() < 0.1, "the line at {} mm jumps from {a} to {b} zooming from {hh} to {next_hh}", *key as f64 * 1e-6);
            if a > 0.05 && b > 0.05 {
                assert!((ma - mb).abs() < 0.1, "the line at {} mm turns from major {ma} to {mb} zooming from {hh} to {next_hh}", *key as f64 * 1e-6);
            }
        }
        let levels = grid_levels(next_hh);
        let total: f32 = levels.iter().map(|g| g.weight).sum();
        assert!((total - 1.0).abs() < 1e-5, "the levels at {next_hh} add up to {total}");
        // The strongest level is the one snapping and the tools use.
        let strongest = levels.iter().fold(levels[0], |a, g| if g.weight > a.weight { *g } else { a });
        assert_eq!((strongest.minor, strongest.major), grid_step(next_hh), "at {next_hh} the drawn grid's main level is grid_step's");
        faded += usize::from(levels.len() == 2);
        prev = next;
        hh = next_hh;
    }
    assert!(faded > 0, "some steps cross-fade");
    // Away from a switch (raw step 3 lies between the 2 and 5 switches), one level at full
    // strength, as the grid always was.
    assert_eq!(grid_levels(30.0), vec![GridLevel { minor: 2.0, major: 10.0, weight: 1.0 }]);
    // grid_step's floor and hostile heights draw one level.
    for hh in [0.0, 1e-9, -5.0, f64::NAN, f64::INFINITY] {
        assert_eq!(grid_levels(hh).len(), 1, "{hh}");
        let _ = grid_lines(hh, f64::NAN, 10.0, 10);
        let _ = grid_lines(hh, 1e300, 1e300, 10);
    }
}

/// #9570: the headless picture's grid cross-fades as the viewport's does: right at a nested
/// switch of the spacing (5 to 10), both levels are drawn, the finer one's own lines at half
/// strength, in the palette's grid colours only. Right at the switch from 2 to 5, which do not
/// nest, neither level's own lines are drawn, only the 10 mm lines both share.
#[test]
fn the_headless_grid_draws_both_levels_at_a_switch() {
    let s = Session::default();
    let mut cam = home_camera(&s);
    cam.pitch = std::f64::consts::FRAC_PI_2;
    cam.target = Vec3::ZERO;
    let strength = |hh: f64, x: f64| {
        let mut cam = cam;
        cam.distance *= hh / cam.half_height();
        assert!((cam.half_height() - hh).abs() < 1e-6);
        let scene = scene(&s, &cam);
        assert!(scene.grid.iter().all(|l| [Palette::DEFAULT.grid, Palette::DEFAULT.grid_major].contains(&l.color)));
        // The strongest piece of the line in each grid colour, added: a line two levels share
        // with different kinds is drawn in both.
        let on = |l: &&SceneLine| (l.points[0].x - x).abs() < 1e-6 && (l.points[1].x - x).abs() < 1e-6;
        [Palette::DEFAULT.grid, Palette::DEFAULT.grid_major]
            .iter()
            .map(|c| scene.grid.iter().filter(on).filter(|l| l.color == *c).map(|l| l.alpha).fold(0.0f32, f32::max))
            .sum::<f32>()
    };
    // A half height where grid_step switches from 5 to 10: the 5 mm line (only the finer level)
    // is drawn at half strength, the 10 mm line (both) at full.
    let a = strength(100.0, 5.0);
    assert!(a > 0.3 && a < 0.7, "the line at 5 mm is drawn at {a}, half strength while the levels cross-fade");
    assert!(strength(100.0, 10.0) > 0.9);
    // A half height where grid_step switches from 2 to 5.
    for x in [2.0, 5.0] {
        let a = strength(50.0, x);
        assert!(a < 1e-6, "the line at {x} mm is drawn at {a} while 2 and 5 swap");
    }
    assert!(strength(50.0, 10.0) > 0.9);
}

/// #9570: the levels 2 and 5 do not nest, and drawn together they gave lines at 0, 2, 4, 5, 6,
/// 8, 10 mm, which looked restless. Zooming through that switch the finer level's own lines are
/// gone before the coarser level's own lines appear, while the lines both share (every 10 mm)
/// stay at full strength, so the grid never goes blank. Checked in every decade drawn.
#[test]
fn the_grid_never_draws_two_levels_that_do_not_nest_at_once() {
    for scale in [0.1, 1.0, 10.0, 100.0] {
        // grid_step switches from 2 to 5 (times `scale`) at a half height of 50 times `scale`.
        let (mut hh, end) = (30.0 * scale, 80.0 * scale);
        let (mut saw_fine, mut saw_coarse) = (false, false);
        while hh < end {
            let lines = grid_lines(hh, 0.0, 60.0 * scale, 4000);
            let at = |x: f64| lines.iter().filter(|l| (l.offset - x * scale).abs() < 1e-6 * scale).map(|l| l.weight).sum::<f32>();
            // 2, 4, 6, 8: only the finer level; 5, 15: only the coarser.
            let fine = [2.0, 4.0, 6.0, 8.0].iter().map(|x| at(*x)).fold(0.0f32, f32::max);
            let coarse = [5.0, 15.0].iter().map(|x| at(*x)).fold(0.0f32, f32::max);
            assert!(fine == 0.0 || coarse == 0.0, "at {hh} the 2 and 5 levels are both drawn ({fine}, {coarse})");
            saw_fine |= fine > 0.0 && fine < 1.0;
            saw_coarse |= coarse > 0.0 && coarse < 1.0;
            for x in [0.0, 10.0, 20.0] {
                assert!(at(x) > 0.999, "at {hh} the line at {x} mm both levels share is drawn at {}", at(x));
            }
            hh *= 1.005;
        }
        assert!(saw_fine && saw_coarse, "both levels fade rather than swap (scale {scale})");
    }
    // The nested switches still cross-fade both levels together: from 1 to 2 (half height 20).
    let lines = grid_lines(20.0, 0.0, 20.0, 4000);
    let at = |x: f64| lines.iter().filter(|l| (l.offset - x).abs() < 1e-6).map(|l| l.weight).sum::<f32>();
    assert!(at(1.0) > 0.3 && at(1.0) < 0.7 && at(2.0) > 0.999, "{} {}", at(1.0), at(2.0));
}

/// No colour here is one of [`colors`] or an appearance below, and each field has its own.
const MARK: Palette = Palette {
    background: (Rgb(1, 2, 3), Rgb(4, 5, 6)),
    grid: Rgb(7, 8, 9),
    grid_major: Rgb(10, 11, 12),
    edge: Rgb(13, 14, 15),
    body: Rgb(16, 17, 18),
    body_selected: Rgb(19, 20, 21),
    sketch: Rgb(22, 23, 24),
    sketch_fixed: Rgb(25, 26, 27),
    sketch_done: Rgb(28, 29, 30),
    sketch_locked: Rgb(31, 32, 33),
    sketch_projected: Rgb(34, 35, 36),
    sketch_lost: Rgb(37, 38, 39),
    sketch_construction: Rgb(40, 41, 42),
    sketch_centerline: Rgb(43, 44, 45),
    axes: [Rgb(46, 47, 48), Rgb(49, 50, 51), Rgb(52, 53, 54)],
    cap: Rgb(55, 56, 57),
    analysis: solidcraft_render::AnalysisColors {
        draft: [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9]],
        access: [[0.11, 0.12, 0.13], [0.14, 0.15, 0.16]],
        curvature: [[0.17, 0.18, 0.19], [0.21, 0.22, 0.23], [0.24, 0.25, 0.26]],
        hatch: true,
    },
};

/// Each curve's lines at x = its index, so a line tells which curve it draws.
fn by_index(_: &Sketch, i: usize, _: &Plane) -> Vec<Vec<Vec3>> {
    vec![vec![Vec3::new(i as f64, 0., 0.), Vec3::new(i as f64, 1., 0.)]]
}

#[test]
fn the_default_palette_is_the_colour_table() {
    let p = Palette::default();
    assert_eq!(p, Palette::DEFAULT);
    assert_eq!(p.background, (colors::BG_TOP, colors::BG_BOTTOM));
    assert_eq!(
        (p.grid, p.grid_major, p.edge, p.body, p.body_selected),
        (colors::GRID, colors::GRID_MAJOR, colors::EDGE, colors::BODY, colors::BODY_SELECTED)
    );
    assert_eq!(
        (p.sketch, p.sketch_fixed, p.sketch_done, p.sketch_locked),
        (colors::SKETCH, colors::SKETCH_FIXED, colors::SKETCH_DONE, colors::SKETCH_LOCKED)
    );
    assert_eq!(
        (p.sketch_projected, p.sketch_lost, p.sketch_construction, p.sketch_centerline),
        (colors::SKETCH_PROJECTED, colors::SKETCH_LOST, colors::SKETCH_CONSTRUCTION, colors::SKETCH_CENTERLINE)
    );
    assert_eq!((p.axes, p.cap), ([colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z], colors::SECTION_CAP));
    assert_eq!(p.analysis, solidcraft_render::AnalysisColors::DEFAULT);
}

#[test]
fn each_sketch_state_takes_its_palette_colour() {
    let line = |sk: &mut Sketch, y: f64| sk.add_line(Vec2::new(0., y), Vec2::new(2., y), None, None, None).unwrap();
    let mut sk = Sketch::new();
    let ordinary = line(&mut sk, 0.);
    let fixed = line(&mut sk, 1.);
    sk.curves[fixed].fixed = true;
    let centerline = line(&mut sk, 2.);
    sk.curves[centerline].centerline = true;
    let construction = line(&mut sk, 3.);
    sk.curves[construction].construction = true;
    let determined = line(&mut sk, 4.);
    sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(Vec2::new(0., 5.), Vec2::new(2., 5.))]).unwrap();
    let projected = sk.curves.len() - 1;
    let lost_link = sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(Vec2::new(0., 6.), Vec2::new(2., 6.))]).unwrap();
    sk.set_link_lost(&lost_link, true);
    let lost = sk.curves.len() - 1;
    let text = LinkSource::Text {
        text: "A".into(),
        at: Vec2::ZERO,
        height: 2.,
        angle: 0.,
        font: None,
        style: Default::default(),
        exprs: Default::default(),
    };
    sk.add_link(LinkKind::Text, text, &[LinkGeom::Line(Vec2::new(0., 7.), Vec2::new(2., 7.))]).unwrap();
    let text = sk.curves.len() - 1;
    let mut flags = vec![false; sk.curves.len()];
    flags[determined] = true;
    let colour_of = |active: bool, i: usize| {
        let lines = sketch_lines_in(&sk, &Plane::XY, active, &flags, by_index, &MARK);
        let mut of_curve = lines.iter().filter(|(pts, _, _)| pts.first().is_some_and(|p| p.x == i as f64)).map(|(_, c, _)| *c);
        let c = of_curve.next();
        assert!(of_curve.all(|other| Some(other) == c), "curve {i} drawn in two colours");
        c
    };
    for (i, want) in [
        (ordinary, MARK.sketch),
        (determined, MARK.sketch_fixed),
        (fixed, MARK.sketch_locked),
        (centerline, MARK.sketch_centerline),
        (construction, MARK.sketch_construction),
        (projected, MARK.sketch_projected),
        (lost, MARK.sketch_lost),
        (text, MARK.sketch),
    ] {
        assert_eq!(colour_of(true, i), Some(want), "curve {i} in the active sketch");
    }
    for i in [ordinary, determined, text] {
        assert_eq!(colour_of(false, i), Some(MARK.sketch_done), "curve {i} in a finished sketch");
    }
    // The public function is the default palette: the colours the GPU viewport reads as tags.
    assert_eq!(
        sketch_lines_in(&sk, &Plane::XY, true, &flags, by_index, &Palette::DEFAULT),
        sketch_lines_with_sampler(&sk, &Plane::XY, true, &flags, by_index)
    );
}

/// Every colour of a scene comes from its palette, except what the design says: a body's
/// appearance (even one equal to the default grey) and a face's own colour.
#[test]
fn every_scene_colour_comes_from_its_palette_and_appearances_stay() {
    let mut s = Session::default();
    let new_body = |s: &mut Session, name: &str, x: f64| {
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "body_name": name}))
            .unwrap();
    };
    new_body(&mut s, "Grey", 0.);
    new_body(&mut s, "Picked", 20.);
    new_body(&mut s, "Plain", 40.);
    // Exactly the default body colour, as an appearance: the design's, so it stays.
    s.execute("appearance.assign", &json!({"bodies": ["Grey"], "color": "#b0bac6"})).unwrap();
    s.execute("appearance.assign", &json!({"body": "Plain", "faces": [[45, 5, 10]], "color": "#00ff00"})).unwrap();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s.execute("sketch.line", &json!({"points": [[0, -20], [30, -20]]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    s.selection = vec![Sel::Body { name: "Picked".into() }];
    s.section = Some((Vec3::new(0., 0., 5.), Vec3::Z));
    let cam = home_camera(&s);
    let names: Vec<String> = s.world_state().bodies.iter().filter(|b| s.body_visible(&b.name)).map(|b| b.name.clone()).collect();
    let mesh = |sc: &Scene, name: &str| {
        names.iter().position(|n| n == name).and_then(|i| sc.meshes.get(i)).map(|m| (m.color, m.face_colors.clone())).unwrap()
    };

    let marked = scene_with_palette(&s, &cam, s.section, world_curve_parts, &MARK);
    assert_eq!((marked.background, marked.cap), (Some(MARK.background), Some(MARK.cap)));
    let allowed = [
        MARK.grid,
        MARK.grid_major,
        MARK.edge,
        MARK.axes[0],
        MARK.axes[1],
        MARK.axes[2],
        MARK.sketch,
        MARK.sketch_fixed,
        MARK.sketch_done,
        MARK.sketch_locked,
        MARK.sketch_projected,
        MARK.sketch_lost,
        MARK.sketch_construction,
        MARK.sketch_centerline,
    ];
    let all_lines = || marked.grid.iter().chain(&marked.lines).chain(&marked.unclipped_lines);
    let stray: Vec<Rgb> = all_lines().map(|l| l.color).filter(|c| !allowed.contains(c)).collect();
    assert!(stray.is_empty(), "line colours not from the palette: {stray:?}");
    for want in [MARK.grid, MARK.grid_major, MARK.edge, MARK.axes[0], MARK.axes[1], MARK.axes[2], MARK.sketch_done] {
        assert!(all_lines().any(|l| l.color == want), "no line in {want:?}");
    }
    assert!(marked.grid.iter().all(|l| [MARK.grid, MARK.grid_major].contains(&l.color)), "only the grid is drawn under the model");
    assert_eq!(mesh(&marked, "Grey").0, colors::BODY, "an appearance equal to the default grey is the design's");
    assert_eq!(mesh(&marked, "Picked").0, MARK.body_selected);
    let (plain, faces) = mesh(&marked, "Plain");
    assert_eq!(plain, MARK.body);
    assert!(!faces.is_empty() && faces.iter().all(|(_, c)| *c == Rgb(0, 255, 0)), "face colours are the design's: {faces:?}");

    // The default palette is the scene everyone else gets, line for line.
    let default = scene_with_palette(&s, &cam, s.section, world_curve_parts, &Palette::DEFAULT);
    let plain_scene = scene(&s, &cam);
    assert_eq!(
        (default.background, default.cap, default.clip, default.radius),
        (plain_scene.background, plain_scene.cap, plain_scene.clip, plain_scene.radius)
    );
    for (default, plain) in
        [(&default.lines, &plain_scene.lines), (&default.grid, &plain_scene.grid), (&default.unclipped_lines, &plain_scene.unclipped_lines)]
    {
        assert_eq!(default.len(), plain.len());
        for (a, b) in default.iter().zip(plain) {
            assert_eq!((&a.points, a.color, a.width, a.on_top), (&b.points, b.color, b.width, b.on_top));
        }
    }
    for name in &names {
        assert_eq!(mesh(&default, name), mesh(&plain_scene, name), "{name}");
    }
    assert_eq!(mesh(&default, "Plain").0, colors::BODY);
    assert_eq!(mesh(&default, "Picked").0, colors::BODY_SELECTED);
}

/// A see-through appearance reaches the picture: the scene carries the body's opacity, so the
/// renderers draw it as glass and what is inside it shows. A selected body stays solid, as in
/// the viewport, so the selection colour reads.
#[test]
fn a_see_through_appearance_reaches_the_scene() {
    let mut s = Session::default();
    for (name, x) in [("Target", 0.), ("Tool", 20.), ("Picked", 40.)] {
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "body_name": name}))
            .unwrap();
    }
    s.execute("appearance.assign", &json!({"bodies": ["Target"], "color": "#b0bac6", "opacity": 0.3})).unwrap();
    s.execute("appearance.assign", &json!({"bodies": ["Picked"], "color": "#b0bac6", "opacity": 0.3})).unwrap();
    s.selection = vec![Sel::Body { name: "Picked".into() }];
    let cam = home_camera(&s);
    let sc = scene(&s, &cam);
    let names: Vec<String> = s.world_state().bodies.iter().filter(|b| s.body_visible(&b.name)).map(|b| b.name.clone()).collect();
    let opacity = |name: &str| names.iter().position(|n| n == name).and_then(|i| sc.meshes.get(i)).map(|m| (m.opacity, m.glass())).unwrap();

    assert_eq!(opacity("Target"), (0.3, true), "the design's opacity is the one the renderers draw");
    assert_eq!(opacity("Tool"), (1.0, false), "a body with no appearance of its own is solid");
    assert_eq!(opacity("Picked"), (1.0, false), "a selected body is drawn solid, so the selection colour reads");

    // Clearing the appearance makes it solid again.
    s.execute("appearance.assign", &json!({"bodies": ["Target"], "color": null})).unwrap();
    let sc = scene(&s, &cam);
    assert_eq!(sc.meshes.first().map(|m| m.opacity), Some(1.0));
}
