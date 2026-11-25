//! Owned shared command data. Only public Vec/String capacities are retained;
//! no serde_json object allocation survives conversion into the plan.
use crate::{EngineError, Result};
use serde_json::{Number, Value};
use solidcraft_render::job::CancelToken;
use std::mem::size_of;

fn bad(message: &str) -> EngineError {
    EngineError::Other(message.into())
}
fn add(a: u64, b: usize) -> Result<u64> {
    a.checked_add(u64::try_from(b).map_err(|_| bad("batch payload byte overflow"))?).ok_or_else(|| bad("batch payload byte overflow"))
}
#[derive(Clone, Copy)]
enum Numeric {
    Signed(i64),
    Unsigned(u64),
    Float(u64),
}
impl Numeric {
    fn capture(value: Number) -> Result<Self> {
        if value.is_i64() {
            return value.as_i64().map(Self::Signed).ok_or_else(|| bad("batch signed number"));
        }
        if value.is_u64() {
            return value.as_u64().map(Self::Unsigned).ok_or_else(|| bad("batch unsigned number"));
        }
        if value.is_f64() {
            return value.as_f64().filter(|n| n.is_finite()).map(|n| Self::Float(n.to_bits())).ok_or_else(|| bad("batch finite number"));
        }
        Err(bad("batch command number is not representable without changing its value"))
    }
    fn value(self) -> Result<Value> {
        Ok(Value::Number(match self {
            Self::Signed(v) => v.into(),
            Self::Unsigned(v) => v.into(),
            Self::Float(v) => Number::from_f64(f64::from_bits(v)).ok_or_else(|| bad("batch stored number"))?,
        }))
    }
}
enum Node {
    Pending(Value),
    Null,
    Bool(bool),
    Number(Numeric),
    String(String),
    Array { begin: usize, end: usize },
    Object { begin: usize, end: usize },
}
struct Edge {
    key: Option<String>,
    child: usize,
}
#[derive(Clone, Copy)]
pub(crate) struct Counts {
    nodes: usize,
    edges: usize,
    strings: u64,
}
impl Counts {
    pub(crate) fn bytes(self) -> Result<u64> {
        let bytes = self
            .nodes
            .checked_mul(size_of::<Node>())
            .and_then(|n| self.edges.checked_mul(size_of::<Edge>()).and_then(|e| n.checked_add(e)))
            .ok_or_else(|| bad("batch arena storage overflow"))?;
        add(self.strings, bytes)
    }
}

