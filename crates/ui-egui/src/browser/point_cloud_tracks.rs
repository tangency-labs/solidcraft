//! What the Edit Point Cloud card's range tracks draw, worked out off the frame: each track's
//! bars over the whole span and over its loupe, and the values its ends snap to. A track asks
//! every frame; a worker thread answers the newest asks (on the web, where there are no threads,
//! they are answered at once), and until then the track shows its last answer. The view asks too,
//! for the values the coloured property takes over what is kept: the ramp's automatic range.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use egui::Color32;
use solidcraft_engine::doc::point_cloud::{Filter, PALETTE, PLAIN, PointCloud, byte, height_ramp};
use solidcraft_engine::doc::point_cloud_distribution::{Colour, Geometry, histogram, label_changes};

use super::range_track::Bars;
use crate::theme::model_color;

/// Equal bands a ramp's bars are split into.
const RAMP_BANDS: usize = 32;

/// How a track's bars are coloured.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Paint {
    /// One colour.
    One,
    /// The ramp over `range` of `property`; painted at each value when the track is that property.
    Ramp { property: String, range: [f64; 2] },
    /// The palette of a whole-numbered `property`.
    Palette { property: String },
}

/// What a track asks for: its property's bars over the whole span and over the loupe, in `bins`
/// bins each, over what `filters` keep; and the values where `changes_of` changes along it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ask {
    pub property: String,
    pub span: [f64; 2],
    pub loupe: [f64; 2],
    pub bins: usize,
    pub paint: Paint,
    pub filters: Vec<Filter>,
    pub changes_of: Option<String>,
}

/// A track's answer.
#[derive(Clone, Debug, Default)]
pub(crate) struct Answer {
    pub(super) overview: Bars,
    pub(super) loupe: Bars,
    pub(super) changes: Arc<Vec<f64>>,
    /// The least and the greatest value the property takes over what is kept.
    pub extent: Option<[f64; 2]>,
}

/// The cloud's data as far as the tracks are concerned: its points, cells, columns and placement.
type DataKey = Vec<u64>;

fn data_key(c: &PointCloud) -> DataKey {
    let mut k = vec![
        Arc::as_ptr(c.points.values()).cast::<f32>() as u64,
        Arc::as_ptr(&c.complex.segments.0).cast::<u32>() as u64,
        Arc::as_ptr(&c.complex.triangles.0).cast::<u32>() as u64,
        c.origin.x.to_bits(),
        c.origin.y.to_bits(),
        c.origin.z.to_bits(),
    ];
    k.extend(c.complex.columns.iter().map(|col| Arc::as_ptr(&col.values.0).cast::<f32>() as u64));
    k.extend(c.transform.iter().flatten().map(|v| v.to_bits()));
    k
}

/// What answers need of a cloud, worked out once for its data.
struct Prepared {
    key: DataKey,
    cloud: Arc<PointCloud>,
    geometry: Option<Arc<Geometry>>,
    changes: HashMap<(String, String), Arc<Vec<f64>>>,
}

impl Prepared {
    fn answer(&mut self, ask: &Ask) -> Answer {
        let c = &self.cloud;
        let geometry = self.geometry.get_or_insert_with(|| Arc::new(Geometry::of(c))).clone();
        let (colour, colours, ramp) = match &ask.paint {
            // Coloured by its own property, each value has its colour: the bars are one slot,
            // painted along the ramp.
            Paint::Ramp { property, range } if *property == ask.property => (Colour::One, vec![plain()], Some(*range)),
            Paint::Ramp { property, range } => (
                Colour::Ramp { property, range: *range, bands: RAMP_BANDS },
                (0..RAMP_BANDS).map(|k| model_color(height_ramp((k as f64 + 0.5) / RAMP_BANDS as f64))).chain([plain()]).collect(),
                None,
            ),
            Paint::Palette { property } => {
                (Colour::Palette { property }, PALETTE.iter().map(|c| model_color(*c)).chain([plain(), plain()]).collect(), None)
            }
            Paint::One => (Colour::One, vec![plain()], None),
        };
        let bars = |axis: [f64; 2]| {
            let h = histogram(c, &geometry, &ask.property, axis, ask.bins, &colour, &ask.filters);
            let extent = h.as_ref().and_then(|h| h.extent);
            (Bars { axis, slots: h.map(|h| h.slots).unwrap_or_default(), colours: colours.clone(), ramp }, extent)
        };
        let (overview, extent) = bars(ask.span);
        let loupe = if ask.loupe == ask.span { overview.clone() } else { bars(ask.loupe).0 };
        let changes = match &ask.changes_of {
            Some(label) => self
                .changes
                .entry((label.clone(), ask.property.clone()))
                .or_insert_with(|| Arc::new(label_changes(c, &geometry, label, &ask.property)))
                .clone(),
            None => Arc::default(),
        };
        Answer { overview, loupe, changes, extent }
    }
}

