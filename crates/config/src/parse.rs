//! Reads the YAML text into setting values, keeping each value's exact text.
//!
//! A YAML library that builds typed values would read `pair_phone: 0612` as
//! the number 612, and `1e3` as 1000. Like the Go and Python code, this works
//! on the parser's events instead and keeps the text as written.

use std::collections::BTreeMap;

use yaml_rust2::Event;
use yaml_rust2::parser::Parser;
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::{ConfigError, Setting};

/// The values in a configuration file, by setting. A setting the file leaves
/// empty (`pair_phone:`) is not in the map.
pub(crate) type FileValues = BTreeMap<Setting, String>;

pub(crate) fn parse(text: &str) -> Result<FileValues, ConfigError> {
    let mut events = Events {
        parser: Parser::new_from_str(text),
    };
    let mut values = FileValues::new();

    expect_stream_start(&mut events)?;
    let (event, mark) = events.next()?;
    match event {
        // No document at all: an empty file, or only comments.
        Event::StreamEnd => return Ok(values),
        Event::DocumentStart => {}
        other => return Err(unexpected(&other, mark)),
    }

    let (event, mark) = events.next()?;
    match event {
        Event::MappingStart(..) => read_settings(&mut events, &mut values)?,
        // `---` followed by nothing, or an empty plain value.
        Event::Scalar(ref text, style, _, ref tag) if is_null(text, style, tag.as_ref()) => {}
        _ => {
            return Err(ConfigError::invalid(format!(
                "line {}: the file must contain settings as `name: value` lines",
                mark.line()
            )));
        }
    }

    // The end of the document, then the end of the file: one document only.
    let (event, mark) = events.next()?;
    if event != Event::DocumentEnd {
        return Err(unexpected(&event, mark));
    }
    let (event, mark) = events.next()?;
    if event != Event::StreamEnd {
        return Err(ConfigError::invalid(format!(
            "line {}: the file must contain one set of settings, not several YAML documents",
            mark.line()
        )));
    }
    Ok(values)
}

fn read_settings(events: &mut Events<'_>, values: &mut FileValues) -> Result<(), ConfigError> {
    loop {
        let (event, mark) = events.next()?;
        let name = match event {
            Event::MappingEnd => return Ok(()),
            Event::Scalar(name, ..) => name,
            other => {
                return Err(ConfigError::invalid(format!(
                    "line {}: a setting name must be a single word, not {}",
                    mark.line(),
                    describe(&other)
                )));
            }
        };
        let Some(setting) = Setting::from_name(&name) else {
            return Err(ConfigError::invalid(format!(
                "line {}: unknown setting {name:?}",
                mark.line()
            )));
        };
        if values.contains_key(&setting) {
            return Err(ConfigError::invalid(format!(
                "line {}: {name} is set twice",
                mark.line()
            )));
        }
        if let Some(value) = read_value(events, setting)? {
            values.insert(setting, value);
        }
    }
}

/// The value of one setting: `None` when it is left empty.
fn read_value(events: &mut Events<'_>, setting: Setting) -> Result<Option<String>, ConfigError> {
    let name = setting.name();
    let (event, mark) = events.next()?;
    match event {
        Event::Scalar(text, style, _, tag) => {
            if is_null(&text, style, tag.as_ref()) {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        }
        Event::SequenceStart(..) if setting.may_be_list() => {
            let mut items = Vec::new();
            loop {
                let (event, mark) = events.next()?;
                match event {
                    Event::SequenceEnd => break,
                    Event::Scalar(text, ..) => items.push(text),
                    _ => {
                        return Err(ConfigError::invalid(format!(
                            "line {}: {name} entries must be single values",
                            mark.line()
                        )));
                    }
                }
            }
            Ok(Some(items.join(", ")))
        }
        Event::Alias(_) => Err(ConfigError::invalid(format!(
            "line {}: {name}: YAML aliases (*name) are not supported",
            mark.line()
        ))),
        _ => Err(ConfigError::invalid(format!(
            "line {}: {name} must be a single value",
            mark.line()
        ))),
    }
}

/// Whether a scalar is YAML's null: empty, `~` or `null` without quotes, or
/// tagged `!!null`. Go's yaml.v3 and Python's PyYAML agree on this.
fn is_null(text: &str, style: TScalarStyle, tag: Option<&yaml_rust2::parser::Tag>) -> bool {
    match tag {
        Some(tag) => tag.suffix == "null",
        None => style == TScalarStyle::Plain && matches!(text, "" | "~" | "null" | "Null" | "NULL"),
    }
}

fn expect_stream_start(events: &mut Events<'_>) -> Result<(), ConfigError> {
    let (event, mark) = events.next()?;
    if event == Event::StreamStart {
        Ok(())
    } else {
        Err(unexpected(&event, mark))
    }
}

fn unexpected(event: &Event, mark: Marker) -> ConfigError {
    ConfigError::invalid(format!(
        "line {}: unexpected {}",
        mark.line(),
        describe(event)
    ))
}

fn describe(event: &Event) -> &'static str {
    match event {
        Event::SequenceStart(..) => "a list",
        Event::MappingStart(..) => "a group of settings",
        Event::Alias(_) => "an alias",
        Event::DocumentStart | Event::DocumentEnd => "a document marker",
        _ => "YAML",
    }
}

struct Events<'a> {
    parser: Parser<core::str::Chars<'a>>,
}

impl Events<'_> {
    fn next(&mut self) -> Result<(Event, Marker), ConfigError> {
        self.parser
            .next_token()
            .map_err(|e| ConfigError::invalid(format!("invalid YAML: {e}")))
    }
}
