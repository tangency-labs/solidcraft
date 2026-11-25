//! Stored preferences on the Navigation page (`PAGE_NAVIGATION`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! navigation_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// While the Browser has the keyboard (the last click was on one of its rows), Up/Down move
            /// the selection, Left/Right fold or unfold, and the arrows do not turn the view (#17931).
            /// Off: the arrows always turn the view, as before.
            browser_arrow_keys: bool = true, PAGE_NAVIGATION => check_tr("prefs.browser_arrow_keys");
            /// Browser-only wheel classification; native platforms keep their scroll phases.
            browser_scroll_wheel: BrowserScrollWheel = BrowserScrollWheel::Automatic, PAGE_NAVIGATION;
            /// The browser's one-time question to make a smooth mouse wheel zoom was answered "don't ask again".
            browser_wheel_hint_dismissed: bool = false, PAGE_NAVIGATION;
            /// Opt in to rotation through both poles with roll, without the Shift axis-snap spring.
            /// Independent of mouse bindings; false also preserves existing saved presets' behavior.
            free_orbit: bool = false, PAGE_NAVIGATION;
            /// With free orbit, mouse drags turn sideways about the world's up axis (a turntable without
            /// the pole limit) instead of the screen's.
            free_orbit_upright: bool = false, PAGE_NAVIGATION => check_tr("nav.free_orbit_upright.label");
            /// How much of the view the model fills when Home, Fit, an opened design or a navigator
            /// view change frames it (0.1 to 1; #10091): 1, the default, fills the room between the
            /// panels, the compass and the navigation bar, as before.
            home_fit_size: f32 = 1.0, PAGE_NAVIGATION, accept(0.1..=1.0);
            /// The navigator's two curved arrows roll the view the other way: the clockwise arrow rolls
            /// counterclockwise and the counterclockwise arrow rolls clockwise (`invert_curved_roll_arrows`,
            /// #10483). Off by default: each arrow rolls the view the way it curls.
            invert_curved_roll_arrows: bool = false, PAGE_NAVIGATION;
            /// The navigator's directional arrows orbit the camera the way the arrow points, as in
            /// Fusion and Inventor (`invert_navigator_arrows`, #10254). Off by default: an arrow rotates
            /// the view toward the side it sits on, so the arrow on the left swings the camera left.
            invert_navigator_arrows: bool = false, PAGE_NAVIGATION;
            /// The face under the pointer keeps its highlight while the view moves under it (#2130).
            keep_hover_while_navigating: bool = true, PAGE_NAVIGATION;
            /// View changes (axis view, Look At, Home) animate; `look_at_speed` scales how fast.
            look_at_anim: bool = true, PAGE_NAVIGATION => check("Animate view changes", "ui.look_at_anim_desc");
            look_at_speed: f32 = 1.0, PAGE_NAVIGATION, accept(0.25..=4.0) => enabled_slider(look_at_anim, "Animation speed", 0.5..=3.0, 0.25, |v| format!("{v:.2}×"));
            /// While an orbit turns about the origin or the model point under the cursor (Orbit around),
            /// a faint ball marks that point, as it does for the 3D mouse (#17474); false draws nothing,
            /// as before.
            mark_orbit_point: bool = true, PAGE_NAVIGATION => check_tr("nav.mark_orbit_point.label");
            /// Maya Alt+right drag uses horizontal motion; off restores the former vertical zoom.
            maya_horizontal_zoom: bool = true, PAGE_NAVIGATION => check_id("nav.maya_horizontal_zoom.label");
            /// What a middle-button double-click in the view does: `home`, `fit` or `off`.
            middle_double_click: String = "home".into(), PAGE_NAVIGATION;
            /// Mouse gestures: a ring of tools on the right button.
            mouse_gestures: crate::mouse_gesture::Config = Default::default(), PAGE_NAVIGATION;
            /// Mouse mapping: `fusion`, `solidworks` or `inventor`.
            nav: String = "fusion".into(), PAGE_NAVIGATION;
            /// What dragging a mouse button does when the user has chosen it, by button (`left`, `middle`,
            /// `right`, `extra1`, `extra2`): `orbit`, `pan`, `zoom` or `none`, after any keys (`shift+`,
            /// `ctrl+`, `alt+`). A button that is absent follows the mouse mapping. Empty by default.
            nav_button_map: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new(), PAGE_NAVIGATION;
            /// A middle or right drag keeps the mode it started with until its button is released, as a
            /// left drag with the mapping's keys does: letting go of Shift partway through a Shift+middle
            /// orbit doesn't turn it into a pan (#9557). Off: the keys held decide on every frame, as before.
            nav_drag_keeps_mode: bool = true, PAGE_NAVIGATION => check_tr("nav.drag_keeps_mode.label");
            /// The custom view gizmo has the orbit and roll arrows around it.
            navigator_arrows: bool = true, PAGE_NAVIGATION;
            /// A view chosen on the navigator (a face, corner or arrow, an axis tip) also frames the
            /// whole model, as Home does (#10091). Off by default: the view turns in place, keeping its
            /// target and zoom.
            navigator_fit: bool = false, PAGE_NAVIGATION;
            /// The custom view gizmo has the Home button beside it.
            navigator_home: bool = true, PAGE_NAVIGATION;
            /// View orientation widget: original signed axes (default) or a custom body.
            navigator_style: NavigatorStyle = NavigatorStyle::default(), PAGE_NAVIGATION;
            /// The custom view gizmo's colours: the theme's (themed) or the body's own (modeled).
            navigator_texture: NavigatorTexture = NavigatorTexture::default(), PAGE_NAVIGATION;
            /// Show the axes navigator's half-turn button; disabling it restores the previous layout.
            navigator_turn_around: bool = true, PAGE_NAVIGATION;
            /// What an orbit turns around.
            orbit_around: OrbitAround = OrbitAround::default(), PAGE_NAVIGATION;
            /// The cursor preference as releases before the origin option saved it (`true` = the point
            /// under the cursor). [`migrate`] folds it into `orbit_around` when saved preferences are
            /// read, so an existing choice survives the upgrade; this version only writes `false`.
            orbit_cursor: bool = false, PAGE_NAVIGATION;
            /// An orbit drag that reaches the edge of the view moves the cursor to the opposite edge and
            /// keeps turning; false stops the drag at the edge of the screen, as before.
            orbit_cursor_wrap: bool = true, PAGE_NAVIGATION => check_tr("nav.orbit_cursor_wrap.label");
            /// Shift with two-finger trackpad scrolling orbits the view the other way, both ways at once,
            /// as natural scrolling turns it (#8410).
            orbit_reverse: bool = false, PAGE_NAVIGATION => check("ui.reverse_trackpad_orbit_direction", "Shift with two-finger scrolling orbits the other way");
            /// Horizontal trackpad and thumb-wheel scrolling pans the view the other way (#2007).
            pan_reverse_x: bool = false, PAGE_NAVIGATION => check("ui.reverse_horizontal_pan_direction", "Horizontal two-finger scrolling and the thumb wheel pan the other way");
            /// Vertical trackpad scrolling pans the view the other way (#5240).
            pan_reverse_y: bool = false, PAGE_NAVIGATION => check("ui.reverse_vertical_pan_direction", "Vertical two-finger scrolling pans the other way");
            /// The quick menu (`quick_menu`) also opens by itself shortly after a click in the viewport
            /// selects something a command takes (#5501). Off by default: a click only selects.
            quick_menu_after_selection: bool = false, PAGE_NAVIGATION => check_tr("prefs.quick_menu_after_selection");
            /// Show the radial ring and enable hold/flick gestures for viewport context menus.
            radial_context_menu: bool = false, PAGE_NAVIGATION => check("Radial context menu");
            /// Off: a right drag that the mouse mapping would orbit leaves the camera alone (#6492).
            right_drag_orbits: bool = true, PAGE_NAVIGATION => check_tr("nav.right_drag_orbits.label");
            /// Spread discrete wheel input across frames (off: apply each event immediately).
            smooth_wheel_zoom: bool = true, PAGE_NAVIGATION => check("Smooth wheel zoom", "Turn off to apply each wheel event immediately, without a smoothing tail");
            /// Touchscreen drags orbit/pan; off preserves the mouse mapping's selection behavior.
            touchscreen_navigation: bool = false, PAGE_NAVIGATION => check_tr("nav.touchscreen.label");
            /// Two-finger trackpad scroll pans (off: it zooms like the wheel); pinch always zooms.
            trackpad_pan: bool = true, PAGE_NAVIGATION;
            /// A two-finger twist on the trackpad turns the view about the world's up axis (#6286). Off
            /// by default: the twist does nothing, as before.
            trackpad_twist_turns: bool = false, PAGE_NAVIGATION => check_id_help("prefs.trackpad_twist_turns", "prefs.trackpad_twist_turns_tip");
            /// Hold this modifier alone to zoom with vertical wheel or two-finger scroll.
            trackpad_zoom_modifier: TrackpadZoomModifier = TrackpadZoomModifier::default(), PAGE_NAVIGATION;
            /// The upright orbit stands a rolled view up as the pointer moves, turning the way the
            /// pointer goes on screen (#12458). Off: a rolled view keeps its roll and turns about the
            /// world's up axis, as before, so an upside-down view stays upside down.
            upright_orbit_unrolls: bool = true, PAGE_NAVIGATION => check_tr("nav.upright_orbit_unrolls.label");
            /// Speed multiplier for wheel zoom, independent of the 3D mouse.
            wheel_zoom_speed: f32 = 1.0, PAGE_NAVIGATION, accept(0.25..=4.0) => slider("ui.wheel_zoom_speed", 0.25..=4.0, 0.05, |v| format!("{v:.2}×"), "prefs:wheel_zoom_speed");
            /// The wheel zooms out when turned forward.
            zoom_reverse: bool = false, PAGE_NAVIGATION => check("Reverse zoom direction");
            /// Opt-in surface anchoring for the 3D mouse, navigation bridge and drag-zoom. The wheel
            /// and pinch retain their independent `zoom_to_cursor` preference.
            zoom_stop_at_model: bool = false, PAGE_NAVIGATION => check_id("prefs.zoom_stop_at_model");
            /// The wheel zooms toward the point under the cursor (else toward the view's centre).
            zoom_to_cursor: bool = true, PAGE_NAVIGATION => check("Zoom to the cursor", "The wheel zooms toward the point under the cursor (off: toward the view's center)");
        }
    };
}
