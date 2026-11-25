//! Rigid motions of bodies, applied on the GPU.
//!
//! A scene's triangles and lines are built where the world has their bodies. Showing a body
//! somewhere else (a part dragged along its joints, a component moved by its triad) meant
//! building and uploading it again, or cutting it out of its scene into a scene of its own
//! drawn with another model matrix. Instead every palette entry of a body can name a slot of
//! this table ([`super::Palette::set_motion`]), and every vertex stage moves what it draws by
//! the slot's rigid motion: moving a body is writing one motion, and whatever is drawn from the
//! body's own triangles (its highlight drawn over it, its edges) moves with it.
//!
//! The table is a texture, as the palette is, so WebGL2 draws it too. A vertex reads it only
//! when its body's slot lies between the first and the last slot a frame moves (`U.depth.w`,
//! `U.screen.z`): a frame without motions pays nothing for it, and in a frame with some the
//! bodies outside that range pay nothing either (a design's bodies take their slots in the order
//! its parts are placed, so the bodies of a moving assembly lie together). Each slot holds its
//! motion less the identity: a texture is zeroed when made, and a zero slot leaves its body where
//! it is.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Slots on one texel row of the table: the shader reads `x = 3 * (slot & 511)`, `y = slot >> 9`.
pub const MOTIONS_PER_ROW: u32 = 512;
/// Texel rows of the table.
const ROWS: u32 = 128;
/// Slots the table has, the first of them ([`UNMOVED_SLOT`]) for bodies that are never moved. A
/// design with more bodies than this leaves the rest without a slot: they are drawn where they
/// were built, and whoever moves bodies builds those again as before.
pub const MAX_MOTIONS: u32 = MOTIONS_PER_ROW * ROWS;
/// The slot of a body that has none: never moved.
pub const UNMOVED_SLOT: u32 = 0;
/// Texels a slot takes: the three rows of its motion.
const SLOT_TEXELS: u32 = 3;
/// Bytes of one texel (`Rgba32Float`).
const TEXEL: usize = 16;

/// The rows of a rigid motion: a point's x, y and z are each row's dot product with (p, 1), as an
/// instanced mesh's placement is.
pub type MotionRows = [[f32; 4]; 3];

/// The motion that moves nothing.
pub const UNMOVED: MotionRows = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// `outer` after `inner`: the motion that moves a point by `inner`, then by `outer`.
pub fn after(outer: &MotionRows, inner: &MotionRows) -> MotionRows {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let turned: f32 = (0..3).map(|k| outer[r][k] * inner[k][c]).sum();
            if c == 3 { turned + outer[r][3] } else { turned }
        })
    })
}

/// The motions a frame draws bodies moved by: each slot's, for the slots that move. Cheap to
/// clone and to compare: the frame's key takes [`Motions::key`].
#[derive(Clone, Debug, Default)]
pub struct Motions {
    slots: Arc<BTreeMap<u32, MotionRows>>,
    key: u64,
}

impl Motions {
    /// The motions `moved` gives: (slot, motion). A slot outside the table, the unmoved slot, a
    /// motion that is not finite and one that moves nothing are left out; of two motions for one
    /// slot the later stands.
    pub fn new(moved: impl IntoIterator<Item = (u32, MotionRows)>) -> Motions {
        // Collected first, so the later of two motions for a slot stands even when it is one that
        // is then left out (it does not bring the earlier one back).
        let mut slots: BTreeMap<u32, MotionRows> = moved.into_iter().collect();
        slots.retain(|slot, rows| *slot != UNMOVED_SLOT && *slot < MAX_MOTIONS && *rows != UNMOVED && rows.iter().flatten().all(|v| v.is_finite()));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (slot, rows) in &slots {
            slot.hash(&mut h);
            rows.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
        }
        slots.len().hash(&mut h);
        Motions { key: if slots.is_empty() { 0 } else { h.finish() | 1 }, slots: Arc::new(slots) }
    }

    /// Nothing moves.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// How many slots move.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Names these motions: 0 for none, the same for the same slots moved the same way.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The first slot that moves and one past the last, as the shader's uniform takes them: both
    /// 0 when nothing moves.
    pub fn range(&self) -> [f32; 2] {
        match (self.slots.keys().next(), self.slots.keys().next_back()) {
            (Some(first), Some(last)) => [*first as f32, (*last + 1) as f32],
            _ => [0.0, 0.0],
        }
    }