/// What has no colour of its own: the cloud's plain colour.
fn plain() -> Color32 {
    model_color(PLAIN.map(byte))
}

#[derive(Default)]
struct Inner {
    cloud: Option<(DataKey, Arc<PointCloud>)>,
    asked: HashMap<String, Ask>,
    answers: HashMap<String, (Ask, Arc<Answer>)>,
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    asked: Condvar,
}

/// The answers of one card's tracks; clones share them.
#[derive(Clone, Default)]
pub(crate) struct Tracks {
    shared: Arc<Shared>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Arc<std::sync::OnceLock<()>>,
}

impl std::fmt::Debug for Tracks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tracks")
    }
}

impl Tracks {
    /// The last answer to the track `key` (maybe to an older ask), asking `ask` of `cloud` when
    /// that is new.
    pub(crate) fn answer(&self, ctx: &egui::Context, cloud: &PointCloud, key: &str, ask: Ask) -> Option<Arc<Answer>> {
        let Ok(mut inner) = self.shared.inner.lock() else { return None };
        let data = data_key(cloud);
        if inner.cloud.as_ref().is_none_or(|(k, _)| *k != data) {
            inner.cloud = Some((data, Arc::new(cloud.clone())));
        }
        let last = inner.answers.get(key).cloned();
        let answered = last.as_ref().is_some_and(|(a, _)| *a == ask);
        if !answered && inner.asked.get(key) != Some(&ask) {
            inner.asked.insert(key.to_string(), ask);
            drop(inner);
            self.work(ctx);
        }
        last.map(|(_, a)| a)
    }

    /// Whether the track `key` has an answer to its last ask.
    #[cfg(test)]
    pub(super) fn answered(&self, key: &str) -> bool {
        self.shared.inner.lock().is_ok_and(|i| !i.asked.contains_key(key) && i.answers.contains_key(key))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn work(&self, ctx: &egui::Context) {
        self.shared.asked.notify_one();
        self.worker.get_or_init(|| {
            let shared = Arc::downgrade(&self.shared);
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name("point-cloud-tracks".into()).spawn(move || work(&shared, &ctx));
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn work(&self, ctx: &egui::Context) {
        let mut prepared = None;
        while answer_one(&self.shared, &mut prepared) {}
        ctx.request_repaint();
    }
}

/// Answer one ask, if there is one.
fn answer_one(shared: &Shared, prepared: &mut Option<Prepared>) -> bool {
    let (key, ask, data, cloud) = {
        let Ok(mut inner) = shared.inner.lock() else { return false };
        let Some(key) = inner.asked.keys().next().cloned() else { return false };
        let Some(ask) = inner.asked.remove(&key) else { return false };
        let Some((data, cloud)) = inner.cloud.clone() else { return false };
        (key, ask, data, cloud)
    };
    if prepared.as_ref().is_none_or(|p| p.key != data) {
        *prepared = Some(Prepared { key: data, cloud, geometry: None, changes: HashMap::new() });
    }
    let Some(p) = prepared.as_mut() else { return false };
    let answer = Arc::new(p.answer(&ask));
    if let Ok(mut inner) = shared.inner.lock() {
        inner.answers.insert(key, (ask, answer));
    }
    true
}

/// The worker: answers asks as they come, until the card is gone.
#[cfg(not(target_arch = "wasm32"))]
fn work(shared: &std::sync::Weak<Shared>, ctx: &egui::Context) {
    let mut prepared = None;
    loop {
        let Some(s) = shared.upgrade() else { return };
        if answer_one(&s, &mut prepared) {
            ctx.request_repaint();
            continue;
        }
        let Ok(inner) = s.inner.lock() else { return };
        if inner.asked.is_empty() {
            let _ = s.asked.wait_timeout(inner, std::time::Duration::from_millis(500));
        }
    }
}
