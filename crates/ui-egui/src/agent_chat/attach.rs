//! Files a person adds to a message (the + button, a drop on the chat, a paste): pictures go to the
//! agent with it, text files go in as text, PDFs go as documents to agents that take them, CAD
//! files are imported into the design, and any other file is named by reference (its type, size
//! and place on this machine) for the agent to read with its own tools. A folder goes by its place
//! too, with what it holds; and past [`MAX_FILES`] files, every further one does, so a batch of
//! photos reaches the agent whole, at full size, however many there are. The agent is told what
//! arrived. Each file is checked and bounded before it is kept; a refused one says why.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::backend::{self, PromptPart};
use solidcraft_ui_text::format as trf;

/// A refusal in the app's language: the file's name, then why.
fn refused(key: &str, name: &str, args: &[(&str, &str)]) -> String {
    let mut all = vec![("name", name)];
    all.extend_from_slice(args);
    trf(key, &all)
}

/// A refusal carrying the system's or the decoder's own words.
fn failed(name: &str, e: impl std::fmt::Display) -> String {
    refused("ui.agent_chat_file_error", name, &[("error", &e.to_string())])
}

/// Most files whose contents go with one message (pictures, PDFs, text, imported parts); past it,
/// a file goes by its place ([`reference`]).
pub const MAX_FILES: usize = 8;
/// Most files and folders on one message, those that go by their place included.
pub const MAX_ON_MESSAGE: usize = 200;
/// A folder's entries read to say what it holds, and the most of them named in the message.
const MAX_FOLDER_SCAN: usize = 10_000;
const MAX_FOLDER_NAMED: usize = 200;
/// Largest picture file read, before it is scaled down.
const MAX_PICTURE_BYTES: u64 = 40 * 1024 * 1024;
/// Largest text file sent in full.
const MAX_TEXT_BYTES: u64 = 256 * 1024;
/// Largest PDF sent as a document; a bigger one goes by reference.
const MAX_PDF_BYTES: u64 = 12 * 1024 * 1024;
/// Pages whose text is taken from a PDF, and the most characters of it kept.
const MAX_PDF_PAGES: usize = 300;
const MAX_PDF_CHARS: usize = 200_000;
/// A PDF page's content may inflate to at most this (a compressed bomb is refused by the reader).
const MAX_PDF_PAGE_BYTES: usize = 32 * 1024 * 1024;
/// A picture's longer side is scaled down to this before it is sent.
pub const MAX_SIDE: u32 = 2048;
/// Largest picture sent, as it came or re-encoded: its base64 (a third larger) stays under 5 MB,
/// so eight of them still fit in a prompt.
pub const MAX_SENT_BYTES: usize = 3_750_000;
/// A picture is not halved below this to fit; past it, it is refused.
const MIN_SIDE: u32 = 64;
/// Largest picture decoded (pixels on a side), so a hostile header can't ask for gigabytes.
const MAX_DECODE_SIDE: u32 = 20_000;
/// The chip's picture.
const THUMB: u32 = 64;

/// Pictures the agents read as they are; BMP and TIFF are sent as PNG.
const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
/// Text sent in full; SVG goes as its source, which the agents read well.
const TEXT_EXTS: &[&str] = &["svg", "txt", "md", "markdown", "json", "csv", "tsv", "yaml", "yml", "toml", "xml", "log", "ini", "cfg"];
/// CAD files and meshes, imported into the design (the file picker's list; [`kind`] asks the
/// importers themselves).
const CAD_EXTS: &[&str] = &["step", "stp", "igs", "iges", "FCStd", "3dm", "stl", "3mf", "obj", "glb", "gltf"];

/// Every extension the file picker lists first, pictures first (any other file can be chosen too).
pub fn extensions() -> Vec<&'static str> {
    PICTURE_EXTS.iter().chain(TEXT_EXTS).chain(CAD_EXTS).chain(&["pdf"]).copied().collect()
}

/// What a file is, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Text,
    Cad,
    Pdf,
    /// Anything else, attached by reference.
    Other,
}

pub fn kind(path: &Path) -> Kind {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match ext.as_str() {
        e if PICTURE_EXTS.contains(&e) => Kind::Picture,
        e if TEXT_EXTS.contains(&e) => Kind::Text,
        "pdf" => Kind::Pdf,
        _ => {
            let p = path.to_string_lossy();
            if solidcraft_engine::io::is_step_path(&p) || solidcraft_engine::io::is_mesh_path(&p) { Kind::Cad } else { Kind::Other }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// The picture's bytes as sent (PNG, JPEG, GIF or WebP), and its size in pixels.
    Picture {
        mime: &'static str,
        bytes: Arc<Vec<u8>>,
        size: [u32; 2],
    },
    Text(Arc<String>),
    /// A PDF, sent whole as a document to an agent that takes them; to the others, its text.
    Pdf {
        bytes: Arc<Vec<u8>>,
        path: PathBuf,
        text: Option<Arc<PdfText>>,
    },
    /// A file the agent is pointed to, not given: what it is, its size and where it is (and a
    /// picture's size in pixels, from its header).
    Reference {
        path: PathBuf,
        size: u64,
        mime: &'static str,
        pixels: Option<[u32; 2]>,
    },
    /// A folder the agent is pointed to: where it is and what it holds, none of its files.
    Folder {
        path: PathBuf,
        listing: Arc<Listing>,
    },
    /// A CAD file imported into the design as its own component.
    Imported,
}

/// What a folder holds at its top level, as far as it was read (hidden entries left out).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    /// The first [`MAX_FOLDER_NAMED`] names in order, a file's with its size; a folder's ends in
    /// `/` and has none.
    pub entries: Vec<(String, Option<u64>)>,
    pub files: usize,
    pub folders: usize,
    /// The files' sizes together.
    pub bytes: u64,
    /// Reading stopped at [`MAX_FOLDER_SCAN`] entries: it holds more than counted.
    pub cut: bool,
}

/// What was taken out of a PDF: its page count and its text, without layout, pictures or fonts.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfText {
    pub pages: usize,
    pub text: String,
    /// The text stops short: of the pages or of the characters kept.
    pub cut: bool,
}

