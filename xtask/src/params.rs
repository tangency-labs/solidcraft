//! The command registry's parameter notation, read into the rows of a parameter table.
//!
//! The engine writes a command's parameters as one line: `name: type (what it does)`,
//! separated by `;`, with `,` between a name and the next. A `?` marks a parameter
//! optional, a `|` between names offers alternative names for one parameter, and a `|`
//! after the colon lists the types a parameter accepts. Text after `→` is the reply the
//! command gives, not a parameter.

/// One parameter of a command, as the registry's notation describes it.
pub struct Param {
    /// The name the command takes the parameter under.
    pub name: String,
    /// The type the registry writes after the colon, empty when it writes none.
    pub kind: String,
    /// Whether the command needs the parameter.
    pub required: bool,
    /// What the registry writes in parentheses, empty when it writes none.
    pub description: String,
}

/// Words that join two parameters in one clause, as in `thin?: true (...) with curves?: [...]`.
const CONNECTORS: &[&str] = &["and", "or", "with", "then", "also", "plus"];

/// The parameters of a command, in the order the registry lists them.
pub fn parse(notation: &str) -> Vec<Param> {
    let mut params = Vec::new();
    for clause in split_top(cut_at_reply(notation), ';') {
        let entries = clause_entries(clause);
        let shared = shared_kind(&entries);
        for entry in entries {
            let kind = if entry.kind.is_empty() { shared.clone().unwrap_or_default() } else { entry.kind.clone() };
            for (name, optional) in entry.names {
                params.push(Param { name: name.to_string(), kind: kind.clone(), required: !optional, description: entry.description.clone() });
            }
        }
    }
    params
}

/// One clause of the notation: the names it lists, the type they share and what it says.
struct Entry<'a> {
    /// The names the parameter goes by, and whether the command needs it.
    names: Vec<(&'a str, bool)>,
    /// The type written after the colon, empty when the clause writes none.
    kind: String,
    /// What the clause writes in parentheses.
    description: String,
}

/// The type a clause gives to the bare names it lists with it, as in `length, width, height: expr`.
fn shared_kind(entries: &[Entry<'_>]) -> Option<String> {
    let (last, rest) = entries.split_last()?;
    if last.kind.is_empty() || rest.iter().any(|entry| !entry.kind.is_empty() || !entry.description.is_empty()) {
        return None;
    }
    Some(last.kind.clone())
}

/// The entries of one clause, or none when the clause is prose rather than parameters.
fn clause_entries(clause: &str) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    for piece in split_top(clause, ',') {
        let mut parsed = piece_entries(piece);
        if parsed.is_empty() {
            // A clause that reads as prose names no parameter.
            return Vec::new();
        }
        entries.append(&mut parsed);
    }
    entries
}

/// The entries of one comma-separated piece of a clause.
fn piece_entries(piece: &str) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    let mut i = 0;
    while i < piece.len() {
        i = skip_ws(piece, i);
        if i >= piece.len() {
            break;
        }
        if piece[i..].starts_with('|') {
            i += 1;
            continue;
        }
        if let Some(next) = connector_before(piece, i) {
            i = next;
            continue;
        }
        let Some((name, optional, after)) = read_name(piece, i) else { break };
        let mut names = vec![(name, optional)];
        let mut cursor = after;
        // A pipe between names offers alternative names for one parameter.
        loop {
            let k = skip_ws(piece, cursor);
            if !piece[k..].starts_with('|') {
                break;
            }
            let k = skip_ws(piece, k + 1);
            match read_name(piece, k) {
                Some((other, other_optional, other_after)) => {
                    names.push((other, other_optional));
                    cursor = other_after;
                }
                None => break,
            }
        }
        // The group is optional when any of its names is.
        let optional = names.iter().any(|(_, optional)| *optional);
        let names: Vec<(&str, bool)> = names.into_iter().map(|(name, _)| (name, optional)).collect();
        let mut kind = String::new();
        let mut description = String::new();
        let k = skip_ws(piece, cursor);
        if piece[k..].starts_with(':') {
            let (types, descriptions, after) = read_type(piece, k + 1);
            kind = types;
            description = descriptions.join("; ");
            cursor = after;
        } else if piece[k..].starts_with('(') {
            let Some((content, after)) = read_group(piece, k) else { break };
            description = content.trim().to_string();
            cursor = after;
        } else {
            // A bare name: the piece must end here, or it is prose.
            let end = skip_ws(piece, cursor);
            if end < piece.len() && !piece[end..].starts_with('|') {
                break;
            }
        }
        entries.push(Entry { names, kind, description });
        i = cursor;
    }
    entries
}

