//! E57 import, from the format's public description (ASTM E2807): the open file for 3D scans that
//! laser scanners and their software write. The file is cut into pages, each ending in a CRC-32C
//! checksum of the rest of the page. An XML part describes the scans (`/data3D`): for each one the
//! fields of a point (its `prototype`), the number of points, where the scan stood (`pose`, a
//! rotation and a translation) and where its points are. The points are in a binary section of
//! packets, each packet holding a run of bytes for every field. A field's values are packed one
//! after another with as many bits as its range needs (the `bitPackCodec`): floats as they are in
//! memory, whole numbers as their distance from the field's minimum.
//!
//! A point's position is `cartesianX`, `cartesianY` and `cartesianZ`, or `sphericalRange`,
//! `sphericalAzimuth` and `sphericalElevation` turned into them. Each scan is moved by its pose,
//! and all the scans are read into one cloud. `colorRed`, `colorGreen` and `colorBlue` are the
//! colours, over the range the scan's `colorLimits` (or else the field) gives, and `intensity`
//! becomes a column. A point the file marks as having no position (`cartesianInvalidState` or
//! `sphericalInvalidState` not 0) is left out. Other fields, of any type, and the pictures
//! (`/images2D`) are passed over. The file is untrusted: every checksum is checked, every offset
//! and count is checked against its size, and the packets of all the scans together may not add
//! up to more than the file (scans that share their points would be read again and again).

use solidcraft_doc::point_cloud::{Column, Complex, Floats, MAX_POINTS, On};

use crate::ply_complex::PlyCloud;
use crate::{IoError, Result};

const SIGNATURE: &[u8] = b"ASTM-E57";
/// The file header: the signature, the version, the file's length, where the XML is and how long
/// it is, and the page size.
const HEADER: usize = 48;
/// Bytes at the end of each page that hold its checksum.
const CHECKSUM: usize = 4;
/// Largest XML part read (bytes), most elements in it and how deeply they may nest.
const MAX_XML: usize = 64 << 20;
const MAX_ELEMENTS: usize = 1 << 20;
const MAX_DEPTH: usize = 64;
const MAX_SCANS: usize = 4096;
/// Most fields in one point.
const MAX_FIELDS: usize = 256;

fn invalid(m: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("E57: {m}"))
}

fn early() -> IoError {
    invalid("the file ends early")
}

fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).ok_or_else(early)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(early)
}

fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    b.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).ok_or_else(early)
}

/// The CRC-32C (Castagnoli) table, for the reflected polynomial.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 { (c >> 1) ^ 0x82F6_3B78 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

fn crc32c(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |c, &b| CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8))
}

/// The file's header.
struct Header {
    xml_offset: u64,
    xml_length: u64,
    page: usize,
}

fn header(bytes: &[u8]) -> Result<Header> {
    if bytes.get(..SIGNATURE.len()) != Some(SIGNATURE) {
        return Err(invalid("the file does not start with `ASTM-E57`"));
    }
    let major = u32_at(bytes, 8)?;
    if major != 1 {
        return Err(invalid(format!("version {major} of the format is not read (only version 1)")));
    }
    let length = u64_at(bytes, 16)?;
    let (xml_offset, xml_length, page) = (u64_at(bytes, 24)?, u64_at(bytes, 32)?, u64_at(bytes, 40)?);
    if !(HEADER as u64 + CHECKSUM as u64..=1 << 24).contains(&page) {
        return Err(invalid(format!("a page of {page} bytes")));
    }
    if length != bytes.len() as u64 {
        return Err(invalid(format!("the file says it is {length} bytes long, but it is {}", bytes.len())));
    }
    Ok(Header { xml_offset, xml_length, page: page as usize })
}

/// The file's bytes without the checksum that ends each page, after checking each checksum.
fn logical(bytes: &[u8], page: usize) -> Result<Vec<u8>> {
    if !bytes.len().is_multiple_of(page) {
        return Err(invalid("the file is not a whole number of pages"));
    }
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len() / page * (page - CHECKSUM)).map_err(|_| invalid("the file is too large to hold in memory"))?;
    for (i, p) in bytes.chunks_exact(page).enumerate() {
        let (data, sum) = p.split_at(page - CHECKSUM);
        let stored = sum.try_into().map(u32::from_be_bytes).map_err(|_| early())?;
        if crc32c(data) != stored {
            return Err(invalid(format!("page {} of the file is damaged: its checksum does not match", i + 1)));
        }
        out.extend_from_slice(data);
    }
    Ok(out)
}

/// Where an offset into the file is once the checksums are taken out.
fn to_logical(physical: u64, page: usize) -> Result<usize> {
    let page = page as u64;
    let (n, at) = (physical / page, physical % page);
    if at >= page - CHECKSUM as u64 {
        return Err(invalid(format!("an offset ({physical}) points into a page's checksum")));
    }
    usize::try_from(n * (page - CHECKSUM as u64) + at).map_err(|_| early())
}

/// An element of the XML part: its name (without a namespace prefix), its attributes, its text and
/// the elements in it.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The element's number: its text, 0 when it has none (as the format says).
    fn number(&self) -> Result<f64> {
        let t = self.text.trim();
        if t.is_empty() {
            return Ok(0.0);
        }
        t.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| invalid(format!("`{}` is not a number", short(t))))
    }

    /// The number in the child `name`, if there is one.
    fn number_in(&self, name: &str) -> Result<Option<f64>> {
        self.child(name).map(Node::number).transpose()
    }

    fn integer_attr(&self, key: &str, default: i64) -> Result<i64> {
        self.attr(key).map_or(Ok(default), |v| v.trim().parse::<i64>().map_err(|_| invalid(format!("`{}` is not a whole number", short(v)))))
    }

    fn float_attr(&self, key: &str, default: f64) -> Result<f64> {
        self.attr(key).map_or(Ok(default), |v| {
            v.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| invalid(format!("`{}` is not a number", short(v))))
        })
    }
}

