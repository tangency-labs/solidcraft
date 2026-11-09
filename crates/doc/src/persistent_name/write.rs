//! Writing names back as text: `Display`.

use std::fmt;

use super::*;

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Top => f.write_str("top"),
            Role::Bottom => f.write_str("bottom"),
            Role::Start => f.write_str("start"),
            Role::End => f.write_str("end"),
            Role::Base => f.write_str("base"),
            Role::Section => f.write_str("section"),
            Role::Wall => f.write_str("side"),
            Role::Side { profile: Some(k), curve } => write!(f, "side:s{k}.{curve}"),
            Role::Side { profile: None, curve } => write!(f, "side:{curve}"),
            Role::BoxFace { axis, high } => {
                let a = match axis {
                    Axis::X => 'x',
                    Axis::Y => 'y',
                    Axis::Z => 'z',
                };
                write!(f, "{}{a}", if *high { '+' } else { '-' })
            }
            Role::Index(k) => write!(f, "face:{k}"),
            Role::Other(t) => f.write_str(t),
        }
    }
}

impl fmt::Display for FaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.base {
            Base::Made { feature, role } => write!(f, "F{}:{role}", feature.0)?,
            Base::Blend { feature, rounds: Rounds::Edge(e) } => write!(f, "F{}:blend:{e}", feature.0)?,
            Base::Blend { feature, rounds: Rounds::Index(k) } => write!(f, "F{}:blend:{k}", feature.0)?,
            Base::Inner { feature, offsets } => write!(f, "F{}:inner:{offsets}", feature.0)?,
            Base::BodyFace { body, index } => write!(f, "{body}:face:{index}")?,
            Base::Other(t) => f.write_str(t)?,
        }
        for s in &self.suffixes {
            match s {
                Suffix::Copy { feature, instance } => write!(f, "@F{}.{instance}", feature.0)?,
                Suffix::Piece(q) => {
                    f.write_str("#[")?;
                    for (i, a) in q.around.iter().enumerate() {
                        if i > 0 {
                            f.write_str(",")?;
                        }
                        write!(f, "{a}")?;
                    }
                    f.write_str("]")?;
                    if let Some(k) = q.tie {
                        write!(f, "#{k}")?;
                    }
                }
                Suffix::Numbered(k) => write!(f, "#{k}")?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for EdgeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, face) in self.faces.iter().enumerate() {
            if i > 0 {
                f.write_str("|")?;
            }
            write!(f, "{face}")?;
        }
        match self.tie {
            Some(k) => write!(f, "~{k}"),
            None => Ok(()),
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::TooLong => write!(f, "a name is at most {MAX_NAME_LEN} bytes"),
            NameError::TooDeep => write!(f, "a name nests at most {MAX_DEPTH} deep"),
            NameError::Expected { at, what } => write!(f, "expected {what} at byte {at}"),
        }
    }
}

impl std::error::Error for NameError {}