/// The text of a PDF, from its pages in order (up to [`MAX_PDF_PAGES`] pages and
/// [`MAX_PDF_CHARS`] characters). `None` when it can't be read at all (damaged, or a reader
/// panic, which is caught); an encrypted or scanned one gives its page count and no text.
#[cfg(not(target_arch = "wasm32"))]
pub fn pdf_text(bytes: &[u8]) -> Option<PdfText> {
    std::panic::catch_unwind(|| {
        let doc = lopdf::Document::load_mem(bytes).ok()?;
        let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
        let mut text = String::new();
        let mut chars = 0usize;
        let mut cut = pages.len() > MAX_PDF_PAGES;
        if !doc.is_encrypted() {
            for page in pages.iter().take(MAX_PDF_PAGES) {
                // A page that can't be read is skipped; the others still give their text.
                let chunks = doc.extract_text_chunks_with_limit(&[*page], MAX_PDF_PAGE_BYTES);
                let page_text: String = chunks.into_iter().filter_map(Result::ok).collect();
                let page_text = page_text.trim();
                if page_text.is_empty() {
                    continue;
                }
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                let room = MAX_PDF_CHARS.saturating_sub(chars);
                let n = page_text.chars().count();
                if n > room {
                    text.extend(page_text.chars().take(room));
                    cut = true;
                    break;
                }
                text.push_str(page_text);
                chars += n;
            }
        }
        Some(PdfText { pages: pages.len(), text, cut })
    })
    .ok()
    .flatten()
}

#[cfg(target_arch = "wasm32")]
pub fn pdf_text(_bytes: &[u8]) -> Option<PdfText> {
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// Its own on the message, never reused (set when it is added there; 0 until then): the
    /// chip's thumbnail is kept by it.
    pub id: u64,
    pub name: String,
    pub content: Content,
    /// A small copy of a picture for its chip.
    pub thumb: Option<Arc<egui::ColorImage>>,
}

impl Attachment {
    pub fn imported(name: String) -> Self {
        Attachment { id: 0, name, content: Content::Imported, thumb: None }
    }
    pub fn is_picture(&self) -> bool {
        matches!(self.content, Content::Picture { .. })
    }
    /// The agent is told where it is, not given it: it costs the message no more than its line.
    pub fn by_place(&self) -> bool {
        matches!(self.content, Content::Reference { .. } | Content::Folder { .. })
    }
    /// The chip's mark for a file with no thumbnail: its extension, in capitals, cut to fit.
    pub fn mark(&self) -> String {
        if matches!(self.content, Content::Folder { .. }) {
            return String::new();
        }
        let ext = Path::new(&self.name).extension().and_then(|e| e.to_str()).unwrap_or("");
        ext.chars().take(4).collect::<String>().to_uppercase()
    }
}

/// What a file is called to the agent, by its extension.
fn mime_of(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "txt" | "log" | "ini" | "cfg" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "gz" | "tgz" => "application/gzip",
        "dxf" => "image/vnd.dxf",
        "dwg" => "image/vnd.dwg",
        "step" | "stp" => "model/step",
        "iges" | "igs" => "model/iges",
        "stl" => "model/stl",
        "obj" => "model/obj",
        "3mf" => "model/3mf",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp4" | "mov" | "webm" => "video/mp4",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// A size for people: `1.5 MB`.
fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} bytes")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else {
        format!("{:.1} GB", b / KB / KB / KB)
    }
}

/// A `file://` link to a path, for a document sent to an agent.
fn file_uri(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    let mut out = String::from("file://");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(crate) fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// Read a picture or text file for a message. CAD files are imported by the caller, not read here.
pub fn read(path: &Path) -> Result<Attachment, String> {
    let name = name_of(path);
    let meta = std::fs::metadata(path).map_err(|e| failed(&name, e))?;
    if !meta.is_file() {
        return Err(refused("ui.agent_chat_file_not_a_file", &name, &[]));
    }
    match kind(path) {
        Kind::Picture => {
            if meta.len() > MAX_PICTURE_BYTES {
                return Err(refused("ui.agent_chat_file_picture_too_big", &name, &[("max", &(MAX_PICTURE_BYTES / 1024 / 1024).to_string())]));
            }
            let bytes = std::fs::read(path).map_err(|e| failed(&name, e))?;
            picture(name, &bytes)
        }
        Kind::Text => {
            if meta.len() > MAX_TEXT_BYTES {
                return Err(refused("ui.agent_chat_file_text_too_big", &name, &[("max", &(MAX_TEXT_BYTES / 1024).to_string())]));
            }
            let bytes = std::fs::read(path).map_err(|e| failed(&name, e))?;
            let text = String::from_utf8(bytes).map_err(|_| refused("ui.agent_chat_file_not_utf8", &name, &[]))?;
            Ok(Attachment { id: 0, name, content: Content::Text(Arc::new(text)), thumb: None })
        }
        Kind::Cad => Err(refused("ui.agent_chat_file_cad_not_read", &name, &[])),
        Kind::Pdf => {
            let path = absolute(path);
            // Too big to send: the agent is pointed to it instead.
            if meta.len() > MAX_PDF_BYTES {
                return Ok(Attachment {
                    id: 0,
                    name,
                    content: Content::Reference { path, size: meta.len(), mime: "application/pdf", pixels: None },
                    thumb: None,
                });
            }
            let bytes = std::fs::read(&path).map_err(|e| failed(&name, e))?;
            // Read here, off the window thread: for an agent that takes no documents.
            let text = pdf_text(&bytes).map(Arc::new);
            Ok(Attachment { id: 0, name, content: Content::Pdf { bytes: Arc::new(bytes), path, text }, thumb: None })
        }
        Kind::Other if heic(path) => Err(refused("ui.agent_chat_file_heic", &name, &[])),
        Kind::Other => {
            let content = Content::Reference { path: absolute(path), size: meta.len(), mime: mime_of(path), pixels: None };
            Ok(Attachment { id: 0, name, content, thumb: None })
        }
    }
}

fn heic(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("heic" | "heif"))
}

