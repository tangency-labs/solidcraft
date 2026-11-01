//! The largest files SolidCraft reads (#4086). [`crate::MAX_STEP_BYTES`] and
//! [`crate::MAX_DESIGN_BYTES`] are the defaults; the desktop app may raise them (Preferences ›
//! General › Largest file to open, saved by the app) or for one run (`--max-file-size`), never
//! past [`CEILING`]. Every read site asks [`max_step_bytes`] / [`max_design_bytes`] for the limit
//! in force, so the size check still happens before a file is read. In the browser (wasm32) the
//! defaults always apply: the web app's address space can't hold more.

use std::sync::atomic::{AtomicU64, Ordering};

/// One megabyte as the limits count it (2^20 bytes, like the defaults).
pub const MB: u64 = 1 << 20;
/// One gigabyte (2^30 bytes).
pub const GB: u64 = 1 << 30;
/// The highest limit either kind may be raised to.
pub const CEILING: u64 = 16 * GB;
/// Whether this build lets the limits rise: not in the browser.
pub const ADJUSTABLE: bool = cfg!(not(target_arch = "wasm32"));

/// The two kinds of file with a size limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// STEP, IGES and the other CAD files read as an Import feature.
    Step,
    /// Design files (`.sc`, older `.solidcraft`, JSON).
    Design,
}

impl Kind {
    /// The limit in force for this kind, in bytes.
    pub fn limit(self) -> u64 {
        let l = effective();
        match self {
            Kind::Step => l.step,
            Kind::Design => l.design,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Kind::Step => "STEP and import files",
            Kind::Design => "design files",
        }
    }
}

/// The largest STEP/import file and the largest design file, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLimits {
    pub step: u64,
    pub design: u64,
}

impl Default for FileLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl FileLimits {
    /// The limits SolidCraft has always had.
    pub const DEFAULT: Self = Self { step: crate::MAX_STEP_BYTES as u64, design: crate::MAX_DESIGN_BYTES as u64 };

    /// Limits from whole megabytes, as the preference keeps them.
    pub fn from_mb(step_mb: u64, design_mb: u64) -> Self {
        Self { step: step_mb.saturating_mul(MB), design: design_mb.saturating_mul(MB) }
    }

    /// One size for both kinds (`--max-file-size`). A kind whose default is larger keeps its
    /// default: the flag only ever raises a limit.
    pub fn both(bytes: u64) -> Self {
        Self { step: bytes.max(Self::DEFAULT.step), design: bytes.max(Self::DEFAULT.design) }
    }

    /// These limits, if each is between its default and [`ceiling`].
    pub fn validate(self) -> Result<Self, String> {
        for (kind, value, default) in [(Kind::Step, self.step, Self::DEFAULT.step), (Kind::Design, self.design, Self::DEFAULT.design)] {
            if value < default || value > ceiling() {
                return Err(format!(
                    "the limit for {} must be between {} and {}, not {}",
                    kind.what(),
                    describe(default),
                    describe(ceiling()),
                    describe(value)
                ));
            }
        }
        Ok(self)
    }

    /// These limits brought into range (a hand-edited settings file).
    pub fn clamped(self) -> Self {
        Self {
            step: self.step.clamp(Self::DEFAULT.step, ceiling().max(Self::DEFAULT.step)),
            design: self.design.clamp(Self::DEFAULT.design, ceiling().max(Self::DEFAULT.design)),
        }
    }
}

/// [`CEILING`], or less where the platform can't address that much.
pub fn ceiling() -> u64 {
    if !ADJUSTABLE {
        return FileLimits::DEFAULT.step.max(FileLimits::DEFAULT.design);
    }
    CEILING.min(u64::try_from(usize::MAX).unwrap_or(u64::MAX))
}

static PREFERRED_STEP: AtomicU64 = AtomicU64::new(FileLimits::DEFAULT.step);
static PREFERRED_DESIGN: AtomicU64 = AtomicU64::new(FileLimits::DEFAULT.design);
/// `--max-file-size` for this run, in bytes; 0 when not given.
static OVERRIDE: AtomicU64 = AtomicU64::new(0);

/// Use these limits from now on (the saved preference). Refused, changing nothing, when out of
/// range, and in the browser for anything but the defaults.
pub fn set_preferred(limits: FileLimits) -> Result<(), String> {
    if !ADJUSTABLE && limits != FileLimits::DEFAULT {
        return Err(BROWSER_NOTE.into());
    }
    let limits = limits.validate()?;
    PREFERRED_STEP.store(limits.step, Ordering::Relaxed);
    PREFERRED_DESIGN.store(limits.design, Ordering::Relaxed);
    Ok(())
}

/// The saved preference's limits, whether or not `--max-file-size` overrides them.
pub fn preferred() -> FileLimits {
    FileLimits { step: PREFERRED_STEP.load(Ordering::Relaxed), design: PREFERRED_DESIGN.load(Ordering::Relaxed) }
}

