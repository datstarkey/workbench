//! Validate the declared MCP primitive form schema. Advanced schemas are not
//! negotiated: unsupported constraints fail closed, with the form still open.
use anyhow::{bail, Result};
use serde_json::Value;

pub(super) fn validate(schema: &Value, content: &Value) -> Result<()> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("Unsupported form schema"))?;
    let values = content
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Form content must be an object"))?;
    for name in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !values.contains_key(name) {
            bail!("{name} is required");
        }
    }
    for (name, value) in values {
        let field = properties
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown form field: {name}"))?;
        validate_field(name, field, value)?;
    }
    Ok(())
}
fn validate_field(name: &str, field: &Value, value: &Value) -> Result<()> {
    let kind = field
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "string" => {
            let s = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("{name} must be text"))?;
            let len = s.chars().count() as u64;
            if field
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|v| len < v)
                || field
                    .get("maxLength")
                    .and_then(Value::as_u64)
                    .is_some_and(|v| len > v)
            {
                bail!("{name} has an invalid length");
            }
            // The CLI's form schema offers these formats. Keep validation local
            // and reject additional formats rather than accepting them silently.
            if let Some(format) = field.get("format").and_then(Value::as_str) {
                let valid = match format {
                    "email" => s.split_once('@').is_some_and(|(local, host)| {
                        !local.is_empty()
                            && host.contains('.')
                            && !s.chars().any(char::is_whitespace)
                    }),
                    "uri" => reqwest::Url::parse(s).is_ok(),
                    "date" => valid_date(s),
                    "date-time" => valid_datetime(s),
                    _ => false,
                };
                if !valid {
                    bail!("{name} needs a valid {format}");
                }
            }
        }
        "number" | "integer" => {
            let n = value
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| anyhow::anyhow!("{name} must be a number"))?;
            if kind == "integer" && n.fract() != 0.0 {
                bail!("{name} must be an integer");
            }
            if field
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|v| n < v)
                || field
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|v| n > v)
            {
                bail!("{name} is outside the allowed range");
            }
        }
        "boolean" if value.is_boolean() => {}
        "array" => {
            let values = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("{name} must be a list"))?;
            let items = field
                .get("items")
                .ok_or_else(|| anyhow::anyhow!("Unsupported list field"))?;
            if field
                .get("minItems")
                .and_then(Value::as_u64)
                .is_some_and(|v| (values.len() as u64) < v)
                || field
                    .get("maxItems")
                    .and_then(Value::as_u64)
                    .is_some_and(|v| (values.len() as u64) > v)
            {
                bail!("{name} has an invalid number of choices");
            }
            for (i, v) in values.iter().enumerate() {
                if values[..i].contains(v) {
                    bail!("Duplicate choice in {name}");
                }
                validate_field(name, items, v)?;
            }
        }
        _ => bail!("Unsupported field type for {name}"),
    }
    if let Some(options) = field.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            bail!("Invalid choice for {name}");
        }
    }
    if let Some(options) = field.get("oneOf").and_then(Value::as_array) {
        if !options.iter().any(|o| o.get("const") == Some(value)) {
            bail!("Invalid choice for {name}");
        }
    }
    for key in ["pattern", "anyOf", "allOf", "not", "$ref"] {
        if field.get(key).is_some() {
            bail!("Unsupported constraint {key} for {name}");
        }
    }
    Ok(())
}
fn valid_date(s: &str) -> bool {
    if s.len() != 10
        || !s.is_ascii()
        || s.as_bytes()[4] != b'-'
        || s.as_bytes()[7] != b'-'
        || s.bytes()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let n: Vec<u32> = s.split('-').filter_map(|v| v.parse().ok()).collect();
    if n.len() != 3 || s.len() != 10 || !(1..=12).contains(&n[1]) {
        return false;
    }
    let leap = n[0] % 4 == 0 && (n[0] % 100 != 0 || n[0] % 400 == 0);
    let days = match n[1] {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&n[2])
}

fn valid_datetime(s: &str) -> bool {
    let Some((date, time)) = s.split_once('T') else {
        return false;
    };
    if !valid_date(date) || !time.is_ascii() || time.len() < 9 {
        return false;
    }
    let n: Vec<u32> = time[..8]
        .split(':')
        .filter_map(|v| v.parse().ok())
        .collect();
    if n.len() != 3 || n[0] > 23 || n[1] > 59 || n[2] > 59 {
        return false;
    }
    if time.as_bytes()[2] != b':'
        || time.as_bytes()[5] != b':'
        || time[..8]
            .bytes()
            .enumerate()
            .any(|(i, b)| i != 2 && i != 5 && !b.is_ascii_digit())
    {
        return false;
    }
    let mut zone = &time[8..];
    if let Some(fraction) = zone.strip_prefix('.') {
        let end = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if end == 0 {
            return false;
        }
        zone = &fraction[end..];
    }
    if zone == "Z" {
        return true;
    }
    if zone.len() != 6
        || !zone.starts_with(['+', '-'])
        || zone.as_bytes()[3] != b':'
        || zone[1..]
            .bytes()
            .enumerate()
            .any(|(i, b)| i != 2 && !b.is_ascii_digit())
    {
        return false;
    }
    let parts: Vec<u32> = zone[1..]
        .split(':')
        .filter_map(|v| v.parse().ok())
        .collect();
    parts.len() == 2 && parts[0] <= 23 && parts[1] <= 59
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_formats_validate_calendar_and_complete_timezone() {
        assert!(valid_date("2024-02-29"));
        assert!(!valid_date("2023-02-29"));
        assert!(!valid_date("000-001-01"));
        assert!(valid_datetime("2024-02-29T12:30:00.123+01:00"));
        assert!(valid_datetime("2024-02-29T12:30:00Z"));
        assert!(!valid_datetime("2024-02-29T12:30:00.1+garbageZ"));
        assert!(!valid_datetime("2024-02-29T12:30:00+99:00"));
        assert!(!valid_datetime("2024-02-29T12:30:00junk+01:00"));
    }
}
