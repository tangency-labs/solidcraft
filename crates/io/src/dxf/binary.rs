//! Binary DXF, read into the same group-code text the ASCII reader
//! takes, so both kinds of file go through one parser.
//!
//! The layout follows the public AutoCAD DXF reference ("Binary DXF Files" and "Group Code Value
//! Types"): a 22-byte sentinel, then one group after another, each a group code and its value.
//! Releases 13 and later write the code as a 16-bit little-endian integer; release 12 writes one
//! byte, with 255 followed by a 16-bit code for codes above 254. The value's form follows from
//! the code: a NUL-terminated string, a 16-, 32- or 64-bit little-endian integer, a little-endian
//! double, a one-byte flag, or a binary chunk (a length byte and that many bytes). The file ends
//! at an `EOF` group or at its last byte.

use crate::{IoError, Result};

/// The 22 bytes every binary DXF starts with.
pub(crate) const SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";

/// Most groups one binary DXF may have: as many as the largest admitted ASCII file can hold.
const MAX_GROUPS: usize = crate::MAX_DRAWING_BYTES / 2;

/// Is this the start of a binary DXF?
pub(crate) fn is_binary_dxf(bytes: &[u8]) -> bool {
    bytes.starts_with(SENTINEL)
}

/// How a group's value is stored.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Text,
    Int16,
    Int32,
    Int64,
    Double,
    Flag,
    Chunk,
}