/// A file the agent is pointed to instead of given, whatever its kind (one past [`MAX_FILES`]):
/// its type, size and place, and a picture's size in pixels, read from its header. Nothing else
/// is read, so the agent gets it whole, at full size, with its own tools.
pub fn reference(path: &Path) -> Result<Attachment, String> {
    let name = name_of(path);
    let meta = std::fs::metadata(path).map_err(|e| failed(&name, e))?;
    if !meta.is_file() {
        return Err(refused("ui.agent_chat_file_not_a_file", &name, &[]));
    }
    if heic(path) {
        return Err(refused("ui.agent_chat_file_heic", &name, &[]));
    }
    let pixels = if kind(path) == Kind::Picture { pixels(path) } else { None };
    let content = Content::Reference { path: absolute(path), size: meta.len(), mime: mime_of(path), pixels };
    Ok(Attachment { id: 0, name, content, thumb: None })
}

/// A picture's width and height from its header, without decoding it.
fn pixels(path: &Path) -> Option<[u32; 2]> {
    let (w, h) = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?;
    Some([w, h])
}

/// A folder's name on its chip and in the message: its own name, marked as a folder.
pub(crate) fn folder_name(path: &Path) -> String {
    format!("{}/", name_of(path).trim_end_matches(['/', '\\']))
}

/// A folder for a message: the agent is told where it is and what it holds at its top level (up
/// to [`MAX_FOLDER_SCAN`] entries read, [`MAX_FOLDER_NAMED`] named, hidden ones left out), and
/// reads the files it needs with its own tools. None of its files is read here.
pub fn folder(path: &Path) -> Result<Attachment, String> {
    let name = folder_name(path);
    let entries = std::fs::read_dir(path).map_err(|e| failed(&name, e))?;
    let mut listing = Listing::default();
    for (seen, entry) in entries.enumerate() {
        if seen >= MAX_FOLDER_SCAN {
            listing.cut = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        let entry_name = entry.file_name().to_string_lossy().to_string();
        if entry_name.starts_with('.') {
            continue;
        }
        // A link counts as what it leads to, as the agent's tools would see it; a broken one is
        // left out.
        let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
        if meta.is_dir() {
            listing.folders += 1;
            listing.entries.push((format!("{entry_name}/"), None));
        } else {
            listing.files += 1;
            listing.bytes = listing.bytes.saturating_add(meta.len());
            listing.entries.push((entry_name, Some(meta.len())));
        }
    }
    listing.entries.sort_by_cached_key(|(n, _)| n.to_lowercase());
    listing.entries.truncate(MAX_FOLDER_NAMED);
    Ok(Attachment { id: 0, name, content: Content::Folder { path: absolute(path), listing: Arc::new(listing) }, thumb: None })
}

/// The path as an absolute one, so the agent (working in another folder) finds the file.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A picture from a file's bytes: decoded within limits, and sent as it came when the agents read
/// that format (PNG, JPEG, GIF, WebP), it is at most [`MAX_SIDE`] on a side and at most
/// [`MAX_SENT_BYTES`]; otherwise re-encoded (see [`encoded`]).
pub fn picture(name: String, bytes: &[u8]) -> Result<Attachment, String> {
    picture_within(name, bytes, MAX_SENT_BYTES)
}

fn picture_within(name: String, bytes: &[u8], limit: usize) -> Result<Attachment, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| failed(&name, e))?;
    let format = reader.format();
    let mime = match format {
        Some(image::ImageFormat::Png) => Some("image/png"),
        Some(image::ImageFormat::Jpeg) => Some("image/jpeg"),
        Some(image::ImageFormat::Gif) => Some("image/gif"),
        Some(image::ImageFormat::WebP) => Some("image/webp"),
        Some(image::ImageFormat::Bmp | image::ImageFormat::Tiff) => None,
        _ => return Err(refused("ui.agent_chat_file_unreadable_picture", &name, &[])),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| failed(&name, e))?;
    if img.width() == 0 || img.height() == 0 {
        return Err(refused("ui.agent_chat_file_empty_picture", &name, &[]));
    }
    if let Some(mime) = mime
        && img.width().max(img.height()) <= MAX_SIDE
        && bytes.len() <= limit
    {
        let thumb = thumbnail(&img);
        return Ok(Attachment {
            id: 0,
            name,
            content: Content::Picture { mime, bytes: Arc::new(bytes.to_vec()), size: [img.width(), img.height()] },
            thumb,
        });
    }
    encoded(name, img, limit)
}

/// A picture copied to the clipboard (RGBA rows from the system), sent as PNG.
pub fn from_rgba(name: String, width: usize, height: usize, rgba: &[u8]) -> Result<Attachment, String> {
    let (w, h) = (u32::try_from(width).ok(), u32::try_from(height).ok());
    let (Some(w), Some(h)) = (w, h) else { return Err(refused("ui.agent_chat_file_too_large", &name, &[])) };
    if w == 0 || h == 0 || w > MAX_DECODE_SIDE || h > MAX_DECODE_SIDE || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(rgba.len())
    {
        return Err(refused("ui.agent_chat_file_not_a_picture", &name, &[]));
    }
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or_else(|| refused("ui.agent_chat_file_not_a_picture", &name, &[]))?;
    encoded(name, image::DynamicImage::ImageRgba8(img), MAX_SENT_BYTES)
}

/// Scale down to [`MAX_SIDE`] and encode within `limit` bytes: as PNG when that fits; else as JPEG
/// when the picture has no transparency; else halved and tried again. A GIF arrives here as its
/// first frame.
fn encoded(name: String, img: image::DynamicImage, limit: usize) -> Result<Attachment, String> {
    let mut img = if img.width().max(img.height()) > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle) } else { img };
    let opaque = opaque(&img);
    loop {
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| failed(&name, e))?;
        let mut sent = (png.len() <= limit).then_some(("image/png", png));
        if sent.is_none() && opaque {
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode_image(&img.to_rgb8()).map_err(|e| failed(&name, e))?;
            sent = (jpeg.len() <= limit).then_some(("image/jpeg", jpeg));
        }
        if let Some((mime, bytes)) = sent {
            let thumb = thumbnail(&img);
            return Ok(Attachment {
                id: 0,
                name,
                content: Content::Picture { mime, bytes: Arc::new(bytes), size: [img.width(), img.height()] },
                thumb,
            });
        }
        if img.width().max(img.height()) <= MIN_SIDE {
            return Err(refused("ui.agent_chat_file_too_large_to_send", &name, &[]));
        }
        img = img.resize((img.width() / 2).max(1), (img.height() / 2).max(1), image::imageops::FilterType::Triangle);
    }
}

