//! Per-session modeling context (`SmContext`): traversal marks and their locks, the
//! "doing a Boolean" flag, notify callbacks and the context's own model size estimate
//! and zone tolerance; plus the mark-locking helper `SmNewMarkAndLock`, the
//! `SmMassPropertyBehavior` options and the `SM_API` context accessors.
//!
//! Upstream: `source/SMLib/inc/SmContext.h`, `source/SMLib/src/SmContext.cpp`,
//! `source/SMLib/inc/SmTopoTypes.h` (`SmMassPropertyBehavior`),
//! `source/SM_API/src/SmApiGeneral.cpp` (`SmApiCreateContext`, `SmApiGetOrCreateContext`).
//!
//! | Rust                                      | Upstream                                        |
//! |-------------------------------------------|-------------------------------------------------|
//! | [`Context`]                               | `SmContext`                                     |
//! | [`Context::new`], [`Context::default`]    | `SmContext::SmContext(double, double)` (no `SM_USE_GLOBAL_CACHE`) |
//! | [`Context::doing_boolean`]                | `SmContext::GetDoingBoolean`                    |
//! | [`Context::doing_boolean_mut`]            | `SmContext::GetDoingBooleanRef`                 |
//! | [`Context::set_doing_boolean`]            | `SmContext::SetDoingBoolean`                    |
//! | [`Context::user_notify_callback`]         | `SmContext::GetUserNotifyCallback`              |
//! | [`Context::set_user_notify_callback`]     | `SmContext::SetUserNotifyCallback` (unused upstream) |
//! | [`Context::sys_notify_callback`]          | `SmContext::GetSysNotifyCallback`               |
//! | [`Context::user_notify_callback_mut`], [`Context::sys_notify_callback_mut`] | the same getters, for `Execute` |
//! | [`NotifyCallback`], [`NotifyCallback::execute`] | `SmNotifyCallback`, `SmNotifyCallback::Execute` |
//! | [`Context::set_sys_notify_callback`]      | `SmContext::SetSysNotifyCallback`               |
//! | [`Context::this_model_size_estimate`]     | `SmContext::GetThisModelSizeEstimate`           |
//! | [`Context::set_this_model_size_estimate`] | `SmContext::SetThisModelSizeEstimate` (inline, `SM_USE_OLDTOL` build) |
//! | [`Context::set_this_large_small_size_ratio`] | `SmContext::SetThisLargeSmallSizeRatio`      |
//! | [`Context::this_zone_tol_3d`]             | `SmContext::m_sThisZoneTol3d` (`GetThisZoneTol3d`, unused upstream) |
//! | [`Context::stored_model_size_estimate`], [`Context::stored_large_small_size_ratio`] | the raw members `m_dThisModelSizeEstimate`, `m_dThisLargeSmallSizeRatio` |
//! | [`Context::current_mark`]                 | `SmContext::GetCurrentMark`                     |
//! | [`Context::is_mark_locked`]               | `SmContext::IsMarkLocked`                       |
//! | `Context::new_mark` (private)             | `SmContext::NewMark` (private, friend `SmNewMarkAndLock`) |
//! | [`MarkType`]                              | `SmMarkType`                                    |
//! | [`MarkLock`]                              | `SmNewMarkAndLock`                              |
//! | [`MarkLock::new`]                         | `SmNewMarkAndLock(const SmContext*, SmBitArray)` |
//! | [`MarkLock::new_pair`]                    | `SmNewMarkAndLock(SmContext*, SmContext*)`      |
//! | [`MarkLock::unbound`], [`MarkLock::default`] | `SmNewMarkAndLock()`                         |
//! | [`MarkLock::release`]                     | `SmNewMarkAndLock::~SmNewMarkAndLock`           |
//! | [`MarkLock::mark_type`], [`MarkLock::mark_type1`], [`MarkLock::mark_type2`] | `GetMarkType`, `GetMarkType1`, `GetMarkType2` |
//! | [`MarkLock::context`]                     | `SmNewMarkAndLock::GetContext`                  |
//! | [`MarkLock::context2`]                    | `SmNewMarkAndLock::GetContext2`                 |
//! | [`MarkLock::new_mark`]                    | `SmNewMarkAndLock::NewMark`                     |
//! | [`MarkLock::set_context`]                 | `SmNewMarkAndLock::SetContext(SmContext*, SmMarkType)` |
//! | [`MarkLock::set_context_pair`]            | `SmNewMarkAndLock::SetContext(SmContext*, SmContext*)` |
//! | `lock_mark`, `unlock_mark` (private)      | `SmNewMarkAndLock::LockMark`, `UnlockMark`      |
//! | `unlocked_mark` (private)                 | `SmNewMarkAndLock::GetUnlockedMark`             |
//! | [`with_mark_lock`]                        | the scoped `{ SmNewMarkAndLock sMarkLock(pContext, mask); ... }` idiom |
//! | [`MassPropertyBehavior`]                  | `SmMassPropertyBehavior` (and its constructor)  |
//! | [`create_context`]                        | `SmApiCreateContext`                            |
//! | [`get_or_create_context`]                 | `SmApiGetOrCreateContext`                       |
//!
//! Deviations:
//! - `SmNewMarkAndLock` holds `SmContext*` pointers and unlocks in its destructor.
//!   [`MarkLock`] instead stores the *indices* of its contexts in a caller-owned slice
//!   `&mut [Context]` (index equality plays pointer equality) and every method that
//!   touches a context takes that slice; the destructor is the explicit
//!   [`MarkLock::release`]. [`with_mark_lock`] gives the scoped form for a
//!   [`Kernel`]'s own context. A method that upstream would run through a null
//!   context pointer (a crash) returns [`SolidError::InvalidInput`], as does an index
//!   outside the slice.
//! - The upstream two-context constructor locks the *first* context's mark type in the
//!   second context and increments the first context's mark twice; this is ported
//!   as is (see `plan/findings.md`).
//! - `SM_ASSERT*`, `SM_ASSERT_MSG_BREAK` and `SM_DBG_WARN` only print messages in the
//!   release build and are not ported. `SmBoolean` flags are `bool`; `ULONG` marks are
//!   `u64` incremented with wrapping arithmetic, as in C.
//! - Notify callbacks are boxed [`NotifyCallback`] trait objects owned by the context
//!   (upstream stores borrowed raw pointers); `Execute` takes `&mut self` and is
//!   reached through [`Context::user_notify_callback_mut`] /
//!   [`Context::sys_notify_callback_mut`] by [`object::notify`].
//! - `SmApiGeneral.cpp` keeps one process-global `SmContext*`; the port takes the slot
//!   (`&mut Option<Context>`) from the caller.
//! - The attribute-callback list, global user data (`void*`), `operator new/delete`,
//!   `Dump`, `GetTypeString` and `IsLockedMark` are unused upstream and not ported.

