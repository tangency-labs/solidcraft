//! The models of the controllers actually held, as the runtime hands them out
//! (`XR_EXT_render_model`, `XR_EXT_interaction_render_model`): which models there are, each one's
//! glTF asset and the names of its moving parts, where it is, and how its parts (stick, trigger,
//! buttons) stand each frame. The extensions have no safe API in the `openxr` crate; each call is
//! here with its `SAFETY` note.

use std::ptr;

use openxr as xr;
use xr::sys;

use super::super::Error;

/// The most models taken (a runtime offers one per controller, and a few for trackers).
const MAX_MODELS: usize = 16;

/// The longest list of models read: all of them are read (OpenXR refuses a list asked for with
/// less room than it has) and the first [`MAX_MODELS`] kept; a runtime claiming more than this
/// is refused.
const MAX_LISTED: u32 = 256;

/// How often the list is read again when it grew between asking its length and reading it (a
/// tracker switched on in between).
const LIST_TRIES: usize = 4;

/// The most moving parts a model may have.
const MAX_NODES: u32 = 256;

/// The largest asset taken (bytes): a controller's model with its pictures is a few megabytes.
const MAX_ASSET: u32 = 64 << 20;

/// One controller's model, made by the runtime: dropped, its space goes first, then the model.
pub(in crate::session) struct RawModel {
    fns: xr::raw::RenderModelEXT,
    handle: sys::RenderModelEXT,
    /// Where the model is (its glTF origin); taken before the model is destroyed.
    pub space: Option<xr::Space>,
    /// The asset's cache id: models with the same id share one asset.
    pub cache: [u8; 16],
    /// The hand the model's pose follows, where it is one of `hands`' (`None`: another device).
    pub hand: Option<usize>,
    /// The glTF asset (GLB).
    pub glb: Vec<u8>,
    /// The moving parts' names, in the order [`RawModel::state`] reports them.
    pub nodes: Vec<String>,
}

impl Drop for RawModel {
    /// The space first, then the model the runtime made.
    fn drop(&mut self) {
        self.space = None;
        // SAFETY: `handle` was made by `create_render_model` on a session that outlives this
        // (the session's `Controls` and models are dropped before it), and is destroyed once.
        let result = unsafe { (self.fns.destroy_render_model)(self.handle) };
        if result.into_raw() < 0 {
            log::debug!("VR: a controller model was not let go: {result}");
        }
    }
}

/// `result` as an error saying `what` failed.
fn check(what: &'static str, result: sys::Result) -> Result<(), Error> {
    if result.into_raw() < 0 { Err(Error::xr(what, result)) } else { Ok(()) }
}

/// The models of the controllers now held, each with its asset; `hands` the two hands' user
/// paths, left first. Empty where the runtime does not have the extensions. Must come after the
/// actions are attached (the runtime picks the models from the bound controllers).
pub(in crate::session) fn controller_models<G: xr::Graphics>(
    instance: &xr::Instance,
    session: &xr::Session<G>,
    hands: [sys::Path; 2],
) -> Result<Vec<RawModel>, Error> {
    let (Some(fns), Some(ids_fns)) = (instance.exts().ext_render_model, instance.exts().ext_interaction_render_model) else {
        return Ok(Vec::new());
    };
    let info = sys::InteractionRenderModelIdsEnumerateInfoEXT { ty: sys::InteractionRenderModelIdsEnumerateInfoEXT::TYPE, next: ptr::null() };
    let ids = two_call_list("listing the controller models", sys::RenderModelIdEXT::default(), MAX_LISTED, MAX_MODELS, |ids, count| {
        let capacity = u32::try_from(ids.len()).unwrap_or(0);
        let array = if ids.is_empty() { ptr::null_mut() } else { ids.as_mut_ptr() };
        // SAFETY: the session is live and `info` filled in as the spec asks; `array` holds
        // `capacity` elements (`ids` lives through the call) and the runtime writes at most that
        // many, and with a capacity of 0 it is null and only the count is written.
        unsafe { (ids_fns.enumerate_interaction_render_model_ids)(session.as_raw(), &info, capacity, count, array) }
    })?;
    let mut models = Vec::with_capacity(ids.len());
    for id in ids {
        match model(fns, ids_fns, session, id, hands) {
            Ok(m) => models.push(m),
            Err(e) => log::info!("VR: a controller model was not taken: {e}"),
        }
    }
    Ok(models)
}

