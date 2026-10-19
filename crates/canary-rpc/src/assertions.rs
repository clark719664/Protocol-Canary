//! Nested path resolution for RPC response assertions (CA-01 §4.3).
//!
//! Supports both:
//! - Structured `path` arrays from TOML (`path = ["sorobanInclusionFee", "p50"]`
//!   or `path = ["entries", 0, "liveUntilLedgerSeq"]`) per CA-01 §4.3, with
//!   maximum depth [`MAX_PATH_DEPTH`] (8).
//! - Dot-and-bracket path strings (`"result.fee.max"`, `"items[0].id"`) for
//!   programmatic resolution via [`resolve_path_str`] and [`resolve_path`].

use serde_json::Value as JsonValue;
use thiserror::Error;

/// Maximum number of elements allowed in a CA-01 §4.3 `path` array.
pub const MAX_PATH_DEPTH: usize = 8;

/// One literal step in a response path (CA-01 §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// An object key lookup.
    Key(String),
    /// A zero-based array index lookup.
    Index(usize),
}

impl std::fmt::Display for PathSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSegment::Key(k) => write!(f, "{k:?}"),
            PathSegment::Index(i) => write!(f, "[{i}]"),
        }
    }
}

/// Formats a slice of [`PathSegment`]s into a human-readable target name for
/// assertion failure diagnostics.
pub fn format_path(segments: &[PathSegment]) -> String {
    let mut out = String::new();
    for (idx, seg) in segments.iter().enumerate() {
        match seg {
            PathSegment::Key(key) => {
                if idx > 0 {
                    out.push('.');
                }
                out.push_str(key);
            }
            PathSegment::Index(i) => {
                out.push_str(&format!("[{i}]"));
            }
        }
    }
    out
}

/// Errors produced when parsing or walking a nested JSON path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathResolutionError {
    /// Path syntax is empty or has malformed bracket notation.
    #[error("invalid path syntax {path:?}: {reason}")]
    InvalidSyntax { path: String, reason: String },

    /// Path exceeds [`MAX_PATH_DEPTH`] elements.
    #[error("path has {depth} elements, exceeding maximum depth of {max}")]
    DepthExceeded { depth: usize, max: usize },

    /// An object key segment was not present on the target JSON object.
    #[error("field {segment:?} not found at {at}")]
    FieldNotFound { segment: String, at: String },

    /// An array index segment was out of bounds for the target JSON array.
    #[error("array index [{index}] out of bounds (len {len}) at {at}")]
    IndexOutOfBounds {
        index: usize,
        len: usize,
        at: String,
    },

    /// Attempted to look up a key or array index on a JSON value of the wrong
    /// kind (for example indexing into a non-array or accessing a property on
    /// a primitive/null value).
    #[error("cannot access {segment} on {actual_type} value at {at}")]
    TypeMismatch {
        segment: String,
        actual_type: &'static str,
        at: String,
    },
}

fn json_kind_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// Parses a `TOML` `path = [...]` value per CA-01 §4.3.
///
/// Elements must be strings (object keys) or non-negative integers (array
/// indices), and the array must contain between 1 and [`MAX_PATH_DEPTH`]
/// elements inclusive.
pub fn parse_toml_path(value: &toml::Value) -> Result<Vec<PathSegment>, String> {
    let arr = value.as_array().ok_or_else(|| {
        "\"path\" must be an array of string keys or non-negative integer indices".to_string()
    })?;
    if arr.is_empty() {
        return Err("\"path\" must not be empty".to_string());
    }
    if arr.len() > MAX_PATH_DEPTH {
        return Err(format!(
            "\"path\" has {} elements, exceeding the maximum depth of {MAX_PATH_DEPTH}",
            arr.len()
        ));
    }
    let mut segments = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        match item {
            toml::Value::String(s) => segments.push(PathSegment::Key(s.clone())),
            toml::Value::Integer(n) if *n >= 0 => {
                let u = usize::try_from(*n)
                    .map_err(|_| format!("path element at index {idx} ({n}) overflows usize"))?;
                segments.push(PathSegment::Index(u));
            }
            toml::Value::Integer(n) => {
                return Err(format!(
                    "path element at index {idx} ({n}) must be a non-negative integer"
                ));
            }
            other => {
                return Err(format!(
                    "path element at index {idx} must be a string key or non-negative integer index, found {}",
                    other.type_str()
                ));
            }
        }
    }
    Ok(segments)
}

