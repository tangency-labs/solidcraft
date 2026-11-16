//! One profile swept along two rails: the kernel's
//! [`solidcore::sweep::loft_rails::sweep_two_rails`] behind ops. An open profile, its ends on the
//! rails' starts, placed at stations of the rails' common parameter with its ends on the rails,
//! skinned and certified within the caller's tolerance. Ops converts and checks shapes and maps
//! the kernel's refusals as the railed loft's ([`super::railed`]); it computes no geometry.

use solidcore::sweep::loft_rails::{sweep_two_rails as kernel_sweep, RailLoftOptions};
use solidcore::sweep::stations::RailFit as KernelRailFit;

use super::railed::{refused, RailFit};
use super::{parse, Curve};
use crate::body::{finish, Body, Expect};
use crate::session::run;
use crate::{Error, Result};

/// The sheet `profile` (curves end to end, open) sweeps along `rails`, its start on the start
/// of one rail and its end on the start of the other: at each station of the rails' common
/// parameter its ends lie on the rails, the chord between them sizing it (`fit`: evenly, or
/// only across), each rail certified within `tolerance` (mm, finite and positive) of the
/// sheet's guided edge. Returns the sheet and the largest bound proved over both rails.
///
/// # Errors
/// [`Error::Argument`] for a curve list refused; [`Error::RailPastTolerance`] where the
/// certificate is not met within the kernel's default wires; the kernel's other refusals by
/// their status (a closed profile, ends not on the rails' starts, rails starting the opposite
/// ways or meeting, a tolerance not finite and positive); [`Error::Empty`].
pub fn sweep_two_rails(
    profile: &[Curve],
    rails: [&Curve; 2],
    fit: RailFit,
    tolerance: f64,
) -> Result<(Body, f64)> {
    let n = profile.len();
    let all: Vec<Curve> = profile
        .iter()
        .cloned()
        .chain(rails.iter().map(|c| (*c).clone()))
        .collect();
    let (brep, bound) = run("sweep_two_rails", |k| {
        let made = match parse(k, &all) {
            Ok(m) => m,
            Err(e) => return Ok(Err(e)),
        };
        let (Some(curves), Some([r0, r1])) = (made.curves.get(..n), made.curves.get(n..)) else {
            return Ok(Err(Error::Argument));
        };
        let refs: Vec<_> = curves.iter().collect();
        let mut opts = RailLoftOptions::new(tolerance);
        opts.fit = match fit {
            RailFit::Scale => KernelRailFit::Scale,
            RailFit::Stretch => KernelRailFit::Stretch,
        };
        match kernel_sweep(k, &refs, [r0, r1], &opts) {
            Ok(swept) => {
                let bound = swept.bounds.iter().copied().fold(0.0, f64::max);
                Ok(finish(k, swept.skin.brep, Expect::FACES)?.map(|b| (b, bound)))
            }
            Err(e) => Ok(Err(refused(e))),
        }
    })??;
    Ok((Body::new(brep), bound))
}