/// A list read by OpenXR's two calls, the first `keep` of it kept: `call(items, count)` with no
/// items asks only for the length (into `count`), then with room for that many reads them. The
/// list is always read whole, as OpenXR answers a call with less room than the list has with
/// `XR_ERROR_SIZE_INSUFFICIENT` (and the new length): one that grew in between is read again,
/// at most [`LIST_TRIES`] times. A list longer than `cap` is refused before room is made for it.
fn two_call_list<T: Copy>(
    what: &'static str,
    empty: T,
    cap: u32,
    keep: usize,
    mut call: impl FnMut(&mut [T], &mut u32) -> sys::Result,
) -> Result<Vec<T>, Error> {
    let mut count = 0u32;
    check(what, call(&mut [], &mut count))?;
    for _ in 0..LIST_TRIES {
        if count == 0 {
            return Ok(Vec::new());
        }
        if count > cap {
            return Err(Error::Unsupported(format!("{what}: the VR software lists {count}")));
        }
        let mut items = vec![empty; count as usize];
        let result = call(&mut items, &mut count);
        if result == sys::Result::ERROR_SIZE_INSUFFICIENT {
            continue;
        }
        check(what, result)?;
        let listed = (count as usize).min(items.len());
        items.truncate(listed.min(keep));
        if listed > keep {
            log::info!("VR: the VR software lists {listed} for {what}; the first {keep} are taken");
        }
        return Ok(items);
    }
    Err(Error::xr(what, sys::Result::ERROR_SIZE_INSUFFICIENT))
}

/// The model with `id`: made, located, and its asset read.
fn model<G: xr::Graphics>(
    fns: xr::raw::RenderModelEXT,
    ids_fns: xr::raw::InteractionRenderModelEXT,
    session: &xr::Session<G>,
    id: sys::RenderModelIdEXT,
    hands: [sys::Path; 2],
) -> Result<RawModel, Error> {
    let create = sys::RenderModelCreateInfoEXT {
        ty: sys::RenderModelCreateInfoEXT::TYPE,
        next: ptr::null(),
        render_model_id: id,
        // Core glTF only: no extension is asked for, so pictures come as PNG or JPEG.
        gltf_extension_count: 0,
        gltf_extensions: ptr::null(),
    };
    let mut handle = sys::RenderModelEXT::default();
    // SAFETY: the session is live and `create` is filled in as the spec asks.
    check("making a controller model", unsafe { (fns.create_render_model)(session.as_raw(), &create, &mut handle) })?;
    // From here on the model is let go when this returns with an error.
    let mut out = RawModel { fns, handle, space: None, cache: [0; 16], hand: None, glb: Vec::new(), nodes: Vec::new() };

    let get = sys::RenderModelPropertiesGetInfoEXT { ty: sys::RenderModelPropertiesGetInfoEXT::TYPE, next: ptr::null() };
    let mut properties = sys::RenderModelPropertiesEXT {
        ty: sys::RenderModelPropertiesEXT::TYPE,
        next: ptr::null_mut(),
        cache_id: sys::Uuid { data: [0; 16] },
        animatable_node_count: 0,
    };
    // SAFETY: `handle` is the live model made above.
    check("reading a controller model", unsafe { (fns.get_render_model_properties)(handle, &get, &mut properties) })?;
    out.cache = properties.cache_id.data;
    let nodes = properties.animatable_node_count;
    if nodes > MAX_NODES {
        return Err(Error::Unsupported(format!("a controller model has {nodes} moving parts")));
    }

    let which = sys::InteractionRenderModelTopLevelUserPathGetInfoEXT {
        ty: sys::InteractionRenderModelTopLevelUserPathGetInfoEXT::TYPE,
        next: ptr::null(),
        top_level_user_path_count: 2,
        top_level_user_paths: hands.as_ptr(),
    };
    let mut user = sys::Path::default();
    // SAFETY: `which` names the two paths in `hands`, which outlives the call.
    let found = unsafe { (ids_fns.get_render_model_pose_top_level_user_path)(handle, &which, &mut user) };
    if found.into_raw() >= 0 {
        out.hand = hands.iter().position(|h| *h == user);
    }

    let space_info = sys::RenderModelSpaceCreateInfoEXT { ty: sys::RenderModelSpaceCreateInfoEXT::TYPE, next: ptr::null(), render_model: handle };
    let mut space = sys::Space::default();
    // SAFETY: the session and the model are live.
    check("following a controller model", unsafe { (fns.create_render_model_space)(session.as_raw(), &space_info, &mut space) })?;
    // SAFETY: `space` was just made on this session and is owned by the `xr::Space` from here on.
    out.space = Some(unsafe { xr::Space::reference_from_raw(session.clone(), space) });

    let asset_info =
        sys::RenderModelAssetCreateInfoEXT { ty: sys::RenderModelAssetCreateInfoEXT::TYPE, next: ptr::null(), cache_id: properties.cache_id };
    let mut asset = sys::RenderModelAssetEXT::default();
    // SAFETY: the session is live; the cache id is the model's own.
    check("reading a controller model", unsafe { (fns.create_render_model_asset)(session.as_raw(), &asset_info, &mut asset) })?;
    let read = read_asset(fns, asset, nodes);
    // SAFETY: `asset` was made above and is destroyed once, whatever reading it gave.
    unsafe { (fns.destroy_render_model_asset)(asset) };
    let (glb, names) = read?;
    out.glb = glb;
    out.nodes = names;
    Ok(out)
}

