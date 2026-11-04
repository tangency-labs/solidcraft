//! Standalone vector labels for CJK interface languages in the language picker.
//!
//! Drawing these as rasterised vector labels instead of egui text ensures:
//! 1. They never render as tofu boxes before the on-demand font has arrived.
//! 2. They do not trigger `painted_cjk` text monitoring when the language menu is opened.

use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use egui::Rect;
use egui::{Color32, TextureHandle, TextureOptions, Ui, vec2};
use tiny_skia as sk;

const PATH_JA: &str = "M13.8 1.3V15.3H12.5V14H3.5V15.3H2.1V1.3ZM3.5 2.4V7H12.5V2.4ZM3.5 8V12.8H12.5V8ZM25 4.6Q26.2 6.6 27.9 8.4 29.6 10.1 31.6 11.4L30.8 12.5Q28.4 10.8 26.5 8.6 25.4 7.2 24.6 5.7V11.2H27.7V12.4H24.6V15.6H23.3V12.4H20.3V11.2H23.3V5.8Q22.4 7.9 20.6 9.9 19.1 11.5 17.3 12.8L16.4 11.8Q20.4 9.3 22.9 4.6H16.8V3.4H23.3V.4H24.6V3.4H31.2V4.6ZM38.1 10.4V14.8H34.2V15.6H33V10.4ZM34.2 11.4V13.7H36.9V11.4ZM42.8 2.1L42.7 2.4Q42.6 3.4 42.5 4H45.9V7.3H47.6V8.3H38.3V7.3H40.7Q40.9 6.3 41.1 5.3L41.2 5H39.3V4H41.3L41.4 3.9Q41.5 2.8 41.6 2.1H38.7V1.1H47V2.1ZM44.8 5H42.3Q42.1 6.4 41.9 7.3H44.8ZM46.4 9.9V15.6H45.2V14.7H40.7V15.6H39.5V9.9ZM40.7 10.9V13.7H45.2V10.9ZM33.3 1.1H37.8V2.1H33.3ZM32.4 3.4H38.5V4.4H32.4ZM33.3 5.7H37.8V6.7H33.3ZM33.3 8H37.8V9H33.3Z";
const PATH_KO: &str = "M5.1 4.5C3 4.5 1.6 5.6 1.6 7.2 1.6 8.8 3 9.9 5.1 9.9 7.2 9.9 8.6 8.8 8.6 7.2 8.6 5.6 7.2 4.5 5.1 4.5ZM5.1 5.5C6.4 5.5 7.3 6.2 7.3 7.2 7.3 8.2 6.4 8.8 5.1 8.8 3.8 8.8 2.9 8.2 2.9 7.2 2.9 6.2 3.8 5.5 5.1 5.5ZM10.7.9V11.7H12V6.7H14.2V5.6H12V.9ZM4.4.9V2.6H.8V3.7H9.4V2.6H5.8V.9ZM3 10.8V15H12.7V13.9H4.3V10.8ZM16.9 10.4V11.5H25.7V15.3H27V10.4H22.7V7.8H28.6V6.7H26.6C26.9 5 26.9 3.7 26.9 2.6V1.5H17.2V2.6H25.6C25.6 3.7 25.6 5 25.2 6.7H15.5V7.8H21.4V10.4ZM34.1 3.2C35.5 3.2 36.4 4.7 36.4 7 36.4 9.4 35.5 10.9 34.1 10.9 32.7 10.9 31.8 9.4 31.8 7 31.8 4.7 32.7 3.2 34.1 3.2ZM40.8.8V6.4H37.7C37.5 3.7 36.1 2 34.1 2 32 2 30.5 3.9 30.5 7 30.5 10.1 32 12.1 34.1 12.1 36.1 12.1 37.6 10.3 37.7 7.4H40.8V15.3H42.1V.8Z";
const PATH_ZH: &str = "M7.3.6V3.5H1.5V11.1H2.7V10.1H7.3V15.3H8.6V10.1H13.2V11H14.4V3.5H8.6V.6ZM2.7 8.9V4.7H7.3V8.9ZM13.2 8.9H8.6V4.7H13.2ZM22.8.9C23.2 1.7 23.8 2.8 24 3.4L25.3 3C25.1 2.3 24.5 1.3 24 .5ZM16.8 3.5V4.6H19.3C20.2 7.1 21.5 9.2 23.2 10.9 21.4 12.4 19.2 13.4 16.6 14.2 16.8 14.5 17.2 15 17.3 15.3 20 14.5 22.2 13.3 24 11.7 25.8 13.3 28 14.5 30.6 15.2 30.8 14.9 31.2 14.4 31.5 14.1 28.9 13.5 26.7 12.4 25 10.9 26.6 9.2 27.8 7.2 28.7 4.6H31.3V3.5ZM24.1 10C22.6 8.5 21.4 6.7 20.5 4.6H27.4C26.6 6.8 25.5 8.6 24.1 10Z";
const PATH_ZH_TW: &str = "M10.1 13.1C11.5 13.6 13.2 14.4 14 15L15 14.3C14 13.7 12.3 12.9 11 12.5ZM4.7 12.6C3.8 13.2 2.2 13.9.9 14.3 1.1 14.5 1.6 14.9 1.7 15.1 3.1 14.6 4.7 13.8 5.8 13ZM3.1 10.4C3.3 10.3 3.7 10.3 6.5 10.2 5.3 10.6 4.3 10.9 3.9 11.1 3 11.4 2.3 11.6 1.8 11.6 1.9 11.9 2 12.4 2 12.6 2.4 12.4 3 12.4 7.5 12.1V14.2C7.5 14.4 7.5 14.4 7.2 14.4 7 14.4 6.2 14.4 5.3 14.4 5.4 14.7 5.6 15.1 5.7 15.4 6.8 15.4 7.6 15.4 8.1 15.2 8.6 15.1 8.7 14.8 8.7 14.2V12L12.6 11.7C13.1 12.1 13.6 12.5 13.8 12.8L14.7 12.2C13.9 11.5 12.5 10.4 11.4 9.7L10.6 10.2 11.7 11 5.6 11.3C7.5 10.7 9.5 10 11.3 9.1L10.7 8.3C10 8.7 9.3 9 8.6 9.3L5.8 9.4C6.7 9.1 7.6 8.7 8.5 8.3L7.8 7.6C7.4 7.8 7 8 6.6 8.2 6.8 8.2 6.8 8.1 6.9 8 7.1 7.9 7.1 7.7 7.2 7.4H8.2V7.2C8.4 7.4 8.7 7.8 8.8 8 9.9 7.6 10.8 7.1 11.6 6.4 12.4 7.2 13.5 7.9 14.7 8.3 14.9 8 15.2 7.6 15.4 7.4 14.2 7 13.2 6.5 12.3 5.7 13.1 5 13.7 4 14 2.8H15.2V1.9H10.7C10.9 1.6 11 1.2 11.1.8L10.1.6C9.7 2 9 3.2 8.1 4 8.3 4.2 8.7 4.6 8.9 4.8 9.2 4.5 9.4 4.2 9.7 3.8 10 4.5 10.4 5.2 10.9 5.7 10.1 6.4 9.2 6.8 8.2 7.2V6.7H7.4C7.4 6.4 7.5 5.9 7.6 5.4H8.6V4.7H7.6L7.7 3.3C7.7 3.1 7.7 2.8 7.7 2.8H2.4C2.5 2.6 2.7 2.4 2.9 2.2H8.3V1.4H3.4L3.7.8 2.7.6C2.2 1.6 1.4 2.5.5 3.2.8 3.3 1.2 3.6 1.4 3.8 1.7 3.6 2 3.3 2.2 3L2 4.7H.7V5.4H2C1.9 6.2 1.8 6.9 1.7 7.4H6.3L6.2 7.5C6.1 7.7 6 7.7 5.8 7.7 5.6 7.7 5.2 7.7 4.8 7.6 4.9 7.8 4.9 8.1 5 8.3 5.4 8.3 5.9 8.4 6.1 8.3 6.3 8.3 6.4 8.3 6.5 8.3 5.5 8.7 4.4 9.1 4 9.2 3.6 9.3 3.1 9.4 2.8 9.4 2.9 9.7 3 10.2 3.1 10.4ZM10.3 2.8H13C12.7 3.7 12.2 4.4 11.6 5.1 11.1 4.4 10.6 3.7 10.3 2.9ZM4 3.9C4.5 4.1 5.2 4.5 5.5 4.8H2.9L3.1 3.5H4.3ZM3.8 5.7C4.3 6 4.9 6.4 5.2 6.7H2.7L2.9 5.4H4.1ZM4.6 3.5H6.8L6.7 4.8H5.6L5.9 4.3C5.6 4.1 5.1 3.8 4.6 3.5ZM5.4 6.7L5.8 6.3C5.4 6 4.8 5.7 4.4 5.4H6.6C6.6 5.9 6.5 6.4 6.5 6.7ZM23.2 7.6V8.5H31.2V7.6ZM25 10.1H29.3V11.4H25ZM24 9.3V12.2H30.4V9.3ZM18.8 9.6C19.3 10 20.1 10.6 20.4 11L21.1 10.4C20.7 10 19.9 9.5 19.3 9ZM24.8 12.6C25 13.1 25.2 13.6 25.3 14.1H23V15H31.4V14.1H28.9L29.7 12.6 28.6 12.3C28.5 12.8 28.2 13.6 27.9 14.1H26.4C26.2 13.6 26 12.9 25.7 12.3ZM23.5 1.9V6.7H30.8V1.9H28.7V.6H27.7V1.9H26.5V.6H25.5V1.9ZM24.5 4.7H25.7V5.9H24.5ZM26.5 4.7H27.7V5.9H26.5ZM28.5 4.7H29.8V5.9H28.5ZM24.5 2.7H25.7V3.9H24.5ZM26.5 2.7H27.7V3.9H26.5ZM28.5 2.7H29.8V3.9H28.5ZM18.6 12.5L19 13.3 21.2 12V14.1C21.2 14.3 21.1 14.3 21 14.3 20.8 14.4 20.2 14.4 19.6 14.3 19.7 14.6 19.9 15 19.9 15.3 20.8 15.3 21.4 15.2 21.7 15.1 22.1 14.9 22.2 14.6 22.2 14.1V8H22.9V5.6H22.3V1.2H17.5V5.6H16.8V8H17.6V10.6C17.6 11.9 17.5 13.5 16.7 14.8 16.9 14.9 17.4 15.2 17.5 15.4 18.5 14 18.6 12.1 18.6 10.6V8.4H21.2V11.2C20.2 11.7 19.2 12.2 18.6 12.5ZM21.8 7.6H17.8V6.5H22V7.6ZM19.5 3.2V5.6H18.5V2.1H21.2V3.2ZM21.2 5.6H20.2V3.9H21.2ZM39.3.6V3.5H33.5V11.1H34.7V10.1H39.3V15.3H40.6V10.1H45.2V11H46.4V3.5H40.6V.6ZM34.7 8.9V4.7H39.3V8.9ZM45.2 8.9H40.6V4.7H45.2ZM54.8.9C55.2 1.7 55.8 2.8 56 3.4L57.3 3C57.1 2.3 56.5 1.3 56 .5ZM48.8 3.5V4.6H51.3C52.2 7.1 53.5 9.2 55.2 10.9 53.4 12.4 51.2 13.4 48.6 14.2 48.8 14.5 49.2 15 49.3 15.3 52 14.5 54.2 13.3 56 11.7 57.8 13.3 60 14.5 62.6 15.2 62.8 14.9 63.2 14.4 63.5 14.1 60.9 13.5 58.7 12.4 57 10.9 58.6 9.2 59.8 7.2 60.7 4.6H63.3V3.5ZM56.1 10C54.6 8.5 53.4 6.7 52.5 4.6H59.4C58.6 6.8 57.5 8.6 56.1 10Z";

