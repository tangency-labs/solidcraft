//! Stored preferences on the Plugins page (`PAGE_PLUGINS`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! plugins_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// A plugin's background effect, `plugin-id/shader-id` (`shaders@0.1`); empty for none.
            background_shader: String = String::new(), PAGE_PLUGINS;
            /// The picture each chosen texture of a background effect shows, `plugin-id/shader-id/<texture
            /// name>` to an absolute file path (`choose` in the manifest). Absent: the shipped `file`.
            background_shader_pictures: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new(), PAGE_PLUGINS;
            /// Plugin ids turned off (Preferences › Plugins). An installed plugin whose id is absent is on.
            disabled_plugins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new(), PAGE_PLUGINS;
            /// The servers plugins may connect to, on top of each plugin's approval (`net@0.1`).
            /// Empty: no limit beyond those approvals.
            plugin_allowed_hosts: Vec<solidcraft_engine::plugin::AllowedHost> = Vec::new(), PAGE_PLUGINS;
            /// A plugin's shader for see-through panels' backgrounds, `plugin-id/shader-id` (`shaders@0.2`);
            /// empty for none.
            panel_shader: String = String::new(), PAGE_PLUGINS;
            /// Show the global fuel and memory controls on Preferences > Plugins (#18764).
            /// Off keeps advanced controls folded; on exposes the prior always-visible controls.
            plugin_limits_expanded: bool = false, PAGE_PLUGINS;
            /// User ceilings, not automatic increases to a package's approved execution budget.
            plugin_max_fuel: u64 = solidcraft_engine::plugin::HostLimits::default().max_fuel, PAGE_PLUGINS;
            plugin_max_memory_mib: usize = solidcraft_engine::plugin::HostLimits::default().max_memory_mib, PAGE_PLUGINS;
            /// Remember a moved plugin command window's position for the current design, until the
            /// application closes (#18684). Off keeps the previously centred reopen behaviour.
            plugin_remember_dialog_position: bool = false, PAGE_PLUGINS;
            /// Plugin shaders' slider values, keyed `plugin-id/shader-id/setting-id`; a missing one is its default.
            shader_settings: std::collections::BTreeMap<String, f32> = Default::default(), PAGE_PLUGINS;
            /// A plugin's tint for the ribbon and panels, `plugin-id/shader-id` (`shaders@0.2`); empty for none.
            tint_shader: String = String::new(), PAGE_PLUGINS;
        }
    };
}
