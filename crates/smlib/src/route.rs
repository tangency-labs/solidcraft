//! Which kernel answers each operation: the C++ SMLib fork, or solidcore, its Rust port.
//!
//! [`Op::route`] is the routing table. An operation moves to solidcore only once solidcore
//! matches the C++ kernel on the kernel-case rig (`cargo xtask kernel-cases --kernel both`), and
//! the commit that moves it records the scores. A call that fails on solidcore fails: there is no
//! fallback to C++ at run time, which would hide solidcore's bugs.
//!
//! `SOLIDCRAFT_KERNEL=smlib|solidcore` sends every operation to one kernel for the whole process
//! (the rig, the test matrix); otherwise the preference ([`set_choice`]) does, and its default
//! follows the table.
//!
//! `SOLIDCRAFT_KERNEL_ROUTES=<op>=<backend>[,…]` (for example `boolean=solidcore,measure=solidcore`)
//! then moves single families on top of that choice, so the rig can score one family at a time
//! ([`Overrides`]). `.smb` files stay on SMLib whatever either says.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// A kernel an operation runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The C++ SMLib fork (tangency-labs/smlib) behind its C API.
    Smlib,
    /// solidcore (tangency-labs/solidcore), the Rust port of SMLib.
    Solidcore,
}

impl Backend {
    /// The id `SOLIDCRAFT_KERNEL`, preferences and reports spell it with.
    pub fn id(self) -> &'static str {
        match self {
            Backend::Smlib => "smlib",
            Backend::Solidcore => "solidcore",
        }
    }

    /// Back from an id; anything else is `None`.
    pub fn from_id(id: &str) -> Option<Backend> {
        [Backend::Smlib, Backend::Solidcore].into_iter().find(|b| b.id() == id)
    }
}

/// The operation families the table routes, in the order they move to solidcore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// A schema 4 record into a kernel body, and back (`from_record`, `to_native`).
    Convert,
    /// Boxes, cylinders, spheres, cones and tori.
    Primitive,
    /// Copies and affine transforms.
    Transform,
    /// Volume, area, mass properties, bounds, counts, distances.
    Measure,
    /// The body checker.
    Check,
    /// Booleans, unions, 2D booleans, imprints.
    Boolean,
    /// Fillets and chamfers, with setbacks and variable radii.
    Blend,
    /// Shell, face offset, thicken, sheet offset.
    Offset,
    /// Split, trim, untrim, extend, merge faces, draft, stitch.
    Split,
    /// Extrusions, revolutions, sweeps and lofts.
    Sweep,
    /// Face and edge history of an operation.
    History,
    /// Surface-surface intersection.
    Ssi,
    /// Sheet metal: flanges and flat patterns.
    Flat,
    /// Native `.smb` files (C++ only: solidcore reads no SMLib file).
    File,
}

impl Op {
    /// Every family, in table order.
    pub const ALL: [Op; 14] = [
        Op::Convert,
        Op::Primitive,
        Op::Transform,
        Op::Measure,
        Op::Check,
        Op::Boolean,
        Op::Blend,
        Op::Offset,
        Op::Split,
        Op::Sweep,
        Op::History,
        Op::Ssi,
        Op::Flat,
        Op::File,
    ];

