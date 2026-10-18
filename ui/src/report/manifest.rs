//! Bounded, borrowed source manifest for build tools and user-space hosts.
use super::*;
use crate::compiler::Diagnostic;
use alloc::{format, vec::Vec};

pub const MAX_MANIFEST_BYTES: usize = 32768;
#[derive(Debug)]
pub struct Manifest<'a> {
    catalog_version: u16,
    wire_version: u16,
    sources: Vec<Source<'a>>,
    fields: Vec<Field<'a>>,
    actions: Vec<Action<'a>>,
}
impl Manifest<'_> {
    pub fn contract(&self) -> Contract<'_> {
        Contract {
            catalog_version: self.catalog_version,
            wire_version: self.wire_version,
            sources: &self.sources,
            fields: &self.fields,
            actions: &self.actions,
        }
    }
}
pub fn parse_manifest(text: &str) -> Result<Manifest<'_>, Diagnostic> {
    let error = |line, message: &str| Diagnostic {
        source: "contract",
        line,
        column: 1,
        message: message.into(),
    };
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(error(1, "manifest byte limit exceeded"));
    }
    let mut result = Manifest {
        catalog_version: 0,
        wire_version: 0,
        sources: Vec::new(),
        fields: Vec::new(),
        actions: Vec::new(),
    };
    let mut header = false;
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Fixed slots reject excess tokens without allocating attacker-sized vectors.
        let mut tokens = line.split_ascii_whitespace();
        let parts = [tokens.next(), tokens.next(), tokens.next(), tokens.next()];
        if tokens.next().is_some() {
            return Err(error(line_number, "too many manifest tokens"));
        }
        if !header {
            match parts {
                [Some("report"), Some(catalog), Some(wire), None] => {
                    result.catalog_version = catalog
                        .parse()
                        .map_err(|_| error(line_number, "invalid catalog version"))?;
                    result.wire_version = wire
                        .parse()
                        .map_err(|_| error(line_number, "invalid wire version"))?;
                    if result.catalog_version != catalog::VERSION
                        || result.wire_version != crate::VERSION
                    {
                        return Err(error(line_number, "unsupported catalog or wire version"));
                    }
                    header = true;
                    continue;
                }
                _ => {
                    return Err(error(
                        line_number,
                        "expected: report <catalog-version> <wire-version>",
                    ))
                }
            }
        }
        match parts {
            [Some("source"), Some(name), Some(shape), Some(version)] => {
                if result.sources.len() == MAX_SOURCES {
                    return Err(error(line_number, "source limit exceeded"));
                }
                let shape = match shape {
                    "record" => Shape::Record,
                    "rows" => Shape::Rows,
                    _ => return Err(error(line_number, "source requires record or rows")),
                };
                let schema_version = version
                    .parse()
                    .map_err(|_| error(line_number, "invalid source schema version"))?;
                result.sources.push(Source {
                    name,
                    shape,
                    schema_version,
                });
            }
            [Some("field"), Some(name), Some(source), Some(data_type)] => {
                if result.fields.len() == MAX_FIELDS {
                    return Err(error(line_number, "field limit exceeded"));
                }
                let data_type = DataType::parse(data_type)
                    .ok_or_else(|| error(line_number, "unknown data type"))?;
                result.fields.push(Field {
                    name,
                    source,
                    data_type,
                });
            }
            [Some("action"), Some(name), Some(kind), None] => {
                if result.actions.len() == MAX_ACTIONS {
                    return Err(error(line_number, "action limit exceeded"));
                }
                let kind = ActionKind::parse(kind)
                    .ok_or_else(|| error(line_number, "unknown report action"))?;
                result.actions.push(Action { name, kind });
            }
            _ => return Err(error(line_number, "invalid manifest declaration")),
        }
        // Forward references to sources are accepted; validate at the end.
    }
    if !header {
        return Err(error(1, "missing report header"));
    }
    result
        .contract()
        .validate()
        .map_err(|e| error(1, &format!("invalid report contract: {e:?}")))?;
    Ok(result)
}