use crate::error::{Result, SolidError};
use crate::geom::object;
use crate::geom::tol;
use crate::kernel::Kernel;

/// Which traversal mark of a context to use (`SmMarkType`). The discriminants are the
/// upstream bit values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum MarkType {
    /// `SM_MT_NOMARK`: no mark (also the unset value of a [`MarkLock`]).
    NoMark = 0,
    /// `SM_MT_MARK`: the general mark.
    Mark = 1,
    /// `SM_MT_MARK2`: the second general mark.
    Mark2 = 2,
    /// `SM_MT_MARK3`: the third general mark.
    Mark3 = 4,
    /// `SM_MT_MARKIO`: the mark used by draw / IO methods.
    MarkIo = 8,
    /// `SM_MT_MARKASSERT`: the mark used by validity checks.
    MarkAssert = 16,
    /// `SM_MT_ALLMARKS`: all marks; as a single mark it behaves like an unknown
    /// value (the `default:` branches upstream).
    AllMarks = 31,
}

impl MarkType {
    /// The upstream enum value (also the bit of the mark in a mark mask).
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The mark type with upstream value `code`, if any.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::NoMark),
            1 => Some(Self::Mark),
            2 => Some(Self::Mark2),
            4 => Some(Self::Mark3),
            8 => Some(Self::MarkIo),
            16 => Some(Self::MarkAssert),
            31 => Some(Self::AllMarks),
            _ => None,
        }
    }

    /// Index of the mark's counter and lock in a [`Context`]; `None` for
    /// [`MarkType::NoMark`] and [`MarkType::AllMarks`].
    const fn slot(self) -> Option<usize> {
        match self {
            Self::Mark => Some(0),
            Self::Mark2 => Some(1),
            Self::Mark3 => Some(2),
            Self::MarkIo => Some(3),
            Self::MarkAssert => Some(4),
            Self::NoMark | Self::AllMarks => None,
        }
    }
}

/// Mark mask selecting every mark (`SM_MT_ALLMARKS`).
pub const ALL_MARKS: i32 = MarkType::AllMarks as i32;

/// Index of `SM_MT_MARKIO` in the mark arrays (the `default:` mark upstream).
const MARK_IO: usize = 3;

/// Receiver of topology-change notifications (`SmNotifyCallback`).
pub trait NotifyCallback: std::fmt::Debug + Send {
    /// `SmNotifyCallback::Execute`: called once for every [`object::notify`] of an
    /// object whose context holds this callback, with the notifying object
    /// (`caller`) and the operation's data objects (see
    /// [`object::NotifyOperation`] for their meaning per operation). The status is
    /// ignored by [`object::notify`].
    ///
    /// # Errors
    ///
    /// Whatever the callback reports.
    fn execute(
        &mut self,
        operation: object::NotifyOperation,
        caller: object::ObjectRef,
        data1: Option<object::ObjectRef>,
        data2: Option<object::ObjectRef>,
        data3: Option<object::ObjectRef>,
    ) -> Result<()>;

