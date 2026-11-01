//! PROVISIONAL original drafting constructions. These are educated visual approximations,
//! not dimensions verified against the requested current ISO/ASME construction tables.
//!
//! Replacement map: `Pen::characteristic` groups each characteristic's h-normalized paths;
//! constants below group circled modifiers, ASME diameter and text metrics; `Pen::letter`
//! groups the original alphabet. Update drawing coordinates AND metrics together, then run
//! containment, scaling and paper validation tests and inspect actual output. ISO and ASME
//! share characteristic/letter guesses today; split them by `self.standard` when verified
//! evidence requires it. Verified ISO diameter construction stays in the parent module.
//! No standard figures, fonts or other product's assets/code are copied here.

use std::f64::consts::{PI, TAU};

use crate::{Loop2, Region2, Vec2, paper::Item};

use super::super::{Characteristic, Invalid, Result};
use super::Pen;

// All values are in units of the caller's text height h. These are source-level
// adjustment points, not a user-facing style or persisted standards definition.
// The alphabet uses centre-line height h; painted height/spacing still need review
// against ISO 3098 (including its h1=h-d and a1=a+d conventions) and ASME Y14.2.
pub(super) const CHARACTER_GAP_H: f64 = 0.2;
const LETTER_WIDTH_H: f64 = 0.6;
const DECIMAL_WIDTH_H: f64 = 0.2;
const MODIFIER_RADIUS_H: f64 = 0.6;
const MODIFIER_WIDTH_H: f64 = 2.0 * MODIFIER_RADIUS_H;
const MODIFIER_CENTER_H: (f64, f64) = (0.6, 0.5);
const MODIFIER_LETTER_ORIGIN_H: (f64, f64) = (0.405, 0.175);
const MODIFIER_LETTER_HEIGHT_H: f64 = 0.65;
const ASME_DIAMETER_WIDTH_H: f64 = 0.9;
const ASME_DIAMETER_CENTER_H: (f64, f64) = (0.45, 0.5);
const ASME_DIAMETER_RADIUS_H: f64 = 0.4;
const ASME_DIAMETER_SLASH_H: [(f64, f64); 2] = [(0.05, 0.0), (0.85, 1.0)];

pub(super) fn glyph_width(c: char) -> Result<f64> {
    match c {
        'Ⓜ' | 'Ⓛ' | 'Ⓟ' => Ok(MODIFIER_WIDTH_H),
        '⌀' => Ok(ASME_DIAMETER_WIDTH_H),
        '.' => Ok(DECIMAL_WIDTH_H),
        '0'..='9' | 'A'..='Z' => Ok(LETTER_WIDTH_H),
        _ => Err(Invalid("unsupported character in the bounded symbol alphabet")),
    }
}

impl Pen {
    pub(super) fn provisional_glyph(&mut self, c: char, origin: Vec2) -> Result<()> {
        match c {
            'Ⓜ' | 'Ⓛ' | 'Ⓟ' => {
                let (x, y) = MODIFIER_CENTER_H;
                self.arc(origin + Vec2::new(x, y) * self.h, MODIFIER_RADIUS_H * self.h, 0.0, TAU);
                let letter = match c {
                    'Ⓜ' => 'M',
                    'Ⓛ' => 'L',
                    _ => 'P',
                };
                let (x, y) = MODIFIER_LETTER_ORIGIN_H;
                self.letter(letter, origin + Vec2::new(x, y) * self.h, MODIFIER_LETTER_HEIGHT_H * self.h)?;
            }
            '⌀' => {
                let (x, y) = ASME_DIAMETER_CENTER_H;
                self.arc(origin + Vec2::new(x, y) * self.h, ASME_DIAMETER_RADIUS_H * self.h, 0.0, TAU);
                self.path(&ASME_DIAMETER_SLASH_H, origin, self.h, false);
            }
            _ => self.letter(c, origin, self.h)?,
        }
        Ok(())
    }

