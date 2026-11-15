//! Shell regressions found by the robustness fuzzer (docs/robustness.md).
//!
//! The box-on-box designs below have a saddle corner: where the top box's left face meets the
//! lower box's back face, four planar faces meet with convex and concave edges in turn (the
//! top box overhangs the lower box's back face). A shell splits that corner into two corners
//! joined by a new edge.
//!
//! Lower box `[0, 25] × [0, 17.5] × [0, 35]`, top box `[7.5, 22.5] × [0, 17.5 + o] × [35, 42.5]`.
//! With the bottom face open, a shell of thickness `t` leaves this cavity (for `o < 17.5 − 2t`):
//!
//! - in the lower box: `[t, 25 − t] × [t, 17.5 − t] × [0, 35 − t]`
//! - in the top box, over the lower box: `[7.5 + t, 22.5 − t] × [t, 17.5 − t] × [35 − t, 42.5 − t]`
//! - in the overhang: `[7.5 + t, 22.5 − t] × [17.5 − t, 17.5 + o − t] × [35 + t, 42.5 − t]`
//!
//! The shell's volume is the solid's volume less those three boxes.
use serde_json::json;
use solidcraft_engine::Session;

fn run(s: &mut Session, id: &str, p: serde_json::Value) {
    if let Err(e) = s.execute(id, &p) {
        panic!("{id} {p}: {e}");
    }
}

/// The box on a box, overhanging the lower box's back face by `o`.
fn box_on_box(o: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"height": 35.0, "length": 25.0, "width": 17.5}));
    run(&mut s, "solid.box", json!({"corner": [7.5, 0.0, 35.0], "height": 7.5, "length": 15.0, "operation": "join", "width": 17.5 + o}));
    s
}

/// The shell's volume with the bottom face open (see the module comment).
fn open_bottom_shell_volume(o: f64, t: f64) -> f64 {
    let solid = 25.0 * 17.5 * 35.0 + 15.0 * (17.5 + o) * 7.5;
    let lower = (25.0 - 2.0 * t) * (17.5 - 2.0 * t) * (35.0 - t);
    let above = (15.0 - 2.0 * t) * (17.5 - 2.0 * t) * 7.5;
    let overhang = (15.0 - 2.0 * t) * o * (7.5 - 2.0 * t);
    solid - lower - above - overhang
}

/// Seed 83 (minimised): the overhang is 2.5 mm, and the shell opens the top box's left face.
/// The shell refused the saddle corner (`a corner whose faces don't meet the same way`).
#[test]
fn shell_box_on_box_overhanging_saddle_corner() {
    let mut s = box_on_box(2.5);
    run(&mut s, "solid.shell", json!({"faces": [[7.5, 17.5, 38.75]], "thickness": 1.5}));
    sound(&s);
    // The bottom closed and the top box's left face x = 7.5 open, so the cavity in the top box
    // runs out to it: [1.5, 23.5] × [1.5, 16] × [1.5, 33.5] in the lower box,
    // [7.5, 21] × [1.5, 16] × [33.5, 41] over it and [7.5, 21] × [16, 18.5] × [36.5, 41] in the
    // overhang, out of the solid's 25 × 17.5 × 35 + 15 × 20 × 7.5.
    let want = 25.0 * 17.5 * 35.0 + 15.0 * 20.0 * 7.5 - 22.0 * 14.5 * 32.0 - 13.5 * 14.5 * 7.5 - 13.5 * 2.5 * 4.5;
    let got = volume(&s);
    assert!((got - want).abs() < 1e-6 * want, "shell volume {got}, want {want}");
}

/// The same saddle corner with the bottom open: every face round the corner moves, and the
/// volume is exact.
#[test]
fn shell_saddle_corner_volume_is_exact() {
    for t in [1.0, 1.5] {
        let mut s = box_on_box(2.5);
        run(&mut s, "solid.shell", json!({"faces": [[12.5, 8.75, 0.0]], "thickness": t}));
        sound(&s);
        let (got, want) = (volume(&s), open_bottom_shell_volume(2.5, t));
        assert!((got - want).abs() < 1e-6 * want, "thickness {t}: shell volume {got}, want {want}");
    }
}

