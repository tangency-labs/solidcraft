//! Spoken narration for tutorial mode: a full-sentence explanation of each tool, step-by-step
//! commentary on what the acted-out demo is doing, and the result.
//!
//! Two voices, in order of preference. The bundled neural one ([`crate::narration_neural`]:
//! Kokoro, offline, synthesized on a worker thread) sounds like a person reading, and it reports
//! which line it is on so the demo can be paced to it. Where it cannot load -- no model beside
//! the executable, no audio device -- the operating system's own synthesizer speaks instead
//! (SAPI/WinRT on Windows, AVSpeechSynthesizer on macOS, Speech Dispatcher on Linux), which
//! bundles nothing and is always there.
//!
//! The lines are written as whole sentences with ordinary punctuation, which either voice reads
//! with more natural pauses and intonation than terse fragments; for the system voice the
//! [`Narrator`] also picks the most natural installed one and reads a little slower than the
//! default.
//!
//! **The lines are English, and are not translated.** That is deliberate, not an oversight: they
//! are built by gluing fragments ("a distance of" + the number + "millimetres"), and a sentence
//! assembled that way cannot be made to work in another language, where the order, the agreement
//! and the number words all differ. Translating narration means whole sentences written per
//! language, with their own voice to read them; until there is one, an English voice reading
//! English is better than an English voice reading half-translated fragments. Nothing here goes
//! through `tr_label`, so nothing claims to be translated that is not.
//!
//! Narration is a nicety, never a risk: a system with no speech, or any synthesizer error or panic,
//! simply stays silent. It never interrupts the tutorial and never brings the app down. On wasm the
//! whole thing is a silent stub, so the UI crate still builds for the browser.

use serde_json::Value;

/// Make sure a sentence ends with a full stop (so the next one doesn't run into it).
fn ended(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() || s.ends_with(['.', '!', '?', ':']) { s.to_string() } else { format!("{s}.") }
}

/// Where a tool lives, spoken: "the Model tab's Create panel" from a `"Model › Create"` group.
fn where_phrase(group: &str) -> Option<String> {
    let mut parts = group.split('\u{203a}').map(str::trim).filter(|p| !p.is_empty());
    match (parts.next(), parts.next()) {
        (Some(tab), Some(panel)) => Some(format!("the {tab} tab's {panel} panel")),
        _ => None,
    }
}

/// The spoken introduction for a lesson: what the tool is and where to find it, its explanation,
/// its keyboard shortcut, and that it is about to be shown on a sample model.
pub fn intro(title: &str, about: &str, group: &str, shortcut: Option<&str>) -> String {
    let mut out = match where_phrase(group) {
        Some(w) => format!("Let's look at {title}, in {w}."),
        None => format!("Let's look at {title}."),
    };
    if !about.trim().is_empty() {
        out.push(' ');
        out.push_str(&ended(about));
    }
    if let Some(key) = shortcut.map(str::trim).filter(|k| !k.is_empty()) {
        out.push(' ');
        out.push_str(&format!("Its keyboard shortcut is {key}."));
    }
    out.push(' ');
    out.push_str("Watch as I perform it on a sample model.");
    out
}

/// Step-by-step commentary for one demo step: which tool is being used and the value being typed,
/// led by an ordinal ("First", "Next", "Finally") that follows the step's place in the demo.
pub fn step_cue(label: &str, params: &Value, first: bool, last: bool) -> String {
    let lead = match (first, last) {
        (true, true) => "Now,",
        (true, false) => "First,",
        (false, true) => "Finally,",
        (false, false) => "Next,",
    };
    let mut out = format!("{lead} I'll use the {label} tool");
    if let Some(phrase) = typed_phrase(params) {
        out.push_str(&format!(", and enter {phrase}"));
    }
    ended(&out)
}

/// The value a person types into this command's panel, spoken in full ("a distance of 20
/// millimetres", "6 sides"); mirrors the tutorial player's list of typed parameters so the
/// commentary matches the keys shown on screen. `None` when nothing is typed.
fn typed_phrase(params: &Value) -> Option<String> {
    for k in crate::tutorial::TYPED {
        let raw = match params.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => continue,
        };
        if *k == "text" || *k == "designation" {
            return Some(format!("the text, {raw}"));
        }
        // "30 deg" / "10 mm" → the bare number; the unit word is added back below.
        let num = raw.trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace()).trim();
        let num = if num.is_empty() { raw.as_str() } else { num };
        let num = tidy_number(num);
        let (lead, unit) = phrase_parts(k);
        let mut out = String::new();
        if !lead.is_empty() {
            out.push_str(lead);
            out.push(' ');
        }
        out.push_str(&num);
        if !unit.is_empty() {
            out.push(' ');
            out.push_str(unit);
        }
        return Some(out);
    }
    None
}