/// The start of a name or value from the file, for a message.
fn short(text: &str) -> String {
    text.chars().take(32).collect()
}

fn element(e: &quick_xml::events::BytesStart) -> Node {
    let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let k = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let v = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
            (k, v)
        })
        .collect();
    Node { name, attrs, ..Node::default() }
}

/// Read the XML part into its elements.
fn parse_xml(text: &str) -> Result<Node> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut count = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| invalid(format!("the XML part cannot be read: {e}")))?;
        match event {
            Event::Start(e) | Event::Empty(e) if count >= MAX_ELEMENTS || stack.len() > MAX_DEPTH => {
                let _ = e;
                return Err(invalid("the XML part has too many elements or nests them too deeply"));
            }
            Event::Start(e) => {
                count += 1;
                stack.push(element(&e));
            }
            Event::Empty(e) => {
                count += 1;
                let node = element(&e);
                stack.last_mut().ok_or_else(early)?.children.push(node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(early)?;
                stack.last_mut().ok_or_else(|| invalid("the XML part closes an element it never opened"))?.children.push(node);
            }
            Event::Text(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::CData(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut document = match (stack.pop(), stack.is_empty()) {
        (Some(d), true) => d,
        _ => return Err(invalid("the XML part ends inside an element")),
    };
    let root = document.children.drain(..).find(|c| c.name == "e57Root").ok_or_else(|| invalid("the XML part has no `e57Root`"))?;
    Ok(root)
}

/// How a field's values are stored.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// Single or double precision floats.
    Float { double: bool },
    /// Whole numbers from `min`, each in `bits` bits, then scaled: `value * scale + offset`.
    Integer { min: i64, bits: u32, scale: f64, offset: f64 },
    /// Another type (a string): passed over, and refused only where a number is needed.
    Other,
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    kind: Kind,
    /// The smallest and largest values the field says it holds.
    range: (f64, f64),
}

impl Field {
    fn of(n: &Node) -> Result<Field> {
        let kind_name = n.attr("type").unwrap_or_default();
        let (kind, range) = match kind_name {
            "Float" => {
                let double = n.attr("precision") != Some("single");
                let range = (n.float_attr("minimum", f64::MIN)?, n.float_attr("maximum", f64::MAX)?);
                (Kind::Float { double }, range)
            }
            "Integer" | "ScaledInteger" => {
                let (min, max) = (n.integer_attr("minimum", i64::MIN)?, n.integer_attr("maximum", i64::MAX)?);
                if min > max {
                    return Err(invalid(format!("the field `{}` has its minimum over its maximum", short(&n.name))));
                }
                // As many bits as the values from the minimum to the maximum need.
                let span = (i128::from(max) - i128::from(min)) as u128;
                let bits = 128 - span.leading_zeros();
                let (scale, offset) =
                    if kind_name == "ScaledInteger" { (n.float_attr("scale", 1.0)?, n.float_attr("offset", 0.0)?) } else { (1.0, 0.0) };
                if scale == 0.0 {
                    return Err(invalid(format!("the field `{}` has a scale of 0", short(&n.name))));
                }
                let ends = [min as f64 * scale + offset, max as f64 * scale + offset];
                (Kind::Integer { min, bits, scale, offset }, (ends[0].min(ends[1]), ends[0].max(ends[1])))
            }
            _ => (Kind::Other, (0.0, 0.0)),
        };
        Ok(Field { name: n.name.clone(), kind, range })
    }

    fn integer(&self) -> bool {
        matches!(self.kind, Kind::Integer { scale, offset, .. } if scale == 1.0 && offset == 0.0)
    }
}

/// A field's packed values: the runs of bytes the packets hold for it, read as one stream of bits,
/// lowest bit first.
#[derive(Clone)]
struct Stream<'a> {
    chunks: std::sync::Arc<Vec<&'a [u8]>>,
    chunk: usize,
    byte: usize,
    bit: u32,
}

impl<'a> Stream<'a> {
    fn new(chunks: std::sync::Arc<Vec<&'a [u8]>>) -> Stream<'a> {
        Stream { chunks, chunk: 0, byte: 0, bit: 0 }
    }

    /// The next `n` bits (at most 64), lowest first.
    fn bits(&mut self, n: u32) -> Result<u64> {
        let mut value = 0u64;
        let mut got = 0;
        while got < n {
            let b = loop {
                let chunk = self.chunks.get(self.chunk).ok_or_else(|| invalid("a scan has fewer values than points"))?;
                match chunk.get(self.byte) {
                    Some(b) => break *b,
                    None => {
                        self.chunk += 1;
                        self.byte = 0;
                    }
                }
            };
            let take = (8 - self.bit).min(n - got);
            let part = u64::from(b >> self.bit) & ((1u64 << take) - 1);
            value |= part << got;
            got += take;
            self.bit += take;
            if self.bit == 8 {
                self.bit = 0;
                self.byte += 1;
            }
        }
        Ok(value)
    }

    fn value(&mut self, kind: Kind) -> Result<f64> {
        Ok(match kind {
            Kind::Float { double: true } => f64::from_bits(self.bits(64)?),
            Kind::Float { double: false } => f64::from(f32::from_bits(self.bits(32)? as u32)),
            Kind::Integer { min, bits, scale, offset } => (i128::from(min) + i128::from(self.bits(bits)?)) as f64 * scale + offset,
            Kind::Other => return Err(invalid("a field the points need is not a number")),
        })
    }
}

/// One scan: its fields, the runs of bytes for each, its number of points and its pose.
struct Scan<'a> {
    fields: Vec<Field>,
    streams: Vec<std::sync::Arc<Vec<&'a [u8]>>>,
    records: u64,
    rotation: [[f64; 3]; 3],
    translation: [f64; 3],
    /// The colour range from the scan's `colorLimits`, red, green and blue.
    color_limits: [Option<(f64, f64)>; 3],
}

/// The rotation a unit quaternion `w, x, y, z` makes.
fn rotation(q: [f64; 4]) -> Result<[[f64; 3]; 3]> {
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !(n.is_finite() && n > 1e-9) {
        return Err(invalid("a scan's rotation is not a rotation"));
    }
    let [w, x, y, z] = q.map(|v| v / n);
    Ok([
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ])
}

/// What all the scans together may take: the bytes of their packets, which scans that share their
/// points would count again and again, and the points of scans whose every field holds one value,
/// which take no bytes at all.
struct Budget {
    bytes: usize,
    constant_points: u64,
}

/// The runs of bytes each of a scan's `fields` fields has in the section at `start`.
fn packets<'a>(l: &'a [u8], start: usize, page: usize, fields: usize, budget: &mut Budget) -> Result<Vec<Vec<&'a [u8]>>> {
    // The section header: its kind (1, points), its length and where its first packet is.
    if l.get(start) != Some(&1) {
        return Err(invalid("a scan's points are not in a section of points"));
    }
    let length = usize::try_from(u64_at(l, start + 8)?).map_err(|_| early())?;
    let end = start.checked_add(length).filter(|e| *e <= l.len()).ok_or_else(early)?;
    let mut at = to_logical(u64_at(l, start + 16)?, page)?;
    let mut out: Vec<Vec<&[u8]>> = vec![Vec::new(); fields];
    while at < end {
        let kind = *l.get(at).ok_or_else(early)?;
        let length = usize::from(u16_at(l, at + 2)?) + 1;
        let packet = l.get(at..at + length).filter(|_| at + length <= end).ok_or_else(early)?;
        budget.bytes =
            budget.bytes.checked_sub(length).ok_or_else(|| invalid("the scans' packets add up to more than the file: scans share their points"))?;
        match kind {
            // Points: the number of runs, their lengths, then the runs.
            1 => {
                let n = usize::from(u16_at(packet, 4)?);
                if n != fields {
                    return Err(invalid(format!("a packet has {n} runs of values for {fields} fields")));
                }
                let mut from = 6 + 2 * n;
                for (i, runs) in out.iter_mut().enumerate() {
                    let len = usize::from(u16_at(packet, 6 + 2 * i)?);
                    runs.push(packet.get(from..from + len).ok_or_else(|| invalid("a packet's runs of values are longer than the packet"))?);
                    from += len;
                }
            }
            // An index of the packets, or padding.
            0 | 2 => {}
            other => return Err(invalid(format!("a packet of an unknown kind ({other})"))),
        }
        at += length;
    }
    Ok(out)
}

