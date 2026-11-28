//! A permission round-trip owned by one interactive file worker.
use super::{Grants, HostError, grant_store};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Once,
    Always,
    Cancel,
}

#[derive(Debug)]
pub struct Question {
    pub plugin: String,
    pub folder: String,
    pub reply: mpsc::Sender<Answer>,
}

#[derive(Debug)]
struct State {
    sender: mpsc::Sender<Question>,
    context: Option<(String, String, PathBuf, String, Grants)>,
    answer: Mutex<Option<bool>>,
}

/// Trusted UI authority, deliberately absent from serialized command parameters.
#[derive(Clone, Debug)]
pub struct Prompt(Arc<State>);
impl PartialEq for Prompt {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Prompt {
    pub fn new(sender: mpsc::Sender<Question>) -> Self {
        Self(Arc::new(State { sender, context: None, answer: Mutex::new(None) }))
    }
    pub(super) fn for_import(&self, plugin: String, folder: String, root: PathBuf, id: String, grants: Grants) -> Self {
        Self(Arc::new(State { sender: self.0.sender.clone(), context: Some((plugin, folder, root, id, grants)), answer: Mutex::new(None) }))
    }
    pub(super) fn ask(&self) -> craft_plugin::Result<bool> {
        let Some((plugin, folder, root, id, grants)) = &self.0.context else { return Ok(false) };
        let mut answer = self.0.answer.lock().map_err(|_| HostError::Cancelled)?;
        if let Some(answer) = *answer {
            return Ok(answer);
        }
        let (reply, rx) = mpsc::channel();
        if self.0.sender.send(Question { plugin: plugin.clone(), folder: folder.clone(), reply }).is_err() {
            return Ok(false);
        }
        let chosen = loop {
            if solidcraft_doc::progress::cancelled() {
                return Err(HostError::Cancelled);
            }
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(answer) => break answer,
                Err(mpsc::RecvTimeoutError::Disconnected) => break Answer::Cancel,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        if chosen == Answer::Always {
            grant_store::write(root, id, grants)
                .map_err(|error| HostError::PermissionDenied { permission: format!("input_folder: could not store approval: {error}") })?;
        }
        let allowed = chosen != Answer::Cancel;
        *answer = Some(allowed);
        Ok(allowed)
    }
}