    /// This callback as [`core::any::Any`], so that whoever installed it can
    /// recognise it in a context (upstream compares callback addresses, e.g.
    /// `SmTrackTopologyChanges::IsTracking`). The default, `None`, suits callbacks
    /// nobody needs to recognise.
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }
}

/// Shared modeling state (`SmContext`): traversal marks, the Boolean flag, notify
/// callbacks and the context's model size estimate and zone tolerance.
#[derive(Debug)]
pub struct Context {
    /// `m_lCurrentMark`, `m_lCurrentMark2`, `m_lCurrentMark3`, `m_lCurrentMarkIO`,
    /// `m_lCurrentMarkAssert`.
    current_marks: [u64; 5],
    /// `m_bMarkLock`, `m_bMark2Lock`, `m_bMark3Lock`, `m_bMarkIOLock`,
    /// `m_bMarkAssertLock`.
    mark_locks: [bool; 5],
    /// `m_bDoingBoolean`.
    doing_boolean: bool,
    /// `m_dThisModelSizeEstimate`.
    model_size_estimate: f64,
    /// `m_dThisLargeSmallSizeRatio`.
    large_small_size_ratio: f64,
    /// `m_sThisZoneTol3d`.
    zone_tol_3d: f64,
    /// `m_pUserNotifyCallback`.
    user_notify_callback: Option<Box<dyn NotifyCallback>>,
    /// `m_pSysNotifyCallBack`.
    sys_notify_callback: Option<Box<dyn NotifyCallback>>,
}

impl Default for Context {
    /// `SmContext()`: both sizes [`tol::USE_DEFAULT`].
    fn default() -> Self {
        Self::new(tol::USE_DEFAULT, tol::USE_DEFAULT)
    }
}

impl Context {
    /// Borrow notification callbacks alongside the live kernel without moving
    /// the kernel's traversal marks or tolerances. Notification APIs use mutable
    /// Context while geometry APIs borrow Kernel; both refer to one upstream
    /// context. Callback ownership is restored on every returned error as well.
    /// The snapshot `Context` is read-only apart from its callbacks: mark, mark-lock
    /// and doing-Boolean changes the action makes to it are dropped on return.
    pub(crate) fn with_notification_context<T>(
        k: &mut Kernel,
        action: impl FnOnce(&mut Kernel, &mut Context) -> T,
    ) -> T {
        let context = &mut k.context;
        let mut notifications = context.metadata_copy();
        notifications.user_notify_callback = context.user_notify_callback.take();
        notifications.sys_notify_callback = context.sys_notify_callback.take();
        let result = action(k, &mut notifications);
        k.context.user_notify_callback = notifications.user_notify_callback;
        k.context.sys_notify_callback = notifications.sys_notify_callback;
        result
    }

    /// An immutable metadata snapshot, without copying callback ownership.
    pub(crate) fn metadata_copy(&self) -> Self {
        Self {
            current_marks: self.current_marks,
            mark_locks: self.mark_locks,
            doing_boolean: self.doing_boolean,
            model_size_estimate: self.model_size_estimate,
            large_small_size_ratio: self.large_small_size_ratio,
            zone_tol_3d: self.zone_tol_3d,
            user_notify_callback: None,
            sys_notify_callback: None,
        }
    }

    /// Creates a context (`SmContext::SmContext(double, double)`): every mark at 1 and
    /// unlocked, not in a Boolean, no callbacks; the zone tolerance follows from the
    /// model size estimate as in [`Context::set_this_model_size_estimate`]. Pass
    /// [`tol::USE_DEFAULT`] for the system values.
    #[must_use]
    pub fn new(model_size_estimate: f64, large_small_size_ratio: f64) -> Self {
        Self {
            current_marks: [1; 5],
            mark_locks: [false; 5],
            doing_boolean: false,
            model_size_estimate,
            large_small_size_ratio,
            zone_tol_3d: tol::size_estimate_to_zone_tol_3d(model_size_estimate),
            user_notify_callback: None,
            sys_notify_callback: None,
        }
    }

    /// Whether a Boolean operation is running (`GetDoingBoolean`).
    #[must_use]
    pub const fn doing_boolean(&self) -> bool {
        self.doing_boolean
    }

    /// Mutable access to the Boolean flag (`GetDoingBooleanRef`).
    pub fn doing_boolean_mut(&mut self) -> &mut bool {
        &mut self.doing_boolean
    }

    /// Sets the Boolean flag (`SetDoingBoolean`).
    pub fn set_doing_boolean(&mut self, doing_boolean: bool) {
        self.doing_boolean = doing_boolean;
    }

    /// The application's notify callback (`GetUserNotifyCallback`).
    #[must_use]
    pub fn user_notify_callback(&self) -> Option<&dyn NotifyCallback> {
        self.user_notify_callback.as_deref()
    }

