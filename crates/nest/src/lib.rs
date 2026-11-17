//! Bounded, deterministic stock packing. Coordinates and lengths are millimetres.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// Rotation allowed by the manufacturing grain, relative to stock length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Free,
    Fixed,
    QuarterTurn,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub length: f64,
    pub width: f64,
    pub rotation: Rotation,
}

#[derive(Clone, Debug)]
pub struct Stock {
    pub length: f64,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub length: f64,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub part: usize,
    pub rect: Rect,
    pub rotated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub stock: usize,
    pub placements: Vec<Placement>,
    /// Kerf bands in cutting order. Each spans its current guillotine rectangle.
    pub cuts: Vec<Rect>,
    pub offcuts: Vec<Rect>,
    pub yield_ratio: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub sheets: Vec<Sheet>,
    pub unplaced: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Linear packing consumes full-width crosscuts only.
    pub linear: bool,
    pub kerf: f64,
    /// Trim at both ends, and at both sides for sheets.
    pub trim: f64,
}

/// Best-area-fit decreasing, with deterministic ties. Every placement splits one free
/// rectangle with two edge-to-edge cuts. A crosscut is charged after each linear piece
/// unless it ends exactly at the usable stock edge. Stock choices are unlimited.
pub fn pack(parts: &[Part], stocks: &[Stock], settings: Settings) -> Result<Plan, String> {
    let Settings { linear, kerf, trim } = settings;
    let dimension = |v: f64| v.is_finite() && v > 0.0 && v <= 1e7;
    if !(kerf > 0.0 && kerf <= 1e4) || !trim.is_finite() || !(0.0..=1e6).contains(&trim) {
        return Err("kerf must be positive and at most 10000 mm and trim 0..1000000 mm, both finite".into());
    }
    if parts.len() > 2048 || stocks.len() > 128 {
        return Err("nesting is limited to 2048 parts and 128 stock choices".into());
    }
    if parts.iter().any(|p| !dimension(p.length) || !dimension(p.width)) || stocks.iter().any(|s| !dimension(s.length) || !dimension(s.width)) {
        return Err("part and stock dimensions must be finite, positive and at most 10000000 mm".into());
    }
    let mut order: Vec<_> = (0..parts.len()).collect();
    order.sort_by(|&a, &b| {
        let a = &parts[a];
        let b = &parts[b];
        let size = |p: &Part| if linear { p.length } else { p.length * p.width };
        size(b).total_cmp(&size(a))
    });
    let mut plan = Plan::default();
    let mut work = 0usize;
    for part in order {
        let p = parts.get(part).ok_or("invalid part index")?;
        // Existing offcuts first, then the smallest stock that can hold the part.
        let mut best: Option<(f64, usize, usize, bool)> = None;
        for (sheet, s) in plan.sheets.iter().enumerate() {
            for (free, r) in s.offcuts.iter().enumerate() {
                for rotated in [false, true] {
                    work += 1;
                    if work > 8_000_000 {
                        return Err("nesting work limit exceeded; split the job".into());
                    }
                    let permitted = match p.rotation {
                        Rotation::Free => !linear || !rotated,
                        Rotation::Fixed => !rotated,
                        Rotation::QuarterTurn => rotated && !linear,
                    };
                    let (l, w) = if rotated { (p.width, p.length) } else { (p.length, p.width) };
                    if permitted && l <= r.length && (linear || w <= r.width) {
                        let score = if linear { r.length - l } else { r.length * r.width - l * w };
                        if best.as_ref().is_none_or(|b| score < b.0) {
                            best = Some((score, sheet, free, rotated));
                        }
                    }
                }
            }
        }
        if best.is_none() {
            let mut choice: Option<(f64, usize, bool)> = None;
            for (stock, s) in stocks.iter().enumerate() {
                for rotated in [false, true] {
                    let permitted = match p.rotation {
                        Rotation::Free => !linear || !rotated,
                        Rotation::Fixed => !rotated,
                        Rotation::QuarterTurn => rotated && !linear,
                    };
                    let (l, w) = if rotated { (p.width, p.length) } else { (p.length, p.width) };
                    let usable_width = if linear { s.width } else { s.width - 2.0 * trim };
                    if permitted && l <= s.length - 2.0 * trim && (linear || w <= usable_width) {
                        let score = if linear { s.length } else { s.length * s.width };
                        if choice.as_ref().is_none_or(|c| score < c.0) {
                            choice = Some((score, stock, rotated));
                        }
                    }
                }
            }
            if let Some((_, stock, rotated)) = choice {
                let s = stocks.get(stock).ok_or("invalid stock index")?;
                let sheet = plan.sheets.len();
                plan.sheets.push(Sheet {
                    stock,
                    placements: Vec::new(),
                    cuts: Vec::new(),
                    offcuts: vec![Rect {
                        x: trim,
                        y: if linear { 0.0 } else { trim },
                        length: s.length - 2.0 * trim,
                        width: if linear { s.width } else { s.width - 2.0 * trim },
                    }],
                    yield_ratio: 0.0,
                });
                best = Some((0.0, sheet, 0, rotated));
            }
        }
        let Some((_, sheet, free, rotated)) = best else {
            plan.unplaced.push(part);
            continue;
        };
        let s = plan.sheets.get_mut(sheet).ok_or("invalid sheet index")?;
        let r = *s.offcuts.get(free).ok_or("invalid free rectangle index")?;
        s.offcuts.remove(free);
        let (l, w) = if rotated { (p.width, p.length) } else { (p.length, p.width) };
        let w = if linear { r.width } else { w };
        s.placements.push(Placement { part, rect: Rect { x: r.x, y: r.y, length: l, width: w }, rotated });
        let dx = (r.length - l).min(kerf);
        let dy = (r.width - w).min(kerf);
        if r.length > l {
            s.cuts.push(Rect { x: r.x + l, y: r.y, length: dx, width: r.width });
        }
        if r.width > w {
            s.cuts.push(Rect { x: r.x, y: r.y + w, length: l, width: dy });
        }
        if r.length > l + dx {
            s.offcuts.push(Rect { x: r.x + l + dx, y: r.y, length: r.length - l - dx, width: r.width });
        }
        if r.width > w + dy {
            s.offcuts.push(Rect { x: r.x, y: r.y + w + dy, length: l, width: r.width - w - dy });
        }
    }
    for sheet in &mut plan.sheets {
        let stock = stocks.get(sheet.stock).ok_or("invalid stock index")?;
        sheet.yield_ratio = sheet.placements.iter().map(|p| p.rect.length * p.rect.width).sum::<f64>() / (stock.length * stock.width);
    }
    plan.unplaced.sort_unstable();
    Ok(plan)
}

#[cfg(test)]
#[path = "nest_tests.rs"]
mod tests;