/// An overhang (1 mm) thinner than the wall (1.5 mm): the overhang's cavity is only 1 mm deep.
/// A corner landing outside the solid, or a wall left thin, would change the volume.
#[test]
fn shell_saddle_corner_with_an_overhang_thinner_than_the_wall() {
    let mut s = box_on_box(1.0);
    run(&mut s, "solid.shell", json!({"faces": [[12.5, 8.75, 0.0]], "thickness": 1.5}));
    sound(&s);
    let (got, want) = (volume(&s), open_bottom_shell_volume(1.0, 1.5));
    assert!((got - want).abs() < 1e-6 * want, "shell volume {got}, want {want}");
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(f64::NAN)).sum()
}

fn sound(s: &Session) {
    let errors: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1, "one body");
    for b in &st.bodies {
        assert_eq!(b.body.shell_count(), 1, "one shell");
        assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
    }
}

/// A shell that opens no face (rig shell/4009): a closed hollow body, the void sealed inside. A
/// 40 × 30 × 12.5 box with a 2.5 wall inside leaves a 35 × 25 × 7.5 void; grown 2.5 outside, the
/// walls are a 45 × 35 × 17.5 box less the original.
#[test]
fn a_shell_that_opens_no_face_seals_its_void() {
    for (direction, want, shells) in [("inside", 15000.0 - 35.0 * 25.0 * 7.5, 2), ("outside", 45.0 * 35.0 * 17.5 - 15000.0, 2)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40.0, "width": 30.0, "height": 12.5}));
        run(&mut s, "solid.shell", json!({"faces": [], "thickness": 2.5, "direction": direction}));
        let errors: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
        assert!(errors.is_empty(), "{direction}: {errors:?}");
        let st = s.model.state();
        assert_eq!(st.bodies.len(), 1, "{direction}: one body");
        assert_eq!(st.bodies[0].body.shell_count(), shells, "{direction}: an outer and an inner shell");
        let got = volume(&s);
        assert!((got - want).abs() < 1e-6 * want, "{direction}: volume {got}, want {want}");
    }
}

/// A loft through closed fit-point splines (12 points each) on YZ planes at the given offsets.
fn fit_point_loft(sections: &[(f64, &[[f64; 2]])]) -> Session {
    let mut s = Session::default();
    let mut names = Vec::new();
    for (i, (x, points)) in sections.iter().enumerate() {
        let name = format!("Sec{i}");
        run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": x, "name": name}));
        run(&mut s, "sketch.spline.fit_point", json!({"points": points, "periodic": true}));
        run(&mut s, "sketch.finish", json!({}));
        names.push(json!({"sketch": name}));
    }
    run(&mut s, "solid.loft", json!({"sections": names, "operation": "new"}));
    s
}

/// The shell of `s`'s loft, 1 mm thick with the end face through `open` removed, is one sound
/// body. Its walls are the loft's faces moved 1 mm in; the loft is convex, so the area of the
/// surface 1 mm in is at most the loft's: the wall's volume lies between `t` times the inner
/// wall's area (the shell's area less the loft's, which holds the open cap the rim lies in)
/// and `t` times the loft's area.
fn shells_in_one_mm(mut s: Session, open: [f64; 3]) {
    let loft = solidcraft_kernel::measure(&s.model.state().bodies[0].body).unwrap();
    run(&mut s, "solid.shell", json!({"faces": [open], "thickness": 1.0}));
    sound(&s);
    let shell = solidcraft_kernel::measure(&s.model.state().bodies[0].body).unwrap();
    let (lo, hi) = (shell.area - loft.area, loft.area);
    assert!(lo <= shell.volume && shell.volume <= hi, "wall volume {} outside [{lo}, {hi}]", shell.volume);
}