fn scan<'a>(n: &Node, l: &'a [u8], page: usize, budget: &mut Budget) -> Result<Scan<'a>> {
    let points = n.child("points").filter(|p| p.attr("type") == Some("CompressedVector")).ok_or_else(|| invalid("a scan has no points"))?;
    let prototype = points.child("prototype").ok_or_else(|| invalid("a scan's points have no prototype"))?;
    if prototype.children.len() > MAX_FIELDS {
        return Err(invalid(format!("a point has more than {MAX_FIELDS} fields")));
    }
    let fields = prototype.children.iter().map(Field::of).collect::<Result<Vec<_>>>()?;
    // No codecs means the bit packing for every field; a codec may also name it.
    for codec in points.child("codecs").map(|c| c.children.as_slice()).unwrap_or_default() {
        if codec.child("bitPackCodec").is_none() {
            let name = codec.children.iter().map(|c| c.name.as_str()).find(|n| *n != "inputs").unwrap_or(codec.name.as_str());
            return Err(invalid(format!("the points are packed with `{}`, which is not read", short(name))));
        }
    }
    let records = points.integer_attr("recordCount", 0)?;
    let records = u64::try_from(records).map_err(|_| invalid("a scan has fewer than no points"))?;
    let offset = u64::try_from(points.integer_attr("fileOffset", -1)?).map_err(|_| invalid("a scan does not say where its points are"))?;
    let streams = packets(l, to_logical(offset, page)?, page, fields.len(), budget)?;
    // A point takes at least one bit, unless every field holds one value: then its points take
    // none, and only as many are made as a cloud can hold.
    let bits: u64 = streams.iter().flatten().map(|r| r.len() as u64 * 8).sum();
    if bits == 0 {
        budget.constant_points =
            budget.constant_points.checked_sub(records).ok_or_else(|| invalid(format!("a scan says it has {records} points of one value each")))?;
    } else if records > bits {
        return Err(invalid(format!("a scan says it has {records} points, more than its data holds")));
    }
    let pose = n.child("pose");
    let quaternion = pose.and_then(|p| p.child("rotation"));
    let q = match quaternion {
        Some(r) => {
            [r.number_in("w")?.unwrap_or(1.0), r.number_in("x")?.unwrap_or(0.0), r.number_in("y")?.unwrap_or(0.0), r.number_in("z")?.unwrap_or(0.0)]
        }
        None => [1.0, 0.0, 0.0, 0.0],
    };
    let translation = match pose.and_then(|p| p.child("translation")) {
        Some(t) => [t.number_in("x")?.unwrap_or(0.0), t.number_in("y")?.unwrap_or(0.0), t.number_in("z")?.unwrap_or(0.0)],
        None => [0.0; 3],
    };
    let limits = n.child("colorLimits");
    let mut color_limits = [None; 3];
    for (slot, c) in color_limits.iter_mut().zip(["Red", "Green", "Blue"]) {
        if let Some(l) = limits
            && let (Some(lo), Some(hi)) = (l.number_in(&format!("color{c}Minimum"))?, l.number_in(&format!("color{c}Maximum"))?)
        {
            *slot = (hi > lo).then_some((lo, hi));
        }
    }
    Ok(Scan { fields, streams: streams.into_iter().map(std::sync::Arc::new).collect(), records, rotation: rotation(q)?, translation, color_limits })
}

