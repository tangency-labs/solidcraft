//! Original light and dark palette values. Compiled by the UI theme boundary.
use crate::theme::{AnalysisColors, IconColors, Tokens};
use egui::Color32;

const fn icon_primary(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
impl Tokens {
    pub const fn light() -> Tokens {
        Tokens {
            nav_hover: Color32::from_rgb(233, 237, 242),
            nav_press: Color32::from_rgb(220, 226, 234),
            nav_dim: Color32::from_rgb(136, 149, 167),
            nav_viewport: Color32::from_rgb(238, 241, 245),
            nav_tip: Color32::from_rgb(27, 36, 48),
            nav_tip_ink: Color32::from_rgb(255, 255, 255),
            nav_tip_key_border: Color32::from_rgba_unmultiplied_const(128, 140, 160, 153),
            nav_field: Color32::from_rgb(255, 255, 255),

            borders: crate::theme::Borders::LEGACY,
            icon_palette: None,
            file_dim: Color32::from_rgb(58, 70, 86),
            file_recovery_well: Color32::from_rgb(247, 226, 184),
            file_action_height: 32.0,
            file_back_height: 40.0,
            file_recovery_icon: 32.0,
            file_grid_gap: 14.0,
            file_background: Color32::from_rgb(247, 249, 251),
            file_surface: Color32::WHITE,
            file_border: Color32::from_rgb(224, 229, 236),
            file_image_tint: Color32::WHITE,
            file_rail: Color32::from_rgb(35, 110, 173),
            file_rail_ink: Color32::WHITE,
            file_rail_hover: Color32::from_rgb(43, 118, 180),
            file_primary: Color32::from_rgb(227, 240, 251),
            file_primary_ink: Color32::from_rgb(26, 80, 127),
            file_primary_border: Color32::from_rgb(50, 139, 211),
            file_recovery: Color32::from_rgb(255, 246, 229),
            file_recovery_ink: Color32::from_rgb(122, 74, 5),
            file_recovery_border: Color32::from_rgb(240, 210, 154),
            file_nav_width: 248.0,
            file_nav_height: 38.0,
            file_tile_height: 84.0,
            file_tile_icon: 44.0,
            file_recent_height: 52.0,
            file_sample_image_height: 84.0,
            file_content_inset_x: 40.0,
            file_content_inset_y: 32.0,
            file_section_gap: 28.0,
            font_start_title: 28.0,
            space_2: 2.0,
            space_3: 3.0,
            space_4: 4.0,
            space_6: 6.0,
            space_8: 8.0,
            space_10: 10.0,
            space_12: 12.0,
            space_14: 14.0,
            space_16: 16.0,
            space_24: 24.0,
            radius_sm: 4,
            radius_md: 6,
            radius_lg: 8,
            radius_xl: 10,
            font_badge: 9.5,
            font_micro: 10.0,
            font_panel: 10.5,
            font_small: 11.0,
            font_caption: 11.5,
            font_body: 12.0,
            font_label: 12.5,
            font_button: 13.0,
            font_heading: 14.0,
            font_triad: 8.0,
            font_compass: 13.5,
            font_subtitle: 15.0,
            font_intro: 16.0,
            font_article_heading: 17.0,
            font_title: 22.0,
            font_display: 26.0,
            control_height: 28.0,
            form_ink: Color32::from_rgb(27, 36, 48),
            form_dim: Color32::from_rgb(74, 88, 104),
            form_primary: Color32::from_rgb(35, 110, 173),
            form_primary_fill: Color32::from_rgb(35, 110, 173),
            form_error: Color32::from_rgb(161, 52, 44),
            form_error_fill: Color32::from_rgb(252, 239, 237),
            form_success: Color32::from_rgb(39, 107, 44),
            form_success_fill: Color32::from_rgb(229, 242, 230),
            control_radius: 6,
            form_label_width: 70.0,
            form_gap_y: 5.0,
            form_footer_height: 48.0,
            form_header_height: 40.0,
            form_card_width: 264.0,
            form_icon_size: 19.0,
            form_close_size: 26.0,
            form_inset: 9.0,
            form_icon_inset: 4.5,
            form_section_icon_size: 11.0,
            form_checkbox_height: 22.0,
            form_button_height: 28.0,
            sketch_palette_width: 232.0,
            sketch_mode_size: 36.0,
            sketch_mode_radius: 7,
            sketch_mode_border: 1.5,
            sketch_palette_alpha: 0.96,
            sketch_mode_tint: 0.12,
            sketch_footer_icon_size: 15.0,
            font_section_tracking: 0.04,
            app_bar: Color32::from_rgb(52, 56, 64),
            app_bar_separator: Color32::from_rgb(136, 145, 160),
            toolbar: Color32::from_rgb(246, 247, 249),
            panel: Color32::from_rgb(250, 251, 252),
            panel_header: Color32::from_rgb(236, 238, 242),
            border: Color32::from_rgb(208, 212, 220),
            text: Color32::from_rgb(38, 42, 50),
            text_dim: Color32::from_rgb(110, 116, 128),
            accent: Color32::from_rgb(38, 120, 218),
            accent_soft: Color32::from_rgb(214, 230, 250),
            sketch_accent: icon_primary(crate::icon_svg::SKETCH_PRIMARY[0]),
            // Icon palette active-3 ramps; these accents have no compiled constants yet.
            surface_accent: Color32::from_rgb(164, 80, 0),
            mesh_accent: Color32::from_rgb(130, 103, 0),
            form_accent: Color32::from_rgb(115, 73, 205),
            render_accent: Color32::from_rgb(174, 45, 113),
            sketch_ink: Color32::from_rgb(5, 127, 69),
            sketch_soft: Color32::from_rgb(237, 247, 241),
            surface_fill: Color32::from_rgb(164, 80, 0),
            surface_ink: Color32::from_rgb(164, 80, 0),
            surface_soft: Color32::from_rgb(251, 235, 216),
            mesh_fill: Color32::from_rgb(224, 178, 0),
            mesh_ink: Color32::from_rgb(138, 109, 0),
            mesh_soft: Color32::from_rgb(255, 248, 220),
            form_fill: Color32::from_rgb(115, 73, 205),
            form_tab_ink: Color32::from_rgb(115, 73, 205),
            form_soft: Color32::from_rgb(238, 232, 250),
            sheet_fill: Color32::from_rgb(30, 111, 150),
            sheet_ink: Color32::from_rgb(30, 111, 150),
            sheet_soft: Color32::from_rgb(225, 240, 247),
            render_fill: Color32::from_rgb(168, 50, 110),
            render_ink: Color32::from_rgb(168, 50, 110),
            render_soft: Color32::from_rgb(248, 230, 239),
            tab_on_fill_dark: Color32::BLACK,
            on_accent: Color32::WHITE,
            agent_ok: Color32::from_rgb(76, 194, 122),
            agent_busy: Color32::from_rgb(108, 180, 240),
            error: Color32::from_rgb(214, 54, 54),
            warning: Color32::from_rgb(222, 150, 30),
            icon: Color32::from_rgb(70, 78, 92),
            timeline: Color32::from_rgb(232, 235, 240),
            hover: Color32::from_rgb(226, 234, 246),
            sel_face: Color32::from_rgb(76, 127, 203),
            sel_edge: Color32::from_rgb(0, 127, 255),
            sel_edge_rim: Color32::from_rgb(127, 191, 255),
            hover_edge: Color32::from_rgb(0, 0, 0),
            hover_edge_halo: Color32::from_rgb(203, 203, 203),
            hover_edge_lit: Color32::from_rgb(255, 128, 0),
            hover_face_lift: 40,
            sel_profile: Color32::from_rgb(26, 84, 167),
            sel_profile_edge: Color32::from_rgb(47, 109, 198),
            hover_profile: Color32::from_rgba_unmultiplied_const(75, 129, 253, 48),
            hover_profile_edge: Color32::from_rgb(75, 129, 253),
            sel_vertex: Color32::from_rgb(70, 110, 170),
            origin_plane: Color32::from_rgba_unmultiplied_const(249, 184, 134, 94),
            origin_plane_edge: Color32::from_rgba_unmultiplied_const(214, 140, 84, 200),
            origin_plane_hover: Color32::from_rgba_unmultiplied_const(120, 130, 230, 150),
            origin_point: Color32::from_rgb(214, 215, 218),
            // The one set of X, Y and Z colours (#976), equal to the engine's `view::colors::AXIS_*`
            // so a headless picture draws its origin axes as the window does.
            axis_x: Color32::from_rgb(220, 60, 60),
            axis_y: Color32::from_rgb(60, 170, 60),
            axis_z: Color32::from_rgb(60, 100, 230),
            construction_plane: Color32::from_rgba_unmultiplied_const(249, 184, 134, 70),
            construction_geom: icon_primary(crate::icon_svg::CONSTRUCTION_PRIMARY[0]),
            body: Color32::from_rgb(176, 186, 198),
            body_edge: Color32::from_rgb(40, 44, 52),
            manipulator: Color32::from_rgb(40, 110, 220),
            joint_manipulator: Color32::from_rgb(235, 157, 0),
            joint_axis: Color32::from_rgb(0, 95, 110),
            preview_add: Color32::from_rgb(112, 160, 226),
            preview_new: Color32::from_rgb(240, 225, 105),
            preview_sketch: Color32::from_rgb(32, 190, 96),
            preview_cut: Color32::from_rgba_unmultiplied_const(232, 84, 44, 110),
            cube_face: Color32::from_rgb(236, 239, 244),
            cube_border: Color32::from_rgb(85, 94, 110),
            cube_text: Color32::from_rgb(38, 42, 50),
            cube_hover: Color32::from_rgb(190, 220, 252),
            viewport_top: Color32::from_rgb(240, 243, 247),
            viewport_bottom: Color32::from_rgb(203, 210, 221),
            grid: Color32::from_rgb(196, 202, 212),
            grid_major: Color32::from_rgb(170, 178, 190),
            body_selected: Color32::from_rgb(110, 170, 235),
            sketch: Color32::from_rgb(30, 90, 200),
            sketch_fixed: Color32::from_rgb(20, 20, 20),
            sketch_halo: Color32::from_rgb(240, 243, 247),
            sketch_point: Color32::BLACK,
            sketch_locked: Color32::from_rgb(0, 140, 60),
            // The error red (`error`).
            sketch_conflict: Color32::from_rgb(214, 54, 54),
            // Sketch dimensions in a plum of their own, apart from the blue and black curves for
            // every colour vision mode (#7861).
            dimension: Color32::from_rgb(130, 42, 110),
            dimension_driven: Color32::from_rgb(160, 90, 120),
            sketch_point_free: Color32::from_rgb(30, 90, 200),
            section_cap: Color32::from_rgb(214, 128, 72),
            dialog_bg: Color32::from_rgb(250, 251, 252),
            dialog_border: Color32::from_rgb(213, 219, 227),
            chip: Color32::from_rgb(38, 120, 218),
            profile_fill: Color32::from_rgba_unmultiplied_const(255, 196, 120, 70),
            glyph_bg: Color32::WHITE,
            glyph_edge: Color32::from_rgb(190, 110, 100),
            glyph_text: icon_primary(crate::icon_svg::CONSTRAINT_PRIMARY[0]),
            overlay: Color32::from_rgba_unmultiplied_const(255, 255, 255, 235),
            rubber_band: Color32::from_rgb(30, 120, 230),
            app_bar_text: Color32::from_rgb(230, 234, 242),
            app_bar_hover: Color32::from_rgba_unmultiplied_const(255, 255, 255, 28),
            app_bar_hover_subtle: Color32::from_rgba_unmultiplied_const(255, 255, 255, 18),
            app_bar_hover_button: Color32::from_rgba_unmultiplied_const(255, 255, 255, 30),
            app_bar_hover_strong: Color32::from_rgba_unmultiplied_const(255, 255, 255, 40),
            app_bar_tab_active: Color32::from_rgb(53, 59, 71),
            dialog_body: Color32::from_rgb(255, 255, 255),
            dialog_footer: Color32::from_rgb(244, 246, 249),
            dialog_rule: Color32::from_rgb(230, 234, 240),
            control_fill: Color32::from_rgb(255, 255, 255),
            control_border: Color32::from_rgb(195, 203, 214),
            dialog_label: Color32::from_rgb(58, 70, 86),
            selected_ink: Color32::from_rgb(26, 80, 127),
            selected_bg: Color32::from_rgb(232, 241, 250),
            figure_ground: Color32::from_rgb(247, 248, 250),
            figure_line: Color32::from_rgb(111, 124, 143),
            figure_face_top: Color32::from_rgb(255, 255, 255),
            figure_face_left: Color32::from_rgb(228, 233, 240),
            figure_face_right: Color32::from_rgb(201, 210, 222),
            figure_card: Color32::from_rgb(255, 255, 255),
            figure_white: Color32::WHITE,
            figure_black: Color32::BLACK,
            figure_accent: Color32::from_rgb(35, 110, 173),
            figure_reference: Color32::from_rgb(138, 83, 0),
            figure_axis_x: Color32::from_rgb(214, 69, 69),
            figure_axis_y: Color32::from_rgb(46, 158, 91),
            figure_axis_z: Color32::from_rgb(47, 111, 208),
            figure_ok: Color32::from_rgb(47, 125, 91),
            figure_warn: Color32::from_rgb(201, 138, 0),
            pref_well: Color32::from_rgb(238, 241, 245),
            pref_note: Color32::from_rgb(184, 74, 0),
            pref_note_fill: Color32::from_rgb(251, 235, 221),
            pref_on_accent: Color32::from_rgb(255, 255, 255),
            pref_ok: Color32::from_rgb(39, 107, 44),
            pref_ok_fill: Color32::from_rgb(229, 242, 230),
            pref_disabled: Color32::from_rgb(138, 148, 163),
            timeline_item: Color32::WHITE,
            timeline_item_error: Color32::from_rgb(250, 220, 220),
            timeline_marker: Color32::from_rgb(70, 76, 88),
            field: Color32::WHITE,
            box_window: Color32::from_rgb(38, 120, 218),
            box_crossing: Color32::from_rgb(40, 160, 80),
            // Equal to `view::colors` (the engine's tags), so mapping a tag to its token
            // changes nothing.
            sketch_projected: Color32::from_rgb(150, 60, 190),
            sketch_lost: Color32::from_rgb(200, 60, 90),
            sketch_construction: Color32::from_rgb(230, 130, 40),
            sketch_centerline: Color32::from_rgb(200, 110, 60),
            bend_up: Color32::from_rgb(30, 140, 50),
            bend_down: Color32::from_rgb(200, 90, 20),
            interference: Color32::from_rgb(235, 45, 40),
            continuity: [
                Color32::from_rgb(0, 158, 115),
                Color32::from_rgb(0, 114, 178),
                Color32::from_rgb(240, 200, 0),
                Color32::from_rgb(204, 0, 140),
            ],
            analysis: AnalysisColors::DEFAULT,
            icons: IconColors { highlight: Color32::WHITE, agent: Color32::from_rgb(168, 85, 247), marker_halo: Color32::from_black_alpha(150) },
            logo: [Color32::from_rgb(96, 158, 236), Color32::from_rgb(58, 136, 238), Color32::from_rgb(55, 108, 197)],
        }
    }
    pub const fn dark() -> Tokens {
        let mut t = Tokens::light();
        // Shared typography and dense form dimensions stay identical in both palettes.
        t.font_triad = 8.0;
        t.font_compass = 13.5;
        t.font_subtitle = 15.0;
        t.font_intro = 16.0;
        t.font_article_heading = 17.0;
        t.font_title = 22.0;
        t.font_display = 26.0;
        t.control_height = 28.0;
        t.control_radius = 6;
        t.form_label_width = 70.0;
        t.form_gap_y = 5.0;
        t.form_footer_height = 48.0;
        t.form_header_height = 40.0;
        t.form_card_width = 264.0;
        t.form_icon_size = 19.0;
        t.form_close_size = 26.0;
        t.form_inset = 9.0;
        t.form_icon_inset = 4.5;
        t.form_section_icon_size = 11.0;
        t.form_checkbox_height = 22.0;
        t.form_button_height = 28.0;
        t.sketch_palette_width = 232.0;
        t.sketch_mode_size = 36.0;
        t.sketch_mode_radius = 7;
        t.sketch_mode_border = 1.5;
        t.sketch_palette_alpha = 0.96;
        t.sketch_mode_tint = 0.12;
        t.sketch_footer_icon_size = 15.0;
        t.font_section_tracking = 0.04;
        t.file_dim = Color32::from_rgb(180, 187, 200);
        t.file_recovery_well = Color32::from_rgb(92, 68, 32);
        t.file_background = Color32::from_rgb(31, 35, 43);
        t.file_surface = Color32::from_rgb(39, 43, 52);
        t.file_border = Color32::from_rgb(63, 69, 82);
        t.file_rail = Color32::from_rgb(30, 80, 128);
        t.file_rail_hover = Color32::from_rgb(46, 99, 149);
        t.file_primary = Color32::from_rgb(27, 48, 72);
        t.file_primary_ink = Color32::from_rgb(156, 199, 245);
        t.file_primary_border = Color32::from_rgb(58, 136, 238);
        t.file_recovery = Color32::from_rgb(67, 51, 28);
        t.file_recovery_ink = Color32::from_rgb(245, 204, 137);
        t.file_recovery_border = Color32::from_rgb(115, 88, 43);
        t.app_bar = Color32::from_rgb(28, 31, 38);
        t.toolbar = Color32::from_rgb(39, 43, 52);
        t.panel = Color32::from_rgb(39, 43, 52);
        t.panel_header = Color32::from_rgb(47, 52, 62);
        t.border = Color32::from_rgb(63, 69, 82);
        t.text = Color32::from_rgb(222, 226, 232);
        t.text_dim = Color32::from_rgb(148, 155, 168);
        t.accent = Color32::from_rgb(58, 136, 238);
        t.accent_soft = Color32::from_rgb(42, 70, 110);
        t.sketch_accent = icon_primary(crate::icon_svg::SKETCH_PRIMARY[1]);
        t.surface_accent = Color32::from_rgb(149, 104, 45);
        t.mesh_accent = Color32::from_rgb(133, 113, 0);
        t.form_accent = Color32::from_rgb(123, 99, 166);
        t.render_accent = Color32::from_rgb(224, 116, 174);
        t.sketch_ink = Color32::from_rgb(70, 190, 120);
        t.sketch_soft = Color32::from_rgb(31, 59, 43);
        t.surface_fill = Color32::from_rgb(178, 106, 20);
        t.surface_ink = Color32::from_rgb(224, 145, 74);
        t.surface_soft = Color32::from_rgb(69, 51, 30);
        t.mesh_fill = Color32::from_rgb(198, 168, 0);
        t.mesh_ink = Color32::from_rgb(212, 183, 38);
        t.mesh_soft = Color32::from_rgb(65, 57, 26);
        t.form_fill = Color32::from_rgb(138, 111, 204);
        t.form_tab_ink = Color32::from_rgb(179, 155, 238);
        t.form_soft = Color32::from_rgb(55, 47, 79);
        t.sheet_fill = Color32::from_rgb(42, 127, 168);
        t.sheet_ink = Color32::from_rgb(79, 179, 217);
        t.sheet_soft = Color32::from_rgb(31, 58, 71);
        t.render_fill = Color32::from_rgb(200, 88, 143);
        t.render_ink = Color32::from_rgb(224, 116, 174);
        t.render_soft = Color32::from_rgb(64, 35, 54);
        t.tab_on_fill_dark = Color32::BLACK;
        t.nav_hover = Color32::from_rgb(58, 65, 80);
        t.nav_press = Color32::from_rgb(70, 78, 96);
        t.nav_dim = Color32::from_rgb(124, 135, 154);
        t.nav_viewport = Color32::from_rgb(38, 43, 53);
        t.nav_tip = Color32::from_rgb(238, 241, 245);
        t.nav_tip_ink = Color32::from_rgb(27, 36, 48);
        t.nav_tip_key_border = Color32::from_rgba_unmultiplied_const(128, 140, 160, 153);
        t.nav_field = Color32::from_rgb(47, 52, 62);
        t.dialog_body = Color32::from_rgb(39, 43, 52);
        t.dialog_footer = Color32::from_rgb(47, 52, 62);
        t.dialog_rule = Color32::from_rgb(53, 59, 71);
        t.control_fill = Color32::from_rgb(28, 31, 38);
        t.control_border = Color32::from_rgb(74, 82, 98);
        t.dialog_label = t.text;
        t.selected_ink = Color32::from_rgb(156, 199, 245);
        t.selected_bg = Color32::from_rgb(42, 70, 110);
        t.figure_ground = Color32::from_rgb(28, 31, 38);
        t.figure_line = Color32::from_rgb(163, 174, 194);
        t.figure_face_top = Color32::from_rgb(107, 119, 144);
        t.figure_face_left = Color32::from_rgb(81, 91, 114);
        t.figure_face_right = Color32::from_rgb(57, 66, 86);
        t.figure_card = Color32::from_rgb(47, 52, 62);
        t.figure_accent = Color32::from_rgb(140, 190, 245);
        t.figure_reference = Color32::from_rgb(232, 194, 74);
        t.figure_axis_x = Color32::from_rgb(242, 122, 122);
        t.figure_axis_y = Color32::from_rgb(98, 212, 142);
        t.figure_axis_z = Color32::from_rgb(108, 168, 247);
        t.figure_ok = Color32::from_rgb(127, 209, 174);
        t.figure_warn = Color32::from_rgb(232, 194, 74);
        t.pref_well = Color32::from_rgb(47, 52, 62);
        t.pref_note = Color32::from_rgb(240, 162, 122);
        t.pref_note_fill = Color32::from_rgb(74, 51, 38);
        t.pref_on_accent = Color32::from_rgb(15, 22, 32);
        t.pref_ok = Color32::from_rgb(127, 209, 174);
        t.pref_ok_fill = Color32::from_rgb(31, 58, 49);
        t.pref_disabled = Color32::from_rgb(138, 148, 163);
        t.construction_geom = icon_primary(crate::icon_svg::CONSTRUCTION_PRIMARY[1]);
        t.error = Color32::from_rgb(240, 100, 96);
        t.sketch_conflict = t.error;
        t.warning = Color32::from_rgb(236, 170, 70);
        t.icon = Color32::from_rgb(208, 214, 226);
        t.timeline = Color32::from_rgb(33, 37, 45);
        t.hover = Color32::from_rgb(54, 61, 74);
        t.hover_edge_halo = Color32::from_rgb(203, 203, 203);
        t.origin_point = Color32::from_rgb(214, 215, 218);
        t.joint_axis = Color32::from_rgb(110, 240, 250);
        t.viewport_top = Color32::from_rgb(46, 52, 64);
        t.viewport_bottom = Color32::from_rgb(46, 52, 64);
        t.grid = Color32::from_rgb(59, 65, 76);
        t.grid_major = Color32::from_rgb(73, 79, 90);
        t.body = Color32::from_rgb(152, 156, 162);
        t.body_edge = Color32::from_rgb(16, 16, 18);
        t.sketch = Color32::from_rgb(92, 150, 255);
        t.sketch_fixed = Color32::from_rgb(232, 234, 238);
        t.sketch_halo = Color32::from_rgb(28, 32, 40);
        t.sketch_point = Color32::from_rgb(232, 234, 238);
        t.sketch_locked = Color32::from_rgb(96, 204, 116);
        // Sketch dimensions: Light's plum, lightened for the dark view (#7861).
        t.dimension = Color32::from_rgb(224, 176, 224);
        t.dimension_driven = Color32::from_rgb(176, 104, 144);
        t.sketch_point_free = Color32::from_rgb(92, 150, 255);
        t.profile_fill = Color32::from_rgba_unmultiplied_const(120, 150, 200, 40);
        t.dialog_bg = Color32::from_rgb(39, 43, 52);
        t.dialog_border = Color32::from_rgb(63, 69, 82);
        t.chip = Color32::from_rgb(59, 101, 168);
        t.glyph_bg = Color32::from_rgb(44, 50, 60);
        t.glyph_edge = Color32::from_rgb(200, 130, 120);
        t.glyph_text = icon_primary(crate::icon_svg::CONSTRAINT_PRIMARY[1]);
        t.overlay = Color32::from_rgba_unmultiplied_const(34, 38, 46, 240);
        t.rubber_band = Color32::from_rgb(92, 160, 255);
        t.timeline_item = Color32::from_rgb(52, 58, 70);
        t.timeline_item_error = Color32::from_rgb(110, 50, 54);
        t.timeline_marker = Color32::from_rgb(170, 178, 192);
        t.field = Color32::from_rgb(30, 34, 41);
        t.box_window = Color32::from_rgb(70, 150, 255);
        t.box_crossing = Color32::from_rgb(80, 200, 120);
        t.bend_up = Color32::from_rgb(110, 200, 120);
        t.bend_down = Color32::from_rgb(240, 150, 80);
        t.form_ink = t.text;
        t.form_dim = Color32::from_rgb(180, 187, 200);
        t.form_primary = t.accent;
        t.form_primary_fill = t.form_primary;
        t.form_error = t.error;
        t.form_error_fill = t.timeline_item_error;
        t.form_success = Color32::from_rgb(143, 219, 154);
        t.form_success_fill = Color32::from_rgb(44, 74, 51);
        t
    }

    /// Colour vision, red-green (Protan and Deutan share it, #778): values of the theme's colours
    /// that a weak red or green sense still tells apart. Each was measured against normal vision,
    /// protan and deutan, with two simulations of each loss (`colour_vision_tests`).
    pub(super) const fn red_green(self, dark: bool) -> Tokens {
        let mut t = self;
        let positive = [0.20, 0.45, 0.90];
        let negative = [0.85, 0.40, 0.05];
        t.axis_x = Color32::from_rgb(200, 40, 40);
        t.axis_y = Color32::from_rgb(120, 210, 80);
        t.preview_sketch = Color32::from_rgb(80, 185, 145);
        t.analysis.draft = [positive, [0.95, 0.90, 0.60], negative];
        t.analysis.access = [positive, negative];
        t.analysis.hatch = true;
        // Keep validation badges separate from red errors under both red-green
        // simulations, while retaining their explicit Valid / Not valid labels.
        t.form_success_fill = if dark { Color32::from_rgb(44, 57, 67) } else { Color32::from_rgb(227, 227, 246) };
        if !dark {
            t.form_success = Color32::from_rgb(39, 88, 78);
        }
        t.construction_geom = icon_primary(crate::icon_svg::CONSTRUCTION_RED_GREEN[dark as usize]);
        t.glyph_text = icon_primary(crate::icon_svg::CONSTRAINT_RED_GREEN[dark as usize]);
        // A deeper red keeps conflicting sketch geometry (#7244) apart from the orange
        // construction and centerline lines and the crimson lost links for a red-green loss.
        t.sketch_conflict = Color32::from_rgb(195, 35, 30);
        t.surface_accent = if dark { Color32::from_rgb(200, 135, 55) } else { Color32::from_rgb(164, 80, 0) };
        t.mesh_accent = if dark { Color32::from_rgb(190, 175, 70) } else { Color32::from_rgb(130, 103, 0) };
        t.form_accent = if dark { Color32::from_rgb(165, 135, 220) } else { Color32::from_rgb(115, 73, 205) };
        if dark {
            t.bend_up = Color32::from_rgb(110, 165, 255);
            t.bend_down = Color32::from_rgb(240, 180, 70);
            t.sketch_accent = icon_primary(crate::icon_svg::SKETCH_RED_GREEN[1]);
        } else {
            t.sketch_projected = Color32::from_rgb(205, 120, 215);
            t.sketch_locked = Color32::from_rgb(60, 150, 120);
            t.timeline_item_error = Color32::from_rgb(232, 196, 170);
            t.bend_up = Color32::from_rgb(30, 100, 210);
            t.bend_down = Color32::from_rgb(200, 120, 0);
            t.sketch_accent = icon_primary(crate::icon_svg::SKETCH_RED_GREEN[0]);
        }
        t
    }

    /// Colour vision, blue-yellow (Tritan, #778), measured against normal vision and tritan.
    pub(super) const fn blue_yellow(self, dark: bool) -> Tokens {
        let mut t = self;
        t.analysis.hatch = true;
        t.surface_accent = if dark { Color32::from_rgb(210, 120, 95) } else { Color32::from_rgb(164, 65, 50) };
        t.mesh_accent = if dark { Color32::from_rgb(200, 175, 135) } else { Color32::from_rgb(130, 103, 65) };
        t.form_accent = if dark { Color32::from_rgb(150, 140, 210) } else { Color32::from_rgb(115, 73, 170) };
        // Green and blue status lights merge for tritan vision; use rose for working.
        t.agent_busy = Color32::from_rgb(232, 150, 190);
        t.preview_sketch = Color32::from_rgb(125, 180, 105);
        t.glyph_text = icon_primary(crate::icon_svg::CONSTRAINT_BLUE_YELLOW[dark as usize]);
        t.construction_geom = icon_primary(crate::icon_svg::CONSTRUCTION_BLUE_YELLOW[dark as usize]);
        // Conflicting sketch geometry (#7244) apart from the lost links and the construction and
        // centerline lines for a blue-yellow loss: darker on the light view, lighter on the dark.
        t.sketch_conflict = if dark { Color32::from_rgb(255, 170, 160) } else { Color32::from_rgb(165, 15, 30) };
        if dark {
            t.sketch_locked = Color32::from_rgb(140, 225, 110);
            t.box_crossing = Color32::from_rgb(240, 110, 170);
        } else {
            t.sketch_locked = Color32::from_rgb(20, 165, 70);
            t.box_crossing = Color32::from_rgb(200, 60, 120);
        }
        t
    }
}
