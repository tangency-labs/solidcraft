//! SolidCraft: the drops on the rainy window, simulated on the CPU so they remember. Small drops
//! land and stick; drops that meet merge; a drop heavy enough slides, faster the heavier it is,
//! leaving a wiped trail and a few beads. A drop keeps its size for as long as it lives and changes
//! only in a merge, in one step. The shader refracts through them, finding each pixel's drops
//! through a coarse grid.
//!
//! Lengths are in window heights, x across from the left (0 to the aspect), y down from the top.

/// Drops this size and up slide; the weight of a sliding drop sets how fast it runs, from [`R_STOP`].
const R_STICK: f32 = 0.0075;
const R_STOP: f32 = 0.0055;
/// The largest drop: a bigger one still fits the shader's grid ([`CELL`]) when stretched.
const R_MAX: f32 = 0.017;
/// New drops landing per second over a window as wide as it is tall: none at an amount of 0, then
/// [`DRIZZLE`] rising by a steady multiple per step up to [`DOWNPOUR`] at 1.
const DRIZZLE: f32 = 3.0;
const DOWNPOUR: f32 = 400.0;
#[cfg(test)]
const AMOUNT_AT_LANDING: f32 = 0.5;
/// The rain amount when nothing says otherwise (a share, 0 to 1).
pub const DEFAULT_AMOUNT: f32 = 0.6;
/// How much faster sliding drops run than the first tuning had them.
const SLIDE: f32 = 1.3;
/// A sliding drop may leave a bead every this far.
const BEAD_EVERY: f32 = 0.011;
/// How long a wiped trail takes to fog over again, in seconds.
const REFOG: f32 = 7.0;
/// At most this many drops: when the glass is this full, new rain joins the nearest drop and
/// sliding drops leave no beads until merging or running off makes room. No drop is ever taken away
/// to make room.
pub(crate) const MAX_DROPS: usize = 5_000;

/// The shader's grid: a cell this wide, at most this many across and down.
pub(crate) const CELL: f32 = 0.04;
pub(crate) const CELLS_W: u32 = 128;
pub(crate) const CELLS_H: u32 = 32;
/// The drop texture's width; drop `i` is at (`i % DROPS_W`, `i / DROPS_W`).
pub(crate) const DROPS_W: u32 = 64;
/// The wiped map's size, stretched over the window.
pub(crate) const WIPED_W: u32 = 512;
pub(crate) const WIPED_H: u32 = 256;

#[derive(Clone, Copy, Debug)]
struct Drop {
    p: [f32; 2],
    r: f32,
    vy: f32,
    vx: f32,
    sliding: bool,
    /// Distance slid since the last bead.
    since_bead: f32,
    /// Where it started sliding, for the tests.
    from_y: f32,
    /// Which drop this is, for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    id: u32,
}

impl Drop {
    fn water(&self) -> f32 {
        self.r * self.r * self.r
    }
}

/// What a step hands the shader: the drops sorted by cell, the cells' first drop and count, and
/// the wiped map.
pub(crate) struct Upload {
    /// Per drop: x, y, radius, how far it stretches above (0 at rest).
    pub drops: Vec<[f32; 4]>,
    /// Per cell, row by row over [`CELLS_W`]: first drop, count.
    pub cells: Vec<[f32; 4]>,
    pub wiped: Vec<u8>,
}

/// Why a drop left the glass (the tests read it).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gone {
    /// Into another drop.
    Merged,
    /// Off the bottom, or off the side when the window grew narrower.
    OffTheEdge,
}

/// What became of the drops that slid, for the tests: how many reached the bottom.
#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct Fates {
    pub reached: u32,
}

pub(crate) struct Rain {
    drops: Vec<Drop>,
    wiped: Vec<f32>,
    aspect: f32,
    last: Option<f32>,
    seed: u64,
    landing_owed: f32,
    /// How hard it rains, 0 to 1.
    amount: f32,
    next_id: u32,
    pub(crate) fates: Fates,
    /// Every drop that left the glass, and why (kept only in the tests).
    #[cfg(test)]
    gone: Vec<(u32, Gone)>,
}

