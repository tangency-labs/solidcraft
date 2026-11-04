//! When each item of the conversation came in, and how long the agent has been at it (#11986).
//!
//! Times are wall-clock milliseconds since the epoch ([`crate::now_ms`], `web_time` on the web),
//! read the way the Messages window reads its own: "3 min ago", with the exact time (UTC) on hover.
//! They live with the conversation in memory only; a conversation is not saved.

use crate::i18n::format as trf;

/// One item's time, kept beside it in [`super::AgentChat`]'s `times`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stamp {
    /// When the item came in, in ms since the epoch; 0: not known, shown without a time.
    pub at: f64,
    /// On the last item of an answer: how long the answer took (ms), and whether it ended by
    /// itself (not stopped, not failed).
    pub took: Option<(f64, bool)>,
}

impl Stamp {
    pub fn now() -> Self {
        Self { at: crate::now_ms(), took: None }
    }

    pub fn known(&self) -> bool {
        self.at.is_finite() && self.at > 0.0
    }
}

/// A length of time, rounded down: "40 s", "12 min", "1 h 5 min".
pub(crate) fn span(ms: f64) -> String {
    let s = if ms.is_finite() { (ms / 1000.0).max(0.0).floor() } else { 0.0 };
    // A clock set far back still reads as a number, not as a counter of years in seconds.
    let s = s.min(1e9);
    if s < 60.0 {
        trf("ui.agent_chat_span_s", &[("n", &format!("{s:.0}"))])
    } else if s < 3600.0 {
        trf("ui.agent_chat_span_min", &[("n", &format!("{:.0}", (s / 60.0).floor()))])
    } else {
        let h = (s / 3600.0).floor();
        let min = ((s - h * 3600.0) / 60.0).floor();
        trf("ui.agent_chat_span_h", &[("h", &format!("{h:.0}")), ("min", &format!("{min:.0}"))])
    }
}

/// How long ago an item came, as the Messages window says it ("3 min ago").
pub(crate) fn ago(stamp: &Stamp, now: f64) -> Option<String> {
    stamp.known().then(|| crate::messages::ago(now, stamp.at))
}

/// The exact time of an item, for its hover: "2026-11-04 14:17:05 UTC".
pub(crate) fn exact(stamp: &Stamp) -> Option<String> {
    if !stamp.known() {
        return None;
    }
    // Whole seconds, far inside what `utc` can write.
    let secs = (stamp.at / 1000.0).floor().min(253_402_300_799.0) as u64;
    let iso = solidcraft_engine::crash::utc(secs);
    Some(format!("{} UTC", iso.trim_end_matches('Z').replacen('T', " ", 1)))
}

/// The live line under the conversation while the agent works with nothing running:
/// "Thinking for 4 min", counted from the last thing the agent did.
pub(crate) fn thinking(since: f64, now: f64) -> String {
    trf("ui.agent_chat_thinking_for", &[("time", &span(now - since))])
}

/// A running tool card's state: "running for 30 s".
pub(crate) fn running(stamp: &Stamp, now: f64) -> Option<String> {
    stamp.known().then(|| trf("ui.agent_chat_tool_running_for", &[("time", &span(now - stamp.at))]))
}

/// The line after an answer: "Answered in 23 min", or "Ended after 2 min" when it was stopped or failed.
pub(crate) fn took(stamp: &Stamp) -> Option<String> {
    let (ms, answered) = stamp.took?;
    let id = if answered { "ui.agent_chat_answered_in" } else { "ui.agent_chat_ended_after" };
    Some(trf(id, &[("time", &span(ms))]))
}
