//! The provenance hook: data an application attaches to faces and edges, which the kernel's
//! operations carry through their edits as SMLib's `SmAObject::Notify` does. A face or edge cut
//! in two hands it to both pieces, two merged into one hand theirs to the result, a copy carries
//! it, and where a boolean finds one operand's entity lying on the other's the survivor gets it.
//!
//! What the data is and how it joins is the application's ([`Provenance`]); ops only stores it on
//! the kernel's attribute store and calls the application's rule at each event. An application's
//! construction history is built on it.

use solidcore::geom::attribute::{
    ids, ApplicationValue, Attribute, AttributeHandlers, AttributeValue, AttributeView, Behavior,
    ObjectKey, Placement,
};
use solidcore::{Brep, EntityRef};

/// An application's provenance data and its rules for the kernel's edits. Each rule returns what
/// the entity it names carries afterwards: `Some` replaces what it carried (`None` there before
/// means it carried none), `None` leaves it as it was.
pub trait Provenance: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static {
    /// The attribute id this data is stored under, above the kernel's own (`ids::USER_1`
    /// and up); one id per kind of data an application attaches.
    const ID: u64;

    /// `self` was on an entity cut in two; `child` is one piece and what it carries now.
    fn split(&self, child: Option<&Self>) -> Option<Self>;

    /// `self` was on one of two entities merged into one; `result` is what the survivor carries
    /// now (the survivor may be the other entity, or this one).
    fn merge(&self, result: Option<&Self>) -> Option<Self>;

    /// `self` was on an entity found lying on another, which survives; `onto` is what that one
    /// carries now.
    fn coincident(&self, onto: Option<&Self>) -> Option<Self>;
}

/// The lowest [`Provenance::ID`] an application may take.
pub const FIRST_ID: u64 = ids::USER_1;

/// `P::ID` is an application's, checked when the code using `P` is compiled.
struct ApplicationId<P>(std::marker::PhantomData<P>);

impl<P: Provenance> ApplicationId<P> {
    const OK: () = assert!(
        P::ID >= FIRST_ID,
        "a provenance ID below ids::USER_1 is the kernel's"
    );
}

/// The attribute id `P`'s data is stored under, [`Provenance::ID`], checked when the code using
/// it is compiled: an ID below [`FIRST_ID`] is the kernel's and does not build.
///
/// ```
/// # use solidcore_ops::provenance::{id, Provenance, FIRST_ID};
/// # #[derive(Clone, PartialEq, Debug)]
/// # struct Mine;
/// # impl Provenance for Mine {
/// #     const ID: u64 = FIRST_ID;
/// #     fn split(&self, _: Option<&Self>) -> Option<Self> { None }
/// #     fn merge(&self, _: Option<&Self>) -> Option<Self> { None }
/// #     fn coincident(&self, _: Option<&Self>) -> Option<Self> { None }
/// # }
/// assert_eq!(id::<Mine>(), FIRST_ID);
/// ```
///
/// ```compile_fail,E0080
/// # use solidcore_ops::provenance::{id, Provenance, FIRST_ID};
/// # #[derive(Clone, PartialEq, Debug)]
/// # struct Kernels;
/// # impl Provenance for Kernels {
/// #     const ID: u64 = FIRST_ID - 1;
/// #     fn split(&self, _: Option<&Self>) -> Option<Self> { None }
/// #     fn merge(&self, _: Option<&Self>) -> Option<Self> { None }
/// #     fn coincident(&self, _: Option<&Self>) -> Option<Self> { None }
/// # }
/// let _ = id::<Kernels>();
/// ```
#[must_use]
pub const fn id<P: Provenance>() -> u64 {
    let () = ApplicationId::<P>::OK;
    P::ID
}

/// Which provenance type took each ID: one type per ID, so one kind of data never reads as
/// another.
static TAKEN: std::sync::Mutex<Vec<(u64, std::any::TypeId)>> = std::sync::Mutex::new(Vec::new());

/// `P` takes its ID, or has it already.
///
/// # Errors
/// [`Error::Argument`](crate::Error::Argument) when another type took `P::ID` first.
pub(crate) fn take<P: Provenance>() -> crate::Result<()> {
    let id = id::<P>();
    let me = std::any::TypeId::of::<P>();
    let mut taken = TAKEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match taken.iter().find(|(i, _)| *i == id) {
        Some((_, t)) if *t == me => Ok(()),
        Some(_) => Err(crate::Error::Argument),
        None => {
            taken.push((id, me));
            Ok(())
        }
    }
}