    /// The lower-case name `diagnostics.kernel` and `SOLIDCRAFT_KERNEL_ROUTES` spell it with.
    pub fn id(self) -> &'static str {
        match self {
            Op::Convert => "convert",
            Op::Primitive => "primitive",
            Op::Transform => "transform",
            Op::Measure => "measure",
            Op::Check => "check",
            Op::Boolean => "boolean",
            Op::Blend => "blend",
            Op::Offset => "offset",
            Op::Split => "split",
            Op::Sweep => "sweep",
            Op::History => "history",
            Op::Ssi => "ssi",
            Op::Flat => "flat",
            Op::File => "file",
        }
    }

    /// Back from [`Op::id`]; anything else is `None`.
    pub fn from_id(id: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.id() == id)
    }

    /// Where this family sits in [`Op::ALL`].
    fn index(self) -> usize {
        Op::ALL.iter().position(|op| *op == self).unwrap_or(0)
    }

    /// The routing table: the kernel each family runs on when nothing overrides it. A family
    /// stays on SMLib until the rig shows solidcore's parity for it.
    pub fn route(self) -> Backend {
        match self {
            // At parity on the rig, routed alone: 469 of 469 cases agree (solidcore 3546c9bc); as
            // the default, 449 of 479 pass against SMLib-only 445, none worse.
            Op::Primitive => Backend::Solidcore,
            // On the rig, routed alone: 450 of 479 pass against SMLib's 449; 478 agree, and the
            // other (fillets/1557) fails on SMLib and passes on solidcore. As the default with
            // primitives (solidcore 3c17ddb1): 450 of 479 against SMLib-only 449, none worse.
            Op::Convert => Backend::Solidcore,
            Op::Transform
            | Op::Measure
            | Op::Check
            | Op::Boolean
            | Op::Blend
            | Op::Offset
            | Op::Split
            | Op::Sweep
            | Op::History
            | Op::Ssi
            | Op::Flat
            | Op::File => Backend::Smlib,
        }
    }

    /// The kernel this family runs on now: `SOLIDCRAFT_KERNEL_ROUTES` for it, else
    /// `SOLIDCRAFT_KERNEL`, else the preference, else [`Op::route`]. `.smb` files stay on SMLib
    /// whatever is chosen.
    pub fn backend(self) -> Backend {
        self.backend_under(choice())
    }

    /// The kernel this family runs on under `c`, whatever is in force now: what [`Op::backend`]
    /// answers once `c` is the choice (`SOLIDCRAFT_KERNEL_ROUTES` still applies on top).
    pub fn backend_under(self, c: Choice) -> Backend {
        self.backend_with(c, overrides())
    }

    /// The kernel this family runs on under `c` with the overrides `o`, read from neither the
    /// environment nor the preference. `.smb` files stay on SMLib whatever is chosen.
    pub fn backend_with(self, c: Choice, o: &Overrides) -> Backend {
        if self == Op::File {
            return Backend::Smlib;
        }
        if let Some(b) = o.get(self) {
            return b;
        }
        match c {
            Choice::Routed => self.route(),
            Choice::Only(b) => b,
        }
    }
}

/// What answers the operations: the routing table, or one kernel for all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    /// Each family on its kernel in [`Op::route`].
    #[default]
    Routed,
    /// Every family on this kernel.
    Only(Backend),
}

impl Choice {
    /// `routed`, `smlib` or `solidcore`.
    pub fn id(self) -> &'static str {
        match self {
            Choice::Routed => "routed",
            Choice::Only(b) => b.id(),
        }
    }

    /// Back from [`Choice::id`]; anything else is `None`.
    pub fn from_id(id: &str) -> Option<Choice> {
        if id == "routed" { Some(Choice::Routed) } else { Backend::from_id(id).map(Choice::Only) }
    }
}

/// `SOLIDCRAFT_KERNEL`, read once. An unknown value is no override (and says so in the log).
fn environment() -> Option<Choice> {
    static ENV: OnceLock<Option<Choice>> = OnceLock::new();
    *ENV.get_or_init(|| {
        let v = std::env::var("SOLIDCRAFT_KERNEL").ok()?;
        let c = Choice::from_id(&v);
        if c.is_none() {
            log::warn!(target: "smlib", "SOLIDCRAFT_KERNEL={v}: not smlib, solidcore or routed; ignored");
        }
        c
    })
}

/// Families moved to a kernel one by one, on top of the choice: `SOLIDCRAFT_KERNEL_ROUTES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Overrides([Option<Backend>; Op::ALL.len()]);

impl Overrides {
    /// No family moved.
    pub const NONE: Overrides = Overrides([None; Op::ALL.len()]);

    /// Longest `SOLIDCRAFT_KERNEL_ROUTES` read; a longer one is ignored whole.
    pub const MAX_LEN: usize = 4096;
    /// Items read; the rest are ignored.
    pub const MAX_ITEMS: usize = 64;
    /// Longest item read; a longer one is ignored.
    const MAX_ITEM_LEN: usize = 64;

    /// The kernel `op` is moved to, if any.
    pub fn get(&self, op: Op) -> Option<Backend> {
        self.0.get(op.index()).copied().flatten()
    }