    // Shared visual approximations for now; no normative characteristic grids verified.
    pub(super) fn characteristic(&mut self, characteristic: Characteristic, center: Vec2) {
        use Characteristic::*;
        let h = self.h;
        let at = |x, y| center + Vec2::new(x, y) * h;
        match characteristic {
            Straightness => self.line(at(-0.7, 0.0), at(0.7, 0.0)),
            Flatness => self.path(&[(-0.7, -0.35), (0.3, -0.35), (0.7, 0.35), (-0.3, 0.35)], center, h, true),
            Roundness => self.arc(center, h / 2.0, 0.0, TAU),
            Cylindricity => {
                self.arc(center, 0.4 * h, 0.0, TAU);
                self.line(at(-0.75, -0.5), at(-0.45, 0.5));
                self.line(at(0.45, -0.5), at(0.75, 0.5));
            }
            LineProfile | SurfaceProfile => {
                self.arc(at(0.0, -0.35), 0.7 * h, 0.0, PI);
                if characteristic == SurfaceProfile {
                    self.line(at(-0.7, -0.35), at(0.7, -0.35));
                }
            }
            Parallelism => {
                self.line(at(-0.5, -0.5), at(0.0, 0.5));
                self.line(at(0.0, -0.5), at(0.5, 0.5));
            }
            Perpendicularity => {
                self.line(at(-0.6, -0.5), at(0.6, -0.5));
                self.line(at(0.0, -0.5), at(0.0, 0.5));
            }
            Angularity => self.path(&[(0.65, -0.5), (-0.65, -0.5), (0.05, 0.5)], center, h, false),
            Position => {
                self.arc(center, 0.4 * h, 0.0, TAU);
                self.line(at(-0.65, 0.0), at(0.65, 0.0));
                self.line(at(0.0, -0.65), at(0.0, 0.65));
            }
            Concentricity => {
                self.arc(center, 0.5 * h, 0.0, TAU);
                self.arc(center, 0.25 * h, 0.0, TAU);
            }
            Symmetry => {
                self.line(at(-0.65, 0.0), at(0.65, 0.0));
                for y in [-0.35, 0.35] {
                    self.line(at(-0.45, y), at(0.45, y));
                }
            }
            CircularRunout | TotalRunout => {
                let offsets: &[f64] = if characteristic == TotalRunout { &[-0.2, 0.2] } else { &[0.0] };
                for &x in offsets {
                    self.line(at(x - 0.35, -0.5), at(x + 0.35, 0.5));
                    self.items.push(Item::Fill {
                        region: Region2 { outer: Loop2::polygon(&[at(x - 0.02, 0.35), at(x + 0.35, 0.5), at(x + 0.33, 0.1)]), holes: Vec::new() },
                    });
                }
                if characteristic == TotalRunout {
                    self.line(at(-0.55, -0.5), at(-0.15, -0.5));
                }
            }
        }
    }