/// For a typed parameter, how to say it: a leading phrase and a trailing unit word (either may be
/// empty). Lengths are millimetres, angles degrees, counts their own nouns.
fn phrase_parts(key: &str) -> (&'static str, &'static str) {
    match key {
        "distance" => ("a distance of", "millimetres"),
        "radius" => ("a radius of", "millimetres"),
        "diameter" => ("a diameter of", "millimetres"),
        "thickness" => ("a thickness of", "millimetres"),
        "length" => ("a length of", "millimetres"),
        "width" => ("a width of", "millimetres"),
        "height" => ("a height of", "millimetres"),
        "major" => ("a major radius of", "millimetres"),
        "minor" => ("a minor radius of", "millimetres"),
        "size" => ("a size of", "millimetres"),
        "angle" => ("an angle of", "degrees"),
        "count" | "count1" => ("", "copies"),
        "sides" => ("", "sides"),
        "factor" => ("a factor of", ""),
        _ => ("a value of", ""),
    }
}

/// The completion line: the command's result read as a natural sentence (unit symbols spelt out,
/// trailing zeros trimmed), or a plain "that completes it" when there is nothing to report.
///
/// The labels it reads are the ones [`crate::tutorial`] writes, which come from the catalog: in
/// English they are the words matched below, and in another language the fall-through reads the
/// label as it stands, which is the best this fragment-built sentence can do.
pub fn result(summary: &str) -> String {
    let facts: Vec<String> = summary.lines().filter_map(phrase_fact).collect();
    if facts.is_empty() { "And that completes it.".to_string() } else { format!("And there's the result: {}.", facts.join(", ")) }
}

/// One line of the result summary ("Volume: 3000.00 mm³") spoken ("a volume of 3000 cubic
/// millimetres").
fn phrase_fact(line: &str) -> Option<String> {
    let (label, rest) = line.split_once(':')?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let value = tidy_numbers(&spoken_units(rest));
    let lead = match label.trim() {
        "Volume" => "a volume of",
        "Area" => "an area of",
        "Size" => "measuring",
        "Centre" | "Center" => "centred at",
        "Mass" => "a mass of",
        other => return Some(format!("{}, {value}", other.to_lowercase())),
    };
    Some(format!("{lead} {value}"))
}

/// Unit symbols spelt out for speech.
fn spoken_units(s: &str) -> String {
    let s = s
        .replace("mm\u{b3}", "cubic millimetres")
        .replace("mm\u{b2}", "square millimetres")
        .replace("mm", "millimetres")
        .replace('\u{d7}', "by");
    match s.strip_suffix(" g") {
        Some(head) => format!("{head} grams"),
        None => s,
    }
}

/// Trim trailing zeros from the decimals in every number of a string ("20.0 by 10.00" → "20 by 10").
fn tidy_numbers(s: &str) -> String {
    s.split(' ').map(tidy_number).collect::<Vec<_>>().join(" ")
}

/// Trim trailing zeros (and a trailing decimal point) from one token, keeping any surrounding
/// punctuation; non-numbers are returned unchanged.
fn tidy_number(tok: &str) -> String {
    let trail_len = tok.chars().rev().take_while(|c| !c.is_ascii_digit()).count();
    let Some(core) = tok.get(..tok.len() - trail_len) else { return tok.to_string() };
    let Some(trail) = tok.get(tok.len() - trail_len..) else { return tok.to_string() };
    if core.contains('.')
        && let Ok(n) = core.parse::<f64>()
        && n.is_finite()
    {
        return format!("{n}{trail}");
    }
    tok.to_string()
}

/// Speaks tutorial narration: the bundled neural voice when it loads, otherwise the operating
/// system's own synthesizer.
///
/// Native builds start the neural voice on first use and keep the system voice as a fallback;
/// wasm is an empty stub. Every call is best-effort: a missing engine, or any error or panic, is
/// swallowed so narration can never crash the app or block the tutorial.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct Narrator {
    /// Whether the neural voice has been started yet.
    started: bool,
    /// The bundled neural voice; preferred whenever it loads.
    neural: Option<crate::narration_neural::Neural>,
    /// The system voice, used only where the neural voice is absent or failed. Made on first use.
    system: Option<tts::Tts>,
    /// Whether making the system voice has been tried (a system with no speech is not retried
    /// every line).
    tried: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Narrator {
    /// Speak `text`, cutting off whatever is being said when `interrupt` is set (otherwise it is
    /// queued after it). Empty text, a missing engine, and any error or panic are ignored.
    ///
    /// Returns the neural voice's sequence number for this line, for pacing the demo to it, or
    /// `None` when the system voice spoke it (which has no synthesis lag to pace around).
    pub fn speak(&mut self, text: &str, interrupt: bool) -> Option<u64> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if !self.started {
            self.started = true;
            self.neural = crate::narration_neural::Neural::start();
        }
        // Prefer the neural voice; fall back to the system one if it is absent or failed.
        if let Some(neural) = self.neural.as_ref().filter(|n| !n.failed()) {
            return Some(neural.speak(text, interrupt));
        }
        self.system_speak(text, interrupt);
        None
    }

    /// Speak through the operating system's synthesizer (the fallback), made and tuned on first
    /// use.
    fn system_speak(&mut self, text: &str, interrupt: bool) {
        if !self.tried {
            self.tried = true;
            // `Tts::default` picks the platform's own synthesizer; tune it, or stay silent.
            self.system = guard(|| {
                let mut tts = tts::Tts::default().ok()?;
                configure(&mut tts);
                Some(tts)
            })
            .flatten();
        }
        if let Some(tts) = self.system.as_mut() {
            guard(|| {
                let _ = tts.speak(text, interrupt);
            });
        }
    }

    /// Stop anything being spoken (the lesson changed, or the window closed).
    pub fn stop(&mut self) {
        if let Some(neural) = self.neural.as_ref() {
            neural.stop();
        }
        if let Some(tts) = self.system.as_mut() {
            guard(|| {
                let _ = tts.stop();
            });
        }
    }

    /// `(started, done)` spoken-line counts, for pacing the demo to the neural voice; `None` when
    /// the system voice is in use (it has no synthesis lag to pace around) or no voice has spoken.
    pub fn progress(&self) -> Option<(u64, u64)> {
        self.neural.as_ref().filter(|n| !n.failed()).map(crate::narration_neural::Neural::progress)
    }
}

