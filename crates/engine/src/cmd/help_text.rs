//! Parameter help with a number read from a constant at compile time, so the text cannot drift from
//! the limit it states (Cell Grid's and Living Hinge's caps on the cuts made in one operation).

/// The number of decimal digits of `n`.
pub(crate) const fn decimal_len(mut n: usize) -> usize {
    let mut len = 1;
    while n >= 10 {
        n /= 10;
        len += 1;
    }
    len
}

/// `head`, `n` in decimal and `tail` as one byte array of length `N`
/// (`head.len() + decimal_len(n) + tail.len()`).
pub(crate) const fn join_with_number<const N: usize>(head: &str, n: usize, tail: &str) -> [u8; N] {
    let mut out = [0u8; N];
    let mut at = 0;
    let mut i = 0;
    while i < head.len() {
        out[at] = head.as_bytes()[i];
        at += 1;
        i += 1;
    }
    let mut rest = n;
    let mut d = decimal_len(n);
    while d > 0 {
        d -= 1;
        out[at + d] = b'0' + (rest % 10) as u8;
        rest /= 10;
    }
    at += decimal_len(n);
    let mut i = 0;
    while i < tail.len() {
        out[at] = tail.as_bytes()[i];
        at += 1;
        i += 1;
    }
    out
}

/// A `&'static str` of `$head`, the `usize` constant `$cap` in decimal and `$tail`, built at compile
/// time.
macro_rules! help_with_cap {
    ($head:expr, $cap:expr, $tail:expr) => {{
        const HEAD: &str = $head;
        const TAIL: &str = $tail;
        const CAP: usize = $cap;
        const LEN: usize = HEAD.len() + $crate::cmd::help_text::decimal_len(CAP) + TAIL.len();
        const BYTES: [u8; LEN] = $crate::cmd::help_text::join_with_number::<LEN>(HEAD, CAP, TAIL);
        match ::std::str::from_utf8(&BYTES) {
            Ok(h) => h,
            Err(_) => HEAD,
        }
    }};
}
pub(crate) use help_with_cap;

#[cfg(test)]
mod tests {
    #[test]
    fn the_cap_is_written_in_decimal_between_head_and_tail() {
        const A: &str = help_with_cap!("at most ", 250, " slots");
        const B: &str = help_with_cap!("", 0, "");
        const C: &str = help_with_cap!("x", 1_000_007, "y");
        assert_eq!(A, "at most 250 slots");
        assert_eq!(B, "0");
        assert_eq!(C, "x1000007y");
    }
}