/// The value form of a group code (DXF reference, "Group Code Value Types"). `None` for a code
/// the reference gives no form, which cannot be skipped because its length is unknown.
fn value_of(code: i32) -> Option<Value> {
    Some(match code {
        0..=9 | 100..=102 | 105 | 300..=309 | 320..=369 | 390..=399 | 410..=419 | 430..=439 | 470..=481 | 999 => Value::Text,
        1004 | 310..=319 => Value::Chunk,
        1000..=1009 => Value::Text,
        10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => Value::Double,
        60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => Value::Int16,
        90..=99 | 420..=429 | 440..=459 | 1071 => Value::Int32,
        160..=169 => Value::Int64,
        290..=299 => Value::Flag,
        _ => return None,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|end| *end <= self.bytes.len()).ok_or_else(short)?;
        let out = self.bytes.get(self.pos..end).ok_or_else(short)?;
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

fn short() -> IoError {
    IoError::Invalid("the binary DXF file ends in the middle of a group".into())
}

/// The binary DXF `bytes` as ASCII DXF text: one group code line and one value line per group.
/// Doubles are written in the shortest form that reads back to the same value; strings lose any
/// line breaks (an ASCII value cannot hold one). The drawing's version decides how strings read
/// (DXF reference, header variables): from AutoCAD 2007 (`$ACADVER` AC1021) on they are UTF-8; an
/// older drawing's are in the code page its header declares (`$DWGCODEPAGE`), even where the
/// bytes happen to form UTF-8 (the ANSI_932 `ﾅｼ`, `C5 BC`, would read `ż`). Without a known code
/// page, bytes that are not UTF-8 are replaced, as the ASCII reader does.
pub(crate) fn binary_to_text(bytes: &[u8]) -> Result<String> {
    let body = bytes.strip_prefix(SENTINEL).ok_or_else(|| IoError::Invalid("this is not a binary DXF file".into()))?;
    // The first group is `0 SECTION`: a zero byte and then the name means one-byte codes
    // (release 12); two zero bytes, two-byte codes.
    let wide = match body {
        [0, 0, ..] => true,
        [0, b, ..] if *b != 0 => false,
        _ => return Err(IoError::Invalid("the binary DXF file does not start with a section".into())),
    };
    let mut c = Cursor { bytes: body, pos: 0 };
    let mut out = String::new();
    let mut groups = 0usize;
    // The declared code page, once read, and whether the next code 3 group names it; the same
    // for `$ACADVER` and its code 1 group, and whether that version's strings are UTF-8.
    let mut page: Option<&'static encoding_rs::Encoding> = None;
    let mut page_next = false;
    let mut utf8_version = false;
    let mut version_next = false;
    while !c.done() {
        groups += 1;
        if groups > MAX_GROUPS {
            return Err(IoError::Invalid("the binary DXF file has too many groups".into()));
        }
        let code = if wide {
            i32::from(i16::from_le_bytes(c.array()?))
        } else {
            match c.array::<1>()? {
                [255] => i32::from(i16::from_le_bytes(c.array()?)),
                [b] => i32::from(b),
            }
        };
        let form =
            value_of(code).ok_or_else(|| IoError::Invalid(format!("the binary DXF file has group code {code}, whose value form is unknown")))?;
        let value = match form {
            Value::Text => {
                let rest = c.bytes.get(c.pos..).unwrap_or_default();
                let end = rest.iter().position(|b| *b == 0).ok_or_else(short)?;
                let raw = c.take(end)?;
                c.take(1)?;
                let text = match page.filter(|_| !utf8_version) {
                    Some(page) => page.decode_without_bom_handling(raw).0,
                    None => String::from_utf8_lossy(raw),
                };
                let text: String = text.chars().map(|ch| if ch == '\r' || ch == '\n' { ' ' } else { ch }).collect();
                // The header's `$DWGCODEPAGE` names the code page of the text after it, and its
                // `$ACADVER` the version.
                if code == 9 {
                    page_next = text == "$DWGCODEPAGE";
                    version_next = text == "$ACADVER";
                } else if code == 3 && std::mem::take(&mut page_next) {
                    page = code_page(&text);
                } else if code == 1 && std::mem::take(&mut version_next) {
                    utf8_version = strings_are_utf8(&text);
                }
                text
            }
            Value::Int16 => i16::from_le_bytes(c.array()?).to_string(),
            Value::Int32 => i32::from_le_bytes(c.array()?).to_string(),
            Value::Int64 => i64::from_le_bytes(c.array()?).to_string(),
            Value::Double => {
                let v = f64::from_le_bytes(c.array()?);
                if !v.is_finite() {
                    return Err(IoError::Invalid(format!("the binary DXF file has a value that is not a number in group {code}")));
                }
                format!("{v:?}")
            }
            Value::Flag => c.array::<1>()?[0].to_string(),
            Value::Chunk => {
                let [n] = c.array::<1>()?;
                c.take(usize::from(n))?.iter().map(|b| format!("{b:02X}")).collect()
            }
        };
        if out.len().saturating_add(value.len()).saturating_add(8) > crate::MAX_DRAWING_BYTES {
            return Err(IoError::Invalid("the DXF file is too large".into()));
        }
        out.push_str(&code.to_string());
        out.push('\n');
        out.push_str(&value);
        out.push('\n');
        if code == 0 && value == "EOF" {
            break;
        }
    }
    Ok(out)
}

/// The encoding of a `$DWGCODEPAGE` value (DXF reference,
/// header variables): the Windows code pages `ANSI_874` and `ANSI_1250` to `ANSI_1258`, and the
/// double-byte ones `ANSI_932` (Shift_JIS), `ANSI_936` (GBK), `ANSI_949` (EUC-KR) and `ANSI_950`
/// (Big5). `None` for any other (DOS code pages, Mac ones), whose text reads as before.
fn code_page(name: &str) -> Option<&'static encoding_rs::Encoding> {
    let number = name.trim().strip_prefix("ANSI_").or_else(|| name.trim().strip_prefix("ansi_"))?;
    let label = match number {
        "874" | "1250" | "1251" | "1252" | "1253" | "1254" | "1255" | "1256" | "1257" | "1258" => format!("windows-{number}"),
        "932" => "shift_jis".into(),
        "936" => "gbk".into(),
        "949" => "euc-kr".into(),
        "950" => "big5".into(),
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label.as_bytes())
}

/// Whether a drawing of the `$ACADVER` `version` stores its strings in UTF-8: AC1021 (AutoCAD
/// 2007) and later.
fn strings_are_utf8(version: &str) -> bool {
    version.trim().strip_prefix("AC").and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n >= 1021)
}

