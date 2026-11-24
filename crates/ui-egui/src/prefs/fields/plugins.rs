//! Stored preferences on the Plugins page (`PAGE_PLUGINS`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! plugins_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// Plugin ids turned off (Preferences › Plugins). An installed plugin whose id is absent is on.
            disabled_plugins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new(), PAGE_PLUGINS;
            /// Last-run importer options, by plugin id and entry name. Bookkeeping, never design data.
            plugin_importer_options: std::collections::BTreeMap<String, std::collections::BTreeMap<String, serde_json::Value>> = std::collections::BTreeMap::new();
            /// Show the global fuel and memory controls on Preferences > Plugins (#18764).
            /// Off keeps advanced controls folded; on exposes the prior always-visible controls.
            plugin_limits_expanded: bool = false, PAGE_PLUGINS;
            /// User ceilings, not automatic increases to a package's approved execution budget.
            plugin_max_fuel: u64 = solidcraft_engine::plugin::HostLimits::default().max_fuel, PAGE_PLUGINS;
            plugin_max_memory_mib: usize = solidcraft_engine::plugin::HostLimits::default().max_memory_mib, PAGE_PLUGINS;
            /// Remember a moved plugin command window's position for the current design, until the
            /// application closes (#18684). Off keeps the previously centred reopen behaviour.
            plugin_remember_dialog_position: bool = false, PAGE_PLUGINS;
            /// Opt in to reusing last-run options and asking for them on File > Open and drop.
            /// Off starts menu dialogs from schema defaults and opens chosen files directly.
            plugin_remember_importer_options: bool = false, PAGE_PLUGINS => check_id_help("prefs.plugin_remember_importer_options", "prefs.plugin_remember_importer_options_help");
        }
    };
}
