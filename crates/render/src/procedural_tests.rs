use super::*;

fn oak() -> Oak {
    Oak {
        ring_spacing_mm: 3.0,
        figure: 0.4,
        earlywood: [0.55, 0.38, 0.22],
        latewood: [0.33, 0.20, 0.10],
        roughness: 0.35,
        pore_depth_mm: 0.05,
        amplitude: 1.0,
        seed: 7,
    }
}
fn abs() -> TexturedAbs {
    TexturedAbs { grade_mm: 0.4, depth_mm: 0.03, roughness_valley: 0.25, roughness_peak: 0.55, amplitude: 1.0, seed: 11 }
}
fn brushed() -> Brushed {
    Brushed {
        streak_width_mm: 0.02,
        streak_length_mm: 25.0,
        tint: [0.91, 0.92, 0.92],
        color_variation: 0.08,
        roughness: 0.3,
        roughness_variation: 0.3,
        groove_depth_mm: 0.002,
        amplitude: 1.0,
        seed: 3,
    }
}
fn all() -> [Recipe; 3] {
    [Recipe::Oak(oak()), Recipe::TexturedAbs(abs()), Recipe::Brushed(brushed())]
}
fn with_amplitude(r: Recipe, a: f64) -> Recipe {
    match r {
        Recipe::Oak(o) => Recipe::Oak(Oak { amplitude: a, ..o }),
        Recipe::TexturedAbs(t) => Recipe::TexturedAbs(TexturedAbs { amplitude: a, ..t }),
        Recipe::Brushed(b) => Recipe::Brushed(Brushed { amplitude: a, ..b }),
    }
}

/// Deterministic test points spread over a few centimetres, from a fixed integer sequence.
fn points(n: usize) -> Vec<Point> {
    let mut state = 0x1234_5678_9abc_def0u64;
    (0..n)
        .map(|_| {
            let mut c = [0.0; 3];
            for v in &mut c {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                *v = ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 60.0;
            }
            Point::new(c).unwrap()
        })
        .collect()
}

#[test]
fn zero_amplitude_is_exactly_the_plain_material() {
    for recipe in all() {
        let plain = with_amplitude(recipe, 0.0).validate().unwrap();
        for p in points(500) {
            assert_eq!(evaluate(&plain, p), Sample::PLAIN, "{recipe:?} at {p:?}");
        }
    }
}

#[test]
fn hostile_parameters_and_points_are_refused() {
    let bad = [
        Recipe::Oak(Oak { ring_spacing_mm: f64::NAN, ..oak() }),
        Recipe::Oak(Oak { ring_spacing_mm: 0.0, ..oak() }),
        Recipe::Oak(Oak { earlywood: [1.5, 0.0, 0.0], ..oak() }),
        Recipe::Oak(Oak { amplitude: -0.1, ..oak() }),
        Recipe::Oak(Oak { pore_depth_mm: f64::INFINITY, ..oak() }),
        Recipe::TexturedAbs(TexturedAbs { grade_mm: 0.0, ..abs() }),
        Recipe::TexturedAbs(TexturedAbs { roughness_peak: 0.0, ..abs() }),
        Recipe::TexturedAbs(TexturedAbs { depth_mm: 1.0, ..abs() }),
        Recipe::Brushed(Brushed { streak_width_mm: 1.0e-9, ..brushed() }),
        Recipe::Brushed(Brushed { streak_length_mm: 0.1, streak_width_mm: 0.05, ..brushed() }),
        Recipe::Brushed(Brushed { tint: [f64::NAN, 0.5, 0.5], ..brushed() }),
        Recipe::Brushed(Brushed { amplitude: 2.0, ..brushed() }),
    ];
    for recipe in bad {
        assert!(matches!(recipe.validate(), Err(PatternError::Parameter(_))), "{recipe:?}");
    }
    for p in [[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0], [0.0, 0.0, MAX_COORDINATE_MM * 1.0001], [-2.0e9, 0.0, 0.0]] {
        assert_eq!(Point::new(p), Err(PatternError::Point), "{p:?}");
    }
    // The bounds themselves are valid, and evaluating there stays finite.
    let far = Point::new([MAX_COORDINATE_MM, -MAX_COORDINATE_MM, MAX_COORDINATE_MM]).unwrap();
    for recipe in all() {
        let s = evaluate(&recipe.validate().unwrap(), far);
        assert!(s.height_mm.is_finite() && s.height_gradient.iter().all(|g| g.is_finite()), "{recipe:?}");
    }
}

#[test]
fn outputs_stay_in_range_and_families_choose_their_outputs() {
    for recipe in all() {
        let valid = recipe.validate().unwrap();
        for p in points(2000) {
            let s = evaluate(&valid, p);
            if let Some(c) = s.base_color {
                assert!(c.iter().all(|v| (0.0..=1.0).contains(v)), "{c:?}");
            }
            let r = s.roughness.unwrap();
            assert!((MIN_ROUGHNESS..=1.0).contains(&r));
            assert!(s.height_mm.is_finite());
            match recipe {
                Recipe::Oak(o) => assert!(s.base_color.is_some() && (-o.pore_depth_mm..=0.0).contains(&s.height_mm)),
                Recipe::TexturedAbs(t) => assert!(s.base_color.is_none() && s.height_mm.abs() <= 0.5 * t.depth_mm + 1e-12),
                Recipe::Brushed(b) => assert!(s.base_color.is_some() && s.height_mm.abs() <= b.groove_depth_mm + 1e-12),
            }
        }
    }
}