/// Where in a scan's fields each value a point needs is.
struct Roles {
    /// x, y, z, or range, azimuth, elevation when `spherical`.
    position: [usize; 3],
    spherical: bool,
    invalid: Option<usize>,
    color: Option<[usize; 3]>,
    color_invalid: Option<usize>,
    intensity: Option<usize>,
    intensity_invalid: Option<usize>,
}

impl Roles {
    fn of(fields: &[Field]) -> Result<Roles> {
        let at = |name: &str| fields.iter().position(|f| f.name == name);
        let cartesian = [at("cartesianX"), at("cartesianY"), at("cartesianZ")];
        let spherical = [at("sphericalRange"), at("sphericalAzimuth"), at("sphericalElevation")];
        let (position, spherical, invalid) = match (cartesian, spherical) {
            ([Some(x), Some(y), Some(z)], _) => ([x, y, z], false, at("cartesianInvalidState")),
            (_, [Some(r), Some(a), Some(e)]) => ([r, a, e], true, at("sphericalInvalidState")),
            _ => return Err(invalid("a scan's points have neither `cartesianX`, `cartesianY` and `cartesianZ` nor a range and two angles")),
        };
        let color = match [at("colorRed"), at("colorGreen"), at("colorBlue")] {
            [Some(r), Some(g), Some(b)] => Some([r, g, b]),
            _ => None,
        };
        let roles = Roles {
            position,
            spherical,
            invalid,
            color,
            color_invalid: at("isColorInvalid"),
            intensity: at("intensity"),
            intensity_invalid: at("isIntensityInvalid"),
        };
        // A field the points need must be a number; the others may be anything.
        let mut used: Vec<usize> = roles.position.into_iter().chain(roles.color.into_iter().flatten()).collect();
        used.extend([roles.invalid, roles.color_invalid, roles.intensity, roles.intensity_invalid].into_iter().flatten());
        if let Some(f) = used.iter().filter_map(|i| fields.get(*i)).find(|f| f.kind == Kind::Other) {
            return Err(self::invalid(format!("the field `{}` is not a number", short(&f.name))));
        }
        Ok(roles)
    }
}

/// A scan's points read one at a time.
struct Points<'s, 'a> {
    scan: &'s Scan<'a>,
    roles: &'s Roles,
    streams: Vec<Option<Stream<'a>>>,
}

impl<'s, 'a> Points<'s, 'a> {
    /// Read the fields `used` asks for; the others are passed over.
    fn new(scan: &'s Scan<'a>, roles: &'s Roles, used: &[usize]) -> Points<'s, 'a> {
        let streams = scan.streams.iter().enumerate().map(|(i, s)| used.contains(&i).then(|| Stream::new(s.clone()))).collect();
        Points { scan, roles, streams }
    }

    fn value(&mut self, field: usize) -> Result<f64> {
        let kind = self.scan.fields.get(field).map(|f| f.kind).ok_or_else(early)?;
        self.streams.get_mut(field).and_then(Option::as_mut).ok_or_else(early)?.value(kind)
    }

    /// The next point's position in the cloud, or `None` when the file marks it as having none or
    /// it is not a number.
    fn position(&mut self) -> Result<Option<[f64; 3]>> {
        let [a, b, c] = self.roles.position;
        let v = [self.value(a)?, self.value(b)?, self.value(c)?];
        let gone = match self.roles.invalid {
            Some(i) => self.value(i)? != 0.0,
            None => false,
        };
        let p = if self.roles.spherical {
            let [r, azimuth, elevation] = v;
            [r * elevation.cos() * azimuth.cos(), r * elevation.cos() * azimuth.sin(), r * elevation.sin()]
        } else {
            v
        };
        let (m, t) = (&self.scan.rotation, &self.scan.translation);
        let p = [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + t[i]);
        Ok((!gone && p.iter().all(|v| v.is_finite())).then_some(p))
    }

