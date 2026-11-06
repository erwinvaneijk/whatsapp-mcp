//! Reads tool arguments the way the Python server's pydantic does, in lax
//! mode: an integer may also come as a whole float or as text (`"5"`), a
//! yes/no value as `0`/`1` or text (`"true"`, `"yes"`, `"off"`, …). Unknown
//! arguments are ignored.

use serde_json::{Map, Value};

/// Why the arguments cannot be used; the message names the argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadArgument(pub String);

pub struct Args<'a> {
    values: Option<&'a Map<String, Value>>,
}

impl<'a> Args<'a> {
    pub fn new(values: Option<&'a Map<String, Value>>) -> Self {
        Args { values }
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        self.values?.get(name)
    }

    /// A required text argument.
    pub fn string(&self, name: &str) -> Result<String, BadArgument> {
        match self.get(name) {
            None => Err(BadArgument(format!("'{name}' is required"))),
            Some(value) => as_string(name, value),
        }
    }

    /// Optional text; `null` or absent is `None`.
    pub fn opt_string(&self, name: &str) -> Result<Option<String>, BadArgument> {
        match self.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => as_string(name, value).map(Some),
        }
    }

    /// Text with a default.
    pub fn string_or(&self, name: &str, default: &str) -> Result<String, BadArgument> {
        match self.get(name) {
            None => Ok(default.to_string()),
            Some(value) => as_string(name, value),
        }
    }

    /// An integer with a default.
    pub fn int_or(&self, name: &str, default: i64) -> Result<i64, BadArgument> {
        let Some(value) = self.get(name) else {
            return Ok(default);
        };
        let bad = || BadArgument(format!("'{name}' must be a whole number"));
        match value {
            Value::Number(n) => match (n.as_i64(), n.as_f64()) {
                (Some(i), _) => Ok(i),
                (None, Some(f)) if f.fract() == 0.0 && f.abs() < 9.0e15 => Ok(f as i64),
                _ => Err(bad()),
            },
            Value::String(s) => s.trim().parse().map_err(|_| bad()),
            Value::Bool(b) => Ok(i64::from(*b)),
            _ => Err(bad()),
        }
    }

    /// A yes/no value with a default.
    pub fn bool_or(&self, name: &str, default: bool) -> Result<bool, BadArgument> {
        let Some(value) = self.get(name) else {
            return Ok(default);
        };
        let bad = || BadArgument(format!("'{name}' must be true or false"));
        match value {
            Value::Bool(b) => Ok(*b),
            Value::Number(n) => match n.as_f64() {
                Some(0.0) => Ok(false),
                Some(1.0) => Ok(true),
                _ => Err(bad()),
            },
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "1" | "on" | "t" | "true" | "y" | "yes" => Ok(true),
                "0" | "off" | "f" | "false" | "n" | "no" => Ok(false),
                _ => Err(bad()),
            },
            _ => Err(bad()),
        }
    }

    /// An optional list of text; `null` or absent is `None`.
    pub fn opt_string_list(&self, name: &str) -> Result<Option<Vec<String>>, BadArgument> {
        match self.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| as_string(name, item))
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(BadArgument(format!("'{name}' must be a list of text"))),
        }
    }
}

fn as_string(name: &str, value: &Value) -> Result<String, BadArgument> {
    match value {
        Value::String(s) => Ok(s.clone()),
        _ => Err(BadArgument(format!("'{name}' must be text"))),
    }
}

/// SQLite treats a negative LIMIT as "no limit", so a stray -1 would dump
/// the whole table instead of failing.
pub fn check_non_negative(values: &[(&str, i64)]) -> Result<(), BadArgument> {
    for (name, value) in values {
        if *value < 0 {
            return Err(BadArgument(format!(
                "'{name}' must be 0 or greater, got {value}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn lax_values() {
        let m = args(json!({"a": "5", "b": 5.0, "c": "yes", "d": 0, "e": null, "f": ["x"]}));
        let a = Args::new(Some(&m));
        assert_eq!(a.int_or("a", 1), Ok(5));
        assert_eq!(a.int_or("b", 1), Ok(5));
        assert_eq!(a.int_or("missing", 7), Ok(7));
        assert_eq!(a.bool_or("c", false), Ok(true));
        assert_eq!(a.bool_or("d", true), Ok(false));
        assert_eq!(a.opt_string("e"), Ok(None));
        assert_eq!(a.opt_string_list("f"), Ok(Some(vec!["x".into()])));
        assert!(a.string("missing").is_err());
        assert!(a.int_or("c", 1).is_err());
    }

    #[test]
    fn negative_values_are_refused() {
        assert_eq!(check_non_negative(&[("limit", 0), ("page", 3)]), Ok(()));
        assert_eq!(
            check_non_negative(&[("limit", 1), ("page", -1)]),
            Err(BadArgument("'page' must be 0 or greater, got -1".into()))
        );
    }
}
