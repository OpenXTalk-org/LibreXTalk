//! Just enough JSON to describe the engine's answers. The engine only ever
//! writes JSON; replies come in through typed functions instead, so there
//! is no parser to get wrong.

use std::fmt;

pub enum Json {
    Str(String),
    Number(f64),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
    /// Text that is already JSON.
    Raw(String),
}

impl Json {
    pub fn object<'a>(fields: impl IntoIterator<Item = (&'a str, Json)>) -> Json {
        Json::Object(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn strings(items: &[String]) -> Json {
        Json::Array(items.iter().map(|s| Json::from(s.as_str())).collect())
    }
}

impl From<&str> for Json {
    fn from(text: &str) -> Self {
        Json::Str(text.to_string())
    }
}

fn write_str(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in text.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Str(s) => write_str(f, s),
            // Whole numbers print without a fraction; JSON has no NaN.
            Json::Number(n) if n.is_finite() && n.fract() == 0.0 => write!(f, "{}", *n as i64),
            Json::Number(n) if n.is_finite() => write!(f, "{n}"),
            Json::Number(_) => f.write_str("null"),
            Json::Array(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Object(fields) => {
                f.write_str("{")?;
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write_str(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_str("}")
            }
            Json::Raw(text) => f.write_str(text),
        }
    }
}
