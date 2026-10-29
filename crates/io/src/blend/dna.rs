//! Bounded, endian-aware SDNA inspection. Fields include the explicit padding
//! recorded by DNA, so their offsets are sums of the file's own type lengths.
use super::{Result, bad};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct Numbers {
    pub little: bool,
    pub pointer: usize,
}
impl Numbers {
    pub fn uint(self, bytes: &[u8], offset: usize, size: usize) -> Result<u64> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(bad("invalid integer width"));
        }
        let end = offset.checked_add(size).ok_or_else(|| bad("offset overflow"))?;
        let b = bytes.get(offset..end).ok_or_else(|| bad("truncated data"))?;
        let mut n = 0;
        if self.little {
            for &v in b.iter().rev() {
                n = (n << 8) | u64::from(v);
            }
        } else {
            for &v in b {
                n = (n << 8) | u64::from(v);
            }
        }
        Ok(n)
    }
    pub fn float(self, bytes: &[u8], offset: usize) -> Result<f64> {
        Ok(f64::from(f32::from_bits(self.uint(bytes, offset, 4)? as u32)))
    }
}

pub(super) struct Field {
    pub ty: String,
    pub offset: usize,
    pub size: usize,
}
pub(super) struct Structure {
    pub name: String,
    pub size: usize,
    pub fields: BTreeMap<String, Field>,
}
pub(super) struct Block<'a> {
    pub code: String,
    pub sdna: usize,
    pub count: usize,
    pub address: u64,
    pub bytes: &'a [u8],
}
pub(super) struct File<'a> {
    pub numbers: Numbers,
    pub blocks: Vec<Block<'a>>,
    pub structs: Vec<Structure>,
    addresses: BTreeMap<u64, usize>,
    types: BTreeMap<String, usize>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    numbers: Numbers,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(|| bad("size overflow"))?;
        let out = self.bytes.get(self.at..end).ok_or_else(|| bad("truncated DNA"))?;
        self.at = end;
        Ok(out)
    }
    fn uint(&mut self, n: usize) -> Result<usize> {
        let numbers = self.numbers;
        usize::try_from(numbers.uint(self.take(n)?, 0, n)?).map_err(|_| bad("integer exceeds platform limit"))
    }
    fn tag(&mut self, tag: &[u8]) -> Result<()> {
        if self.take(tag.len())? != tag {
            return Err(bad("invalid DNA section"));
        }
        Ok(())
    }
    fn align(&mut self) -> Result<()> {
        self.take((4 - self.at % 4) % 4)?;
        Ok(())
    }
    fn strings(&mut self) -> Result<Vec<String>> {
        let n = self.uint(4)?;
        if n > 100_000 {
            return Err(bad("too many DNA strings"));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let rest = self.bytes.get(self.at..).ok_or_else(|| bad("invalid DNA offset"))?;
            let len = rest.iter().take(4097).position(|&v| v == 0).ok_or_else(|| bad("unterminated DNA string"))?;
            let s = std::str::from_utf8(self.take(len)?).map_err(|_| bad("invalid DNA text"))?.to_owned();
            self.take(1)?;
            out.push(s);
        }
        self.align()?;
        Ok(out)
    }
}