/// The data as the kernel's application attribute, its handlers the application's rules.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Carried<P: Provenance>(pub(crate) P);

fn on<P: Provenance>(
    view: &AttributeView<'_>,
    x: ObjectKey,
) -> Option<(solidcore::geom::attribute::AttributeHandle, P)> {
    let (h, a) = view.find_attribute(x, P::ID)?;
    a.application_value::<Carried<P>>()
        .map(|c| (h, c.0.clone()))
}

/// What `x` carries afterwards, as placements: `new` replacing what it carries.
fn place<P: Provenance>(view: &AttributeView<'_>, x: ObjectKey, new: Option<P>) -> Vec<Placement> {
    let Some(new) = new else { return Vec::new() };
    if let Some((handle, _)) = view.find_attribute(x, P::ID) {
        return vec![Placement::Edit {
            handle,
            value: AttributeValue::Application(ApplicationValue::new(Carried(new))),
        }];
    }
    vec![Placement::Add {
        on: x,
        attribute: Attribute::application(P::ID, Behavior::Copy, Carried(new)),
    }]
}

impl<P: Provenance> AttributeHandlers for Carried<P> {
    fn split(
        &self,
        view: &AttributeView<'_>,
        split: ObjectKey,
        a: ObjectKey,
        b: ObjectKey,
    ) -> solidcore::Result<Vec<Placement>> {
        let mut out = Vec::new();
        for x in [a, b] {
            if x != split {
                let now = on::<P>(view, x).map(|(_, p)| p);
                out.extend(place(view, x, self.0.split(now.as_ref())));
            }
        }
        Ok(out)
    }

    /// The store runs one merge per attribute id over both sources (`MergeInBrep`'s
    /// `Merge(data1, data2, data1)`, `geom::a_object`), so `self` is only the first source's
    /// value: each source other than the survivor is read from the view and joined onto what
    /// the survivor carries, by the application's rule (as SolidCraft's history `Tag`
    /// unites both sources' origins).
    fn merge(
        &self,
        view: &AttributeView<'_>,
        a: ObjectKey,
        b: ObjectKey,
        result: ObjectKey,
    ) -> solidcore::Result<Vec<Placement>> {
        let mut now = on::<P>(view, result).map(|(_, p)| p);
        let mut changed = false;
        for x in [a, b] {
            if x == result {
                continue;
            }
            let Some((_, from)) = on::<P>(view, x) else {
                continue;
            };
            if let Some(joined) = from.merge(now.as_ref()) {
                now = Some(joined);
                changed = true;
            }
        }
        Ok(if changed {
            place(view, result, now)
        } else {
            Vec::new()
        })
    }

    fn coincident(
        &self,
        view: &AttributeView<'_>,
        _from: ObjectKey,
        to: ObjectKey,
    ) -> solidcore::Result<Vec<Placement>> {
        let now = on::<P>(view, to).map(|(_, p)| p);
        Ok(place(view, to, self.0.coincident(now.as_ref())))
    }
}

/// Attaches `data` to `e` (a face or an edge), replacing what of its kind `e` carried. `P` takes
/// its ID first ([`take`]): an ID below [`FIRST_ID`] does not compile, one another type took is
/// refused.
///
/// # Errors
/// [`Error::Argument`](crate::Error::Argument) for an ID another type took; the kernel's, for an
/// entity not in `b`.
#[allow(dead_code)] // the application's handle on it comes with the record's order
pub(crate) fn attach<P: Provenance>(b: &mut Brep, e: EntityRef, data: P) -> crate::Result<()> {
    take::<P>()?;
    let old = b
        .entity_attributes(e)
        .into_iter()
        .find(|(_, a)| a.attribute_id() == P::ID)
        .map(|(h, _)| h);
    if let Some(h) = old {
        b.remove_attribute(e, h).map_err(crate::error::status)?;
    }
    b.add_attribute(
        e,
        Attribute::application(P::ID, Behavior::Copy, Carried(data)),
    )
    .map_err(crate::error::status)?;
    Ok(())
}

/// What of kind `P` `e` carries.
#[must_use]
#[allow(dead_code)] // as attach
pub(crate) fn read<P: Provenance>(b: &Brep, e: EntityRef) -> Option<P> {
    let (_, a) = b
        .entity_attributes(e)
        .into_iter()
        .find(|(_, a)| a.attribute_id() == P::ID)?;
    a.application_value::<Carried<P>>().map(|c| c.0.clone())
}
