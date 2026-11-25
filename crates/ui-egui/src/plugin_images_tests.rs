//! Package images: which paths are read, the icon check they pass, and where they are drawn.
use super::*;
use serde_json::json;

/// An icon in the app's subset: a filled square in the active accent, outlined in ink.
const SEAT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><title>Seat</title><path d="M12 12L52 12L52 52L12 52Z" fill="#236ead" stroke="#34465b" stroke-width="3"/></svg>"##;

fn package(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    solidcraft_look::install_tables();
    let dir = std::env::temp_dir().join(format!("solidcraft-plugin-images-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, text) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn entry(schema: serde_json::Value) -> solidcraft_engine::plugin::Entry {
    serde_json::from_value(json!({"name":"seat","effect":"edit","input_schema":schema})).unwrap()
}

#[test]
fn only_plain_relative_svg_paths_inside_the_package_are_read() {
    for ok in ["seat.svg", "icons/seat.svg", "icons/m3_short-v2.svg"] {
        assert!(valid_path(ok), "{ok}");
    }
    for bad in [
        "",
        "seat.png",
        "seat.SVG",
        "/etc/seat.svg",
        "../seat.svg",
        "icons/../seat.svg",
        "./seat.svg",
        "icons//seat.svg",
        "C:\\seat.svg",
        "https://example.com/seat.svg",
        "icons\\seat.svg",
        "seat .svg",
        "%2e%2e/seat.svg",
    ] {
        assert!(!valid_path(bad), "{bad}");
    }
    assert!(!valid_path(&format!("{}.svg", "a".repeat(130))));
}

#[test]
fn package_images_pass_the_icon_check_or_are_left_out() {
    let red = SEAT.replace("#236ead", "#ff0000");
    let linked = SEAT.replace("<title>Seat</title>", "<image href='https://example.com/a.png'/>");
    let text = SEAT.replace("<title>Seat</title>", "<text>Hi</text>");
    let wrong_size = SEAT.replace(r#"viewBox="0 0 64 64""#, r#"viewBox="0 0 640 640""#);
    let big = SEAT.replace("<title>Seat</title>", &"<title>x</title>".repeat(5000));
    let dir = package(
        "check",
        &[
            ("icons/seat.svg", SEAT),
            ("icons/red.svg", &red),
            ("icons/linked.svg", &linked),
            ("icons/text.svg", &text),
            ("icons/wrong_size.svg", &wrong_size),
            ("icons/big.svg", &big),
        ],
    );
    let option = |path: &str| json!({"const": path, "x-solidcraft-icon": format!("icons/{path}.svg")});
    let schema = json!({"type":"object","x-solidcraft-icon":"icons/seat.svg","properties":{"series":{"type":"string","oneOf":[
        option("red"), option("linked"), option("text"), option("wrong_size"), option("big"), option("missing"),
        {"const":"outside","x-solidcraft-icon":"../outside.svg"}
    ]}}});
    let images = load(&dir, "inserts", &[entry(schema)]);
    assert_eq!(images.keys().collect::<Vec<_>>(), ["icons/seat.svg"], "only the image in the subset");
    let name = &images["icons/seat.svg"];
    assert!(name.starts_with("plugin:inserts:icons/seat.svg:"), "{name}");
    let resolved = solidcraft_ui_kit::icons::resolve(name);
    assert!(!resolved.placeholder, "drawn as its own artwork");
    // A changed file is another icon: the first one drawn never stands for it.
    std::fs::write(dir.join("icons/seat.svg"), SEAT.replace("M12 12", "M14 14")).unwrap();
    let again = load(&dir, "inserts", &[entry(json!({"type":"object","x-solidcraft-icon":"icons/seat.svg"}))]);
    assert_ne!(&again["icons/seat.svg"], name);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_package_is_not_followed() {
    let outside = package("outside", &[("seat.svg", SEAT)]);
    let dir = package("links", &[]);
    std::os::unix::fs::symlink(outside.join("seat.svg"), dir.join("seat.svg")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("icons")).unwrap();
    let schema = json!({"type":"object","x-solidcraft-icon":"seat.svg","properties":{"p":{"type":"string","oneOf":[
        {"const":"a","x-solidcraft-icon":"icons/seat.svg"}]}}});
    assert!(load(&dir, "links", &[entry(schema)]).is_empty());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn swatches_keep_pixels_and_cached_textures_until_the_package_is_reloaded() {
    let dir = package("swatch-cache", &[]);
    let schema = json!({"type":"object","properties":{"finish":{"type":"string","oneOf":[
        {"const":"oak","x-solidcraft-swatch":"oak.png"}
    ]}}});
    let entries = [entry(schema)];
    let save = |colour| image::RgbaImage::from_pixel(128, 64, image::Rgba(colour)).save(dir.join("oak.png")).unwrap();
    save([137, 61, 23, 128]);
    let images = read_swatches(&dir, &entries);
    let swatch = &images["oak.png"];
    assert_eq!(swatch.image.size, [128, 64]);
    assert_eq!(swatch.image.pixels[0], egui::Color32::from_rgba_unmultiplied(137, 61, 23, 128));
    let ctx = crate::fonts::test_context();
    let texture = swatch.texture(&ctx);
    assert_eq!(swatch.texture(&ctx).id, texture.id);
    std::fs::write(dir.join("oak.png"), b"corrupted after loading").unwrap();
    assert_eq!(swatch.texture(&ctx).id, texture.id, "frames use the cache without touching the file");
    assert!(read_swatches(&dir, &entries).is_empty(), "a bad reload omits the image");
    save([32, 64, 96, 255]);
    let changed = read_swatches(&dir, &entries);
    assert_ne!(changed["oak.png"].texture(&ctx).id, texture.id, "changed artwork gets a fresh texture");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_command_icon_and_option_pictures_are_drawn_where_the_command_is() {
    use crate::plugin_menu::{Input, command_icon, form, open_entry};
    let dir = package("drawn", &[("seat.svg", SEAT), ("std.svg", SEAT), ("short.svg", &SEAT.replace("M12 12", "M20 20"))]);
    let schema = json!({"type":"object","x-solidcraft-icon":"seat.svg","properties":{"series":{"type":"string","default":"standard","oneOf":[
        {"const":"standard","title":"Standard","x-solidcraft-icon":"std.svg"},
        {"const":"short","title":"Short","x-solidcraft-icon":"short.svg"}]}}});
    let seat = entry(schema.clone());
    let mut plugin = crate::plugin_menu::Listed {
        id: "inserts".into(),
        name: "Inserts".into(),
        version: "1.0.0".into(),
        description: String::new(),
        path: dir.to_string_lossy().into_owned(),
        approval: String::new(),
        disabled: false,
        groups: Default::default(),
        entries: vec![seat.clone()],
        images: Default::default(),
        swatches: Default::default(),
        shaders: Vec::new(),
        tints: Vec::new(),
        panels: Vec::new(),
        shader_settings: Default::default(),
    };
    assert_eq!(command_icon(&plugin, &seat), GENERIC, "no image read: the generic plugin icon");
    plugin.images = load(&dir, "inserts", &plugin.entries);
    let icon = command_icon(&plugin, &seat).to_owned();
    assert!(icon.starts_with("plugin:inserts:seat.svg:"), "{icon}");
    // The option pictures, in option order; a value is still what is sent.
    let fields = form(&schema).unwrap();
    assert_eq!(fields[0].0.images, ["std.svg", "short.svg"]);
    assert_eq!(fields[0].1, Input::Choice(Some(0)));
    let some = json!({"type":"string","oneOf":[{"const":"a","x-solidcraft-icon":"a.svg"},{"const":"b"}]});
    assert!(form(&json!({"type":"object","properties":{"p":some}})).unwrap()[0].0.images.is_empty(), "pictures only when every option has one");
    let wrong = json!({"type":"string","oneOf":[{"const":"a","x-solidcraft-icon":7}]});
    assert_eq!(form(&json!({"type":"object","properties":{"p":wrong}})), None);
    // The ribbon button and the dialog's header carry it.
    let mut h = crate::scenario::Harness::new();
    h.app.plugin_menu.listed = vec![plugin];
    assert_eq!(crate::plugin_menu::ribbon_buttons(&h.app)[0].icon, icon);
    open_entry(&mut h.app, 0, 0);
    h.frames(2);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().icon, icon);
    let _ = std::fs::remove_dir_all(dir);
}