/// The asset's bytes and the names of its `nodes` moving parts.
fn read_asset(fns: xr::raw::RenderModelEXT, asset: sys::RenderModelAssetEXT, nodes: u32) -> Result<(Vec<u8>, Vec<String>), Error> {
    let get = sys::RenderModelAssetDataGetInfoEXT { ty: sys::RenderModelAssetDataGetInfoEXT::TYPE, next: ptr::null() };
    let mut data = sys::RenderModelAssetDataEXT {
        ty: sys::RenderModelAssetDataEXT::TYPE,
        next: ptr::null_mut(),
        buffer_capacity_input: 0,
        buffer_count_output: 0,
        buffer: ptr::null_mut(),
    };
    // SAFETY: the asset is live; a capacity of 0 with a null buffer asks only for the size.
    check("reading a controller model", unsafe { (fns.get_render_model_asset_data)(asset, &get, &mut data) })?;
    let size = data.buffer_count_output;
    if size == 0 || size > MAX_ASSET {
        return Err(Error::Unsupported(format!("a controller model of {size} bytes")));
    }
    let mut glb = vec![0u8; size as usize];
    data.buffer_capacity_input = size;
    data.buffer = glb.as_mut_ptr();
    // SAFETY: `glb` holds `size` bytes, the capacity given; the runtime writes at most that many.
    check("reading a controller model", unsafe { (fns.get_render_model_asset_data)(asset, &get, &mut data) })?;
    glb.truncate(data.buffer_count_output.min(size) as usize);

    let get = sys::RenderModelAssetPropertiesGetInfoEXT { ty: sys::RenderModelAssetPropertiesGetInfoEXT::TYPE, next: ptr::null() };
    let mut names = vec![sys::RenderModelAssetNodePropertiesEXT { unique_name: [0; sys::MAX_RENDER_MODEL_ASSET_NODE_NAME_SIZE_EXT] }; nodes as usize];
    let mut properties = sys::RenderModelAssetPropertiesEXT {
        ty: sys::RenderModelAssetPropertiesEXT::TYPE,
        next: ptr::null_mut(),
        node_property_count: nodes,
        node_properties: names.as_mut_ptr(),
    };
    if nodes > 0 {
        // SAFETY: `names` holds exactly the model's count of moving parts, as the spec asks.
        check("reading a controller model's parts", unsafe { (fns.get_render_model_asset_properties)(asset, &get, &mut properties) })?;
    }
    let names = names
        .iter()
        .map(|n| {
            // The runtime ends each name with a zero within the array; one that does not is cut.
            let bytes: Vec<u8> = n.unique_name.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .collect();
    Ok((glb, names))
}

impl RawModel {
    /// Each moving part's pose (relative to its parent node, replacing the asset's own) and
    /// whether it shows, at `time`; `None` when the runtime does not say.
    pub(in crate::session) fn state(&self, time: xr::Time) -> Option<Vec<(xr::Posef, bool)>> {
        let count = self.nodes.len() as u32;
        let get = sys::RenderModelStateGetInfoEXT { ty: sys::RenderModelStateGetInfoEXT::TYPE, next: ptr::null(), display_time: time };
        let mut states = vec![sys::RenderModelNodeStateEXT { node_pose: xr::Posef::IDENTITY, is_visible: sys::FALSE }; count as usize];
        let mut state = sys::RenderModelStateEXT {
            ty: sys::RenderModelStateEXT::TYPE,
            next: ptr::null_mut(),
            node_state_count: count,
            node_states: states.as_mut_ptr(),
        };
        // SAFETY: the model is live and `states` holds exactly its count of moving parts.
        let result = unsafe { (self.fns.get_render_model_state)(self.handle, &get, &mut state) };
        if result.into_raw() < 0 {
            return None;
        }
        Some(states.into_iter().map(|s| (s.node_pose, s.is_visible != sys::FALSE)).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A runtime's two-call answer for a list that is `list(calls)` long on its `calls`-th call
    /// (counting from 0): the length always, the items (0, 1, 2, ...) when there is room for all
    /// of them, `XR_ERROR_SIZE_INSUFFICIENT` when there is some but too little.
    fn runtime(list: impl Fn(u32) -> u32, calls: &Cell<u32>) -> impl FnMut(&mut [u32], &mut u32) -> sys::Result {
        move |items, count| {
            let need = list(calls.get());
            calls.set(calls.get() + 1);
            *count = need;
            if items.is_empty() {
                return sys::Result::SUCCESS;
            }
            if items.len() < need as usize {
                return sys::Result::ERROR_SIZE_INSUFFICIENT;
            }
            for (i, item) in items.iter_mut().take(need as usize).enumerate() {
                *item = i as u32;
            }
            sys::Result::SUCCESS
        }
    }

    /// More models than are kept (20 of 16): the list is read whole and the first 16 kept, not
    /// refused for too little room; a list that fits is kept whole.
    #[test]
    fn a_list_longer_than_what_is_kept_is_read_whole_and_cut() {
        let calls = Cell::new(0);
        let ids = two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|_| 20, &calls)).unwrap();
        assert_eq!(ids, (0..16).collect::<Vec<u32>>());
        assert_eq!(calls.get(), 2, "the length, then the list");
        let ids = two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|_| 3, &Cell::new(0))).unwrap();
        assert_eq!(ids, [0, 1, 2]);
    }

    /// A list that grew between asking its length and reading it (a tracker switched on) is read
    /// again at its new length; one that keeps growing is given up after [`LIST_TRIES`] reads.
    #[test]
    fn a_list_that_grows_in_between_is_read_again_a_few_times() {
        let calls = Cell::new(0);
        let ids = two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|call| if call == 0 { 2 } else { 3 }, &calls)).unwrap();
        assert_eq!(ids, [0, 1, 2]);
        assert_eq!(calls.get(), 3);
        let calls = Cell::new(0);
        let error = two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|call| call + 1, &calls)).unwrap_err();
        assert!(error.to_string().contains("listing"), "{error}");
        assert_eq!(calls.get() as usize, 1 + LIST_TRIES, "the length, then a few tries, never forever");
    }

    /// A runtime claiming more than [`MAX_LISTED`] is refused before room is made for its list;
    /// an empty list, a list shorter than its room and a refused call are taken as they are.
    #[test]
    fn a_list_past_the_cap_or_refused_is_an_error() {
        let calls = Cell::new(0);
        assert!(two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|_| u32::MAX, &calls)).is_err());
        assert_eq!(calls.get(), 1, "only the length was asked");
        let calls = Cell::new(0);
        assert_eq!(two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|_| 0, &calls)).unwrap(), Vec::<u32>::new());
        assert_eq!(calls.get(), 1);
        let shrunk = two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, runtime(|call| if call == 0 { 4 } else { 2 }, &Cell::new(0)));
        assert_eq!(shrunk.unwrap(), [0, 1], "fewer than there was room for: only those");
        let refused = |first: bool| {
            let calls = Cell::new(0);
            two_call_list("listing", 0u32, MAX_LISTED, MAX_MODELS, |_: &mut [u32], count: &mut u32| {
                calls.set(calls.get() + 1);
                *count = 2;
                if first || calls.get() > 1 { sys::Result::ERROR_RUNTIME_FAILURE } else { sys::Result::SUCCESS }
            })
        };
        assert!(refused(true).is_err() && refused(false).is_err());
    }
}