fn structures(bytes: &[u8], numbers: Numbers) -> Result<Vec<Structure>> {
    let mut c = Cursor { bytes, at: 0, numbers };
    c.tag(b"SDNA")?;
    c.tag(b"NAME")?;
    let names = c.strings()?;
    c.tag(b"TYPE")?;
    let types = c.strings()?;
    c.tag(b"TLEN")?;
    let lengths = (0..types.len()).map(|_| c.uint(2)).collect::<Result<Vec<_>>>()?;
    c.align()?;
    c.tag(b"STRC")?;
    let n = c.uint(4)?;
    if n > 20_000 {
        return Err(bad("too many DNA structures"));
    }
    let mut out = Vec::with_capacity(n);
    let mut total_fields = 0usize;
    for _ in 0..n {
        let ti = c.uint(2)?;
        let name = types.get(ti).ok_or_else(|| bad("invalid DNA type"))?.clone();
        let size = *lengths.get(ti).ok_or_else(|| bad("invalid DNA size"))?;
        let count = c.uint(2)?;
        total_fields += count;
        if total_fields > 200_000 {
            return Err(bad("too many DNA fields"));
        }
        let mut fields = BTreeMap::new();
        let mut offset = 0usize;
        for _ in 0..count {
            let fi = c.uint(2)?;
            let ni = c.uint(2)?;
            let raw = names.get(ni).ok_or_else(|| bad("invalid DNA field"))?;
            let ty = types.get(fi).ok_or_else(|| bad("invalid DNA field type"))?.clone();
            let mut length = if raw.contains('*') { numbers.pointer } else { *lengths.get(fi).ok_or_else(|| bad("invalid DNA field size"))? };
            for dim in raw.split('[').skip(1) {
                let value = dim.split(']').next().ok_or_else(|| bad("invalid array"))?.parse::<usize>().map_err(|_| bad("invalid DNA array"))?;
                length = length.checked_mul(value).ok_or_else(|| bad("DNA array overflow"))?;
            }
            let field_name = raw.trim_start_matches('*').split('[').next().ok_or_else(|| bad("invalid field name"))?.to_owned();
            fields.insert(field_name, Field { ty, offset, size: length });
            offset = offset.checked_add(length).ok_or_else(|| bad("DNA structure overflow"))?;
        }
        if offset != size {
            return Err(bad(format!("inconsistent DNA layout for {name}")));
        }
        out.push(Structure { name, size, fields });
    }
    Ok(out)
}