/// The index of the parameter a connector word introduces, when there is one.
fn connector_before(text: &str, i: usize) -> Option<usize> {
    let (word, after) = read_ident(text, i)?;
    if !CONNECTORS.contains(&word) {
        return None;
    }
    let next = skip_ws(text, after);
    spec_head(text, next).then_some(next)
}

/// The type a clause writes after its colon, the descriptions it writes with it, and the
/// index after them. The type keeps the notation's own spelling.
fn read_type(text: &str, start: usize) -> (String, Vec<String>, usize) {
    let mut kind = String::new();
    let mut descriptions = Vec::new();
    let mut segment = start;
    let mut i = start;
    loop {
        let (stop, at) = scan_type(text, i);
        kind.push_str(&text[segment..at]);
        match stop {
            Stop::Description => {
                let Some((content, after)) = read_group(text, at) else {
                    return (kind.trim().to_string(), descriptions, at);
                };
                descriptions.push(content.trim().to_string());
                // The description ends the type, unless another alternative follows it.
                let next = skip_ws(text, after);
                if next < text.len() && text[next..].starts_with('|') {
                    let following = skip_ws(text, next + 1);
                    if spec_head(text, following) {
                        return (kind.trim().to_string(), descriptions, next);
                    }
                    kind = kind.trim_end().to_string();
                    segment = next;
                    i = next;
                    continue;
                }
                return (kind.trim().to_string(), descriptions, after);
            }
            Stop::Parameter | Stop::End => return (kind.trim().to_string(), descriptions, at),
        }
    }
}

/// Where the type text of a clause ends.
enum Stop {
    /// A parenthesis opens the description of the parameter.
    Description,
    /// A pipe introduces the next parameter.
    Parameter,
    /// The clause ends.
    End,
}