/// Metadata for a vector language label: path and viewBox width (height is 16.0).
fn label_spec(id: &str) -> Option<(&'static str, f32)> {
    match id {
        "ja" => Some((PATH_JA, 48.0)),
        "ko" => Some((PATH_KO, 44.2)),
        "zh" => Some((PATH_ZH, 32.0)),
        "zh-TW" => Some((PATH_ZH_TW, 64.0)),
        _ => None,
    }
}

/// Custom parser for the minimal SVG path subset emitted by make_language_labels.py (M, L, H, V, C, Q, Z).
pub(crate) fn parse_svg_path(d: &str) -> Result<sk::Path, String> {
    let mut pb = sk::PathBuilder::new();
    let b = d.as_bytes();
    let mut i = 0;
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut open = false;

    let next_num = |i: &mut usize| -> Result<f32, String> {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
        if *i >= b.len() {
            return Err("unexpected end of path".to_string());
        }
        let start = *i;
        if b[*i] == b'-' || b[*i] == b'+' {
            *i += 1;
        }
        let mut has_dot = false;
        while *i < b.len() {
            if b[*i].is_ascii_digit() {
                *i += 1;
            } else if b[*i] == b'.' && !has_dot {
                has_dot = true;
                *i += 1;
            } else {
                break;
            }
        }
        if start == *i || (start + 1 == *i && (b[start] == b'-' || b[start] == b'+')) {
            return Err("expected number".to_string());
        }
        let s = d.get(start..*i).ok_or("number slice")?;
        s.parse::<f32>().map_err(|e| format!("{e}: '{s}'"))
    };

    let mut current_cmd = ' ';
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let ch = *b.get(i).ok_or("path command")? as char;
        if ch.is_ascii_alphabetic() {
            current_cmd = ch;
            i += 1;
        } else if current_cmd == 'M' {
            current_cmd = 'L';
        }

        match current_cmd {
            'M' => {
                x = next_num(&mut i)?;
                y = next_num(&mut i)?;
                pb.move_to(x, y);
                open = true;
                current_cmd = 'L';
            }
            'L' => {
                x = next_num(&mut i)?;
                y = next_num(&mut i)?;
                pb.line_to(x, y);
            }
            'H' => {
                x = next_num(&mut i)?;
                pb.line_to(x, y);
            }
            'V' => {
                y = next_num(&mut i)?;
                pb.line_to(x, y);
            }
            'C' => {
                let x1 = next_num(&mut i)?;
                let y1 = next_num(&mut i)?;
                let x2 = next_num(&mut i)?;
                let y2 = next_num(&mut i)?;
                x = next_num(&mut i)?;
                y = next_num(&mut i)?;
                pb.cubic_to(x1, y1, x2, y2, x, y);
            }
            'Q' => {
                let x1 = next_num(&mut i)?;
                let y1 = next_num(&mut i)?;
                x = next_num(&mut i)?;
                y = next_num(&mut i)?;
                pb.quad_to(x1, y1, x, y);
            }
            'Z' | 'z' => {
                if open {
                    pb.close();
                }
                current_cmd = ' ';
            }
            _ => return Err(format!("unexpected command {current_cmd}")),
        }
    }
    pb.finish().ok_or_else(|| "failed to build path".to_string())
}