    fn color(&mut self) -> Result<Option<[u8; 3]>> {
        let Some(fields) = self.roles.color else { return Ok(None) };
        let mut rgb = [255u8; 3];
        for (k, (out, f)) in rgb.iter_mut().zip(fields).enumerate() {
            let v = self.value(f)?;
            let field = self.scan.fields.get(f).ok_or_else(early)?;
            let (lo, hi) = self
                .scan
                .color_limits
                .get(k)
                .copied()
                .flatten()
                .or_else(|| (field.range.1 > field.range.0 && field.range.1 < 1e300).then_some(field.range))
                .unwrap_or((0.0, 255.0));
            *out = ((v - lo) / (hi - lo) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        if let Some(i) = self.roles.color_invalid
            && self.value(i)? != 0.0
        {
            return Ok(Some([255; 3]));
        }
        Ok(Some(rgb))
    }

    fn intensity(&mut self) -> Result<Option<f32>> {
        let Some(f) = self.roles.intensity else { return Ok(None) };
        let v = self.value(f)? as f32;
        if let Some(i) = self.roles.intensity_invalid
            && self.value(i)? != 0.0
        {
            return Ok(Some(f32::NAN));
        }
        Ok(Some(v))
    }
}

/// Read an E57 file: every point when they fit in `max_points`, else every `stride`th point.
pub(crate) fn read_e57_cloud(bytes: &[u8], max_points: usize) -> Result<PlyCloud> {
    let h = header(bytes)?;
    let l = logical(bytes, h.page)?;
    let xml_length = usize::try_from(h.xml_length).ok().filter(|n| *n <= MAX_XML).ok_or_else(|| invalid("the XML part is too long"))?;
    let xml_start = to_logical(h.xml_offset, h.page)?;
    let xml = l.get(xml_start..).and_then(|x| x.get(..xml_length)).ok_or_else(early)?;
    let xml = std::str::from_utf8(xml).map_err(|_| invalid("the XML part is not UTF-8 text"))?;
    let root = parse_xml(xml)?;
    let data = root.child("data3D").map(|d| d.children.as_slice()).unwrap_or_default();
    if data.len() > MAX_SCANS {
        return Err(invalid(format!("more than {MAX_SCANS} scans")));
    }
    // The packets lie outside the XML part.
    let mut budget = Budget { bytes: l.len().saturating_sub(xml_length), constant_points: MAX_POINTS as u64 };
    let scans = data.iter().map(|n| scan(n, &l, h.page, &mut budget)).collect::<Result<Vec<_>>>()?;
    if scans.is_empty() {
        return Err(invalid("the file has no scans"));
    }
    let roles = scans.iter().map(|s| Roles::of(&s.fields)).collect::<Result<Vec<_>>>()?;
    let total: u64 = scans.iter().map(|s| s.records).sum();
    let max = max_points.max(1) as u64;
    let position_fields = |r: &Roles| r.position.iter().copied().chain(r.invalid).collect::<Vec<_>>();
    // Points without a position are left out before thinning, so they never take the place of
    // points that have one.
    let mut eligible = total;
    if total > max {
        eligible = 0;
        for (s, r) in scans.iter().zip(&roles) {
            let mut points = Points::new(s, r, &position_fields(r));
            for _ in 0..s.records {
                eligible += u64::from(points.position()?.is_some());
            }
        }
    }
    let stride = eligible.div_ceil(max).max(1);
    let colored = roles.iter().any(|r| r.color.is_some());
    let intensity = roles.iter().any(|r| r.intensity.is_some());
    let mut out = PlyCloud {
        positions: Vec::new(),
        colors: colored.then(Vec::new),
        complex: Complex::default(),
        vertices: total,
        bad_faces: 0,
        cells_left_out: 0,
        skipped: 0,
    };
    let mut intensities: Vec<f32> = Vec::new();
    let mut seen = 0u64;
    for (s, r) in scans.iter().zip(&roles) {
        let mut used = position_fields(r);
        used.extend(r.color.into_iter().flatten().chain(r.color_invalid).chain(r.intensity).chain(r.intensity_invalid));
        let mut points = Points::new(s, r, &used);
        for _ in 0..s.records {
            // Every field of a point is read, kept or not, so the next point starts in the right place.
            let p = points.position()?;
            let rgb = points.color()?;
            let level = points.intensity()?;
            let Some(p) = p else {
                out.skipped += 1;
                continue;
            };
            seen += 1;
            if !(seen - 1).is_multiple_of(stride) {
                continue;
            }
            out.positions.push(p);
            if let Some(colors) = out.colors.as_mut() {
                colors.push(rgb.unwrap_or([255; 3]));
            }
            if intensity {
                intensities.push(level.unwrap_or(f32::NAN));
            }
        }
    }
    if intensity {
        let integer = scans.iter().zip(&roles).all(|(s, r)| r.intensity.and_then(|i| s.fields.get(i)).is_some_and(Field::integer));
        let column = Column { name: "intensity".into(), on: On::Point, integer, values: Floats(intensities.into()) };
        out.complex = Complex { columns: vec![column], ..Complex::default() };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field of a test scan, with its values.
    enum F {
        Float(&'static str, bool, Vec<f64>),
        Int(&'static str, i64, i64, Vec<i64>),
        Scaled(&'static str, i64, i64, f64, f64, Vec<i64>),
    }

    #[derive(Default)]
    struct TestScan {
        fields: Vec<F>,
        /// The XML that goes in the scan beside its points: a pose, colour limits.
        extra: String,
        /// Cut each field's bytes over two packets, with an empty packet between them.
        split: bool,
    }

    /// Bits written lowest first.
    #[derive(Default)]
    struct Bits {
        out: Vec<u8>,
        used: u32,
    }

    impl Bits {
        fn put(&mut self, value: u64, n: u32) {
            for i in 0..n {
                if self.used == 0 {
                    self.out.push(0);
                }
                if (value >> i) & 1 == 1 {
                    *self.out.last_mut().unwrap() |= 1 << self.used;
                }
                self.used = (self.used + 1) % 8;
            }
        }
    }

    fn bits_for(min: i64, max: i64) -> u32 {
        128 - ((i128::from(max) - i128::from(min)) as u128).leading_zeros()
    }

    impl F {
        fn xml(&self) -> String {
            match self {
                F::Float(n, double, _) => format!("<{n} type=\"Float\"{}/>", if *double { "" } else { " precision=\"single\"" }),
                F::Int(n, lo, hi, _) => format!("<{n} type=\"Integer\" minimum=\"{lo}\" maximum=\"{hi}\"/>"),
                F::Scaled(n, lo, hi, scale, offset, _) => {
                    format!("<{n} type=\"ScaledInteger\" minimum=\"{lo}\" maximum=\"{hi}\" scale=\"{scale}\" offset=\"{offset}\"/>")
                }
            }
        }

        /// The number of values and their packed bytes.
        fn bytes(&self) -> (usize, Vec<u8>) {
            let mut b = Bits::default();
            let n = match self {
                F::Float(_, true, v) => {
                    v.iter().for_each(|x| b.put(x.to_bits(), 64));
                    v.len()
                }
                F::Float(_, false, v) => {
                    v.iter().for_each(|x| b.put(u64::from((*x as f32).to_bits()), 32));
                    v.len()
                }
                F::Int(_, lo, hi, v) | F::Scaled(_, lo, hi, _, _, v) => {
                    v.iter().for_each(|x| b.put((x - lo) as u64, bits_for(*lo, *hi)));
                    v.len()
                }
            };
            (n, b.out)
        }
    }

    fn packet(runs: &[&[u8]]) -> Vec<u8> {
        let mut p = vec![1, 0, 0, 0];
        p.extend((runs.len() as u16).to_le_bytes());
        runs.iter().for_each(|r| p.extend((r.len() as u16).to_le_bytes()));
        runs.iter().for_each(|r| p.extend(*r));
        while p.len() % 4 != 0 {
            p.push(0);
        }
        let n = (p.len() - 1) as u16;
        p[2..4].copy_from_slice(&n.to_le_bytes());
        p
    }

    /// The bytes of a file whose bytes without checksums are `l` (its header left blank) and whose
    /// XML starts at `xml_start` and is `xml_len` long, with pages of `page` bytes.
    fn paged(mut l: Vec<u8>, xml_start: usize, xml_len: usize, page: usize) -> Vec<u8> {
        let body = page - CHECKSUM;
        let physical = |l: usize| (l / body * page + l % body) as u64;
        let pages = l.len().div_ceil(body);
        l.resize(pages * body, 0);
        let mut h = Vec::from(SIGNATURE);
        h.extend(1u32.to_le_bytes());
        h.extend(0u32.to_le_bytes());
        h.extend(((pages * page) as u64).to_le_bytes());
        h.extend(physical(xml_start).to_le_bytes());
        h.extend((xml_len as u64).to_le_bytes());
        h.extend((page as u64).to_le_bytes());
        l[..HEADER].copy_from_slice(&h);
        l.chunks(body).flat_map(|c| [c, &crc32c(c).to_be_bytes()].concat()).collect()
    }

    /// An E57 file of `scans` with pages of `page` bytes.
    fn e57(scans: &[TestScan], page: usize) -> Vec<u8> {
        let body = page - CHECKSUM;
        let physical = |l: usize| (l / body * page + l % body) as u64;
        let mut l = vec![0u8; HEADER];
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<e57Root type=\"Structure\" xmlns=\"http://www.astm.org/COMMIT/E57/2010-e57-v1.0\">\
             <formatName type=\"String\"><![CDATA[ASTM E57 3D Imaging Data File]]></formatName><data3D type=\"Vector\">",
        );
        for s in scans {
            let start = l.len();
            let runs: Vec<(usize, Vec<u8>)> = s.fields.iter().map(F::bytes).collect();
            let packets = if s.split {
                let first: Vec<&[u8]> = runs.iter().map(|(_, r)| &r[..r.len() / 2]).collect();
                let second: Vec<&[u8]> = runs.iter().map(|(_, r)| &r[r.len() / 2..]).collect();
                [packet(&first), vec![2, 0, 3, 0], packet(&second)].concat()
            } else {
                packet(&runs.iter().map(|(_, r)| r.as_slice()).collect::<Vec<_>>())
            };
            l.push(1);
            l.extend([0; 7]);
            l.extend(((32 + packets.len()) as u64).to_le_bytes());
            l.extend(physical(start + 32).to_le_bytes());
            l.extend(0u64.to_le_bytes());
            l.extend(packets);
            let records = runs.first().map_or(0, |(n, _)| *n);
            let prototype: String = s.fields.iter().map(F::xml).collect();
            xml += &format!(
                "<vectorChild type=\"Structure\">{}<points type=\"CompressedVector\" fileOffset=\"{}\" recordCount=\"{records}\">\
                 <prototype type=\"Structure\">{prototype}</prototype><codecs type=\"Vector\"/></points></vectorChild>",
                s.extra,
                physical(start)
            );
        }
        xml += "</data3D></e57Root>";
        let xml_start = l.len();
        l.extend(xml.as_bytes());
        paged(l, xml_start, xml.len(), page)
    }

    fn pose(q: [f64; 4], t: [f64; 3]) -> String {
        format!(
            "<pose type=\"Structure\"><rotation type=\"Structure\"><w type=\"Float\">{}</w><x type=\"Float\">{}</x><y type=\"Float\">{}</y>\
             <z type=\"Float\">{}</z></rotation><translation type=\"Structure\"><x type=\"Float\">{}</x><y type=\"Float\">{}</y>\
             <z type=\"Float\">{}</z></translation></pose>",
            q[0], q[1], q[2], q[3], t[0], t[1], t[2]
        )
    }

    fn near(a: &[[f64; 3]], b: &[[f64; 3]]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.iter().zip(q).all(|(x, y)| (x - y).abs() < 1e-9))
    }

    fn intensity(r: &PlyCloud) -> Vec<f32> {
        r.complex.column("intensity", On::Point).map(|c| c.values.0.to_vec()).unwrap_or_default()
    }

    fn xyz(x: f64, y: f64, z: f64) -> TestScan {
        TestScan {
            fields: vec![F::Float("cartesianX", true, vec![x]), F::Float("cartesianY", true, vec![y]), F::Float("cartesianZ", true, vec![z])],
            ..TestScan::default()
        }
    }

    #[test]
    fn the_checksum_is_crc_32c() {
        // The check value of CRC-32C (Castagnoli).
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    fn two_scans() -> Vec<TestScan> {
        let a = TestScan {
            fields: vec![
                F::Float("cartesianX", true, vec![1.0, 2.0, 3.0]),
                F::Float("cartesianY", true, vec![0.5, 0.0, -1.0]),
                F::Float("cartesianZ", true, vec![0.0, 0.25, 9.0]),
                F::Int("colorRed", 0, 255, vec![255, 0, 7]),
                F::Int("colorGreen", 0, 255, vec![0, 255, 7]),
                F::Int("colorBlue", 0, 255, vec![0, 0, 7]),
                F::Float("intensity", false, vec![0.25, 0.5, 0.75]),
                F::Int("cartesianInvalidState", 0, 2, vec![0, 0, 2]),
            ],
            ..TestScan::default()
        };
        // Positions in millimetre steps, turned a quarter about z and then moved 10 along x.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let b = TestScan {
            fields: vec![
                F::Scaled("cartesianX", -100_000, 100_000, 0.001, 0.0, vec![1000, 0]),
                F::Scaled("cartesianY", -100_000, 100_000, 0.001, 0.0, vec![0, 2500]),
                F::Scaled("cartesianZ", -100_000, 100_000, 0.001, 0.5, vec![0, -500]),
                F::Int("intensity", 0, 4095, vec![100, 4095]),
            ],
            extra: pose([h, 0.0, 0.0, h], [10.0, 0.0, 0.0]),
            split: true,
        };
        vec![a, b]
    }

    #[test]
    fn two_scans_are_moved_by_their_poses_and_read_into_one_cloud() {
        // Pages of 64 bytes put the packets and the XML over many pages.
        for page in [1024, 64] {
            let r = read_e57_cloud(&e57(&two_scans(), page), 100).unwrap();
            assert!(near(&r.positions, &[[1.0, 0.5, 0.0], [2.0, 0.0, 0.25], [10.0, 1.0, 0.5], [7.5, 0.0, 0.0]]), "{page}: {:?}", r.positions);
            assert_eq!((r.vertices, r.skipped), (5, 1));
            // The scan without colours is white.
            assert_eq!(r.colors, Some(vec![[255, 0, 0], [0, 255, 0], [255; 3], [255; 3]]));
            assert_eq!(intensity(&r), vec![0.25, 0.5, 100.0, 4095.0]);
            assert!(!r.complex.column("intensity", On::Point).unwrap().integer, "one scan's intensity is a float");
        }
    }

    #[test]
    fn a_scan_by_range_and_angles_with_colour_limits() {
        let half = std::f64::consts::FRAC_PI_2;
        let s = TestScan {
            fields: vec![
                F::Float("sphericalRange", false, vec![2.0, 3.0, 4.0]),
                F::Float("sphericalAzimuth", true, vec![0.0, half, 0.0]),
                F::Float("sphericalElevation", true, vec![0.0, 0.0, half]),
                F::Int("colorRed", 0, 65535, vec![65535, 0, 0]),
                F::Int("colorGreen", 0, 65535, vec![0, 65535, 0]),
                F::Int("colorBlue", 0, 65535, vec![0, 0, 65535]),
                F::Int("sphericalInvalidState", 0, 2, vec![0, 1, 0]),
            ],
            extra: "<colorLimits type=\"Structure\"><colorRedMinimum type=\"Integer\"/><colorRedMaximum type=\"Integer\">65535</colorRedMaximum>\
                    <colorGreenMinimum type=\"Integer\"/><colorGreenMaximum type=\"Integer\">65535</colorGreenMaximum>\
                    <colorBlueMinimum type=\"Integer\"/><colorBlueMaximum type=\"Integer\">65535</colorBlueMaximum></colorLimits>"
                .into(),
            ..TestScan::default()
        };
        let r = read_e57_cloud(&e57(&[s], 256), 100).unwrap();
        // The second point has a direction but no range: it is left out.
        assert!(near(&r.positions, &[[2.0, 0.0, 0.0], [0.0, 0.0, 4.0]]), "{:?}", r.positions);
        assert_eq!((r.skipped, r.colors), (1, Some(vec![[255, 0, 0], [0, 0, 255]])));
        assert!(r.complex.columns.is_empty());
    }

    #[test]
    fn a_scan_whose_fields_each_hold_one_value_has_all_its_points() {
        // Every field's minimum is its maximum: the values take no bits, and the points are made.
        let s = TestScan {
            fields: vec![
                F::Scaled("cartesianX", 5, 5, 0.5, 0.0, vec![5, 5]),
                F::Scaled("cartesianY", 0, 0, 1.0, 1.0, vec![0, 0]),
                F::Int("cartesianZ", -3, -3, vec![-3, -3]),
            ],
            ..TestScan::default()
        };
        let r = read_e57_cloud(&e57(&[s], 1024), 10).unwrap();
        assert!(near(&r.positions, &[[2.5, 1.0, -3.0], [2.5, 1.0, -3.0]]), "{:?}", r.positions);
    }

    #[test]
    fn points_without_a_position_are_left_out_before_thinning() {
        let s = TestScan {
            fields: vec![
                F::Float("cartesianX", true, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
                F::Float("cartesianY", true, vec![0.0; 7]),
                F::Float("cartesianZ", true, vec![0.0, f64::NAN, 0.0, 0.0, 0.0, 0.0, 0.0]),
                F::Int("intensity", 0, 7, vec![0, 1, 2, 3, 4, 5, 6]),
                F::Int("cartesianInvalidState", 0, 2, vec![0, 0, 0, 2, 0, 0, 0]),
            ],
            ..TestScan::default()
        };
        let file = e57(&[s], 1024);
        let whole = read_e57_cloud(&file, 100).unwrap();
        assert_eq!((whole.positions.len(), whole.skipped), (5, 2));
        // Five points have a position: one in three of them is kept.
        let r = read_e57_cloud(&file, 2).unwrap();
        assert!(near(&r.positions, &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]), "{:?}", r.positions);
        assert_eq!(intensity(&r), vec![0.0, 5.0]);
        assert!(r.complex.column("intensity", On::Point).unwrap().integer);
    }

    #[test]
    fn a_damaged_page_is_refused() {
        let mut file = e57(&[xyz(1.0, 2.0, 3.0)], 128);
        assert!(read_e57_cloud(&file, 10).is_ok());
        file[200] ^= 1;
        let e = read_e57_cloud(&file, 10).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("page 2") && e.contains("checksum"), "{e}");
    }

    #[test]
    fn hostile_files_are_refused_and_never_panic() {
        let good = e57(&[xyz(1.0, 2.0, 3.0)], 128);
        assert!(read_e57_cloud(&good, 10).is_ok());
        let points = |attrs: &str, prototype: &str| {
            format!(
                "<e57Root type=\"Structure\"><data3D type=\"Vector\"><vectorChild type=\"Structure\"><points type=\"CompressedVector\" {attrs}>\
                 <prototype type=\"Structure\">{prototype}</prototype></points></vectorChild></data3D></e57Root>"
            )
        };
        let xyz_fields = "<cartesianX type=\"Float\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>";
        // A section of points at the start of the data, with one packet of three runs of eight bytes.
        let p = packet(&[&[0; 8], &[0; 8], &[0; 8]]);
        let section = |p: &[u8]| {
            let mut s = vec![1u8, 0, 0, 0, 0, 0, 0, 0];
            s.extend((32 + p.len() as u64).to_le_bytes());
            s.extend((HEADER as u64 + 32).to_le_bytes());
            s.extend(0u64.to_le_bytes());
            s.extend(p);
            s
        };
        // A file with the XML `xml` after the section `data`, `@DATA` standing for where it is.
        let file = |xml: &str, data: &[u8]| {
            let mut l = vec![0u8; HEADER];
            l.extend(data);
            let at = l.len();
            let xml = xml.replace("@DATA", &HEADER.to_string());
            l.extend(xml.as_bytes());
            paged(l, at, xml.len(), 1024)
        };
        let one = |prototype: &str| file(&points("fileOffset=\"@DATA\" recordCount=\"1\"", prototype), &section(&p));
        assert!(read_e57_cloud(&one(xyz_fields), 10).is_ok(), "{:?}", read_e57_cloud(&one(xyz_fields), 10).err());
        // A string field the points do not need is passed over; its run still counts.
        let with_label = section(&packet(&[&[0; 8], &[0; 8], &[0; 8], &[]]));
        let label = format!("{xyz_fields}<label type=\"String\"/>");
        assert!(read_e57_cloud(&file(&points("fileOffset=\"@DATA\" recordCount=\"1\"", &label), &with_label), 10).is_ok());
        // The bit packing named as a codec is read; another codec is not.
        let codec = |name: &str| {
            format!(
                "<e57Root type=\"Structure\"><data3D type=\"Vector\"><vectorChild type=\"Structure\"><points type=\"CompressedVector\" \
                 fileOffset=\"@DATA\" recordCount=\"1\"><prototype type=\"Structure\">{xyz_fields}</prototype><codecs type=\"Vector\">\
                 <vectorChild type=\"Structure\"><inputs type=\"Vector\"/><{name} type=\"Structure\"/></vectorChild></codecs></points>\
                 </vectorChild></data3D></e57Root>"
            )
        };
        assert!(read_e57_cloud(&file(&codec("bitPackCodec"), &section(&p)), 10).is_ok());
        let e = read_e57_cloud(&file(&codec("zipCodec"), &section(&p)), 10).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("zipCodec"), "{e}");
        // Two scans that share their points count the packets twice: more than the file holds.
        let scan = "<vectorChild type=\"Structure\"><points type=\"CompressedVector\" fileOffset=\"@DATA\" recordCount=\"1\">\
                    <prototype type=\"Structure\"><cartesianX type=\"Float\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>\
                    </prototype></points></vectorChild>";
        let shared = |n: usize| format!("<e57Root type=\"Structure\"><data3D type=\"Vector\">{}</data3D></e57Root>", scan.repeat(n));
        assert!(read_e57_cloud(&file(&shared(4000), &section(&p)), 10).is_err());
        let mut unknown = p.clone();
        unknown[0] = 9;
        let mut long_run = p.clone();
        long_run[6] = 200;
        let mut cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            SIGNATURE.to_vec(),
            good[..good.len() - 1].to_vec(),
            file(&points("fileOffset=\"@DATA\" recordCount=\"1000000000000\"", xyz_fields), &section(&p)),
            file(&points("fileOffset=\"@DATA\" recordCount=\"2\"", xyz_fields), &section(&p)),
            file(&points("fileOffset=\"@DATA\" recordCount=\"-1\"", xyz_fields), &section(&p)),
            file(&points("fileOffset=\"1020\" recordCount=\"1\"", xyz_fields), &section(&p)),
            file(&points("fileOffset=\"99999999\" recordCount=\"1\"", xyz_fields), &section(&p)),
            file(&points("recordCount=\"1\"", xyz_fields), &section(&p)),
            file(&points("fileOffset=\"@DATA\" recordCount=\"1\"", xyz_fields), &section(&unknown)),
            file(&points("fileOffset=\"@DATA\" recordCount=\"1\"", xyz_fields), &section(&long_run)),
            one("<cartesianX type=\"Float\"/><cartesianY type=\"Float\"/><intensity type=\"Float\"/>"),
            one(&format!("{xyz_fields}<intensity type=\"Float\"/>")),
            one("<cartesianX type=\"String\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>"),
            one("<cartesianX type=\"Integer\" minimum=\"5\" maximum=\"1\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>"),
            one("<cartesianX type=\"ScaledInteger\" scale=\"0\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>"),
            one("<cartesianX type=\"Integer\" minimum=\"x\"/><cartesianY type=\"Float\"/><cartesianZ type=\"Float\"/>"),
            file("<e57Root type=\"Structure\"><data3D type=\"Vector\"/></e57Root>", &section(&p)),
            file("<e57Root><data3D>", &section(&p)),
            file("<other/>", &section(&p)),
            file(&"<a>".repeat(100), &section(&p)),
            file("not xml at all <<<", &section(&p)),
        ];
        // A wrong version, a wrong length, a page too small, an XML part too long.
        for (at, value) in [(8usize, 2u64), (16, 1), (40, 8), (32, u64::MAX)] {
            let mut f = good.clone();
            if at == 8 {
                f[8..12].copy_from_slice(&(value as u32).to_le_bytes());
            } else {
                f[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            // Mend the first page's checksum, so that the change itself is what is refused.
            let sum = crc32c(&f[..128 - CHECKSUM]);
            f[128 - CHECKSUM..128].copy_from_slice(&sum.to_be_bytes());
            cases.push(f);
        }
        for (i, c) in cases.iter().enumerate() {
            assert!(read_e57_cloud(c, 10).is_err(), "case {i} was read");
        }
    }
}