    /// The application's notify callback, for calling it (`GetUserNotifyCallback`
    /// as used by `SmObject::Notify`).
    pub fn user_notify_callback_mut(&mut self) -> Option<&mut (dyn NotifyCallback + 'static)> {
        self.user_notify_callback.as_deref_mut()
    }

    /// Sets the application's notify callback (`SetUserNotifyCallback`).
    pub fn set_user_notify_callback(&mut self, callback: Option<Box<dyn NotifyCallback>>) {
        self.user_notify_callback = callback;
    }

    /// The system notify callback (`GetSysNotifyCallback`).
    #[must_use]
    pub fn sys_notify_callback(&self) -> Option<&dyn NotifyCallback> {
        self.sys_notify_callback.as_deref()
    }

    /// The system notify callback, for calling it (`GetSysNotifyCallback` as used
    /// by `SmObject::Notify`).
    pub fn sys_notify_callback_mut(&mut self) -> Option<&mut (dyn NotifyCallback + 'static)> {
        self.sys_notify_callback.as_deref_mut()
    }

    /// Sets the system notify callback (`SetSysNotifyCallback`; upstream only warns
    /// in debug builds when one is already set).
    pub fn set_sys_notify_callback(&mut self, callback: Option<Box<dyn NotifyCallback>>) {
        self.sys_notify_callback = callback;
    }

    /// The context's model size estimate, or the system one when it is
    /// [`tol::USE_DEFAULT`] (`GetThisModelSizeEstimate`).
    #[must_use]
    pub fn this_model_size_estimate(&self, k: &Kernel) -> f64 {
        if self.model_size_estimate == tol::USE_DEFAULT {
            tol::model_size_estimate(k)
        } else {
            self.model_size_estimate
        }
    }

    /// Stores a model size estimate and the zone tolerance it implies
    /// ([`tol::size_estimate_to_zone_tol_3d`]); returns that zone tolerance
    /// (`SetThisModelSizeEstimate`).
    pub fn set_this_model_size_estimate(&mut self, model_size_estimate: f64) -> f64 {
        self.model_size_estimate = model_size_estimate;
        self.zone_tol_3d = tol::size_estimate_to_zone_tol_3d(self.model_size_estimate);
        self.zone_tol_3d
    }

    /// Stores the large/small size ratio (`SetThisLargeSmallSizeRatio`).
    pub fn set_this_large_small_size_ratio(&mut self, large_small_size_ratio: f64) {
        self.large_small_size_ratio = large_small_size_ratio;
    }

    /// The stored model size estimate, possibly [`tol::USE_DEFAULT`]
    /// (`m_dThisModelSizeEstimate`).
    #[must_use]
    pub const fn stored_model_size_estimate(&self) -> f64 {
        self.model_size_estimate
    }

    /// The stored large/small size ratio, possibly [`tol::USE_DEFAULT`]
    /// (`m_dThisLargeSmallSizeRatio`).
    #[must_use]
    pub const fn stored_large_small_size_ratio(&self) -> f64 {
        self.large_small_size_ratio
    }

    /// The context's zone tolerance (`m_sThisZoneTol3d`).
    #[must_use]
    pub const fn this_zone_tol_3d(&self) -> f64 {
        self.zone_tol_3d
    }

    /// The current value of a mark (`GetCurrentMark`): an object is marked when its
    /// mark equals this. [`MarkType::NoMark`] gives one more than the largest mark, so
    /// nothing is marked; [`MarkType::AllMarks`] gives the IO mark.
    #[must_use]
    pub fn current_mark(&self, mark_type: MarkType) -> u64 {
        match mark_type {
            MarkType::NoMark => {
                let [m1, m2, m3, mio, massert] = self.current_marks;
                1 + max5(m1, m2, m3, mio, massert)
            }
            _ => self
                .current_marks
                .get(mark_type.slot().unwrap_or(MARK_IO))
                .copied()
                .unwrap_or(0),
        }
    }

    /// Advances a mark by two and returns the new value (`SmContext::NewMark`; the
    /// lock check is a debug-only assertion upstream). [`MarkType::NoMark`] changes
    /// nothing and returns [`Context::current_mark`] of it.
    fn new_mark(&mut self, mark_type: MarkType) -> u64 {
        if mark_type == MarkType::NoMark {
            return self.current_mark(MarkType::NoMark);
        }
        match self
            .current_marks
            .get_mut(mark_type.slot().unwrap_or(MARK_IO))
        {
            Some(m) => {
                *m = m.wrapping_add(2);
                *m
            }
            None => 0,
        }
    }

    /// Whether a mark is locked (`IsMarkLocked`); never for [`MarkType::NoMark`] or
    /// [`MarkType::AllMarks`].
    #[must_use]
    pub fn is_mark_locked(&self, mark_type: MarkType) -> bool {
        mark_type
            .slot()
            .and_then(|s| self.mark_locks.get(s))
            .copied()
            .unwrap_or(false)
    }