type TextureCache = HashMap<(&'static str, bool), TextureHandle>;

fn texture_cache_id() -> egui::Id {
    egui::Id::new("solidcraft_locale_label_textures")
}

/// Retrieve or create the rasterised texture for a vector label.
pub fn get_label_texture(ctx: &egui::Context, id: &str, dark: bool) -> Option<(TextureHandle, egui::Vec2)> {
    let (d, vb_w) = label_spec(id)?;
    let key = match id {
        "ja" => "ja",
        "ko" => "ko",
        "zh" => "zh",
        "zh-TW" => "zh-TW",
        _ => return None,
    };
    let mut cache = ctx.data_mut(|d| d.get_temp::<TextureCache>(texture_cache_id()).unwrap_or_default());
    if let Some(handle) = cache.get(&(key, dark)) {
        return Some((handle.clone(), vec2(vb_w, 16.0)));
    }

    let color = if dark { Color32::from_rgb(220, 224, 230) } else { Color32::from_rgb(30, 34, 40) };
    let scale = 2.0;
    let path = parse_svg_path(d).ok()?;
    let w = (vb_w * scale).round() as u32;
    let h = (16.0 * scale).round() as u32;
    let mut pm = sk::Pixmap::new(w, h)?;
    let mut paint = sk::Paint::default();
    paint.set_color_rgba8(color.r(), color.g(), color.b(), color.a());
    paint.anti_alias = true;
    let tf = sk::Transform::from_scale(scale, scale);
    pm.fill_path(&path, &paint, sk::FillRule::Winding, tf, None);

    let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data());
    let handle = ctx.load_texture(format!("locale_label_{id}_{dark}"), image, TextureOptions::LINEAR);
    cache.insert((key, dark), handle.clone());
    ctx.data_mut(|d| d.insert_temp(texture_cache_id(), cache));
    Some((handle, vec2(vb_w, 16.0)))
}

