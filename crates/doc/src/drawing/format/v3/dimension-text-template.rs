//! A dimension's text (`override_text`) is a template: `{value}` stands for its measured number
//! as it reads alone (Ø6.00), so a text such as `2× {value} THRU` follows the model, and `{{` and `}}` are a brace each. An
//! older drawing's text was shown character for character, so its braces are doubled as it opens
//! and it shows what it showed. The last release opens a drawing saved with a template with a
//! warning about the newer format, and shows the template's characters as they are.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(dimensions) = v.get_mut("dimensions").and_then(Value::as_array_mut) else { return Ok(()) };
    for dimension in dimensions {
        if let Some(text) = dimension.get("override_text").and_then(Value::as_str)
            && text.contains(['{', '}'])
        {
            dimension["override_text"] = Value::String(crate::drawing::dimension::text_literal(text));
        }
    }
    Ok(())
}