/// The DXF file `bytes` as text for the reader: a binary DXF read into ASCII groups, any other
/// file as text (bytes that are not UTF-8 replaced, as before).
pub fn dxf_text_from_bytes(bytes: &[u8]) -> Result<String> {
    if is_binary_dxf(bytes) { binary_to_text(bytes) } else { Ok(String::from_utf8_lossy(bytes).into_owned()) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use solidcraft_geom::prelude::*;

    /// A binary DXF writer for tests, from the same reference: two-byte codes when `wide`.
    pub(crate) struct BinaryDxf {
        pub bytes: Vec<u8>,
        wide: bool,
    }

    impl BinaryDxf {
        pub fn new(wide: bool) -> Self {
            Self { bytes: SENTINEL.to_vec(), wide }
        }
        fn code(&mut self, code: i16) {
            if self.wide {
                self.bytes.extend(code.to_le_bytes());
            } else if (0..255).contains(&code) {
                self.bytes.push(code as u8);
            } else {
                self.bytes.push(255);
                self.bytes.extend(code.to_le_bytes());
            }
        }
        pub fn s(self, code: i16, v: &str) -> Self {
            self.raw(code, v.as_bytes())
        }
        /// A string stored as these bytes (a legacy code page's, say).
        pub fn raw(mut self, code: i16, v: &[u8]) -> Self {
            self.code(code);
            self.bytes.extend(v);
            self.bytes.push(0);
            self
        }
        pub fn d(mut self, code: i16, v: f64) -> Self {
            self.code(code);
            self.bytes.extend(v.to_le_bytes());
            self
        }
        pub fn i16(mut self, code: i16, v: i16) -> Self {
            self.code(code);
            self.bytes.extend(v.to_le_bytes());
            self
        }
        pub fn i32(mut self, code: i16, v: i32) -> Self {
            self.code(code);
            self.bytes.extend(v.to_le_bytes());
            self
        }
        pub fn chunk(mut self, code: i16, v: &[u8]) -> Self {
            self.code(code);
            self.bytes.push(v.len() as u8);
            self.bytes.extend(v);
            self
        }
    }

    /// A drawing in inches with a line, a circle and a closed bulged polyline.
    pub(crate) fn sample(wide: bool) -> Vec<u8> {
        BinaryDxf::new(wide)
            .s(0, "SECTION")
            .s(2, "HEADER")
            .s(9, "$INSUNITS")
            .i16(70, 1)
            .s(0, "ENDSEC")
            .s(0, "SECTION")
            .s(2, "ENTITIES")
            .s(0, "LINE")
            .s(8, "0")
            .d(10, 0.0)
            .d(20, 0.0)
            .d(11, 2.0)
            .d(21, 0.0)
            .s(0, "CIRCLE")
            .s(8, "0")
            .chunk(310, &[0xde, 0xad])
            .d(10, 1.0)
            .d(20, 1.0)
            .d(40, 0.5)
            .s(0, "LWPOLYLINE")
            .s(8, "0")
            .i32(90, 2)
            .i16(70, 1)
            .d(10, 0.0)
            .d(20, 0.0)
            .d(42, 1.0)
            .d(10, 1.0)
            .d(20, 0.0)
            .d(42, 1.0)
            .s(0, "ENDSEC")
            .s(0, "EOF")
            .bytes
    }

    #[test]
    fn a_binary_dxf_reads_as_the_ascii_groups_it_stores() {
        for wide in [true, false] {
            let text = binary_to_text(&sample(wide)).unwrap();
            assert!(text.starts_with("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n"), "{text}");
            assert!(text.contains("310\nDEAD\n10\n1.0\n20\n1.0\n40\n0.5\n"), "{text}");
            assert!(text.ends_with("0\nEOF\n"), "{text}");
        }
        // A code above 254 in a release 12 file is escaped with 255.
        let r12 = BinaryDxf::new(false).s(0, "SECTION").s(1001, "APP").s(0, "EOF").bytes;
        assert_eq!(binary_to_text(&r12).unwrap(), "0\nSECTION\n1001\nAPP\n0\nEOF\n");
        // Group 101 marks an embedded object (MTEXT, from AutoCAD 2018): a string.
        let embedded = BinaryDxf::new(true).s(0, "SECTION").s(101, "Embedded Object").s(0, "EOF").bytes;
        assert_eq!(binary_to_text(&embedded).unwrap(), "0\nSECTION\n101\nEmbedded Object\n0\nEOF\n");
    }

    #[test]
    fn a_binary_dxf_imports_like_its_ascii_twin() {
        use crate::Geom2;
        for wide in [true, false] {
            let geom = crate::read_dxf(&dxf_text_from_bytes(&sample(wide)).unwrap()).unwrap();
            assert_eq!(geom.len(), 4, "{geom:?}");
            assert!(matches!(geom[0], Geom2::Line(a, b) if a.dist(b) == 50.8), "{:?}", geom[0]);
            assert!(matches!(geom[1], Geom2::Circle(_, r) if r == 12.7), "{:?}", geom[1]);
            assert!(geom[2..].iter().all(|g| matches!(g, Geom2::Arc { .. })), "{geom:?}");
        }
        // Any other file stays text.
        assert_eq!(dxf_text_from_bytes(b"0\nEOF\n").unwrap(), "0\nEOF\n");
    }

    /// Text in the drawing's declared code page reads in it (`Müller` in Windows-1252, not
    /// `M\u{FFFD}ller`), by the drawing's version: an AutoCAD 2007 (AC1021) or later drawing's
    /// text is UTF-8 whatever page it declares; an older one's is in its page even where its bytes
    /// form UTF-8 (`ﾅｼ` in ANSI_932, `C5 BC`, not `ż`).
    #[test]
    fn a_binary_dxf_reads_text_in_its_declared_code_page() {
        let versioned = |version: Option<&str>, page: &str, label: &[u8]| {
            let mut b = BinaryDxf::new(true).s(0, "SECTION").s(2, "HEADER");
            if let Some(v) = version {
                b = b.s(9, "$ACADVER").s(1, v);
            }
            let b = b.s(9, "$DWGCODEPAGE").s(3, page).s(0, "ENDSEC");
            binary_to_text(&b.s(0, "SECTION").s(2, "ENTITIES").s(0, "TEXT").raw(1, label).s(0, "ENDSEC").s(0, "EOF").bytes).unwrap()
        };
        let drawing = |page: &str, label: &[u8]| versioned(None, page, label);
        assert!(drawing("ANSI_1252", b"M\xfcller").contains("\n1\nMüller\n"));
        assert!(versioned(Some("AC1021"), "ANSI_1252", "Müller".as_bytes()).contains("\n1\nMüller\n"), "UTF-8 stays UTF-8");
        assert!(versioned(Some("AC1032"), "ANSI_932", "ﾅｼ".as_bytes()).contains("\n1\nﾅｼ\n"), "UTF-8 from AC1021 on");
        assert!(versioned(Some("AC1018"), "ANSI_932", b"\xc5\xbc").contains("\n1\nﾅｼ\n"), "AC1018: the declared page");
        assert!(drawing("ANSI_932", b"\xc5\xbc").contains("\n1\nﾅｼ\n"), "no version: the declared page");
        assert!(drawing("ANSI_1251", b"\xcf\xe8\xf2").contains("\n1\nПит\n"));
        assert!(drawing("ANSI_932", b"\x83\x6c\x83\x57").contains("\n1\nネジ\n"));
        // An unknown code page reads as before.
        assert!(drawing("DOS437", b"M\xfcller").contains("\n1\nM\u{fffd}ller\n"));
    }

    #[test]
    fn a_damaged_binary_dxf_is_an_error_not_a_crash() {
        let whole = sample(true);
        for cut in SENTINEL.len()..whole.len() - 1 {
            let _ = binary_to_text(&whole[..cut]);
        }
        assert!(binary_to_text(&whole[..whole.len() - 3]).is_err());
        assert!(binary_to_text(SENTINEL).is_err());
        let unknown = BinaryDxf::new(true).s(0, "SECTION").i16(1100, 0).bytes;
        assert!(binary_to_text(&unknown).unwrap_err().to_string().contains("1100"));
        let nan = BinaryDxf::new(true).s(0, "SECTION").d(10, f64::NAN).bytes;
        assert!(binary_to_text(&nan).is_err());
        // A line break in a string cannot start a group of its own.
        let injected = BinaryDxf::new(true).s(0, "SECTION").s(1, "a\n0\nEOF").s(0, "EOF").bytes;
        assert_eq!(binary_to_text(&injected).unwrap(), "0\nSECTION\n1\na 0 EOF\n0\nEOF\n");
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            let mut b = whole.clone();
            for _ in 0..4 {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let at = SENTINEL.len() + (rng as usize) % (b.len() - SENTINEL.len());
                b[at] = (rng >> 32) as u8;
            }
            let _ = binary_to_text(&b);
        }
    }
}