/// The next stop in the type text, and where it is.
fn scan_type(text: &str, from: usize) -> (Stop, usize) {
    let mut i = from;
    let mut depth = 0usize;
    while i < text.len() {
        let c = char_at(text, i);
        match c {
            '"' => {
                i = skip_quoted(text, i);
                continue;
            }
            '(' | ')' if depth == 0 => return (Stop::Description, i),
            '|' if depth == 0 => {
                let following = skip_ws(text, i + 1);
                if spec_head(text, following) {
                    return (Stop::Parameter, i);
                }
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += c.len_utf8();
    }
    (Stop::End, i)
}

/// The name at `i`, whether it is optional, and the index after it.
fn read_name(text: &str, i: usize) -> Option<(&str, bool, usize)> {
    let (name, after) = read_ident(text, i)?;
    if text[after..].starts_with('?') { Some((name, true, after + 1)) } else { Some((name, false, after)) }
}

/// Whether a parameter starts at `i`.
fn spec_head(text: &str, i: usize) -> bool {
    let Some((_, _, after)) = read_name(text, i) else { return false };
    text[skip_ws(text, after)..].starts_with(':')
}

/// The identifier at `i`, and the index after it.
fn read_ident(text: &str, i: usize) -> Option<(&str, usize)> {
    if !char_at(text, i).is_ascii_lowercase() {
        return None;
    }
    let mut end = i + 1;
    while end < text.len() {
        let c = char_at(text, end);
        if c.is_ascii_alphanumeric() || c == '_' {
            end += 1;
        } else {
            break;
        }
    }
    Some((&text[i..end], end))
}

/// The text of the group at `i`, and the index after it.
fn read_group(text: &str, i: usize) -> Option<(&str, usize)> {
    let opener = char_at(text, i);
    let closer = match opener {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 1usize;
    let mut j = i + opener.len_utf8();
    while j < text.len() {
        let c = char_at(text, j);
        if c == '"' {
            j = skip_quoted(text, j);
            continue;
        }
        if c == opener {
            depth += 1;
        } else if c == closer {
            depth -= 1;
            if depth == 0 {
                return Some((&text[i + opener.len_utf8()..j], j + closer.len_utf8()));
            }
        }
        j += c.len_utf8();
    }
    None
}

/// The pieces of `text` split at the top level on `sep`.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        let c = char_at(text, i);
        match c {
            '"' => {
                i = skip_quoted(text, i);
                continue;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 && c == sep => {
                pieces.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    pieces.push(&text[start..]);
    pieces
}

/// The text before the reply arrow, which names no parameter.
fn cut_at_reply(text: &str) -> &str {
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        let c = char_at(text, i);
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '→' if depth == 0 => return &text[..i],
            _ => {}
        }
        i += c.len_utf8();
    }
    text
}

/// The index after the quoted text at `i`.
fn skip_quoted(text: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < text.len() {
        let c = char_at(text, j);
        if c == '"' {
            return j + 1;
        }
        j += c.len_utf8();
    }
    j
}

/// The first index at or after `i` that is not whitespace.
fn skip_ws(text: &str, i: usize) -> usize {
    let mut j = i;
    while j < text.len() {
        let c = char_at(text, j);
        if !c.is_whitespace() {
            break;
        }
        j += c.len_utf8();
    }
    j
}

/// The character at `i`.
fn char_at(text: &str, i: usize) -> char {
    text.get(i..).and_then(|rest| rest.chars().next()).unwrap_or(char::REPLACEMENT_CHARACTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(notation: &str) -> Vec<(String, String, bool, String)> {
        parse(notation).into_iter().map(|p| (p.name, p.kind, p.required, p.description)).collect()
    }

    #[test]
    fn names_written_apart_share_the_type_that_follows_them() {
        assert_eq!(
            rows("length, width, height: expr"),
            [
                ("length".into(), "expr".into(), true, String::new()),
                ("width".into(), "expr".into(), true, String::new()),
                ("height".into(), "expr".into(), true, String::new()),
            ]
        );
    }

    #[test]
    fn a_question_mark_marks_a_parameter_optional() {
        assert_eq!(
            rows("distance: expr; taper?: angle expr"),
            [("distance".into(), "expr".into(), true, String::new()), ("taper".into(), "angle expr".into(), false, String::new()),]
        );
    }

    #[test]
    fn a_pipe_between_names_offers_alternative_names_for_one_parameter() {
        assert_eq!(
            rows("radius | diameter: expr"),
            [("radius".into(), "expr".into(), true, String::new()), ("diameter".into(), "expr".into(), true, String::new()),]
        );
    }

    #[test]
    fn a_pipe_after_the_colon_lists_the_types_a_parameter_accepts() {
        assert_eq!(
            rows("direction?: positive|negative|symmetric"),
            [("direction".into(), "positive|negative|symmetric".into(), false, String::new())]
        );
    }

    #[test]
    fn a_parenthesis_says_what_the_parameter_does() {
        assert_eq!(
            rows("center?: [x,y,z] (numbers in mm or length expressions; default: the origin)"),
            [("center".into(), "[x,y,z]".into(), false, "numbers in mm or length expressions; default: the origin".into())]
        );
    }

    #[test]
    fn a_reply_after_an_arrow_names_no_parameter() {
        assert_eq!(rows("cloud: id or name → deleted"), [("cloud".into(), "id or name".into(), true, String::new())]);
    }

    #[test]
    fn prose_between_parameters_is_not_a_parameter() {
        assert_eq!(rows("distance: expr; corners are mitred, crossing walls are one"), [("distance".into(), "expr".into(), true, String::new())]);
    }

    #[test]
    fn a_connector_word_introduces_the_next_parameter() {
        assert_eq!(
            rows("thin?: true (a wall along the sketch) with curves?: [curve ids]"),
            [("thin".into(), "true".into(), false, "a wall along the sketch".into()), ("curves".into(), "[curve ids]".into(), false, String::new()),]
        );
    }
}