/// The analytic gradient matches central differences of the height, away from oak's ring
/// boundaries (a step by design).
#[test]
fn height_gradients_match_central_differences() {
    for recipe in all() {
        let valid = recipe.validate().unwrap();
        // Steps well below each family's smallest feature.
        let h = match recipe {
            // Small enough that a step rarely straddles a smoothstep end (a kink in the slope).
            Recipe::Oak(_) => 1.0e-5,
            Recipe::TexturedAbs(t) => t.grade_mm * 1.0e-4,
            Recipe::Brushed(b) => b.streak_width_mm * 1.0e-4,
        };
        let height = |c: [f64; 3]| evaluate(&valid, Point::new(c).unwrap()).height_mm;
        let (mut checked, mut worst) = (0, 0.0f64);
        for p in points(400) {
            let c = p.mm();
            if let Recipe::Oak(o) = recipe {
                // Skip points within a few steps of a ring boundary.
                let r = (c[1] * c[1] + c[2] * c[2]).sqrt();
                let near = |dr: f64| ((r + dr) / o.ring_spacing_mm).floor() != (r / o.ring_spacing_mm).floor();
                if near(0.01) || near(-0.01) {
                    continue;
                }
            }
            let analytic = evaluate(&valid, p).height_gradient;
            let scale = analytic.iter().fold(0.0f64, |m, g| m.max(g.abs())).max(1.0e-9);
            for i in 0..3 {
                let (mut a, mut b) = (c, c);
                a[i] += h;
                b[i] -= h;
                let numeric = (height(a) - height(b)) / (2.0 * h);
                worst = worst.max((numeric - analytic[i]).abs() / scale);
            }
            checked += 1;
        }
        assert!(checked > 100, "{recipe:?}: only {checked} points checked");
        assert!(worst < 2.0e-3, "{recipe:?}: worst relative gradient error {worst}");
    }
}

/// Same inputs, same bits, and a fixed digest of 1000 evaluations so a platform or refactor that
/// changes any value is noticed.
#[test]
fn evaluation_is_deterministic() {
    let digest = |recipe: Recipe| {
        let valid = recipe.validate().unwrap();
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |v: f64| {
            h ^= v.to_bits();
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        for p in points(1000) {
            let s = evaluate(&valid, p);
            s.base_color.unwrap_or([-1.0; 3]).into_iter().for_each(&mut feed);
            feed(s.roughness.unwrap_or(-1.0));
            feed(s.height_mm);
            s.height_gradient.into_iter().for_each(&mut feed);
        }
        h
    };
    for recipe in all() {
        assert_eq!(digest(recipe), digest(recipe));
    }
    let got = all().map(digest);
    assert_eq!(got, GOLDEN, "pattern values changed: {got:#x?}");
}

/// Digests of `evaluation_is_deterministic` for oak, textured ABS and brushed.
const GOLDEN: [u64; 3] = [0x4155_463c_2888_d771, 0xfeac_f0a1_2c6f_9943, 0xa50c_d3b9_e0d4_6dcb];

#[test]
fn seeds_and_directions_shape_the_patterns() {
    // A different seed gives a different pattern.
    let a = Recipe::TexturedAbs(abs()).validate().unwrap();
    let b = Recipe::TexturedAbs(TexturedAbs { seed: 12, ..abs() }).validate().unwrap();
    let differ = points(200).into_iter().filter(|p| evaluate(&a, *p) != evaluate(&b, *p)).count();
    assert!(differ > 190);
    // Brushed streaks run along X: neighbours along X agree far more than across it.
    let brushed = Recipe::Brushed(brushed()).validate().unwrap();
    let roughness = |c: [f64; 3]| evaluate(&brushed, Point::new(c).unwrap()).roughness.unwrap_or(0.0);
    let (mut along, mut across) = (0.0, 0.0);
    for p in points(300) {
        let c = p.mm();
        along += (roughness(c) - roughness([c[0] + 0.05, c[1], c[2]])).abs();
        across += (roughness(c) - roughness([c[0], c[1] + 0.05, c[2]])).abs();
    }
    assert!(along * 5.0 < across, "along {along}, across {across}");
    // Oak rings: colour repeats with the ring spacing along a radius.
    let oak_recipe = Recipe::Oak(Oak { figure: 0.0, pore_depth_mm: 0.0, ..oak() }).validate().unwrap();
    let colour = |y: f64| evaluate(&oak_recipe, Point::new([0.0, y, 0.0]).unwrap()).base_color.unwrap_or([0.0; 3]);
    for y in [1.0, 2.2, 3.7] {
        let (c0, c1) = (colour(y), colour(y + 3.0));
        assert!(c0.iter().zip(c1).all(|(a, b)| (a - b).abs() < 1e-12), "{y}: {c0:?} vs {c1:?}");
    }
}

#[test]
fn limitations_are_reported_and_the_capability_is_provisional() {
    assert!(Recipe::Oak(oak()).limitations().iter().any(|l| l.contains("end grain")));
    assert!(Recipe::Oak(oak()).limitations().iter().any(|l| l.contains("grain axis")));
    assert!(Recipe::Brushed(brushed()).limitations().iter().any(|l| l.contains("anisotropic")));
    assert!(Recipe::TexturedAbs(abs()).limitations().is_empty());
    assert!(PROVISIONAL_CAPABILITY.ends_with("/draft"));
}