    /// Sets a mark's lock flag (no-op for marks without a slot).
    fn set_lock(&mut self, mark_type: MarkType, locked: bool) {
        if let Some(l) = mark_type.slot().and_then(|s| self.mark_locks.get_mut(s)) {
            *l = locked;
        }
    }
}

/// `smos_5Max` = `smos_Max(smos_3Max(a, b, c), smos_Max(d, e))`.
fn max5(a: u64, b: u64, c: u64, d: u64, e: u64) -> u64 {
    a.max(b).max(c).max(d.max(e))
}

/// `SmNewMarkAndLock::LockMark`.
fn lock_mark(ctx: &mut Context, mark_type: MarkType) {
    ctx.set_lock(mark_type, true);
}

/// `SmNewMarkAndLock::UnlockMark`.
fn unlock_mark(ctx: &mut Context, mark_type: MarkType) {
    ctx.set_lock(mark_type, false);
}

/// The first unlocked mark selected by `mask` in the order mark, mark2, mark3, IO,
/// assert; if all are locked, the mask itself when it names a single mark, else the IO
/// mark (`SmNewMarkAndLock::GetUnlockedMark`).
fn unlocked_mark(ctx: &Context, mask: i32) -> MarkType {
    const ORDER: [MarkType; 5] = [
        MarkType::Mark,
        MarkType::Mark2,
        MarkType::Mark3,
        MarkType::MarkIo,
        MarkType::MarkAssert,
    ];
    if let Some(m) = ORDER
        .iter()
        .find(|m| (mask & m.code()) != 0 && !ctx.is_mark_locked(**m))
    {
        return *m;
    }
    ORDER
        .iter()
        .find(|m| mask == m.code())
        .copied()
        .unwrap_or(MarkType::MarkIo)
}

/// `InvalidInput` for a context index outside the caller's slice.
fn index_error(operation: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation,
        reason: "context index out of range",
    }
}

/// The context at `index` in `contexts`.
fn ctx_at<'a>(
    contexts: &'a mut [Context],
    index: usize,
    operation: &'static str,
) -> Result<&'a mut Context> {
    contexts
        .get_mut(index)
        .ok_or_else(|| index_error(operation))
}

/// Fails when a bound context index is outside `contexts`.
fn check_index(contexts: &[Context], index: usize, operation: &'static str) -> Result<()> {
    if index < contexts.len() {
        Ok(())
    } else {
        Err(index_error(operation))
    }
}

/// A claimed traversal mark (`SmNewMarkAndLock`): advances an unlocked mark of one or
/// two contexts and locks it until [`MarkLock::release`].
///
/// Contexts are given as indices into a caller-owned `&mut [Context]` (see the module
/// deviations); pass the same slice to every method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkLock {
    context1: Option<usize>,
    context2: Option<usize>,
    mark_type1: MarkType,
    mark_type2: MarkType,
}

impl Default for MarkLock {
    fn default() -> Self {
        Self::unbound()
    }
}

impl MarkLock {
    /// A lock bound to no context (`SmNewMarkAndLock()`).
    #[must_use]
    pub const fn unbound() -> Self {
        Self {
            context1: None,
            context2: None,
            mark_type1: MarkType::NoMark,
            mark_type2: MarkType::NoMark,
        }
    }

    /// Picks the first unlocked mark of `contexts[ctx]` allowed by `mask` (usually
    /// [`ALL_MARKS`]), advances and locks it (`SmNewMarkAndLock(const SmContext*,
    /// SmBitArray)`).
    pub fn new(contexts: &mut [Context], ctx: usize, mask: i32) -> Result<Self> {
        const OP: &str = "geom::context::MarkLock::new";
        let c = ctx_at(contexts, ctx, OP)?;
        let mark_type1 = unlocked_mark(c, mask);
        c.new_mark(mark_type1);
        lock_mark(c, mark_type1);
        Ok(Self {
            context1: Some(ctx),
            context2: None,
            mark_type1,
            mark_type2: MarkType::NoMark,
        })
    }

    /// Claims a mark in each of two contexts, e.g. the two operands of a Boolean
    /// (`SmNewMarkAndLock(SmContext*, SmContext*)`). When `ctx1 == ctx2` both marks
    /// are the same. Otherwise, as upstream, the second mark type is picked from
    /// `contexts[ctx2]` but the first mark is advanced again in `contexts[ctx1]` and
    /// the *first* mark type is locked in `contexts[ctx2]`.
    pub fn new_pair(contexts: &mut [Context], ctx1: usize, ctx2: usize) -> Result<Self> {
        const OP: &str = "geom::context::MarkLock::new_pair";
        check_index(contexts, ctx2, OP)?;
        let c1 = ctx_at(contexts, ctx1, OP)?;
        let mark_type1 = unlocked_mark(c1, ALL_MARKS);
        c1.new_mark(mark_type1);
        lock_mark(c1, mark_type1);
        let mark_type2 = if ctx1 == ctx2 {
            mark_type1
        } else {
            let m2 = unlocked_mark(ctx_at(contexts, ctx2, OP)?, ALL_MARKS);
            ctx_at(contexts, ctx1, OP)?.new_mark(mark_type1);
            lock_mark(ctx_at(contexts, ctx2, OP)?, mark_type1);
            m2
        };
        Ok(Self {
            context1: Some(ctx1),
            context2: Some(ctx2),
            mark_type1,
            mark_type2,
        })
    }

