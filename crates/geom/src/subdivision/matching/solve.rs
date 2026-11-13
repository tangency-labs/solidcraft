//! Map topology-qualified control responses to SolidCore Match constraints.
use super::*;
use solidcore_ops::form_match as kernel;

pub(super) struct Response {
    values: Vec<Vec<[f64; 6]>>,
    mirror: Option<kernel::MirrorColumns>,
}

fn mirror_columns(controls: &[usize], symmetry: &Symmetry) -> Result<kernel::MirrorColumns> {
    let partners = controls
        .iter()
        .map(|&id| {
            let partner = symmetry.partners.get(id).ok_or_else(|| bad("invalid Match mirror control"))?;
            controls.binary_search(partner).map_err(|_| bad("missing Match mirror control"))
        })
        .collect::<Result<Vec<_>>>()?;
    kernel::MirrorColumns::new(partners, symmetry.axis).map_err(|error| bad(&error.to_string()))
}

fn component(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}
pub(super) fn responses(
    cage: &Cage,
    order: usize,
    basis: &RootBasis,
    stations: &[Station],
    controls: &[usize],
    symmetry: Option<&super::Symmetry>,
) -> Result<Response> {
    let mut out = vec![vec![[0.0; 6]; controls.len()]; stations.len()];
    for (batch, chunk) in controls.chunks(3).enumerate() {
        let mut scalar = cage.clone();
        scalar.points.fill(Vec3::ZERO);
        for (i, &id) in chunk.iter().enumerate() {
            *scalar.points.get_mut(id).ok_or_else(|| bad("invalid Match response control"))? = match i {
                0 => Vec3::X,
                1 => Vec3::Y,
                _ => Vec3::Z,
            };
        }
        let values = jets(&scalar, order, basis, stations)?;
        for (row, jet) in out.iter_mut().zip(values) {
            for i in 0..chunk.len() {
                *row.get_mut(3 * batch + i).ok_or_else(|| bad("invalid Match response column"))? =
                    [jet.point, jet.du, jet.dv, jet.duu, jet.duv, jet.dvv].map(|v| component(v, i));
            }
        }
    }
    Ok(Response { values: out, mirror: symmetry.map(|s| mirror_columns(controls, s)).transpose()? })
}

fn jet(jet: &SurfaceJet) -> kernel::Jet {
    kernel::Jet { point: jet.point, du: jet.du, dv: jet.dv, duu: jet.duu, duv: jet.duv, dvv: jet.dvv }
}

pub(super) fn step(
    stations: &[Station],
    targets: &[Target],
    original: &[SurfaceJet],
    current: &[SurfaceJet],
    response: &Response,
    config: &Config,
    diagonal: f64,
) -> Result<Vec<Vec3>> {
    let samples = stations.iter().zip(targets).zip(original).zip(current).map(|(((station, target), original), current)| {
        let station =
            kernel::Station { across: station.across, differential: station.differential, direction: station.direction, vertex: station.vertex };
        let target = kernel::Target {
            corner: target.corner,
            point: target.point,
            tangent: target.tangent,
            normal: target.normal,
            support: jet(&target.support),
        };
        (station, target, jet(original), jet(current))
    });
    kernel::step(samples, &response.values, response.mirror.as_ref(), config.order, config.tolerances.curvature_relative, diagonal)
        .map_err(|error| bad(&error.to_string()))
}