/// Parses a dot-and-bracket path string (such as `"result.fee.max"` or
/// `"items[0].id"`) into [`PathSegment`]s.
pub fn parse_path_str(path: &str) -> Result<Vec<PathSegment>, PathResolutionError> {
    if path.is_empty() {
        return Err(PathResolutionError::InvalidSyntax {
            path: path.to_string(),
            reason: "path must not be empty".to_string(),
        });
    }

    let mut segments = Vec::new();
    for raw_part in path.split('.') {
        if raw_part.is_empty() {
            return Err(PathResolutionError::InvalidSyntax {
                path: path.to_string(),
                reason: "empty dot segment".to_string(),
            });
        }

        let mut rest = raw_part;
        if !rest.starts_with('[') {
            let key_end = rest.find('[').unwrap_or(rest.len());
            let key = &rest[..key_end];
            if key.contains(']') {
                return Err(PathResolutionError::InvalidSyntax {
                    path: path.to_string(),
                    reason: format!("unmatched ']' in segment {raw_part:?}"),
                });
            }
            segments.push(PathSegment::Key(key.to_string()));
            rest = &rest[key_end..];
        }

        while !rest.is_empty() {
            if !rest.starts_with('[') {
                return Err(PathResolutionError::InvalidSyntax {
                    path: path.to_string(),
                    reason: format!("unexpected characters {rest:?} after bracket"),
                });
            }
            let Some(close_idx) = rest.find(']') else {
                return Err(PathResolutionError::InvalidSyntax {
                    path: path.to_string(),
                    reason: format!("unclosed '[' in segment {raw_part:?}"),
                });
            };
            let idx_str = &rest[1..close_idx];
            let index: usize = idx_str
                .parse()
                .map_err(|_| PathResolutionError::InvalidSyntax {
                    path: path.to_string(),
                    reason: format!("invalid array index {idx_str:?}"),
                })?;
            segments.push(PathSegment::Index(index));
            rest = &rest[close_idx + 1..];
        }
    }

    if segments.len() > MAX_PATH_DEPTH {
        return Err(PathResolutionError::DepthExceeded {
            depth: segments.len(),
            max: MAX_PATH_DEPTH,
        });
    }

    Ok(segments)
}

/// Walks `segments` in `root`, returning either the resolved `&JsonValue` or a
/// descriptive [`PathResolutionError`] identifying the exact segment that
/// failed.
pub fn resolve_segments<'a>(
    root: &'a JsonValue,
    segments: &[PathSegment],
) -> Result<&'a JsonValue, PathResolutionError> {
    if segments.is_empty() {
        return Err(PathResolutionError::InvalidSyntax {
            path: String::new(),
            reason: "path must not be empty".to_string(),
        });
    }
    if segments.len() > MAX_PATH_DEPTH {
        return Err(PathResolutionError::DepthExceeded {
            depth: segments.len(),
            max: MAX_PATH_DEPTH,
        });
    }

    let mut current = root;
    for (idx, segment) in segments.iter().enumerate() {
        let at = if idx == 0 {
            "<root>".to_string()
        } else {
            format_path(&segments[..idx])
        };

        match segment {
            PathSegment::Key(key) => match current {
                JsonValue::Object(map) => match map.get(key) {
                    Some(next) => current = next,
                    None => {
                        return Err(PathResolutionError::FieldNotFound {
                            segment: key.clone(),
                            at,
                        });
                    }
                },
                other => {
                    return Err(PathResolutionError::TypeMismatch {
                        segment: format!("key {key:?}"),
                        actual_type: json_kind_name(other),
                        at,
                    });
                }
            },
            PathSegment::Index(i) => match current {
                JsonValue::Array(items) => match items.get(*i) {
                    Some(next) => current = next,
                    None => {
                        return Err(PathResolutionError::IndexOutOfBounds {
                            index: *i,
                            len: items.len(),
                            at,
                        });
                    }
                },
                other => {
                    return Err(PathResolutionError::TypeMismatch {
                        segment: format!("index [{i}]"),
                        actual_type: json_kind_name(other),
                        at,
                    });
                }
            },
        }
    }

    Ok(current)
}

/// Resolves a dot-and-bracket path string (e.g. `"result.fee.max"` or
/// `"items[0].id"`) against `root`.
pub fn resolve_path<'a>(
    root: &'a JsonValue,
    path: &str,
) -> Result<&'a JsonValue, PathResolutionError> {
    let segments = parse_path_str(path)?;
    resolve_segments(root, &segments)
}

/// Alias for [`resolve_path`] for callers that distinguish string paths from
/// pre-parsed segment slices.
pub fn resolve_path_str<'a>(
    root: &'a JsonValue,
    path: &str,
) -> Result<&'a JsonValue, PathResolutionError> {
    resolve_path(root, path)
}
