//! The machine-agnostic program: tools, how they turn and where they go.
//! What a backend writes for it — G-code today, another machine language
//! tomorrow — is a [`MachineOutput`] away from the same IR.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::path::Toolpath;
use crate::post::Post;
use crate::tool::Tool;

/// One operation of a program: a tool, how it turns, and where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub tool: Tool,
    /// Spindle speed (rev/min), clockwise.
    pub spindle_rpm: f64,
    pub coolant: bool,
    pub path: Toolpath,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    pub operations: Vec<Operation>,
    /// The work coordinate system the program runs in, numbered 1 to 6. Each
    /// backend maps the number to its own convention (a G-code post writes
    /// `G54` to `G59`); left out, the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_offset: Option<u8>,
}

/// The machine code a backend wrote for a program: the text plus how it is
/// tagged. Backends that speak a binary language will grow a payload beside
/// the text; consumers that need op-to-line maps for progress will find
/// them here too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineCode {
    /// What language the text speaks, lower-case (`"gcode"`).
    pub format: String,
    /// File extension without the dot.
    pub extension: String,
    /// The machine code text.
    pub text: String,
}

/// A backend that writes machine code for a program. Shaped like raygeo's
/// `Encoder` so the two can converge: the G-code post is today's
/// implementation, and read-back verification stays tied to it (it parses
/// the text it wrote).
pub trait MachineOutput {
    /// The backend's name, as a saved post or driver names it.
    fn name(&self) -> &str;
    /// File extension without the dot.
    fn extension(&self) -> &str;
    /// The machine code for `program`.
    fn encode(&self, program: &Program) -> Result<MachineCode>;
}

impl MachineOutput for Post {
    fn name(&self) -> &str {
        &self.name
    }

    fn extension(&self) -> &str {
        &self.extension
    }

    fn encode(&self, program: &Program) -> Result<MachineCode> {
        Ok(MachineCode { format: "gcode".into(), extension: self.extension.clone(), text: crate::gcode::emit(program, self)? })
    }
}