    /// The motion of `slot`: [`UNMOVED`] for one that does not move.
    pub fn of(&self, slot: u32) -> MotionRows {
        self.slots.get(&slot).copied().unwrap_or(UNMOVED)
    }
}

/// The table on the GPU, and the motions it holds.
pub(crate) struct MotionTable {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The motions written into it.
    held: Motions,
}

impl MotionTable {
    /// An empty table: every slot unmoved (a texture is zeroed when made).
    pub fn new(device: &wgpu::Device) -> MotionTable {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_motions"),
            size: wgpu::Extent3d { width: MOTIONS_PER_ROW * SLOT_TEXELS, height: ROWS, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        MotionTable { texture, view, held: Motions::default() }
    }

    /// The table's place in group 0's layout: read by the vertex stages with `textureLoad`.
    pub fn layout_entry() -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }
    }

    /// The view every group-0 bind group binds.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Bring the table to `motions`: only the slots whose motion changed are written, each run of
    /// neighbouring slots on a texel row in one write.
    pub fn set(&mut self, queue: &wgpu::Queue, motions: &Motions) {
        if motions.key() == self.held.key() {
            return;
        }
        for (first, texels) in changed_runs(&self.held, motions) {
            let width = u32::try_from(texels.len() / TEXEL).unwrap_or(0);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: (first % MOTIONS_PER_ROW) * SLOT_TEXELS, y: first / MOTIONS_PER_ROW, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &texels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * TEXEL as u32), rows_per_image: Some(1) },
                wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
            );
        }
        self.held = motions.clone();
    }
}

/// A slot's texels: its motion less the identity (a zero slot moves nothing).
fn slot_texels(rows: &MotionRows, out: &mut Vec<u8>) {
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let less = if r == c { v - 1.0 } else { *v };
            out.extend_from_slice(&less.to_le_bytes());
        }
    }
}