impl Rain {
    /// A window `aspect` wide, already rained on for a while at `amount` (0 to 1).
    pub(crate) fn new(aspect: f32, seed: u64, amount: f32) -> Self {
        let mut rain = Rain {
            drops: Vec::new(),
            wiped: vec![0.0; (WIPED_W * WIPED_H) as usize],
            aspect: aspect.clamp(0.25, 4.0),
            last: None,
            seed: seed | 1,
            landing_owed: 0.0,
            amount: amount.clamp(0.0, 1.0),
            next_id: 0,
            fates: Fates::default(),
            #[cfg(test)]
            gone: Vec::new(),
        };
        for _ in 0..600 {
            rain.step(0.05);
        }
        rain.fates = Fates::default();
        #[cfg(test)]
        rain.gone.clear();
        rain
    }

    fn new_drop(&mut self, p: [f32; 2], r: f32) -> Drop {
        self.next_id = self.next_id.wrapping_add(1);
        Drop { p, r, vy: 0.0, vx: 0.0, sliding: false, since_bead: 0.0, from_y: p[1], id: self.next_id }
    }

    /// Takes away the drops `keep` says no to, noting why for the tests.
    fn remove(&mut self, keep: impl Fn(&Drop) -> bool, why: impl Fn(&Drop) -> Gone) {
        #[cfg(test)]
        for d in self.drops.iter().filter(|d| !keep(d)) {
            self.gone.push((d.id, why(d)));
        }
        #[cfg(not(test))]
        let _ = &why;
        self.drops.retain(|d| keep(d));
    }