    /// Whether no family is moved.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// Every moved family and its kernel, in table order.
    pub fn iter(&self) -> impl Iterator<Item = (Op, Backend)> + '_ {
        Op::ALL.into_iter().filter_map(|op| self.get(op).map(|b| (op, b)))
    }

    /// A `SOLIDCRAFT_KERNEL_ROUTES` value: comma-separated `<op>=<smlib|solidcore>`, spaces
    /// around each part allowed, a later item for the same family winning. Returns the overrides
    /// and a warning for each item ignored: an unknown family or kernel, an empty or overlong
    /// item, `file` (always SMLib), items past [`Overrides::MAX_ITEMS`], or a whole value longer
    /// than [`Overrides::MAX_LEN`].
    pub fn parse(text: &str) -> (Overrides, Vec<String>) {
        let mut o = Overrides::NONE;
        let mut warnings = Vec::new();
        if text.len() > Self::MAX_LEN {
            warnings.push(format!("{} bytes, longer than {}; ignored", text.len(), Self::MAX_LEN));
            return (o, warnings);
        }
        let mut items = text.split(',');
        for item in items.by_ref().take(Self::MAX_ITEMS) {
            let item = item.trim();
            let shown: String = item.chars().take(Self::MAX_ITEM_LEN).collect();
            if item.is_empty() {
                warnings.push("an empty item; ignored".into());
                continue;
            }
            if item.len() > Self::MAX_ITEM_LEN {
                warnings.push(format!("`{shown}…`: longer than {} bytes; ignored", Self::MAX_ITEM_LEN));
                continue;
            }
            let Some((op, backend)) = item.split_once('=') else {
                warnings.push(format!("`{shown}`: not <op>=<smlib|solidcore>; ignored"));
                continue;
            };
            let Some(op) = Op::from_id(op.trim()) else {
                warnings.push(format!("`{shown}`: no operation family `{}`; ignored", op.trim()));
                continue;
            };
            let Some(backend) = Backend::from_id(backend.trim()) else {
                warnings.push(format!("`{shown}`: `{}` is not smlib or solidcore; ignored", backend.trim()));
                continue;
            };
            if op == Op::File {
                warnings.push(format!("`{shown}`: .smb files stay on smlib; ignored"));
                continue;
            }
            if let Some(slot) = o.0.get_mut(op.index()) {
                *slot = Some(backend);
            }
        }
        if items.next().is_some() {
            warnings.push(format!("more than {} items; the rest ignored", Self::MAX_ITEMS));
        }
        (o, warnings)
    }
}

/// `SOLIDCRAFT_KERNEL_ROUTES`, read once; each item it ignores says so in the log.
pub fn overrides() -> &'static Overrides {
    static ENV: OnceLock<Overrides> = OnceLock::new();
    ENV.get_or_init(|| {
        let Some(v) = std::env::var_os("SOLIDCRAFT_KERNEL_ROUTES") else { return Overrides::NONE };
        let v = v.to_string_lossy();
        let (o, warnings) = Overrides::parse(&v);
        for w in warnings {
            log::warn!(target: "smlib", "SOLIDCRAFT_KERNEL_ROUTES: {w}");
        }
        o
    })
}

static PREFERENCE: AtomicU8 = AtomicU8::new(0);

/// The choice in force: `SOLIDCRAFT_KERNEL` when it is set, else the preference.
pub fn choice() -> Choice {
    environment().unwrap_or_else(|| match PREFERENCE.load(Ordering::Relaxed) {
        1 => Choice::Only(Backend::Smlib),
        2 => Choice::Only(Backend::Solidcore),
        _ => Choice::Routed,
    })
}

/// Sets the preference. `SOLIDCRAFT_KERNEL`, when set, still wins.
pub fn set_choice(c: Choice) {
    let v = match c {
        Choice::Routed => 0,
        Choice::Only(Backend::Smlib) => 1,
        Choice::Only(Backend::Solidcore) => 2,
    };
    PREFERENCE.store(v, Ordering::Relaxed);
}