/// No pixel is see-through (a screenshot or a photo, even when stored with an alpha channel).
fn opaque(img: &image::DynamicImage) -> bool {
    !img.color().has_alpha() || img.to_rgba8().pixels().all(|p| p.0[3] == 255)
}

fn thumbnail(img: &image::DynamicImage) -> Option<Arc<egui::ColorImage>> {
    let t = img.thumbnail(THUMB, THUMB).to_rgba8();
    let size = [usize::try_from(t.width()).ok()?, usize::try_from(t.height()).ok()?];
    Some(Arc::new(egui::ColorImage::from_rgba_unmultiplied(size, t.as_raw())))
}

/// Add a file's text to the message under `head`, within the `left` characters shared by the
/// files: whole when it fits; cut, saying how much is missing, when it doesn't; named only when
/// nothing is left.
fn fit(out: &mut String, left: &mut usize, head: &str, text: &str) {
    let t = text.trim_end();
    let len = t.chars().count();
    if *left == 0 {
        *out += &format!("\n\n{head}: left out, the message is at its size limit.");
    } else if len <= *left {
        *out += &format!("\n\n{head}:\n```\n{t}\n```");
        *left -= len;
    } else {
        let kept: String = t.chars().take(*left).collect();
        *out += &format!("\n\n{head} (cut: {} more characters left out):\n```\n{kept}\n```", len - *left);
        *left = 0;
    }
}

/// What the message says about its files, after the person's words: the text files' text, and the
/// pictures and imported parts by name (the agent sees the pictures themselves as images).
/// `pictures_sent` and `documents_sent`: whether this agent takes pictures and PDFs. The text files share `budget` characters
/// in order: one that doesn't fit is cut and says how much is missing, and one past the budget
/// is named only, so the message stays within the prompt cap. The lines that name files (those
/// that go by their place, a folder's listing) take their room from the budget first.
pub fn note(files: &[Attachment], pictures_sent: bool, documents_sent: bool, budget: usize) -> String {
    let mut out = String::new();
    let pictures: Vec<&str> = files.iter().filter(|f| f.is_picture()).map(|f| f.name.as_str()).collect();
    if !pictures.is_empty() {
        let list = pictures.join(", ");
        out += &if pictures_sent {
            format!("\n\nAttached pictures: {list}.")
        } else {
            format!("\n\n(Pictures attached but not sent, this agent takes none: {list}.)")
        };
    }
    let named: usize = files.iter().filter_map(|f| named_line(f, documents_sent)).map(|l| l.chars().count()).sum();
    let mut left = budget.saturating_sub(named);
    for f in files {
        if let Some(line) = named_line(f, documents_sent) {
            out += &line;
            continue;
        }
        match &f.content {
            Content::Text(t) => fit(&mut out, &mut left, &format!("Attached file `{}`", f.name), t),
            // An agent that takes no documents gets the text (one with none is named instead).
            Content::Pdf { path, bytes, text: Some(t) } => {
                let cut = if t.cut { "; cut, the rest is in the file" } else { "" };
                let head = format!(
                    "Attached PDF `{}` ({} pages, {}, at {}): its text only, layout, pictures and fonts are dropped{cut}",
                    f.name,
                    t.pages,
                    size_text(bytes.len() as u64),
                    path.display()
                );
                fit(&mut out, &mut left, &head, &t.text);
            }
            _ => {}
        }
    }
    out
}

/// The line that names a file without its contents: a PDF sent as a document, or one whose text
/// could not be taken; a file or folder that goes by its place; an imported part. `None` for
/// pictures (named together) and for text, which shares the budget.
fn named_line(f: &Attachment, documents_sent: bool) -> Option<String> {
    let line = match &f.content {
        Content::Pdf { path, bytes, .. } if documents_sent => {
            format!("\n\nAttached PDF `{}` ({}, at {}): sent with this message as a document.", f.name, size_text(bytes.len() as u64), path.display())
        }
        Content::Pdf { path, bytes, text } if text.as_deref().is_none_or(|t| t.text.trim().is_empty()) => {
            let why = if text.is_some() { "no text could be taken from it (scanned or protected)" } else { "its text could not be read" };
            format!(
                "\n\nAttached file `{}` (PDF, {}, at {}): {why}; read it with your own tools, it is not in this message.",
                f.name,
                size_text(bytes.len() as u64),
                path.display()
            )
        }
        Content::Reference { path, size, mime, pixels } => {
            let pixels = pixels.map(|[w, h]| format!("{w}×{h} px, ")).unwrap_or_default();
            format!(
                "\n\nAttached file `{}` ({mime}, {pixels}{}, at {}): read it with your own tools; it is not in this message.",
                f.name,
                size_text(*size),
                path.display()
            )
        }
        Content::Folder { path, listing } => folder_line(&f.name, path, listing),
        Content::Imported => format!("\n\n(I added `{}` to the design as a component of its own.)", f.name),
        Content::Text(_) | Content::Pdf { .. } | Content::Picture { .. } => return None,
    };
    Some(line)
}

/// A folder in the message: where it is, what it holds, and the names of the first entries.
fn folder_line(name: &str, path: &Path, l: &Listing) -> String {
    let mut out = format!("\n\nAttached folder `{name}` (at {}): ", path.display());
    if l.files + l.folders == 0 {
        out += "it is empty (hidden files aside).";
        return out;
    }
    let count = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    out += &format!("{}, {} in all", count(l.files, "file"), size_text(l.bytes));
    if l.folders > 0 {
        out += &format!(", and {}", count(l.folders, "folder"));
    }
    if l.cut {
        out += &format!(" in the first {MAX_FOLDER_SCAN} entries read; it holds more");
    }
    out += ". Read what you need from it with your own tools; its files are not in this message, and none was scaled down.\nIn it: ";
    let names: Vec<String> = l
        .entries
        .iter()
        .map(|(n, size)| match size {
            Some(s) => format!("{n} ({})", size_text(*s)),
            None => n.clone(),
        })
        .collect();
    out += &names.join(", ");
    let more = (l.files + l.folders).saturating_sub(l.entries.len());
    if more > 0 || l.cut {
        out += &format!(", and {more}{} more", if l.cut { "+" } else { "" });
    }
    out.push('.');
    out
}