    /// Unlocks the claimed marks that are still locked and unbinds the lock
    /// (`~SmNewMarkAndLock`).
    pub fn release(&mut self, contexts: &mut [Context]) -> Result<()> {
        const OP: &str = "geom::context::MarkLock::release";
        if let Some(i) = self.context1 {
            let c = ctx_at(contexts, i, OP)?;
            if c.is_mark_locked(self.mark_type1) {
                unlock_mark(c, self.mark_type1);
            }
        }
        if let Some(i) = self.context2 {
            let c = ctx_at(contexts, i, OP)?;
            if c.is_mark_locked(self.mark_type2) {
                unlock_mark(c, self.mark_type2);
            }
        }
        *self = Self::unbound();
        Ok(())
    }

    /// The (first) claimed mark (`GetMarkType`).
    #[must_use]
    pub const fn mark_type(&self) -> MarkType {
        self.mark_type1
    }

    /// The mark claimed in the first context (`GetMarkType1`).
    #[must_use]
    pub const fn mark_type1(&self) -> MarkType {
        self.mark_type1
    }

    /// The mark claimed in the second context (`GetMarkType2`).
    #[must_use]
    pub const fn mark_type2(&self) -> MarkType {
        self.mark_type2
    }

    /// Index of the (first) bound context (`GetContext`).
    #[must_use]
    pub const fn context(&self) -> Option<usize> {
        self.context1
    }

    /// Index of the second bound context, if any (`GetContext2`).
    #[must_use]
    pub const fn context2(&self) -> Option<usize> {
        self.context2
    }

    /// Advances the claimed mark(s) while keeping them locked, invalidating every
    /// mark set so far (`SmNewMarkAndLock::NewMark`).
    pub fn new_mark(&self, contexts: &mut [Context]) -> Result<()> {
        const OP: &str = "geom::context::MarkLock::new_mark";
        let i1 = self.context1.ok_or(SolidError::InvalidInput {
            operation: OP,
            reason: "no context bound",
        })?;
        ctx_at(contexts, i1, OP)?.new_mark(self.mark_type1);
        if let Some(i2) = self.context2 {
            if i1 != i2 {
                ctx_at(contexts, i2, OP)?.new_mark(self.mark_type2);
            }
        }
        Ok(())
    }

    /// Rebinds the first context: unlocks the old first mark, then claims and locks
    /// (without advancing) the first unlocked mark of `contexts[ctx]` allowed by
    /// `mask`; returns it (`SmNewMarkAndLock::SetContext(SmContext*, SmMarkType)`).
    /// The second context binding is kept.
    pub fn set_context(
        &mut self,
        contexts: &mut [Context],
        ctx: usize,
        mask: i32,
    ) -> Result<MarkType> {
        const OP: &str = "geom::context::MarkLock::set_context";
        check_index(contexts, ctx, OP)?;
        if let Some(i) = self.context1 {
            if self.mark_type1 != MarkType::NoMark {
                unlock_mark(ctx_at(contexts, i, OP)?, self.mark_type1);
            }
        }
        self.context1 = Some(ctx);
        let c = ctx_at(contexts, ctx, OP)?;
        self.mark_type1 = unlocked_mark(c, mask);
        lock_mark(c, self.mark_type1);
        Ok(self.mark_type1)
    }

    /// Rebinds both contexts: unlocks the old marks, then claims and locks (without
    /// advancing) the first unlocked mark of each new context; returns the first
    /// (`SmNewMarkAndLock::SetContext(SmContext*, SmContext*)`).
    pub fn set_context_pair(
        &mut self,
        contexts: &mut [Context],
        ctx1: usize,
        ctx2: usize,
    ) -> Result<MarkType> {
        const OP: &str = "geom::context::MarkLock::set_context_pair";
        check_index(contexts, ctx1, OP)?;
        check_index(contexts, ctx2, OP)?;
        if let Some(i) = self.context1 {
            if self.mark_type1 != MarkType::NoMark {
                unlock_mark(ctx_at(contexts, i, OP)?, self.mark_type1);
            }
        }
        if let Some(i2) = self.context2 {
            if self.context1 != Some(i2) && self.mark_type2 != MarkType::NoMark {
                unlock_mark(ctx_at(contexts, i2, OP)?, self.mark_type2);
            }
        }
        self.context1 = Some(ctx1);
        self.context2 = Some(ctx2);
        let c1 = ctx_at(contexts, ctx1, OP)?;
        self.mark_type1 = unlocked_mark(c1, ALL_MARKS);
        lock_mark(c1, self.mark_type1);
        if ctx1 == ctx2 {
            self.mark_type2 = self.mark_type1;
        } else {
            let c2 = ctx_at(contexts, ctx2, OP)?;
            self.mark_type2 = unlocked_mark(c2, ALL_MARKS);
            lock_mark(c2, self.mark_type2);
        }
        Ok(self.mark_type1)
    }
}