/// #15859: a loft through three fit-point splines (loft A) was refused by Shell ("a gap in its
/// offset's loops cannot be bridged within tolerance"). Each section's pieces are G1 but not
/// C1, with their own derivative ratios at the joints, so the loft's side had creases between
/// the sections and its exact offset parted there. The loft now meets its sections with one
/// ratio per joint, so its side is G1 everywhere.
#[test]
fn shell_of_a_loft_through_fit_point_splines() {
    let a = fit_point_loft(&[
        (
            -30.0,
            &[
                [0.0, 10.0],
                [7.5, 11.34],
                [12.99, 15.0],
                [15.0, 20.0],
                [12.99, 25.0],
                [7.5, 28.66],
                [0.0, 30.0],
                [-7.5, 28.66],
                [-12.99, 25.0],
                [-15.0, 20.0],
                [-12.99, 15.0],
                [-7.5, 11.34],
            ],
        ),
        (
            0.0,
            &[
                [0.0, 8.0],
                [10.0, 9.608],
                [17.321, 14.0],
                [20.0, 20.0],
                [17.321, 26.0],
                [10.0, 30.392],
                [0.0, 32.0],
                [-10.0, 30.392],
                [-17.321, 26.0],
                [-20.0, 20.0],
                [-17.321, 14.0],
                [-10.0, 9.608],
            ],
        ),
        (
            30.0,
            &[
                [0.0, 12.0],
                [7.5, 13.34],
                [12.99, 17.0],
                [15.0, 22.0],
                [12.99, 27.0],
                [7.5, 30.66],
                [0.0, 32.0],
                [-7.5, 30.66],
                [-12.99, 27.0],
                [-15.0, 22.0],
                [-12.99, 17.0],
                [-7.5, 13.34],
            ],
        ),
    ]);
    shells_in_one_mm(a, [30.0, 0.0, 22.0]);
}

/// #15859 (loft C): six superellipse sections, fit points at equal angle steps.
#[test]
fn shell_of_a_six_section_loft_through_fit_point_splines() {
    let c = fit_point_loft(&[
        (
            -50.0,
            &[
                [2.070552, 15.170371],
                [5.656854, 16.464466],
                [7.727407, 18.705905],
                [7.727407, 21.294095],
                [5.656854, 23.535534],
                [2.070552, 24.829629],
                [-2.070552, 24.829629],
                [-5.656854, 23.535534],
                [-7.727407, 21.294095],
                [-7.727407, 18.705905],
                [-5.656854, 16.464466],
                [-2.070552, 15.170371],
            ],
        ),
        (
            -30.0,
            &[
                [4.753197, 10.144903],
                [13.641449, 12.421417],
                [17.739175, 17.359335],
                [17.739175, 22.640665],
                [13.641449, 27.578583],
                [4.753197, 29.855097],
                [-4.753197, 29.855097],
                [-13.641449, 27.578583],
                [-17.739175, 22.640665],
                [-17.739175, 17.359335],
                [-13.641449, 12.421417],
                [-4.753197, 10.144903],
            ],
        ),
        (
            -10.0,
            &[
                [6.390064, 6.088642],
                [19.048813, 8.888193],
                [23.848042, 16.272463],
                [23.848042, 23.727537],
                [19.048813, 31.111807],
                [6.390064, 33.911358],
                [-6.390064, 33.911358],
                [-19.048813, 31.111807],
                [-23.848042, 23.727537],
                [-23.848042, 16.272463],
                [-19.048813, 8.888193],
                [-6.390064, 6.088642],
            ],
        ),
        (
            10.0,
            &[
                [6.537189, 5.361726],
                [18.243501, 9.053899],
                [24.397123, 16.077686],
                [24.397123, 23.922314],
                [18.243501, 30.946101],
                [6.537189, 34.638274],
                [-6.537189, 34.638274],
                [-18.243501, 30.946101],
                [-24.397123, 23.922314],
                [-24.397123, 16.077686],
                [-18.243501, 9.053899],
                [-6.537189, 5.361726],
            ],
        ),
        (
            30.0,
            &[
                [5.311867, 9.096713],
                [15.614184, 11.412199],
                [19.824158, 17.078473],
                [19.824158, 22.921527],
                [15.614184, 28.587801],
                [5.311867, 30.903287],
                [-5.311867, 30.903287],
                [-15.614184, 28.587801],
                [-19.824158, 22.921527],
                [-19.824158, 17.078473],
                [-15.614184, 11.412199],
                [-5.311867, 9.096713],
            ],
        ),
        (
            50.0,
            &[
                [2.329371, 14.204445],
                [6.363961, 15.757359],
                [8.693332, 18.447086],
                [8.693332, 21.552914],
                [6.363961, 24.242641],
                [2.329371, 25.795555],
                [-2.329371, 25.795555],
                [-6.363961, 24.242641],
                [-8.693332, 21.552914],
                [-8.693332, 18.447086],
                [-6.363961, 15.757359],
                [-2.329371, 14.204445],
            ],
        ),
    ]);
    shells_in_one_mm(c, [50.0, 0.0, 20.0]);
}