/// The pictures and PDFs as parts of the message, to the agents that take them.
pub fn parts(files: &[Attachment], pictures: bool, documents: bool) -> Vec<PromptPart> {
    files
        .iter()
        .filter_map(|f| match &f.content {
            Content::Picture { mime, bytes, .. } if pictures => Some(PromptPart::Image { mime: (*mime).to_string(), base64: backend::base64(bytes) }),
            Content::Pdf { bytes, path, .. } if documents => Some(PromptPart::Document {
                name: f.name.clone(),
                uri: file_uri(path),
                mime: "application/pdf".to_string(),
                base64: backend::base64(bytes),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-attach-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(path: &Path, w: u32, h: u32) {
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255])).save(path).unwrap();
    }

    #[test]
    fn pictures_are_kept_or_scaled_down_and_text_goes_in_whole() {
        let d = dir("read");
        let small = d.join("board.png");
        png(&small, 1600, 1120);
        let a = read(&small).unwrap();
        let Content::Picture { mime, size, bytes } = &a.content else { panic!("a picture") };
        assert_eq!((*mime, *size), ("image/png", [1600, 1120]));
        assert_eq!(bytes.as_slice(), std::fs::read(&small).unwrap().as_slice(), "small enough: sent as it came");
        assert!(a.thumb.as_ref().is_some_and(|t| t.size[0] <= 64 && t.size[1] <= 64));
        let big = d.join("photo.JPG");
        image::RgbImage::from_pixel(4000, 1000, image::Rgb([10, 10, 10])).save_with_format(&big, image::ImageFormat::Jpeg).unwrap();
        let b = read(&big).unwrap();
        let Content::Picture { mime, size, .. } = &b.content else { panic!("a picture") };
        assert_eq!((*mime, *size), ("image/png", [2048, 512]), "scaled to 2048 on the long side");
        let notes = d.join("brief.md");
        std::fs::write(&notes, "# Brief\nwall 2 mm\n").unwrap();
        let c = read(&notes).unwrap();
        assert!(matches!(&c.content, Content::Text(t) if t.contains("wall 2 mm")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Pixels that don't compress, so a small picture can be made larger than a small limit.
    fn noise(w: u32, h: u32, alpha: bool) -> image::DynamicImage {
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed & 0xff) as u8
        };
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(w, h, |_, _| {
            image::Rgba([next(), next(), next(), if alpha { next() } else { 255 }])
        }))
    }

    fn encode(img: &image::DynamicImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), format).unwrap();
        out
    }

    /// Review of #1744: a picture up to 2048 px went byte for byte up to the 40 MB file cap, and
    /// eight of those don't fit in a prompt. Over the limit a picture is re-encoded: PNG when that
    /// fits, JPEG when it has no transparency, halved until it fits; a GIF goes as its first frame.
    #[test]
    fn pictures_over_the_send_limit_are_reencoded_to_fit() {
        let limit = 40_000;
        let photo = encode(&noise(300, 200, false), image::ImageFormat::Png);
        assert!(photo.len() > limit, "the test picture is over the limit");
        let a = picture_within("photo.png".into(), &photo, limit).unwrap();
        let Content::Picture { mime, bytes, size } = &a.content else { panic!("a picture") };
        assert!(bytes.len() <= limit, "{} bytes", bytes.len());
        assert_eq!(*mime, "image/jpeg", "an opaque picture goes as JPEG when PNG is too big");
        assert!(size[0] <= 300 && size[1] <= 200 && size[0] * 2 == size[1] * 3, "{size:?}: smaller, same shape");
        let clear = encode(&noise(300, 200, true), image::ImageFormat::Png);
        let b = picture_within("overlay.png".into(), &clear, limit).unwrap();
        let Content::Picture { mime, bytes, size } = &b.content else { panic!("a picture") };
        assert_eq!(*mime, "image/png", "transparency stays PNG");
        assert!(bytes.len() <= limit && size[0] < 300, "{} bytes, {size:?}: halved to fit", bytes.len());
        let gif = encode(
            &image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(400, 300, image::Rgba([40, 90, 200, 255]))),
            image::ImageFormat::Gif,
        );
        let c = picture_within("loop.gif".into(), &gif, gif.len() - 1).unwrap();
        let Content::Picture { mime, size, bytes } = &c.content else { panic!("a picture") };
        assert_eq!(*mime, "image/png", "a GIF over the limit goes as its first frame, as PNG");
        assert!(bytes.len() < gif.len() && size[0] * 3 == size[1] * 4, "{size:?}: within the limit, same shape");
        assert!(a.thumb.is_some() && b.thumb.is_some() && c.thumb.is_some());
        let under = encode(&noise(60, 40, false), image::ImageFormat::Png);
        let d = picture_within("small.png".into(), &under, limit).unwrap();
        assert!(matches!(&d.content, Content::Picture { bytes, .. } if bytes.as_slice() == under.as_slice()), "under the limit: as it came");
        assert_eq!(MAX_SENT_BYTES, 3_750_000, "its base64 stays under 5 MB");
    }

    #[test]
    fn bad_files_are_refused_with_a_reason() {
        crate::i18n::install();
        let d = dir("bad");
        let fake = d.join("fake.png");
        std::fs::write(&fake, b"not a picture at all").unwrap();
        assert!(read(&fake).unwrap_err().contains("fake.png"));
        let text = d.join("big.txt");
        std::fs::write(&text, vec![b'a'; 300 * 1024]).unwrap();
        assert!(read(&text).unwrap_err().contains("at most"));
        let binary = d.join("bin.json");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(read(&binary).unwrap_err().contains("UTF-8"));
        assert!(read(&d.join("missing.png")).is_err());
        assert_eq!(kind(Path::new("part.STEP")), Kind::Cad);
        assert_eq!(kind(Path::new("mount.stl")), Kind::Cad);
        assert_eq!(kind(Path::new("frame.3dm")), Kind::Cad);
        assert!(from_rgba("x".into(), 2, 2, &[0; 15]).is_err(), "wrong length");
        assert!(from_rgba("x".into(), 0, 2, &[]).is_err());
        assert!(from_rgba("x".into(), usize::MAX, 2, &[]).is_err());
        let ok = from_rgba("pasted.png".into(), 3, 2, &[255; 24]).unwrap();
        assert!(matches!(ok.content, Content::Picture { size: [3, 2], .. }));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Review of #1744: the refusals were English whatever the app's language.
    #[test]
    fn refusals_speak_the_apps_language() {
        crate::i18n::install();
        let d = dir("lang");
        let binary = d.join("bin.json");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        {
            let _ja = solidcraft_ui_text::scope("ja");
            assert!(read(&binary).unwrap_err().contains("UTF-8 のテキストではありません"));
        }
        {
            let _zh = solidcraft_ui_text::scope("zh");
            assert!(read(&binary).unwrap_err().contains("不是 UTF-8 文本"));
        }
        assert!(read(&binary).unwrap_err().contains("not UTF-8 text"), "English again outside the scope");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every picture format the agents read goes as it is (PNG, JPEG, GIF, WebP); BMP and TIFF are
    /// sent as PNG; SVG goes as its source text; HEIC says why not.
    #[test]
    fn each_picture_format_goes_the_way_the_agent_reads_it() {
        crate::i18n::install();
        let d = dir("formats");
        let img = image::RgbaImage::from_pixel(30, 20, image::Rgba([20, 120, 220, 255]));
        for (file, format, mime) in [
            ("a.gif", image::ImageFormat::Gif, "image/gif"),
            ("a.webp", image::ImageFormat::WebP, "image/webp"),
            ("a.bmp", image::ImageFormat::Bmp, "image/png"),
            ("a.tiff", image::ImageFormat::Tiff, "image/png"),
        ] {
            let path = d.join(file);
            image::DynamicImage::ImageRgba8(img.clone()).save_with_format(&path, format).unwrap();
            let a = read(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
            let Content::Picture { mime: m, size, bytes } = &a.content else { panic!("{file}: a picture") };
            assert_eq!((*m, *size), (mime, [30, 20]), "{file}");
            if mime != "image/png" {
                assert_eq!(bytes.as_slice(), std::fs::read(&path).unwrap().as_slice(), "{file}: sent as it came");
            }
            assert!(a.thumb.is_some(), "{file}: a thumbnail");
        }
        let svg = d.join("sketch.svg");
        std::fs::write(&svg, "<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='5'/></svg>").unwrap();
        assert!(matches!(&read(&svg).unwrap().content, Content::Text(t) if t.contains("<rect")));
        let heic = d.join("photo.HEIC");
        std::fs::write(&heic, b"....ftypheic").unwrap();
        assert!(read(&heic).unwrap_err().contains("HEIC"));
        for ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "svg", "md", "txt", "step", "FCStd", "3dm", "stl", "3mf", "obj", "glb"] {
            assert!(extensions().contains(&ext), "{ext} can be chosen");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Review of #1744: eight 256 KB text files took a message to about 2 MB, past the prompt cap
    /// that only trimmed the typed text. The files' text now gets a budget: a file that doesn't
    /// fit is cut and says so, and a file past it is named only.
    #[test]
    fn attached_text_is_kept_within_the_prompt_budget() {
        let file = |n: &str, len: usize| Attachment { id: 0, name: n.into(), content: Content::Text(Arc::new("x".repeat(len))), thumb: None };
        let files = vec![file("a.md", 600), file("b.md", 600), file("c.md", 600)];
        let n = note(&files, true, false, 1000);
        assert!(n.contains("`a.md`") && n.contains("`b.md`") && n.contains("`c.md`"), "every file is named: {n}");
        assert!(n.matches('x').count() <= 1000, "the files' text stays within the budget");
        assert!(n.contains("200 more characters"), "a cut file says how much is missing: {n}");
        assert!(n.contains("left out"), "a file past the budget is named only: {n}");
        assert_eq!(note(&files, true, false, usize::MAX).matches('x').count(), 1800, "with room, every file goes whole");
    }

    #[test]
    fn the_message_names_its_files_and_carries_the_pictures() {
        let pic = from_rgba("target.png".into(), 2, 2, &[9; 16]).unwrap();
        let text = Attachment { id: 0, name: "brief.md".into(), content: Content::Text(Arc::new("wall 2 mm\n".into())), thumb: None };
        let part = Attachment::imported("motor.step".into());
        let files = vec![pic, text, part];
        let n = note(&files, true, false, usize::MAX);
        assert!(n.contains("Attached pictures: target.png."), "{n}");
        assert!(n.contains("Attached file `brief.md`:\n```\nwall 2 mm\n```"), "{n}");
        assert!(n.contains("`motor.step` to the design"), "{n}");
        assert!(note(&files, false, false, usize::MAX).contains("not sent"));
        let p = parts(&files, true, true);
        assert_eq!(p.len(), 1);
        assert!(matches!(&p[0], PromptPart::Image { mime, base64 } if mime == "image/png" && base64.starts_with("iVBORw0KGgo")));
    }

    /// Any file that is neither a picture, text, CAD nor a PDF is named by reference (what it is,
    /// its size and its place) instead of refused; the agent reads it with its own tools.
    #[test]
    fn other_files_go_by_reference() {
        crate::i18n::install();
        let d = dir("reference");
        let zip = d.join("fixtures.zip");
        std::fs::write(&zip, vec![0u8; 3000]).unwrap();
        let dxf = d.join("outline.DXF");
        std::fs::write(&dxf, b"0\nSECTION\n").unwrap();
        let a = read(&zip).unwrap();
        let Content::Reference { path, size, mime, .. } = &a.content else { panic!("a reference") };
        assert_eq!((*size, *mime), (3000, "application/zip"));
        assert!(path.is_absolute() && path.ends_with("fixtures.zip"));
        assert!(a.thumb.is_none() && a.mark() == "ZIP");
        assert!(matches!(read(&dxf).unwrap().content, Content::Reference { mime: "image/vnd.dxf", .. }), "by extension, in any case");
        let bare = d.join("LICENSE_BLOB");
        std::fs::write(&bare, b"x").unwrap();
        assert!(matches!(read(&bare).unwrap().content, Content::Reference { mime: "application/octet-stream", .. }), "no extension");
        let files = vec![a];
        let n = note(&files, true, true, usize::MAX);
        assert!(n.contains("Attached file `fixtures.zip` (application/zip, 3 KB, at "), "{n}");
        assert!(n.contains("fixtures.zip): read it with your own tools"), "{n}");
        assert!(parts(&files, true, true).is_empty(), "a reference sends nothing but the note");
        assert!(read(&d).unwrap_err().contains("not a file"), "a folder is refused");
        assert!(read(&d.join("missing.bin")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Past the files whose contents go with a message, a file of any kind goes by its place:
    /// nothing is read or scaled, a picture's size in pixels comes from its header, and the
    /// message points the agent to it. HEIC is refused here too.
    #[test]
    fn a_file_by_its_place_is_left_whole() {
        crate::i18n::install();
        let d = dir("place");
        let photo = d.join("IMG_0001.JPG");
        image::RgbImage::from_pixel(4032, 3024, image::Rgb([90, 90, 90])).save_with_format(&photo, image::ImageFormat::Jpeg).unwrap();
        let size = std::fs::metadata(&photo).unwrap().len();
        let a = reference(&photo).unwrap();
        let Content::Reference { path, size: s, mime, pixels } = &a.content else { panic!("a reference: {:?}", a.content) };
        assert_eq!((*s, *mime, *pixels), (size, "image/jpeg", Some([4032, 3024])), "its own size, not scaled down");
        assert!(path.is_absolute() && a.thumb.is_none() && a.by_place());
        let n = note(std::slice::from_ref(&a), true, true, usize::MAX);
        assert!(n.contains("Attached file `IMG_0001.JPG` (image/jpeg, 4032×3024 px, "), "{n}");
        assert!(n.contains("read it with your own tools; it is not in this message"), "{n}");
        assert!(parts(&[a], true, true).is_empty(), "nothing but the note");
        let notes = d.join("brief.md");
        std::fs::write(&notes, "wall 2 mm").unwrap();
        assert!(
            matches!(reference(&notes).unwrap().content, Content::Reference { mime: "text/markdown", pixels: None, .. }),
            "text goes by its place too"
        );
        let heic = d.join("photo.heic");
        std::fs::write(&heic, b"....ftypheic").unwrap();
        assert!(reference(&heic).unwrap_err().contains("HEIC"));
        assert!(reference(&d).unwrap_err().contains("not a file"));
        assert!(reference(&d.join("missing.jpg")).is_err());
        let fake = d.join("fake.png");
        std::fs::write(&fake, b"not a picture").unwrap();
        assert!(matches!(reference(&fake).unwrap().content, Content::Reference { pixels: None, .. }), "a bad header just has no size");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A folder goes by its place: the message says where it is, how many files and folders it
    /// holds and their size, and names its entries in order (hidden ones left out, a long list cut
    /// with how many more there are). None of its files is read.
    #[test]
    fn a_folder_goes_by_its_place_with_what_it_holds() {
        crate::i18n::install();
        let d = dir("folder");
        let photos = d.join("site photos");
        std::fs::create_dir_all(photos.join("raw")).unwrap();
        std::fs::write(photos.join("b.jpg"), vec![0u8; 2048]).unwrap();
        std::fs::write(photos.join("A.jpg"), vec![0u8; 1024]).unwrap();
        std::fs::write(photos.join(".DS_Store"), b"x").unwrap();
        let a = folder(&photos).unwrap();
        assert_eq!(a.name, "site photos/");
        assert!(a.by_place() && a.mark().is_empty());
        let Content::Folder { path, listing } = &a.content else { panic!("a folder: {:?}", a.content) };
        assert!(path.is_absolute() && path.ends_with("site photos"));
        assert_eq!((listing.files, listing.folders, listing.bytes, listing.cut), (2, 1, 3072, false));
        let names: Vec<&str> = listing.entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["A.jpg", "b.jpg", "raw/"], "in order, whatever the case, hidden ones left out");
        let n = note(std::slice::from_ref(&a), true, true, usize::MAX);
        assert!(n.contains("Attached folder `site photos/` (at "), "{n}");
        assert!(n.contains("2 files, 3 KB in all, and 1 folder."), "{n}");
        assert!(n.contains("Read what you need from it with your own tools"), "{n}");
        assert!(n.contains("In it: A.jpg (1 KB), b.jpg (2 KB), raw/."), "{n}");
        assert!(parts(&[a], true, true).is_empty());
        let empty = d.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(note(&[folder(&empty).unwrap()], true, true, usize::MAX).contains("it is empty"));
        let many = d.join("many");
        std::fs::create_dir_all(&many).unwrap();
        for i in 0..MAX_FOLDER_NAMED + 5 {
            std::fs::write(many.join(format!("p{i:04}.png")), b"x").unwrap();
        }
        let m = folder(&many).unwrap();
        let Content::Folder { listing, .. } = &m.content else { panic!("a folder") };
        assert_eq!((listing.files, listing.entries.len()), (MAX_FOLDER_NAMED + 5, MAX_FOLDER_NAMED), "all counted, the first named");
        assert!(note(&[m], true, true, usize::MAX).ends_with("p0199.png (1 bytes), and 5 more."));
        assert!(folder(&photos.join("A.jpg")).is_err(), "a file is not a folder");
        assert!(folder(&d.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The lines naming files that go by their place take their room before the text files share
    /// the budget, so the whole message stays within it.
    #[test]
    fn named_files_take_their_room_from_the_text_budget() {
        let place = Attachment {
            id: 0,
            name: "big.zip".into(),
            content: Content::Reference { path: "/a/big.zip".into(), size: 10, mime: "application/zip", pixels: None },
            thumb: None,
        };
        let text = Attachment { id: 0, name: "a.md".into(), content: Content::Text(Arc::new("x".repeat(1000))), thumb: None };
        let files = vec![text, place];
        let named = named_line(&files[1], true).unwrap().chars().count();
        let n = note(&files, true, true, 500 + named);
        assert_eq!(n.matches('x').count(), 500, "the text gets what the named file leaves: {n}");
        assert!(n.contains("`big.zip`"));
    }

    /// A PDF goes whole as a document to an agent that takes them (and is named with its place);
    /// to one that doesn't, and when it is too big to send, the agent is pointed to the file.
    #[test]
    fn pdfs_go_as_documents_or_by_reference() {
        let d = dir("pdf");
        let pdf = d.join("brief 1.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\n%%EOF\n").unwrap();
        let a = read(&pdf).unwrap();
        assert!(matches!(&a.content, Content::Pdf { bytes, .. } if bytes.starts_with(b"%PDF")));
        assert_eq!(a.mark(), "PDF");
        let files = vec![a];
        let sent = parts(&files, false, true);
        let [PromptPart::Document { name, uri, mime, base64 }] = sent.as_slice() else { panic!("one document: {sent:?}") };
        assert_eq!((name.as_str(), mime.as_str()), ("brief 1.pdf", "application/pdf"));
        assert!(uri.starts_with("file://") && uri.ends_with("/brief%201.pdf"), "{uri}");
        assert!(base64.starts_with("JVBERi0"), "the PDF's bytes: {base64}");
        assert!(note(&files, false, true, usize::MAX).contains("sent with this message as a document"));
        assert!(parts(&files, true, false).is_empty(), "an agent with no documents gets none");
        let n = note(&files, true, false, usize::MAX);
        assert!(n.contains("(PDF, 15 bytes, at ") && n.contains("read it with your own tools"), "{n}");
        let big = d.join("drawing set.pdf");
        std::fs::write(&big, vec![b'x'; usize::try_from(MAX_PDF_BYTES).unwrap() + 1]).unwrap();
        let b = read(&big).unwrap();
        assert!(matches!(b.content, Content::Reference { mime: "application/pdf", .. }), "too big to send: by reference");
        assert!(parts(&[b], true, true).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn links_to_files_are_escaped() {
        assert_eq!(file_uri(Path::new("/home/a b/ü.pdf")), "file:///home/a%20b/%C3%BC.pdf");
        assert_eq!(file_uri(Path::new("C:\\Users\\a\\x.pdf")), "file:///C:/Users/a/x.pdf");
        assert_eq!(size_text(1536), "2 KB");
        assert_eq!(size_text(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
    }

    /// A PDF with one line of text on each page.
    fn sample_pdf(pages: &[&str]) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"});
        let resources = doc.add_object(dictionary! {"Font" => dictionary! {"F1" => font}});
        let mut kids = Vec::new();
        for line in pages {
            let mut ops = vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
            ];
            if !line.is_empty() {
                ops.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
            }
            ops.push(Operation::new("ET", vec![]));
            let content = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
            kids.push(Object::Reference(doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages_id, "Contents" => content})));
        }
        let tree = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(tree));
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    /// An agent that takes no documents gets the PDF's text, with its page count and a note that
    /// the layout was dropped; one that takes documents gets the PDF and not the text.
    #[test]
    fn a_pdf_gives_its_text_to_agents_without_documents() {
        let d = dir("pdftext");
        let path = d.join("brief.pdf");
        std::fs::write(&path, sample_pdf(&["Wall 2 mm", "Hole 5 mm"])).unwrap();
        let a = read(&path).unwrap();
        let Content::Pdf { text: Some(t), .. } = &a.content else { panic!("a PDF with its text: {:?}", a.content) };
        assert_eq!(t.pages, 2);
        assert!(!t.cut && t.text.contains("Wall 2 mm") && t.text.contains("Hole 5 mm"), "{:?}", t.text);
        assert!(t.text.find("Wall").unwrap() < t.text.find("Hole").unwrap(), "pages in order");
        let files = vec![a];
        let n = note(&files, false, false, usize::MAX);
        assert!(n.contains("Attached PDF `brief.pdf` (2 pages"), "{n}");
        assert!(n.contains("layout, pictures and fonts are dropped") && n.contains("Wall 2 mm"), "{n}");
        let short = note(&files, false, false, 5);
        assert!(short.contains("more characters left out"), "the text shares the message's budget: {short}");
        let with = note(&files, false, true, usize::MAX);
        assert!(with.contains("as a document") && !with.contains("Wall 2 mm"), "{with}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A PDF that is all pictures (a scan), or can't be read, says so and points to the file; the
    /// text kept is capped.
    #[test]
    fn pdfs_without_text_or_too_long_are_said_so() {
        let blank = pdf_text(&sample_pdf(&[""])).unwrap();
        assert_eq!((blank.pages, blank.text.as_str()), (1, ""));
        let a = Attachment {
            id: 0,
            name: "scan.pdf".into(),
            content: Content::Pdf { bytes: Arc::new(vec![1; 10]), path: "/a/scan.pdf".into(), text: Some(Arc::new(blank)) },
            thumb: None,
        };
        assert!(note(&[a], false, false, usize::MAX).contains("no text could be taken from it"));
        let broken = Attachment {
            id: 0,
            name: "x.pdf".into(),
            content: Content::Pdf { bytes: Arc::new(vec![1; 10]), path: "/a/x.pdf".into(), text: None },
            thumb: None,
        };
        assert!(note(&[broken], false, false, usize::MAX).contains("its text could not be read"));
        let long = "a".repeat(MAX_PDF_CHARS + 5000);
        let t = pdf_text(&sample_pdf(&[&long])).unwrap();
        assert!(t.cut && t.text.chars().count() == MAX_PDF_CHARS, "{} characters", t.text.chars().count());
    }

    /// Hostile PDFs never panic: truncated and damaged copies of a real one are read or refused.
    #[test]
    fn damaged_pdfs_never_panic() {
        let good = sample_pdf(&["Wall 2 mm", "Hole 5 mm"]);
        assert!(pdf_text(&good).is_some());
        assert!(pdf_text(b"").is_none() && pdf_text(b"%PDF-1.7\n").is_none() && pdf_text(b"not a pdf").is_none());
        for cut in (0..good.len()).step_by(37) {
            let _ = pdf_text(&good[..cut]);
        }
        let mut seed = 0x9e37_79b9_u32;
        for _ in 0..200 {
            let mut bad = good.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let at = usize::try_from(seed >> 8).unwrap() % bad.len();
                bad[at] = (seed & 0xff) as u8;
            }
            let _ = pdf_text(&bad);
        }
    }
}
