//! Bounded reading and writing of the wire's numbers: unsigned LEB128 varints, zig-zag for signed
//! ones. A reader never reads past its slice and never allocates; every malformed byte is an
//! error.

use crate::{WireError, require};

/// Appends `v` as an unsigned LEB128 varint (1 to 10 bytes).
pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Signed to unsigned, small magnitudes to small numbers: 0, -1, 1, -2, … become 0, 1, 2, 3, ….
pub(crate) fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

pub(crate) fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Reads a byte slice front to back.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    /// Bytes not read yet.
    pub(crate) fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    /// Where the reader is, for [`since`](Reader::since).
    pub(crate) fn mark(&self) -> usize {
        self.at
    }

    /// The bytes read since `mark`.
    pub(crate) fn since(&self, mark: usize) -> &'a [u8] {
        self.bytes.get(mark..self.at).unwrap_or_default()
    }

    /// The bytes not read yet; the reader is then at the end.
    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let rest = self.bytes.get(self.at..).unwrap_or_default();
        self.at = self.bytes.len();
        rest
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.at.checked_add(n).ok_or(WireError("truncated"))?;
        let head = self.bytes.get(self.at..end).ok_or(WireError("truncated"))?;
        self.at = end;
        Ok(head)
    }

    pub(crate) fn byte(&mut self) -> Result<u8, WireError> {
        match self.take(1)? {
            &[b] => Ok(b),
            _ => Err(WireError("truncated")),
        }
    }

    /// An unsigned LEB128 varint in its shortest form: a longer spelling of the same number is
    /// refused, so equal numbers are equal bytes and hashes over the bytes depend on the values
    /// only.
    pub(crate) fn varint(&mut self) -> Result<u64, WireError> {
        let mut v = 0u64;
        for k in 0..10 {
            let b = self.byte()?;
            let bits = u64::from(b & 0x7f);
            // The tenth byte holds bit 63 only.
            require(k < 9 || bits <= 1, "varint over 64 bits")?;
            v |= bits << (7 * k);
            if b & 0x80 == 0 {
                require(k == 0 || b != 0, "varint not in its shortest form")?;
                return Ok(v);
            }
        }
        Err(WireError("varint over 64 bits"))
    }

    /// A count of items each at least `min_bytes` long: at most `cap`, and no more than the bytes
    /// left could hold, so nothing is reserved for items that can't be there.
    pub(crate) fn count(&mut self, cap: usize, min_bytes: usize, what: &'static str) -> Result<usize, WireError> {
        let n = usize::try_from(self.varint()?).map_err(|_| WireError(what))?;
        require(n <= cap && n.saturating_mul(min_bytes) <= self.left(), what)?;
        Ok(n)
    }
}