impl<'a> File<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self> {
        if !bytes.starts_with(b"BLENDER") {
            return Err(bad("missing file header"));
        }
        let modern = bytes.get(7..9) == Some(b"17");
        let (mut at, numbers) = if modern {
            if bytes.get(7..17) != Some(b"17-01v0502")
                && !bytes.get(7..17).is_some_and(|h| h.starts_with(b"17-01v") && h[6..].iter().all(u8::is_ascii_digit))
            {
                return Err(bad("unsupported extended header"));
            }
            (17, Numbers { little: true, pointer: 8 })
        } else {
            let pointer = match bytes.get(7) {
                Some(b'_') => 4,
                Some(b'-') => 8,
                _ => return Err(bad("invalid pointer header")),
            };
            let little = match bytes.get(8) {
                Some(b'v') => true,
                Some(b'V') => false,
                _ => return Err(bad("invalid endian header")),
            };
            if !bytes.get(9..12).is_some_and(|s| s.iter().all(u8::is_ascii_digit)) {
                return Err(bad("invalid version"));
            }
            (12, Numbers { little, pointer })
        };
        let head = if modern { 32 } else { 16 + numbers.pointer };
        let mut blocks = Vec::new();
        let mut addresses = BTreeMap::new();
        let mut ended = false;
        while at < bytes.len() {
            if blocks.len() >= 200_000 {
                return Err(bad("too many data blocks"));
            }
            let h = bytes.get(at..at.checked_add(head).ok_or_else(|| bad("header overflow"))?).ok_or_else(|| bad("truncated block header"))?;
            let code = String::from_utf8_lossy(&h[..4]).trim_end_matches('\0').to_owned();
            let (sdna, address, len, count) = if modern {
                (numbers.uint(h, 4, 4)?, numbers.uint(h, 8, 8)?, numbers.uint(h, 16, 8)?, numbers.uint(h, 24, 8)?)
            } else {
                (
                    numbers.uint(h, 8 + numbers.pointer, 4)?,
                    numbers.uint(h, 8, numbers.pointer)?,
                    numbers.uint(h, 4, 4)?,
                    numbers.uint(h, 12 + numbers.pointer, 4)?,
                )
            };
            let len = usize::try_from(len).map_err(|_| bad("block too large"))?;
            let sdna = usize::try_from(sdna).map_err(|_| bad("type index too large"))?;
            let count = usize::try_from(count).map_err(|_| bad("block count too large"))?;
            at += head;
            let end = at.checked_add(len).ok_or_else(|| bad("block overflow"))?;
            let data = bytes.get(at..end).ok_or_else(|| bad("truncated block"))?;
            at = end;
            if address != 0
                && !matches!(code.as_str(), "REND" | "TEST" | "GLOB" | "DNA1" | "ENDB")
                && addresses.insert(address, blocks.len()).is_some()
            {
                return Err(bad("duplicate data address"));
            }
            ended = code == "ENDB";
            blocks.push(Block { code, sdna, address, count, bytes: data });
            if ended {
                break;
            }
        }
        if !ended || at != bytes.len() {
            return Err(bad("missing end block or trailing data"));
        }
        let dna = blocks.iter().find(|b| b.code == "DNA1").ok_or_else(|| bad("missing DNA"))?;
        let structs = structures(dna.bytes, numbers)?;
        let types = structs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
        Ok(Self { numbers, blocks, structs, addresses, types })
    }
    pub fn structure(&self, name: &str) -> Result<&Structure> {
        self.types.get(name).and_then(|&i| self.structs.get(i)).ok_or_else(|| bad(format!("missing {name} layout")))
    }
    pub fn data(&self, address: u64) -> Result<&'a [u8]> {
        let &index = self.addresses.get(&address).ok_or_else(|| bad("unresolved data pointer"))?;
        Ok(self.blocks.get(index).ok_or_else(|| bad("invalid block index"))?.bytes)
    }
    pub fn block(&self, address: u64) -> Result<&Block<'a>> {
        self.addresses.get(&address).and_then(|&i| self.blocks.get(i)).ok_or_else(|| bad("unresolved block"))
    }
    pub fn view<'f>(&'f self, bytes: &'a [u8], name: &str) -> Result<View<'f, 'a>> {
        let structure = self.structure(name)?;
        if bytes.len() < structure.size {
            return Err(bad("truncated structured data"));
        }
        Ok(View { file: self, bytes, structure })
    }
}

#[derive(Clone, Copy)]
pub(super) struct View<'f, 'a> {
    pub file: &'f File<'a>,
    pub bytes: &'a [u8],
    pub structure: &'f Structure,
}
impl<'f, 'a> View<'f, 'a> {
    pub fn has(self, name: &str) -> bool {
        self.structure.fields.contains_key(name)
    }
    pub fn field(self, name: &str) -> Result<&'a [u8]> {
        let f = self.structure.fields.get(name).ok_or_else(|| bad(format!("missing {}.{name}", self.structure.name)))?;
        self.bytes.get(f.offset..f.offset + f.size).ok_or_else(|| bad("field outside structure"))
    }
    pub fn uint(self, name: &str, size: usize) -> Result<u64> {
        self.file.numbers.uint(self.field(name)?, 0, size)
    }
    pub fn ptr(self, name: &str) -> Result<u64> {
        self.uint(name, self.file.numbers.pointer)
    }
    pub fn nested(self, name: &str) -> Result<Self> {
        let f = self.structure.fields.get(name).ok_or_else(|| bad("missing nested field"))?;
        self.file.view(self.field(name)?, &f.ty)
    }
    pub fn string(self, name: &str) -> Result<String> {
        text(self.field(name)?)
    }
    pub fn floats(self, name: &str, count: usize) -> Result<Vec<f64>> {
        let bytes = self.field(name)?;
        (0..count).map(|i| self.file.numbers.float(bytes, i * 4)).collect()
    }
}
pub(super) fn text(bytes: &[u8]) -> Result<String> {
    let end = bytes.iter().take(4097).position(|&v| v == 0).ok_or_else(|| bad("unterminated text"))?;
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}
