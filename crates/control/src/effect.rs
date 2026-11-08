//! What a client is told about a control request that outlasts its wait (docs/control-protocol.md,
//! #1618): the app's control server and a bridged client both say what the request still does when
//! it finishes. Which requests do what is the app's to say.

/// What a request still running does when it finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// It only reads (the document, the window or the command list): nothing changes.
    ReadsOnly,
    /// It writes a file (an export, a save, a render to a path); the document stays as it is.
    WritesFile,
    /// It changes the document (or may: anything not known to read or only write a file).
    Changes,
}

impl Effect {
    /// What happens when the request finishes, for a message: "it only reads: nothing changes",
    /// "it was not cancelled and will still write the file when it finishes", …
    pub fn when_done(self) -> &'static str {
        match self {
            Effect::ReadsOnly => "it only reads: nothing changes",
            Effect::WritesFile => "it was not cancelled and will still write the file when it finishes; the design does not change",
            Effect::Changes => "it was not cancelled and will still apply when it finishes",
        }
    }
}