/// Runs `f` with a mark of the kernel's context claimed and locked, then releases it:
/// the scoped `{ SmNewMarkAndLock sMarkLock(pContext, mask); ... }` idiom.
pub fn with_mark_lock<R>(
    k: &mut Kernel,
    mask: i32,
    f: impl FnOnce(&mut Kernel, MarkType) -> R,
) -> Result<R> {
    let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, mask)?;
    let r = f(k, lock.mark_type());
    lock.release(std::slice::from_mut(&mut k.context))?;
    Ok(r)
}

/// How mass-property computations treat faces, edges and vertices
/// (`SmMassPropertyBehavior`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassPropertyBehavior {
    /// Density of the material (`m_dDensity`, default 1).
    pub density: f64,
    /// Thickness given to faces (`m_dFaceThickness`, default 0).
    pub face_thickness: f64,
    /// Radius of the tube swept around edges (`m_dEdgeCrossSectionRadius`, default 0).
    pub edge_cross_section_radius: f64,
    /// Radius of the sphere around vertices (`m_dVertexSphereRadius`, default 0).
    pub vertex_sphere_radius: f64,
    /// Face area mode (`m_lFaceAreaBehavior`): 0 solid/void faces only, 1 every face
    /// once, 2 every face twice.
    pub face_area_behavior: u64,
    /// Face volume mode (`m_lFaceVolumeBehavior`): 0 region volumes, 1 thin shells.
    pub face_volume_behavior: u64,
}

impl Default for MassPropertyBehavior {
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0, 0.0, 0, 0)
    }
}

impl MassPropertyBehavior {
    /// Builds the options (`SmMassPropertyBehavior::SmMassPropertyBehavior`).
    #[must_use]
    pub const fn new(
        density: f64,
        face_thickness: f64,
        edge_cross_section_radius: f64,
        vertex_sphere_radius: f64,
        face_area_behavior: u64,
        face_volume_behavior: u64,
    ) -> Self {
        Self {
            density,
            face_thickness,
            edge_cross_section_radius,
            vertex_sphere_radius,
            face_area_behavior,
            face_volume_behavior,
        }
    }
}

/// Creates the default context in `slot` unless one exists (`SmApiCreateContext`).
pub fn create_context(slot: &mut Option<Context>) {
    if slot.is_none() {
        *slot = Some(Context::default());
    }
}

