//! Stored preferences on the Security page (`PAGE_SECURITY`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! security_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// Report a bug (the title bar's button, Help › Report an Issue, Create Issue) opens the
            /// public GitHub draft it opened before tickets, instead of the report that files a ticket.
            report_on_github: bool = false, PAGE_SECURITY => check_id_help("prefs.reports.github", "prefs.reports.github_help");
            /// Sign every saved design with a GnuPG key. The key may be on a smartcard, where it stays.
            sign_saved_designs: bool = false, PAGE_SECURITY => check_id_help("prefs.security.sign_saved", "prefs.security.sign_saved_help");
            /// The GnuPG key to sign with: a fingerprint or user id. Empty: GnuPG's default signing key.
            signing_key: String = String::new(), PAGE_SECURITY;
            /// Fingerprints of signing keys the user trusts, separated by commas or spaces. Only a
            /// signature from one of these is shown as verified; the user ids in a file are not proof.
            trusted_signing_keys: String = String::new(), PAGE_SECURITY;
            /// Verify a design's signature when it opens. Off: opening reads no payload twice.
            verify_opened_designs: bool = false, PAGE_SECURITY => check_id_help("prefs.security.verify_opened", "prefs.security.verify_opened_help");
        }
    };
}