    fn random(&mut self) -> f32 {
        // xorshift64*: plenty for raindrops.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// How hard it rains from now on, 0 to 1.
    pub(crate) fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    /// New drops a second over a window as wide as it is tall.
    fn landing_rate(&self) -> f32 {
        if self.amount <= 0.0 {
            return 0.0;
        }
        DRIZZLE * (DOWNPOUR / DRIZZLE).powf(self.amount)
    }

    /// Moves on to `time` seconds (the rain's clock): the window's `aspect` may have changed.
    pub(crate) fn advance(&mut self, time: f32, aspect: f32) {
        let aspect = aspect.clamp(0.25, 4.0);
        if (aspect - self.aspect).abs() > 1e-3 {
            // A wider or narrower window: the drops keep their place from the left.
            self.aspect = aspect;
            self.remove(|d| d.p[0] - d.r < aspect + 0.02, |_| Gone::OffTheEdge);
        }
        // Back to the start each hour, or a long pause: a usual step.
        let dt = match self.last {
            Some(last) if time > last => (time - last).min(0.1),
            _ => 1.0 / 30.0,
        };
        self.last = Some(time);
        self.step(dt);
    }

    fn land(&mut self, dt: f32) {
        self.landing_owed += self.landing_rate() * self.aspect * dt;
        while self.landing_owed >= 1.0 {
            self.landing_owed -= 1.0;
            let (x, y, size, big) = (self.random() * self.aspect, self.random(), self.random(), self.random());
            // Mostly fine spray, now and then a fat drop.
            let r = if big < 0.035 { 0.0065 + 0.004 * size } else { 0.0014 + 0.0048 * size * size };
            if self.drops.len() < MAX_DROPS {
                let drop = self.new_drop([x, y], r);
                self.drops.push(drop);
            } else if let Some(d) = self.drops.iter_mut().min_by(|a, b| {
                let far = |d: &Drop| (d.p[0] - x).powi(2) + (d.p[1] - y).powi(2);
                far(a).total_cmp(&far(b))
            }) {
                // A full window: the rain joins the drop nearest where it lands, so drops still
                // grow, slide and clear the glass.
                d.r = (d.water() + r * r * r).cbrt().min(R_MAX);
            }
        }
        self.landing_owed = self.landing_owed.min(4.0);
    }

    fn step(&mut self, dt: f32) {
        self.land(dt);
        let mut beads = Vec::new();
        let mut room = MAX_DROPS.saturating_sub(self.drops.len());
        for i in 0..self.drops.len() {
            let wobble = self.random() - 0.5;
            let d = &mut self.drops[i];
            if !d.sliding {
                if d.r >= R_STICK {
                    d.sliding = true;
                    d.from_y = d.p[1];
                }
                continue;
            }
            // Heavier, faster: the pull grows with the water, the glass holds back about the same.
            let target = (0.13 + 32.0 * (d.r - R_STOP).max(0.0)) * SLIDE;
            d.vy += (target - d.vy) * (dt * 3.0).min(1.0);
            d.vx = (d.vx + wobble * 0.9 * dt) * (1.0 - (dt * 4.0).min(1.0));
            d.vx = d.vx.clamp(-0.12 * d.vy, 0.12 * d.vy);
            let was = d.p;
            d.p[1] += d.vy * dt;
            d.p[0] += d.vx * dt;
            d.since_bead += d.vy * dt;
            stamp(&mut self.wiped, self.aspect, was, d.p, d.r * 0.7);
            if d.since_bead >= BEAD_EVERY {
                d.since_bead -= BEAD_EVERY;
                // More often than not a bead stays behind, the size of fine spray.
                let size = 0.0016 + 0.0014 * fract(d.p[1] * 917.0 + d.p[0] * 311.0);
                if room > 0 && fract(d.p[1] * 613.0 + d.r * 7919.0) < 0.6 {
                    room -= 1;
                    beads.push(([was[0], was[1] - d.r * 0.8], size));
                }
            }
        }
        for (p, r) in beads {
            let bead = self.new_drop(p, r);
            self.drops.push(bead);
        }
        self.merge();
        self.fates.reached += self.drops.iter().filter(|d| d.p[1] - d.r >= 1.02).count() as u32;
        // Drops leave only by merging into a bigger one (above) or running off the bottom: a drop
        // at rest stays, however small.
        self.remove(|d| d.p[1] - d.r < 1.02, |_| Gone::OffTheEdge);
        let fade = dt / REFOG;
        self.wiped.iter_mut().for_each(|w| *w = (*w - fade).max(0.0));
    }

    /// Drops that touch become one, with both's water, where the heavier one was; one that slid
    /// keeps sliding, faster.
    fn merge(&mut self) {
        const GRID: f32 = 0.02;
        let cols = (self.aspect / GRID).ceil() as i32 + 1;
        let rows = (1.1 / GRID).ceil() as i32 + 1;
        let cell = |p: [f32; 2]| ((p[0] / GRID).floor() as i32).clamp(0, cols - 1) + ((p[1] / GRID).floor() as i32).clamp(0, rows - 1) * cols;
        let mut heads = vec![usize::MAX; (cols * rows) as usize];
        let mut next = vec![usize::MAX; self.drops.len()];
        for (i, d) in self.drops.iter().enumerate() {
            let c = cell(d.p) as usize;
            next[i] = heads[c];
            heads[c] = i;
        }
        let mut gone = vec![false; self.drops.len()];
        for i in 0..self.drops.len() {
            if gone[i] {
                continue;
            }
            let (cx, cy) = ((self.drops[i].p[0] / GRID).floor() as i32, (self.drops[i].p[1] / GRID).floor() as i32);
            for y in cy - 1..=cy + 1 {
                for x in cx - 1..=cx + 1 {
                    if x < 0 || y < 0 || x >= cols || y >= rows {
                        continue;
                    }
                    let mut j = heads[(x + y * cols) as usize];
                    while j != usize::MAX {
                        if j != i && !gone[j] {
                            let (a, b) = (self.drops[i], self.drops[j]);
                            let gap = ((a.p[0] - b.p[0]).powi(2) + (a.p[1] - b.p[1]).powi(2)).sqrt();
                            if gap < (a.r + b.r) * 0.85 {
                                self.drops[i] = joined(a, b);
                                gone[j] = true;
                                // The joined drop goes on as the heavier of the two.
                                #[cfg(test)]
                                self.gone.push((if self.drops[i].id == a.id { b.id } else { a.id }, Gone::Merged));
                            }
                        }
                        j = next[j];
                    }
                }
            }
        }
        let mut k = 0;
        self.drops.retain(|_| {
            k += 1;
            !gone[k - 1]
        });
    }

    /// The drops and the wiped map for the shader.
    pub(crate) fn upload(&self) -> Upload {
        let cols = ((self.aspect / CELL).ceil() as u32).clamp(1, CELLS_W);
        let cell = |d: &Drop| {
            let x = ((d.p[0] / CELL).floor().max(0.0) as u32).min(cols - 1);
            let y = ((d.p[1] / CELL).floor().max(0.0) as u32).min(CELLS_H - 1);
            (x + y * CELLS_W) as usize
        };
        let mut counts = vec![0u32; (CELLS_W * CELLS_H) as usize];
        for d in &self.drops {
            counts[cell(d)] += 1;
        }
        let mut starts = vec![0u32; counts.len()];
        let mut total = 0;
        for (s, c) in starts.iter_mut().zip(&counts) {
            *s = total;
            total += c;
        }
        let mut drops = vec![[0.0; 4]; self.drops.len()];
        let mut fill = starts.clone();
        for d in &self.drops {
            let c = cell(d);
            // A sliding drop is drawn longer above, more the faster it goes.
            let stretch = if d.sliding { (d.vy * 2.2).min(0.9) } else { 0.0 };
            drops[fill[c] as usize] = [d.p[0], d.p[1], d.r, stretch];
            fill[c] += 1;
        }
        let cells = starts.iter().zip(&counts).map(|(&s, &c)| [s as f32, c as f32, 0.0, 0.0]).collect();
        let wiped = self.wiped.iter().map(|w| (w.clamp(0.0, 1.0) * 255.0) as u8).collect();
        Upload { drops, cells, wiped }
    }

    #[cfg(test)]
    fn sliding(&self) -> usize {
        self.drops.iter().filter(|d| d.sliding).count()
    }
}

fn fract(v: f32) -> f32 {
    v - v.floor()
}

/// Two drops as one: all the water, at the heavier one's place (nudged towards the other), sliding
/// if either slid or it is now heavy enough, a little faster than the faster one.
fn joined(a: Drop, b: Drop) -> Drop {
    let (big, small) = if a.water() >= b.water() { (a, b) } else { (b, a) };
    let water = a.water() + b.water();
    let share = small.water() / water;
    let mut d = big;
    d.r = water.cbrt().min(R_MAX);
    d.p = [big.p[0] + (small.p[0] - big.p[0]) * share, big.p[1] + (small.p[1] - big.p[1]) * share];
    let was_sliding = a.sliding || b.sliding;
    d.sliding = was_sliding || d.r >= R_STICK;
    if d.sliding {
        d.vy = a.vy.max(b.vy) + 0.06;
        d.vx = if a.sliding { a.vx } else { b.vx };
        d.from_y = if was_sliding {
            if a.sliding && b.sliding {
                a.from_y.min(b.from_y)
            } else if a.sliding {
                a.from_y
            } else {
                b.from_y
            }
        } else {
            d.p[1]
        };
    }
    d
}

/// Wipes the glass along a drop's path from `a` to `b`, `half` wide each side.
fn stamp(wiped: &mut [f32], aspect: f32, a: [f32; 2], b: [f32; 2], half: f32) {
    let (sx, sy) = (WIPED_W as f32 / aspect, WIPED_H as f32);
    let steps = (((b[1] - a[1]) * sy).abs().max(((b[0] - a[0]) * sx).abs()).ceil() as i32).max(1);
    let reach_x = (half * sx).max(0.5);
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let (x, y) = ((a[0] + (b[0] - a[0]) * t) * sx, (a[1] + (b[1] - a[1]) * t) * sy);
        let row = y.floor() as i32;
        if row < 0 || row >= WIPED_H as i32 {
            continue;
        }
        let (lo, hi) = ((x - reach_x).floor() as i32, (x + reach_x).ceil() as i32);
        for col in lo.max(0)..=hi.min(WIPED_W as i32 - 1) {
            let edge = 1.0 - ((col as f32 + 0.5 - x).abs() / reach_x).min(1.0);
            let w = &mut wiped[(row as u32 * WIPED_W + col as u32) as usize];
            *w = w.max((edge * 1.6).min(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rain: &mut Rain, seconds: f32) {
        let start = rain.last.unwrap_or(0.0);
        let mut t = 0.0;
        while t < seconds {
            t += 1.0 / 30.0;
            rain.advance(start + t, rain.aspect);
        }
    }

    /// Small drops land and stay; some grow by merging until they slide.
    #[test]
    fn the_window_gets_wet_and_some_drops_slide() {
        let mut rain = Rain::new(1.6, 7, AMOUNT_AT_LANDING);
        let resting = rain.drops.iter().filter(|d| !d.sliding).count();
        assert!(resting > 300, "spray at rest: {resting}");
        let mut slid = 0;
        for _ in 0..60 {
            run(&mut rain, 0.5);
            slid = slid.max(rain.sliding());
        }
        assert!(slid >= 3, "drops slide: {slid}");
        assert!(rain.drops.len() <= MAX_DROPS);
    }

    /// Two drops that meet are one with both's water, and a sliding one goes faster.
    #[test]
    fn drops_that_meet_become_one_and_speed_up() {
        let at_rest = Drop { p: [0.5, 0.5], r: 0.005, vy: 0.0, vx: 0.0, sliding: false, since_bead: 0.0, from_y: 0.5, id: 0 };
        let sliding = Drop { p: [0.5, 0.494], r: 0.008, vy: 0.2, sliding: true, ..at_rest };
        let one = joined(at_rest, sliding);
        assert!((one.water() - at_rest.water() - sliding.water()).abs() < 1e-12);
        assert!(one.sliding && one.vy > sliding.vy);
        let two_small = joined(at_rest, Drop { p: [0.503, 0.5], ..at_rest });
        assert!(two_small.sliding == (two_small.r >= R_STICK));
    }

    /// Heavier drops slide faster.
    #[test]
    fn heavier_is_faster() {
        let mut rain = Rain::new(1.0, 3, AMOUNT_AT_LANDING);
        rain.drops.clear();
        for (x, r) in [(0.3, 0.008), (0.7, 0.014)] {
            rain.drops.push(Drop { p: [x, 0.05], r, vy: 0.0, vx: 0.0, sliding: true, since_bead: 0.0, from_y: 0.05, id: 0 });
        }
        for _ in 0..15 {
            rain.step(1.0 / 30.0);
        }
        let speed = |x: f32| rain.drops.iter().filter(|d| d.sliding && (d.p[0] - x).abs() < 0.1).map(|d| d.vy).fold(0.0, f32::max);
        assert!(speed(0.7) > speed(0.3) * 1.3, "{} vs {}", speed(0.7), speed(0.3));
    }

    /// Sliding drops run to the bottom: none runs dry on the way.
    #[test]
    fn sliding_drops_reach_the_bottom() {
        let mut rain = Rain::new(1.6, 11, AMOUNT_AT_LANDING);
        run(&mut rain, 120.0);
        let slid_off = rain.fates.reached;
        assert!(slid_off >= 10, "drops reach the bottom: {:?}", rain.fates);
        assert!(rain.drops.iter().all(|d| d.sliding || d.r < R_STICK), "a drop that slides keeps sliding");
    }

    /// The trail a drop wipes fogs over again, slowly.
    #[test]
    fn a_wiped_trail_fogs_over() {
        let mut wiped = vec![0.0; (WIPED_W * WIPED_H) as usize];
        stamp(&mut wiped, 1.0, [0.5, 0.2], [0.5, 0.3], 0.006);
        let at = (0.25 * WIPED_H as f32) as usize * WIPED_W as usize + WIPED_W as usize / 2;
        assert!(wiped[at] > 0.9);
        let mut rain = Rain::new(1.0, 5, AMOUNT_AT_LANDING);
        rain.drops.clear();
        rain.wiped = wiped;
        for _ in 0..30 {
            rain.step(0.1);
        }
        assert!(rain.wiped[at] < 0.9 && rain.wiped[at] > 0.0, "{}", rain.wiped[at]);
    }

    /// The drops go to the shader sorted by cell, each cell pointing at its own.
    #[test]
    fn the_upload_finds_each_drop_in_its_cell() {
        let rain = Rain::new(1.6, 9, AMOUNT_AT_LANDING);
        let up = rain.upload();
        assert_eq!(up.drops.len(), rain.drops.len());
        for (c, cell) in up.cells.iter().enumerate() {
            let (cx, cy) = ((c as u32 % CELLS_W) as f32, (c as u32 / CELLS_W) as f32);
            for d in &up.drops[cell[0] as usize..(cell[0] + cell[1]) as usize] {
                assert!((d[0] / CELL).floor().max(0.0) == cx || cx as u32 == ((1.6 / CELL).ceil() as u32 - 1), "x {d:?} in {cx}");
                assert!((d[1] / CELL).floor().clamp(0.0, (CELLS_H - 1) as f32) == cy, "y {d:?} in {cy}");
            }
        }
    }

    /// No drop just vanishes: one leaves the glass only by merging into another or running off the
    /// bottom; none dries away. A full window takes no new drops instead of taking old ones away.
    #[test]
    fn no_drop_vanishes_without_a_reason() {
        for amount in [AMOUNT_AT_LANDING, DEFAULT_AMOUNT, 1.0] {
            let mut rain = Rain::new(1.6, 21, amount);
            let dt = 1.0 / 30.0;
            let mut t = 0.0;
            let (mut merged, mut off) = (0, 0);
            for _ in 0..1800 {
                let before: std::collections::HashMap<u32, Drop> = rain.drops.iter().map(|d| (d.id, *d)).collect();
                rain.gone.clear();
                t += dt;
                rain.advance(t, 1.6);
                assert!(rain.drops.len() <= MAX_DROPS, "{} drops", rain.drops.len());
                let now: std::collections::HashSet<u32> = rain.drops.iter().map(|d| d.id).collect();
                for (id, was) in &before {
                    if now.contains(id) {
                        continue;
                    }
                    let why = rain.gone.iter().find(|(g, _)| g == id).map(|&(_, why)| why);
                    match why {
                        Some(Gone::Merged) => merged += 1,
                        Some(Gone::OffTheEdge) => {
                            assert!(was.p[1] - was.r + was.vy * dt * 1.5 >= 0.98, "ran off from {was:?}");
                            off += 1;
                        }
                        None => panic!("drop {id} vanished: {was:?}"),
                    }
                }
            }
            assert!(merged > 0 && off > 0, "amount {amount}: merged {merged}, off {off}");
        }
    }

    /// A full window doesn't freeze: the rain joins the drops there, and they still run off.
    #[test]
    fn a_full_window_keeps_running() {
        let mut rain = Rain::new(1.6, 5, 1.0);
        let dt = 1.0 / 30.0;
        let mut t = 0.0;
        while rain.drops.len() < MAX_DROPS {
            let (x, y) = (rain.random() * 1.6, rain.random());
            let spray = rain.new_drop([x, y], 0.0015);
            rain.drops.push(spray);
        }
        let before = rain.fates.reached;
        for _ in 0..900 {
            t += dt;
            rain.advance(t, 1.6);
        }
        assert!(rain.fates.reached > before + 10, "full, drops still run off: {}", rain.fates.reached - before);
    }

    /// A drop keeps one size for as long as it lives: it changes only in a merge, in one step, and
    /// never shrinks; a new drop is drawn at its size at once.
    #[test]
    fn a_drops_radius_stays_constant_between_merges() {
        let mut rain = Rain::new(1.6, 4, 0.5);
        let d = rain.new_drop([0.5, 0.5], 0.004);
        rain.drops.push(d);
        assert!(rain.upload().drops.iter().any(|u| u[2] == 0.004), "a new drop is drawn at its size at once");
        let (dt, mut t, mut slid) = (1.0 / 30.0, 0.0, 0);
        for _ in 0..1800 {
            let before: std::collections::HashMap<u32, f32> = rain.drops.iter().map(|d| (d.id, d.r)).collect();
            rain.gone.clear();
            t += dt;
            rain.advance(t, 1.6);
            let merged = rain.gone.iter().any(|&(_, why)| why == Gone::Merged);
            for d in &rain.drops {
                let Some(&was) = before.get(&d.id) else { continue };
                slid += d.sliding as u32;
                assert!(d.r >= was, "drop {} shrank from {was} to {}", d.id, d.r);
                assert!(d.r == was || merged, "drop {} changed size with no merge", d.id);
            }
        }
        assert!(slid > 100, "drops slid: {slid}");
    }

    #[test]
    fn more_rain_puts_more_drops_on_the_glass() {
        let light = Rain::new(1.6, 13, 0.3).drops.len();
        let default = Rain::new(1.6, 13, DEFAULT_AMOUNT).drops.len();
        let none = Rain::new(1.6, 13, 0.0).drops.len();
        assert!(none == 0 && default as f32 > light as f32 * 1.5, "none {none}, light {light}, default {default}");
    }

    /// The amount is the rain's strength: from a drizzle to a downpour the landing rate grows
    /// at least sixty times, and every step up is more rain.
    #[test]
    fn the_amount_sets_how_many_drops_land() {
        let rate = |a: f32| Rain::new(1.6, 3, a).landing_rate();
        let mut last = 0.0;
        for step in 1..=10 {
            let r = rate(step as f32 / 10.0);
            assert!(r > last, "{step}: {r} after {last}");
            last = r;
        }
        assert!(rate(1.0) > rate(0.1) * 60.0, "{} vs {}", rate(1.0), rate(0.1));
        let (drizzle, downpour) = (Rain::new(1.6, 3, 0.2).drops.len(), Rain::new(1.6, 3, 1.0).drops.len());
        assert!(downpour > drizzle * 3, "the glass is wetter in a downpour: {drizzle} vs {downpour}");
        let mut rain = Rain::new(1.6, 3, 0.1);
        let few = rain.drops.len();
        rain.set_amount(1.0);
        run(&mut rain, 2.0);
        assert!(rain.drops.len() > few * 2, "the new amount reaches the sim: {few} -> {}", rain.drops.len());
    }
}