/// Tune a fresh synthesizer toward a natural narration voice: pick the best installed voice and
/// read a little slower than the default. Every step is best-effort; an unsupported one is ignored.
#[cfg(not(target_arch = "wasm32"))]
fn configure(tts: &mut tts::Tts) {
    if let Ok(voices) = tts.voices()
        && let Some(best) = voices.iter().max_by_key(|v| voice_score(v))
        && voice_score(best) > 0
    {
        let _ = tts.set_voice(best);
    }
    // A touch slower than normal reads as less rushed and less robotic.
    let (normal, min) = (tts.normal_rate(), tts.min_rate());
    let rate = normal - (normal - min) * 0.12;
    if rate.is_finite() {
        let _ = tts.set_rate(rate);
    }
}

/// Rank a voice for narration: a "natural"/neural voice beats a plain one, and an English voice
/// beats a non-English one (the lines are English). 0 means nothing to prefer.
#[cfg(not(target_arch = "wasm32"))]
fn voice_score(v: &tts::Voice) -> i32 {
    let name = v.name().to_lowercase();
    let natural = name.contains("natural") || name.contains("neural");
    let english = v.language().to_string().to_lowercase().starts_with("en");
    i32::from(natural) * 2 + i32::from(english)
}

/// Run a synthesizer call so a panic inside the third-party speech code can never escape (the same
/// care the kernel takes around its own third-party calls). The result is discarded on a panic.
#[cfg(not(target_arch = "wasm32"))]
fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

/// wasm has no speech synthesizer here: every call is a no-op so the UI crate still builds for the
/// browser.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct Narrator;

#[cfg(target_arch = "wasm32")]
impl Narrator {
    pub fn speak(&mut self, _text: &str, _interrupt: bool) -> Option<u64> {
        None
    }
    pub fn stop(&mut self) {}
    pub fn progress(&self) -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn intro_gives_context_explanation_and_shortcut() {
        assert_eq!(
            intro("Extrude", "Pulls a sketch profile into a solid.", "Model \u{203a} Create", Some("E")),
            "Let's look at Extrude, in the Model tab's Create panel. Pulls a sketch profile into a solid. \
             Its keyboard shortcut is E. Watch as I perform it on a sample model."
        );
        // No group, no shortcut, and an explanation missing its full stop.
        assert_eq!(intro("Box", "A simple box", "", None), "Let's look at Box. A simple box. Watch as I perform it on a sample model.");
    }

    #[test]
    fn a_step_cue_leads_with_an_ordinal_and_speaks_the_typed_value() {
        assert_eq!(
            step_cue("Extrude", &json!({"distance": 20}), true, true),
            "Now, I'll use the Extrude tool, and enter a distance of 20 millimetres."
        );
        assert_eq!(
            step_cue("Fillet", &json!({"radius": 4.0, "edges": [[0, 0, 0]]}), true, false),
            "First, I'll use the Fillet tool, and enter a radius of 4 millimetres."
        );
        assert_eq!(
            step_cue("Chamfer", &json!({"angle": "30 deg"}), false, true),
            "Finally, I'll use the Chamfer tool, and enter an angle of 30 degrees."
        );
        // Nothing is typed for a command picked only on the model.
        assert_eq!(step_cue("Combine", &json!({"bodies": ["A", "B"]}), false, false), "Next, I'll use the Combine tool.");
    }

    #[test]
    fn the_result_reads_as_a_sentence_with_units_and_tidy_numbers() {
        assert_eq!(result(""), "And that completes it.");
        assert_eq!(
            result("Volume: 3000.00 mm\u{b3}\nSize: 20.0 \u{d7} 15.0 \u{d7} 10.0 mm"),
            "And there's the result: a volume of 3000 cubic millimetres, measuring 20 by 15 by 10 millimetres."
        );
        assert_eq!(result("Mass: 12.50 g"), "And there's the result: a mass of 12.5 grams.");
    }
}