/// What to write to take the table from `old` to `new`: each run of neighbouring slots on one
/// texel row whose motion differs, as (its first slot, its texels).
fn changed_runs(old: &Motions, new: &Motions) -> Vec<(u32, Vec<u8>)> {
    let changed = old.slots.keys().chain(new.slots.keys()).filter(|slot| old.of(**slot) != new.of(**slot));
    let changed: std::collections::BTreeSet<u32> = changed.copied().collect();
    let mut runs: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut last = None;
    for slot in changed {
        let joins = last.is_some_and(|l: u32| l + 1 == slot && l / MOTIONS_PER_ROW == slot / MOTIONS_PER_ROW);
        if !joins {
            runs.push((slot, Vec::new()));
        }
        if let Some((_, texels)) = runs.last_mut() {
            slot_texels(&new.of(slot), texels);
        }
        last = Some(slot);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A motion that shifts by `x` along X.
    fn shift(x: f32) -> MotionRows {
        [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]
    }

    /// The texels of a run as f32s.
    fn floats(texels: &[u8]) -> Vec<f32> {
        texels.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
    }

    /// Motions keep only what moves a body the table has a slot for: the unmoved slot, a slot
    /// past the table, a motion that moves nothing and one that is not finite are left out, so
    /// hostile input writes nothing outside the table and no NaN into it.
    #[test]
    fn motions_leave_out_what_the_table_cannot_hold() {
        let m = Motions::new([
            (UNMOVED_SLOT, shift(1.0)),
            (MAX_MOTIONS, shift(1.0)),
            (u32::MAX, shift(1.0)),
            (5, UNMOVED),
            (6, shift(f32::NAN)),
            (7, shift(f32::INFINITY)),
            (8, shift(2.0)),
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!(m.of(8), shift(2.0));
        assert_eq!(m.of(5), UNMOVED);
        assert_eq!(m.of(UNMOVED_SLOT), UNMOVED);
        assert!(Motions::new([(5, UNMOVED)]).is_empty());
        // Of two motions for one slot the later stands, also when it is one that moves nothing
        // or is not finite: the earlier does not come back.
        assert_eq!(Motions::new([(5, shift(1.0)), (5, shift(2.0))]).of(5), shift(2.0));
        assert!(Motions::new([(5, shift(1.0)), (5, UNMOVED)]).is_empty());
        assert!(Motions::new([(5, shift(1.0)), (5, shift(f32::NAN))]).is_empty());
        // A huge finite motion is kept as it is and composes without a panic.
        let far = Motions::new([(5, shift(f32::MAX))]);
        assert_eq!(far.len(), 1);
        assert!(after(&far.of(5), &far.of(5)).iter().flatten().all(|v| !v.is_nan()));
    }

    /// The key names the motions: none is 0, the same slots moved the same way give the same
    /// key in any order, and another motion or another slot gives another.
    #[test]
    fn the_key_follows_the_slots_and_their_motions() {
        assert_eq!(Motions::default().key(), 0);
        assert_eq!(Motions::new([(5, UNMOVED)]).key(), 0, "nothing moved is none");
        let a = Motions::new([(3, shift(1.0)), (9, shift(2.0))]);
        assert_ne!(a.key(), 0);
        assert_eq!(a.key(), Motions::new([(9, shift(2.0)), (3, shift(1.0))]).key());
        assert_ne!(a.key(), Motions::new([(3, shift(1.0)), (9, shift(2.5))]).key(), "a slot moved otherwise");
        assert_ne!(a.key(), Motions::new([(3, shift(1.0)), (10, shift(2.0))]).key(), "another slot");
        assert_ne!(a.key(), Motions::new([(3, shift(1.0))]).key(), "a slot fewer");
        // The range the shader tests a body's slot against: from the first to one past the last.
        assert_eq!(a.range(), [3.0, 10.0]);
        assert_eq!(Motions::default().range(), [0.0, 0.0]);
    }

    /// Only what changed is written: neighbouring slots on a texel row in one run, a slot that
    /// stops moving written back to zero, and a slot that keeps its motion not at all. A run
    /// does not go on from the end of one texel row into the next.
    #[test]
    fn only_the_slots_that_changed_are_written_in_runs() {
        let none = Motions::default();
        let first = Motions::new([(4, shift(1.0)), (5, shift(2.0)), (9, shift(3.0))]);
        let runs = changed_runs(&none, &first);
        assert_eq!(runs.iter().map(|(slot, t)| (*slot, t.len() / (TEXEL * SLOT_TEXELS as usize))).collect::<Vec<_>>(), vec![(4, 2), (9, 1)]);
        // Less the identity: only the shift is left.
        let texels = floats(&runs[0].1);
        assert_eq!(texels.len(), 24);
        assert_eq!(texels[..12], [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(texels[15], 2.0);
        // Slot 5 keeps its motion, 4 stops and 9 changes.
        let second = Motions::new([(5, shift(2.0)), (9, shift(4.0))]);
        let runs = changed_runs(&first, &second);
        assert_eq!(runs.iter().map(|(slot, _)| *slot).collect::<Vec<_>>(), vec![4, 9]);
        assert!(floats(&runs[0].1).iter().all(|v| *v == 0.0), "a slot that stops moving is zeroed");
        assert_eq!(floats(&runs[1].1)[3], 4.0);
        assert!(changed_runs(&second, &second).is_empty());
        // The last slot of a texel row and the first of the next: two writes.
        let across = Motions::new([(MOTIONS_PER_ROW - 1, shift(1.0)), (MOTIONS_PER_ROW, shift(1.0))]);
        assert_eq!(changed_runs(&none, &across).len(), 2);
    }

    /// `after` moves by the inner motion first: a shift after a quarter turn about Z.
    #[test]
    fn one_motion_after_another_moves_by_the_inner_first() {
        let turn: MotionRows = [[0.0, -1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let both = after(&shift(10.0), &turn);
        let at = |m: &MotionRows, p: [f32; 3]| [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3]);
        assert_eq!(at(&both, [1.0, 0.0, 0.0]), [10.0, 1.0, 0.0]);
        assert_eq!(after(&UNMOVED, &turn), turn);
        assert_eq!(after(&turn, &UNMOVED), turn);
    }
}