/// The context in `slot`, created with defaults first if empty
/// (`SmApiGetOrCreateContext`).
pub fn get_or_create_context(slot: &mut Option<Context>) -> &mut Context {
    slot.get_or_insert_with(Context::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [MarkType; 7] = [
        MarkType::NoMark,
        MarkType::Mark,
        MarkType::Mark2,
        MarkType::Mark3,
        MarkType::MarkIo,
        MarkType::MarkAssert,
        MarkType::AllMarks,
    ];

    #[test]
    fn mark_type_codes_round_trip() {
        for m in ALL {
            assert_eq!(MarkType::from_code(m.code()), Some(m));
        }
        assert_eq!(MarkType::from_code(3), None);
    }

    #[test]
    fn new_context_defaults() {
        let k = Kernel::default();
        let c = Context::default();
        for m in ALL {
            assert!(!c.is_mark_locked(m));
        }
        assert_eq!(c.current_mark(MarkType::Mark), 1);
        assert_eq!(c.current_mark(MarkType::NoMark), 2);
        assert_eq!(c.current_mark(MarkType::AllMarks), 1);
        assert!(!c.doing_boolean());
        assert_eq!(c.this_zone_tol_3d(), 1.0e-5);
        assert_eq!(c.this_model_size_estimate(&k), 50.0);
        assert!(c.user_notify_callback().is_none());
        assert!(c.sys_notify_callback().is_none());
    }

    #[test]
    fn model_size_sets_zone_tol() {
        let k = Kernel::default();
        let mut c = Context::new(1000.0, tol::USE_DEFAULT);
        assert_eq!(c.this_zone_tol_3d(), 1.0e-4);
        assert_eq!(c.this_model_size_estimate(&k), 1000.0);
        assert_eq!(c.set_this_model_size_estimate(0.01), 1.0e-6);
        assert_eq!(c.set_this_model_size_estimate(tol::USE_DEFAULT), 1.0e-5);
        assert_eq!(c.this_model_size_estimate(&k), 50.0);
        c.set_this_large_small_size_ratio(10.0);
        *c.doing_boolean_mut() = true;
        assert!(c.doing_boolean());
    }

    #[test]
    fn single_lock_cycle() {
        let mut cs = [Context::default()];
        let mut a = MarkLock::new(&mut cs, 0, ALL_MARKS).unwrap();
        assert_eq!(a.mark_type(), MarkType::Mark);
        assert_eq!(cs[0].current_mark(MarkType::Mark), 3);
        assert!(cs[0].is_mark_locked(MarkType::Mark));
        let mut b = MarkLock::new(&mut cs, 0, ALL_MARKS).unwrap();
        assert_eq!(b.mark_type(), MarkType::Mark2);
        b.new_mark(&mut cs).unwrap();
        assert_eq!(cs[0].current_mark(MarkType::Mark2), 5);
        b.release(&mut cs).unwrap();
        a.release(&mut cs).unwrap();
        assert!(!cs[0].is_mark_locked(MarkType::Mark));
        assert!(!cs[0].is_mark_locked(MarkType::Mark2));
        assert_eq!(cs[0].current_mark(MarkType::NoMark), 6);
        assert!(MarkLock::unbound().new_mark(&mut cs).is_err());
    }

    #[test]
    fn exhausted_marks_fall_back() {
        let mut cs = [Context::default()];
        let locks: Vec<MarkLock> = (0..5)
            .map(|_| MarkLock::new(&mut cs, 0, ALL_MARKS).unwrap())
            .collect();
        assert_eq!(locks[4].mark_type(), MarkType::MarkAssert);
        let x = MarkLock::new(&mut cs, 0, ALL_MARKS).unwrap();
        assert_eq!(x.mark_type(), MarkType::MarkIo);
        let y = MarkLock::new(&mut cs, 0, MarkType::Mark3.code()).unwrap();
        assert_eq!(y.mark_type(), MarkType::Mark3);
    }

    #[test]
    fn pair_lock_upstream_quirk() {
        let mut cs = [Context::default(), Context::default()];
        let _first = MarkLock::new(&mut cs, 1, ALL_MARKS).unwrap();
        let mut p = MarkLock::new_pair(&mut cs, 0, 1).unwrap();
        assert_eq!(p.mark_type1(), MarkType::Mark);
        assert_eq!(p.mark_type2(), MarkType::Mark2);
        // context 0's mark advanced twice; context 1 gets mark type 1 locked
        assert_eq!(cs[0].current_mark(MarkType::Mark), 5);
        assert!(cs[1].is_mark_locked(MarkType::Mark));
        assert!(!cs[1].is_mark_locked(MarkType::Mark2));
        p.release(&mut cs).unwrap();
        assert!(!cs[0].is_mark_locked(MarkType::Mark));
        assert!(cs[1].is_mark_locked(MarkType::Mark));
        let same = MarkLock::new_pair(&mut cs, 0, 0).unwrap();
        assert_eq!(same.mark_type2(), same.mark_type1());
        assert!(MarkLock::new_pair(&mut cs, 0, 2).is_err());
    }

    #[test]
    fn set_context_rebinds() {
        let mut cs = [Context::default(), Context::default()];
        let mut l = MarkLock::unbound();
        assert_eq!(
            l.set_context(&mut cs, 1, ALL_MARKS).unwrap(),
            MarkType::Mark
        );
        assert_eq!(cs[1].current_mark(MarkType::Mark), 1);
        assert_eq!(l.set_context_pair(&mut cs, 0, 1).unwrap(), MarkType::Mark);
        assert!(cs[0].is_mark_locked(MarkType::Mark));
        assert!(cs[1].is_mark_locked(MarkType::Mark));
        l.release(&mut cs).unwrap();
        assert!(!cs[0].is_mark_locked(MarkType::Mark));
        assert!(!cs[1].is_mark_locked(MarkType::Mark));
    }

    #[test]
    fn scoped_kernel_lock() {
        let mut k = Kernel::default();
        let m = with_mark_lock(&mut k, ALL_MARKS, |k, m| {
            assert!(k.context.is_mark_locked(m));
            m
        })
        .unwrap();
        assert_eq!(m, MarkType::Mark);
        assert!(!k.context.is_mark_locked(m));
    }

    #[test]
    fn api_context_slot() {
        let mut slot = None;
        create_context(&mut slot);
        get_or_create_context(&mut slot).set_doing_boolean(true);
        create_context(&mut slot);
        assert!(get_or_create_context(&mut slot).doing_boolean());
        let mp = MassPropertyBehavior::default();
        assert_eq!(mp.density, 1.0);
        assert_eq!(mp.face_area_behavior, 0);
    }
}