    // Original simple single-stroke construction for the bounded numeric/datum alphabet.
    // This is not extracted from a font, standards figure or another application's artwork.
    fn letter(&mut self, c: char, origin: Vec2, h: f64) -> Result<()> {
        let paths: &[&[(f64, f64)]] = match c {
            '0' => &[&[(0.15, 0.0), (0.0, 0.2), (0.0, 0.8), (0.15, 1.0), (0.45, 1.0), (0.6, 0.8), (0.6, 0.2), (0.45, 0.0), (0.15, 0.0)]],
            '1' => &[&[(0.1, 0.75), (0.3, 1.0), (0.3, 0.0)], &[(0.05, 0.0), (0.55, 0.0)]],
            '2' => &[&[(0.0, 0.8), (0.15, 1.0), (0.45, 1.0), (0.6, 0.8), (0.6, 0.65), (0.0, 0.0), (0.6, 0.0)]],
            '3' => &[&[(0.0, 1.0), (0.6, 1.0), (0.3, 0.55), (0.6, 0.35), (0.6, 0.15), (0.45, 0.0), (0.0, 0.0)]],
            '4' => &[&[(0.45, 0.0), (0.45, 1.0), (0.0, 0.3), (0.6, 0.3)]],
            '5' => &[&[(0.6, 1.0), (0.0, 1.0), (0.0, 0.55), (0.45, 0.55), (0.6, 0.4), (0.6, 0.15), (0.45, 0.0), (0.0, 0.0)]],
            '6' => {
                &[&[(0.6, 1.0), (0.15, 1.0), (0.0, 0.75), (0.0, 0.15), (0.15, 0.0), (0.45, 0.0), (0.6, 0.15), (0.6, 0.4), (0.45, 0.55), (0.0, 0.55)]]
            }
            '7' => &[&[(0.0, 1.0), (0.6, 1.0), (0.15, 0.0)]],
            '8' => &[&[
                (0.15, 0.5),
                (0.0, 0.7),
                (0.0, 0.85),
                (0.15, 1.0),
                (0.45, 1.0),
                (0.6, 0.85),
                (0.6, 0.7),
                (0.45, 0.5),
                (0.15, 0.5),
                (0.0, 0.3),
                (0.0, 0.15),
                (0.15, 0.0),
                (0.45, 0.0),
                (0.6, 0.15),
                (0.6, 0.3),
                (0.45, 0.5),
            ]],
            '9' => {
                &[&[(0.6, 0.45), (0.15, 0.45), (0.0, 0.6), (0.0, 0.85), (0.15, 1.0), (0.45, 1.0), (0.6, 0.85), (0.6, 0.25), (0.45, 0.0), (0.0, 0.0)]]
            }
            '.' => {
                self.arc(origin + Vec2::new(0.1, 0.05) * h, 0.05 * h, 0.0, TAU);
                return Ok(());
            }
            'A' => &[&[(0.0, 0.0), (0.3, 1.0), (0.6, 0.0)], &[(0.15, 0.5), (0.45, 0.5)]],
            'B' => &[&[(0.0, 0.0), (0.0, 1.0), (0.4, 1.0), (0.6, 0.8), (0.4, 0.5), (0.0, 0.5)], &[(0.4, 0.5), (0.6, 0.3), (0.4, 0.0), (0.0, 0.0)]],
            'C' => &[&[(0.6, 0.85), (0.45, 1.0), (0.15, 1.0), (0.0, 0.8), (0.0, 0.2), (0.15, 0.0), (0.45, 0.0), (0.6, 0.15)]],
            'D' => &[&[(0.0, 0.0), (0.0, 1.0), (0.3, 1.0), (0.6, 0.7), (0.6, 0.3), (0.3, 0.0), (0.0, 0.0)]],
            'E' => &[&[(0.6, 1.0), (0.0, 1.0), (0.0, 0.0), (0.6, 0.0)], &[(0.0, 0.5), (0.45, 0.5)]],
            'F' => &[&[(0.0, 0.0), (0.0, 1.0), (0.6, 1.0)], &[(0.0, 0.5), (0.45, 0.5)]],
            'G' => &[&[(0.6, 0.85), (0.45, 1.0), (0.15, 1.0), (0.0, 0.8), (0.0, 0.2), (0.15, 0.0), (0.6, 0.0), (0.6, 0.5), (0.35, 0.5)]],
            'H' => &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.6, 0.0), (0.6, 1.0)], &[(0.0, 0.5), (0.6, 0.5)]],
            'J' => &[&[(0.0, 0.2), (0.15, 0.0), (0.4, 0.0), (0.55, 0.2), (0.55, 1.0)]],
            'K' => &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.6, 1.0), (0.0, 0.45), (0.6, 0.0)]],
            'L' => &[&[(0.0, 1.0), (0.0, 0.0), (0.6, 0.0)]],
            'M' => &[&[(0.0, 0.0), (0.0, 1.0), (0.3, 0.45), (0.6, 1.0), (0.6, 0.0)]],
            'N' => &[&[(0.0, 0.0), (0.0, 1.0), (0.6, 0.0), (0.6, 1.0)]],
            'P' => &[&[(0.0, 0.0), (0.0, 1.0), (0.4, 1.0), (0.6, 0.8), (0.6, 0.65), (0.4, 0.5), (0.0, 0.5)]],
            'R' => &[&[(0.0, 0.0), (0.0, 1.0), (0.4, 1.0), (0.6, 0.8), (0.6, 0.65), (0.4, 0.5), (0.0, 0.5)], &[(0.3, 0.5), (0.6, 0.0)]],
            'S' => {
                &[&[(0.6, 0.85), (0.45, 1.0), (0.15, 1.0), (0.0, 0.8), (0.0, 0.65), (0.6, 0.35), (0.6, 0.2), (0.45, 0.0), (0.15, 0.0), (0.0, 0.15)]]
            }
            'T' => &[&[(0.0, 1.0), (0.6, 1.0)], &[(0.3, 1.0), (0.3, 0.0)]],
            'U' => &[&[(0.0, 1.0), (0.0, 0.2), (0.15, 0.0), (0.45, 0.0), (0.6, 0.2), (0.6, 1.0)]],
            'V' => &[&[(0.0, 1.0), (0.3, 0.0), (0.6, 1.0)]],
            'W' => &[&[(0.0, 1.0), (0.15, 0.0), (0.3, 0.55), (0.45, 0.0), (0.6, 1.0)]],
            'X' => &[&[(0.0, 0.0), (0.6, 1.0)], &[(0.0, 1.0), (0.6, 0.0)]],
            'Y' => &[&[(0.0, 1.0), (0.3, 0.5), (0.6, 1.0)], &[(0.3, 0.5), (0.3, 0.0)]],
            'Z' => &[&[(0.0, 1.0), (0.6, 1.0), (0.0, 0.0), (0.6, 0.0)]],
            _ => return Err(Invalid("unsupported character in the bounded symbol alphabet")),
        };
        for points in paths {
            self.path(points, origin, h, false);
        }
        Ok(())
    }
}