/// Whether `SOLIDCRAFT_KERNEL` overrides the preference in this process.
pub fn overridden() -> bool {
    environment().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for c in [Choice::Routed, Choice::Only(Backend::Smlib), Choice::Only(Backend::Solidcore)] {
            assert_eq!(Choice::from_id(c.id()), Some(c));
        }
        assert_eq!(Choice::from_id("truck"), None);
    }

    #[test]
    fn files_stay_on_smlib() {
        assert_eq!(Op::File.backend(), Backend::Smlib);
    }

    /// Each choice sends the families where it says, read without setting the process-wide
    /// preference: the other tests in this binary build bodies meanwhile, and one flipped to
    /// solidcore for an instant would build theirs there.
    #[test]
    fn each_choice_routes_every_family_without_touching_the_preference() {
        for op in Op::ALL {
            let want = |b: Backend| if op == Op::File { Backend::Smlib } else { b };
            let none = &Overrides::NONE;
            assert_eq!(op.backend_with(Choice::Routed, none), op.route(), "{op:?}");
            assert_eq!(op.backend_with(Choice::Only(Backend::Smlib), none), want(Backend::Smlib), "{op:?}");
            assert_eq!(op.backend_with(Choice::Only(Backend::Solidcore), none), want(Backend::Solidcore), "{op:?}");
            assert_eq!(op.backend_under(Choice::Routed), op.backend_with(Choice::Routed, overrides()), "{op:?}");
        }
    }

    #[test]
    fn op_ids_are_the_lower_case_names_and_round_trip() {
        for op in Op::ALL {
            assert_eq!(op.id(), format!("{op:?}").to_lowercase());
            assert_eq!(Op::from_id(op.id()), Some(op));
        }
        assert_eq!(Op::from_id("Boolean"), None);
        assert_eq!(Op::from_id(""), None);
    }

    /// Overrides move only the families they name, on top of any choice; read from a string, not
    /// the process's environment.
    #[test]
    fn overrides_move_single_families_on_top_of_the_choice() {
        let (o, warnings) = Overrides::parse("boolean=solidcore, measure = solidcore");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(o.iter().collect::<Vec<_>>(), [(Op::Measure, Backend::Solidcore), (Op::Boolean, Backend::Solidcore)]);
        for op in Op::ALL {
            let moved = matches!(op, Op::Boolean | Op::Measure);
            let want = if moved { Backend::Solidcore } else { op.route() };
            assert_eq!(op.backend_with(Choice::Routed, &o), want, "{op:?}");
            let want = if moved { Backend::Solidcore } else { Backend::Smlib };
            assert_eq!(op.backend_with(Choice::Only(Backend::Smlib), &o), want, "{op:?}");
        }
        // Back to SMLib under an all-solidcore choice; the later item wins.
        let (o, _) = Overrides::parse("check=solidcore,check=smlib");
        assert_eq!(Op::Check.backend_with(Choice::Only(Backend::Solidcore), &o), Backend::Smlib);
        assert_eq!(Op::Sweep.backend_with(Choice::Only(Backend::Solidcore), &o), Backend::Solidcore);
        assert!(Overrides::parse("").0.is_empty());
    }

    /// Hostile values: every bad item is ignored with a warning, the good ones still apply, and
    /// nothing panics or reads past the caps.
    #[test]
    fn overrides_ignore_bad_items_with_a_warning() {
        let (o, w) = Overrides::parse("file=solidcore,truck=solidcore,boolean=truck,,=,boolean,BOOLEAN=solidcore,blend=solidcore=x,sweep=solidcore");
        assert_eq!(o.iter().collect::<Vec<_>>(), [(Op::Sweep, Backend::Solidcore)]);
        assert_eq!(w.len(), 8, "{w:?}");
        assert_eq!(Op::File.backend_with(Choice::Routed, &o), Backend::Smlib);
        let (o, w) = Overrides::parse(&"x".repeat(Overrides::MAX_LEN + 1));
        assert!(o.is_empty() && w.len() == 1, "{w:?}");
        let (o, w) = Overrides::parse(&format!("{}=solidcore,blend=solidcore", "a".repeat(200)));
        assert_eq!((o.get(Op::Blend), w.len()), (Some(Backend::Solidcore), 1), "{w:?}");
        let many = vec!["measure=solidcore"; Overrides::MAX_ITEMS].join(",") + ",blend=solidcore";
        let (o, w) = Overrides::parse(&many);
        assert_eq!((o.get(Op::Measure), o.get(Op::Blend), w.len()), (Some(Backend::Solidcore), None, 1), "{w:?}");
        for hostile in [",,,,", "\0=\0", "boolean=solidcore\0", "é=ü,ü", "=solidcore", "boolean="] {
            let (o, w) = Overrides::parse(hostile);
            assert!(o.is_empty() && !w.is_empty(), "{hostile:?}: {w:?}");
        }
    }

    /// The preference is what `backend` follows. Only a choice that routes every family as the
    /// table does is set here, so no other test's body changes kernel.
    #[test]
    fn the_preference_routes_unless_the_environment_overrides() {
        if overridden() || !overrides().is_empty() {
            return;
        }
        let harmless = [Choice::Only(Backend::Smlib), Choice::Only(Backend::Solidcore)]
            .into_iter()
            .find(|c| Op::ALL.iter().all(|op| op.backend_under(*c) == op.route()));
        if let Some(c) = harmless {
            set_choice(c);
            assert_eq!(choice(), c);
            assert!(Op::ALL.iter().all(|op| op.backend() == op.backend_under(c)));
        }
        set_choice(Choice::Routed);
        assert_eq!(choice(), Choice::Routed);
        assert!(Op::ALL.iter().all(|op| op.backend() == op.route()));
    }
}