/// `--max-file-size` for this run (`None` clears it). Refused above [`ceiling`], and in the browser.
pub fn set_override(bytes: Option<u64>) -> Result<(), String> {
    match bytes {
        None => OVERRIDE.store(0, Ordering::Relaxed),
        Some(_) if !ADJUSTABLE => return Err(BROWSER_NOTE.into()),
        Some(b) => {
            check_flag(b)?;
            OVERRIDE.store(b, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// The `--max-file-size` this run started with, in bytes.
pub fn override_bytes() -> Option<u64> {
    Some(OVERRIDE.load(Ordering::Relaxed)).filter(|&b| b > 0 && ADJUSTABLE)
}

/// The limits in force: `--max-file-size` if given, else the preference; the defaults in the browser.
pub fn effective() -> FileLimits {
    resolve(ADJUSTABLE, preferred(), OVERRIDE.load(Ordering::Relaxed))
}

/// The limits in force from the preference and the flag (0: none); always the defaults where the
/// build can't raise them (`adjustable` false: the browser).
fn resolve(adjustable: bool, preferred: FileLimits, flag: u64) -> FileLimits {
    if !adjustable {
        FileLimits::DEFAULT
    } else if flag > 0 {
        FileLimits::both(flag)
    } else {
        preferred
    }
}

/// The largest STEP or import file read now.
pub fn max_step_bytes() -> usize {
    usize::try_from(effective().step).unwrap_or(usize::MAX)
}

/// The largest design file read (and written) now.
pub fn max_design_bytes() -> usize {
    usize::try_from(effective().design).unwrap_or(usize::MAX)
}

const BROWSER_NOTE: &str = "the web app can't open larger files: a browser tab can't hold them";

/// How to raise a limit, for the messages that refuse a file.
pub fn how_to_raise() -> &'static str {
    if ADJUSTABLE {
        "Raise the limit in Preferences > General > Largest file to open, or start SolidCraft with --max-file-size (for example --max-file-size 2GB)."
    } else {
        "The web app can't open larger files; the desktop app can raise the limit."
    }
}

/// The message refusing a `size`-byte file of this kind.
pub fn too_large(kind: Kind, size: u64) -> String {
    format!("file too large ({}; the limit for {} is {}). {}", describe(size), kind.what(), describe(kind.limit()), how_to_raise())
}

/// Refuse a `size`-byte file of this kind before it is read.
pub fn check(kind: Kind, size: u64) -> Result<(), String> {
    if size > kind.limit() { Err(too_large(kind, size)) } else { Ok(()) }
}

/// [`crate::vfs::read`] with this kind's limit; a file over it is refused, before reading, with
/// [`too_large`]'s message.
pub fn read(path: &str, kind: Kind) -> std::io::Result<Vec<u8>> {
    if let Ok(size) = crate::vfs::len(path) {
        check(kind, size).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    }
    crate::vfs::read(path, usize::try_from(kind.limit()).unwrap_or(usize::MAX)).map_err(|e| explain(kind, e))
}

/// A read error from [`crate::vfs`] at this kind's limit, with the refusal of a file over it
/// saying how to raise the limit.
pub fn explain(kind: Kind, error: std::io::Error) -> std::io::Error {
    if error.kind() == std::io::ErrorKind::InvalidData && error.to_string() == "file too large" {
        let message = format!("file too large (over the {} limit for {}). {}", describe(kind.limit()), kind.what(), how_to_raise());
        std::io::Error::new(std::io::ErrorKind::InvalidData, message)
    } else {
        error
    }
}

/// A size as people write it: whole gigabytes as `2 GB`, anything else in megabytes.
pub fn describe(bytes: u64) -> String {
    if bytes >= GB && bytes.is_multiple_of(GB) {
        format!("{} GB", bytes / GB)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes.div_ceil(MB))
    }
}

/// A size such as `2GB`, `1.5 GB`, `1500MB` or `1500M` (megabytes and gigabytes of 2^20 and 2^30
/// bytes, any case), in bytes. A number needs its unit.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let (number, unit) = (t.get(..split).unwrap_or(""), t.get(split..).unwrap_or("").trim());
    let unit_bytes = match unit.to_ascii_uppercase().as_str() {
        "MB" | "M" | "MIB" => MB,
        "GB" | "G" | "GIB" => GB,
        "" => return Err(format!("`{t}` needs a unit: write it as, for example, 2GB or 1500MB")),
        _ => return Err(format!("`{t}`: the unit must be MB or GB")),
    };
    let value: f64 = number.parse().map_err(|_| format!("`{t}` is not a size such as 2GB or 1500MB"))?;
    if !value.is_finite() || value <= 0.0 || value > 1e9 {
        return Err(format!("`{t}` is not a size such as 2GB or 1500MB"));
    }
    let bytes = (value * unit_bytes as f64).round();
    // Below 2^63, so the conversion is exact enough and never saturates.
    if bytes >= 9.0e18 {
        return Err(format!("`{t}` is over the {} limit", describe(ceiling())));
    }
    Ok(bytes as u64)
}

/// A `--max-file-size` value must be at least 1 MB and at most [`ceiling`].
fn check_flag(bytes: u64) -> Result<u64, String> {
    if bytes < MB {
        return Err("--max-file-size must be at least 1MB".into());
    }
    if bytes > ceiling() {
        return Err(format!("--max-file-size can be at most {}, not {}", describe(ceiling()), describe(bytes)));
    }
    Ok(bytes)
}

/// Parse a `--max-file-size` value ([`parse_size`], then the range check).
pub fn parse_flag(text: &str) -> Result<u64, String> {
    if !ADJUSTABLE {
        return Err(BROWSER_NOTE.into());
    }
    check_flag(parse_size(text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_limits_there_always_were() {
        assert_eq!(FileLimits::default(), FileLimits { step: 512 * MB, design: 256 * MB });
        // Nothing in this test binary raises the limits: the read sites see the defaults.
        assert_eq!((max_step_bytes(), max_design_bytes()), (crate::MAX_STEP_BYTES, crate::MAX_DESIGN_BYTES));
        assert_eq!(FileLimits::from_mb(512, 256), FileLimits::DEFAULT);
        assert!(FileLimits::DEFAULT.validate().is_ok());
    }

    #[test]
    fn sizes_parse_with_their_units_and_bad_ones_are_refused() {
        for (text, bytes) in
            [("2GB", 2 * GB), ("2 gb", 2 * GB), ("1.5GB", 3 * GB / 2), ("1500MB", 1500 * MB), ("1500m", 1500 * MB), ("16GiB", 16 * GB)]
        {
            assert_eq!(parse_size(text), Ok(bytes), "{text}");
        }
        for text in ["", "2", "GB", "2TB", "-1GB", "1e3MB", "0GB", "NaNGB", "1.2.3GB", "99999999999GB"] {
            assert!(parse_size(text).is_err(), "{text}");
        }
        if ADJUSTABLE {
            assert_eq!(parse_flag("16GB"), Ok(CEILING));
            let over = parse_flag("17GB").unwrap_err();
            assert!(over.contains("at most 16 GB"), "{over}");
            assert!(parse_flag("0.5MB").is_err());
        }
    }

    #[test]
    fn limits_are_refused_outside_the_default_to_the_ceiling() {
        assert!(FileLimits { step: 4 * GB, design: 2 * GB }.validate().is_ok() || !ADJUSTABLE);
        let over = FileLimits { step: CEILING + MB, ..FileLimits::DEFAULT }.validate().unwrap_err();
        assert!(over.contains("STEP and import files"), "{over}");
        assert!(FileLimits { design: 100 * MB, ..FileLimits::DEFAULT }.validate().is_err());
        let wild = FileLimits { step: u64::MAX, design: 0 }.clamped();
        assert_eq!(wild, FileLimits { step: ceiling().max(FileLimits::DEFAULT.step), design: FileLimits::DEFAULT.design });
        // The flag never lowers a limit below its default.
        assert_eq!(FileLimits::both(300 * MB), FileLimits { step: 512 * MB, design: 300 * MB });
        assert_eq!(FileLimits::both(2 * GB), FileLimits { step: 2 * GB, design: 2 * GB });
    }

    #[test]
    fn a_refused_file_says_how_to_raise_the_limit() {
        let message = too_large(Kind::Step, 600 * MB);
        assert!(message.contains("600 MB") && message.contains("512 MB"), "{message}");
        if ADJUSTABLE {
            assert!(message.contains("Preferences > General > Largest file to open") && message.contains("--max-file-size"), "{message}");
        }
        assert!(check(Kind::Design, 256 * MB).is_ok() && check(Kind::Design, 256 * MB + 1).is_err());
        assert_eq!(describe(2 * GB), "2 GB");
        assert_eq!(describe(1536 * MB), "1.5 GB");
    }

    #[test]
    fn the_flag_overrides_the_preference_and_the_browser_keeps_the_defaults() {
        let preferred = FileLimits { step: 4 * GB, design: GB };
        assert_eq!(resolve(true, preferred, 0), preferred);
        assert_eq!(resolve(true, preferred, 2 * GB), FileLimits { step: 2 * GB, design: 2 * GB });
        // The browser (wasm32) build: neither the preference nor a flag raises anything.
        assert_eq!(resolve(false, preferred, 2 * GB), FileLimits::DEFAULT);
        if !ADJUSTABLE {
            assert!(set_preferred(preferred).is_err() && set_override(Some(2 * GB)).is_err());
            assert_eq!(effective(), FileLimits::DEFAULT);
        }
    }
}