/// List picker item that uses a vector label for CJK languages on Web builds (wasm32) to
/// prevent unwanted font downloads, and standard selectable buttons on native/desktop builds.
pub(crate) fn list_picker_locale(ui: &mut Ui, id: &str, fallback_name: &str, selected: bool) -> egui::Response {
    #[cfg(target_arch = "wasm32")]
    {
        let dark = ui.visuals().dark_mode;
        if let Some((texture, size)) = get_label_texture(ui.ctx(), id, dark) {
            let response = crate::ui_kit::list_picker(ui, id, "", selected);
            if ui.is_rect_visible(response.rect) {
                let label_h = 13.0;
                let label_w = size.x * (label_h / size.y);
                let pos_x = response.rect.center().x - (label_w / 2.0);
                let pos_y = response.rect.center().y - (label_h / 2.0);
                let target_rect = Rect::from_min_size(egui::pos2(pos_x, pos_y), vec2(label_w, label_h));
                ui.painter().image(texture.id(), target_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            return response;
        }
    }
    crate::ui_kit::list_picker(ui, id, fallback_name, selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_cjk_label_specs_parse_successfully() {
        for id in ["ja", "ko", "zh", "zh-TW"] {
            let (d, w) = label_spec(id).expect("spec exists");
            assert!(w > 0.0);
            let path = parse_svg_path(d);
            assert!(path.is_ok(), "{id} path parses: {:?}", path.err());
        }
    }

    #[test]
    fn label_texture_creation_succeeds() {
        let ctx = egui::Context::default();
        for id in ["ja", "ko", "zh", "zh-TW"] {
            let (tex, size) = get_label_texture(&ctx, id, true).expect("texture renders");
            assert!(size.x > 0.0);
            assert_eq!(size.y, 16.0);
            assert_eq!(tex.size(), [(size.x * 2.0).round() as usize, 32]);
        }
    }
}
