//! Stored preferences on the Graphics page (`PAGE_GRAPHICS`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! graphics_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// Soft contact shading in the GPU viewport; optional and applied immediately.
            ambient_occlusion: bool = false, PAGE_GRAPHICS;
            /// Optional schematic markings on threaded cylindrical walls; off for new and older preferences.
            cosmetic_threads: bool = false, PAGE_GRAPHICS;
            /// A preview that removes material shows the removed shape as a red see-through ghost. Off:
            /// the ghost is fully transparent and only its outline is drawn (`preview::Colors::cut_tint`).
            /// A Combine's preview always shows its ghosts.
            cut_preview_tint: bool = true, PAGE_GRAPHICS => check_id("prefs.cut_preview_tint");
            /// Shade the viewport in linear light with a sky fill, so dark parts keep their shape
            /// (#7974). Off by default: the viewport shades as it always has.
            dark_parts_shading: bool = false, PAGE_GRAPHICS => check_id("prefs.dark_parts_shading");
            /// How finely the viewport tessellates curved faces and edges: `coarse`, `normal`, `fine` or
            /// `ultra` (#1287). Only the display plan the viewport draws; never the design or exports.
            display_quality: String = "normal".into(), PAGE_GRAPHICS;
            /// An idle Escape (nothing else owns it) turns off the newest analysis or the section view,
            /// as before. Off by default: they close only from their status item, toggle or command.
            escape_closes_analyses: bool = false, PAGE_GRAPHICS;
            /// With the frame rate: a frame-time graph, a histogram and what the GPU drew (#2548).
            frame_rate_details: bool = false, PAGE_GRAPHICS;
            /// Exact graphics adapter name; empty selects automatically on the next start.
            graphics_adapter: String = String::new(), PAGE_GRAPHICS;
            /// Graphics API for the next start; Automatic preserves the platform startup order.
            graphics_backend: String = "auto".into(), PAGE_GRAPHICS;
            /// Restore the shadow plane at the lowest visible body, including above the origin (#9838).
            ground_shadow_follows_model: bool = false, PAGE_GRAPHICS => check_id("prefs.ground_shadow_follows_model");
            /// Joint badges draw quiet unless the joint lets its parts move, which shows what a
            /// construction has fully constrained. Off: every joint draws alike. The ring and axis
            /// style draws every joint alike either way.
            highlight_moving_joints: bool = true, PAGE_GRAPHICS => check_id("prefs.highlight_moving_joints");
            /// How joints look in the viewport: 0 the ring and axis drawn before badges, or a badge with
            /// the joint type's pictogram: 1 white on dark, 2 solid accent, 3 solid amber, 4 outline,
            /// 5 a colour and outline for each family of types, 6 dots (`joint_symbols::STYLES`).
            joint_symbols: u8 = 0, PAGE_GRAPHICS, accept(0..=6);
            /// Anti-aliasing samples (applies on restart).
            msaa: u32 = 4, PAGE_GRAPHICS;
            /// Perspective view's vertical field of view in radians (#5364), 15° to 90°. The view reads a
            /// saved value outside that range as the default ([`perspective_fov`]).
            perspective_fov: f64 = PERSPECTIVE_FOV, PAGE_GRAPHICS, accept(15f64.to_radians()..=90f64.to_radians());
            /// A preview of a feature that changes a body draws the surface it adds in the preview
            /// colour, working out face by face which surface that is. Off: the body is drawn as itself
            /// and nothing is worked out (`preview::Colors::tint_new`).
            preview_tint_new_faces: bool = true, PAGE_GRAPHICS => check_id("prefs.preview_tint_new_faces");
            /// Record where the time goes (SMLib's calls, kernel operations, features, commands, imports)
            /// for `diagnostics.profile` (docs/perf.md). Off by default: nothing is recorded.
            profile_operations: bool = false, PAGE_GRAPHICS => check_tr("prefs.profile_operations");
            /// The selection's length, area or distance in the view's bottom right corner (#7000). Off by
            /// default.
            quick_measure: bool = false, PAGE_GRAPHICS;
            /// Draw reference geometry such as inactive sketches and datum planes through shaded bodies.
            /// Off by default to preserve the depth-tested viewport behaviour.
            reference_geometry_through_bodies: bool = false, PAGE_GRAPHICS => check_id("prefs.graphics_page.reference_geometry_through_bodies");
            /// Restore Raster for new Render setups, fixed CPU sampling and its mode selector.
            /// Refit on entry and keep shot activation camera-neutral, with the previous preview
            /// framing, navigation, picking and selection overlays. Disable automatic live feedback
            /// and automatic result opening; retain the previous result/cancel viewing behavior.
            /// GPU sample edits round to powers of two.
            render_legacy_image_defaults: bool = false, PAGE_GRAPHICS => check_id("prefs.render_legacy_image_defaults");
            /// Restore native software-pool admission without refusing an available-RAM shortage.
            /// Shared/host software limits and device/allocation checks remain in force.
            render_legacy_memory_admission: bool = false, PAGE_GRAPHICS => check_id("prefs.render_legacy_memory_admission");
            /// Restore the previous platform-specific fixed Render preview geometry budget.
            render_previous_geometry_budget: bool = false, PAGE_GRAPHICS => check_id("prefs.render_previous_geometry_budget");
            /// A section hatches each cut body its own way (the default), or every one alike, in the cap
            /// colour at 45 degrees, as before.
            section_hatch: SectionHatch = SectionHatch::ByPart, PAGE_GRAPHICS;
            /// Show the frame rate in the view's corner (the view then redraws every frame).
            show_frame_rate: bool = false, PAGE_GRAPHICS;
            /// A selected whole body draws its hidden edges dimmed through the model, as hovering one did
            /// before. Off by default: a selected body is tinted, and neither hover nor selection shows
            /// the edges the model hides.
            show_selected_hidden_edges: bool = false, PAGE_GRAPHICS => check_id("prefs.show_selected_hidden_edges");
            /// Show the view's pitch, yaw and roll and its projection under the orientation gizmo (#6032).
            /// Off by default; View ▸ View angles and `view.angle_readout` turn it on.
            show_view_angles: bool = false, PAGE_GRAPHICS;
            /// Share shading and Continuity; false preserves their independent legacy behavior.
            single_surface_analysis: bool = true, PAGE_GRAPHICS => check_id("prefs.graphics_page.single_surface_analysis");
            /// Draw a wider rim under finished sketch curves to improve contrast on shaded faces.
            sketch_halo: bool = false, PAGE_GRAPHICS => check("Halo under sketch curves");
            /// Supersampling (#1288): the view drawn at this many times its pixels each way and filtered
            /// down (1, 1.5 or 2; 1 is off).
            supersampling: f32 = 1.0, PAGE_GRAPHICS;
            /// Supersample only while the view holds still (full speed while orbiting).
            supersampling_adaptive: bool = true, PAGE_GRAPHICS => check_tr("prefs.graphics_page.supersampling_adaptive");
            viewport_analysis_status: bool = true, PAGE_GRAPHICS;
            /// Opaque bodies are lit by a soft studio in the viewport: metal and glossy appearances (PBR)
            /// reflect it, other bodies get a satin finish. Off: shaded flat in their colour, as before.
            viewport_material_finish: bool = true, PAGE_GRAPHICS => check_id("prefs.viewport_material_finish");
            viewport_section_status: bool = true, PAGE_GRAPHICS;
            viewport_status_details: bool = true, PAGE_GRAPHICS;
            viewport_status_summary: bool = true, PAGE_GRAPHICS;
            /// Linux: the window system the app opens with at its next start (#3083): `auto` (XWayland
            /// whenever there is an X display, native Wayland on niri and Hyprland), `wayland` or `x11`.
            window_system: String = "auto".into(), PAGE_GRAPHICS;
        }
    };
}