// This is the existing serde_json command parser nesting envelope, not a
// material/light/shot count. Valid typed rendering commands are shallower.
const JSON_DEPTH: usize = 128;
enum Frame<'a> {
    Array(std::slice::Iter<'a, Value>),
    Object(serde_json::map::Iter<'a>),
}
fn cancelled(cancel: &CancelToken) -> Result<()> {
    cancel.check().map_err(crate::render_jobs::job_error)
}
pub(crate) fn count(value: &Value, cancel: &CancelToken) -> Result<Counts> {
    let mut counts = Counts { nodes: 0, edges: 0, strings: 0 };
    let mut stack: [Option<Frame<'_>>; JSON_DEPTH] = std::array::from_fn(|_| None);
    let mut depth = 0;
    let mut next = Some(value);
    loop {
        cancelled(cancel)?;
        if let Some(value) = next.take() {
            counts.nodes = counts.nodes.checked_add(1).ok_or_else(|| bad("batch arena node overflow"))?;
            match value {
                Value::String(v) => counts.strings = add(counts.strings, v.capacity())?,
                Value::Array(v) => {
                    counts.edges = counts.edges.checked_add(v.len()).ok_or_else(|| bad("batch arena edge overflow"))?;
                    *stack.get_mut(depth).ok_or_else(|| bad("batch request exceeds the existing JSON nesting envelope"))? =
                        Some(Frame::Array(v.iter()));
                    depth += 1;
                }
                Value::Object(v) => {
                    counts.edges = counts.edges.checked_add(v.len()).ok_or_else(|| bad("batch arena edge overflow"))?;
                    *stack.get_mut(depth).ok_or_else(|| bad("batch request exceeds the existing JSON nesting envelope"))? =
                        Some(Frame::Object(v.iter()));
                    depth += 1;
                }
                _ => {}
            }
        }
        while depth != 0 && next.is_none() {
            match stack.get_mut(depth - 1).and_then(Option::as_mut).ok_or_else(|| bad("batch arena traversal"))? {
                Frame::Array(iter) => next = iter.next(),
                Frame::Object(iter) => {
                    if let Some((key, value)) = iter.next() {
                        counts.strings = add(counts.strings, key.capacity())?;
                        next = Some(value);
                    }
                }
            }
            if next.is_none() {
                *stack.get_mut(depth - 1).ok_or_else(|| bad("batch arena traversal pop"))? = None;
                depth -= 1;
            }
        }
        if next.is_none() {
            return Ok(counts);
        }
    }
}

pub(crate) struct Payload {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    strings: u64,
}
impl Payload {
    /// Caller reserves Counts::bytes plus its fixed plan/rows/scope backing
    /// before this new bulk allocation. The old command Value is caller-owned
    /// metadata and is dismantled here on the preparation worker.
    pub(crate) fn new(value: Value, counts: Counts, cancel: &CancelToken) -> Result<Self> {
        cancelled(cancel)?;
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        nodes.try_reserve_exact(counts.nodes).map_err(|_| bad("could not allocate batch arena nodes"))?;
        edges.try_reserve_exact(counts.edges).map_err(|_| bad("could not allocate batch arena edges"))?;
        if nodes.capacity() != counts.nodes || edges.capacity() != counts.edges {
            return Err(bad("batch arena capacity exceeds its reservation"));
        }
        nodes.push(Node::Pending(value));
        let mut at = 0;
        while at < nodes.len() {
            cancelled(cancel)?;
            let slot = nodes.get_mut(at).ok_or_else(|| bad("batch arena pending node"))?;
            let Node::Pending(value) = std::mem::replace(slot, Node::Null) else {
                return Err(bad("batch arena node already resolved"));
            };
            let begin = edges.len();
            let resolved = match value {
                Value::Null => Node::Null,
                Value::Bool(v) => Node::Bool(v),
                Value::Number(v) => Node::Number(Numeric::capture(v)?),
                Value::String(v) => Node::String(v),
                Value::Array(values) => {
                    for value in values {
                        cancelled(cancel)?;
                        if nodes.len() == nodes.capacity() || edges.len() == edges.capacity() {
                            return Err(bad("batch arena array exceeds reservation"));
                        }
                        let child = nodes.len();
                        nodes.push(Node::Pending(value));
                        edges.push(Edge { key: None, child });
                    }
                    Node::Array { begin, end: edges.len() }
                }
                Value::Object(values) => {
                    for (key, value) in values {
                        cancelled(cancel)?;
                        if nodes.len() == nodes.capacity() || edges.len() == edges.capacity() {
                            return Err(bad("batch arena object exceeds reservation"));
                        }
                        let child = nodes.len();
                        nodes.push(Node::Pending(value));
                        edges.push(Edge { key: Some(key), child });
                    }
                    Node::Object { begin, end: edges.len() }
                }
            };
            *nodes.get_mut(at).ok_or_else(|| bad("batch arena resolved slot"))? = resolved;
            at += 1;
        }
        if nodes.len() != counts.nodes || edges.len() != counts.edges {
            return Err(bad("batch arena accounting disagrees with conversion"));
        }
        Ok(Self { nodes, edges, strings: counts.strings })
    }
    pub(crate) fn heap_bytes(&self) -> Result<u64> {
        add(
            self.strings,
            self.nodes
                .capacity()
                .checked_mul(size_of::<Node>())
                .and_then(|n| self.edges.capacity().checked_mul(size_of::<Edge>()).and_then(|e| n.checked_add(e)))
                .ok_or_else(|| bad("batch arena actual capacity overflow"))?,
        )
    }
    /// Exactly one per-shot raw command Value is reconstructed on the worker.
    /// Like individual command metadata it remains caller-owned, is moved into
    /// Prepared.params, and retires on that worker. It is not decoded renderer
    /// storage or a second retained common batch request.
    pub(crate) fn value(&self, cancel: &CancelToken) -> Result<Value> {
        self.node_value(0, 0, cancel)
    }
    fn node_value(&self, at: usize, depth: usize, cancel: &CancelToken) -> Result<Value> {
        cancelled(cancel)?;
        if depth > JSON_DEPTH {
            return Err(bad("batch arena stored nesting"));
        }
        let copy = |text: &str| -> Result<String> {
            let mut value = String::new();
            value.try_reserve_exact(text.len()).map_err(|_| bad("could not copy batch command text"))?;
            value.push_str(text);
            Ok(value)
        };
        Ok(match self.nodes.get(at).ok_or_else(|| bad("batch arena stored index"))? {
            Node::Null => Value::Null,
            Node::Bool(v) => Value::Bool(*v),
            Node::Number(v) => v.value()?,
            Node::String(v) => Value::String(copy(v)?),
            Node::Array { begin, end } => {
                let entries = self.edges.get(*begin..*end).ok_or_else(|| bad("batch arena array range"))?;
                let mut values = Vec::new();
                values.try_reserve_exact(entries.len()).map_err(|_| bad("could not copy batch command array"))?;
                for edge in entries {
                    values.push(self.node_value(edge.child, depth + 1, cancel)?);
                }
                Value::Array(values)
            }
            Node::Object { begin, end } => {
                let entries = self.edges.get(*begin..*end).ok_or_else(|| bad("batch arena object range"))?;
                let mut values = serde_json::Map::new();
                for edge in entries {
                    let key = edge.key.as_deref().ok_or_else(|| bad("batch arena object key"))?;
                    values.insert(copy(key)?, self.node_value(edge.child, depth + 1, cancel)?);
                }
                Value::Object(values)
            }
            Node::Pending(_) => return Err(bad("batch arena conversion incomplete")),
        })
    }
}

#[cfg(test)]
pub(crate) fn test_element_bytes() -> (usize, usize) {
    (size_of::<Node>(), size_of::<Edge>())
}
#[cfg(test)]
impl Payload {
    pub(crate) fn test_capacities(&self) -> (usize, usize, u64) {
        (self.nodes.capacity(), self.edges.capacity(), self.strings)
    }
}
