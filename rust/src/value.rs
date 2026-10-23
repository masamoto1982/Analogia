//! Values, their source-text display, and the JSON view the playground draws
//! (`Value` in `src/wasm-interpreter-types.ts`).
//!
//! Analogia's numbers are continuous quantities, like the voltages and water
//! levels of an analog computer: an `f64`, not an exact fraction.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Decimal places the playground is shown before a number is marked `≈`.
const SHOWN_DECIMALS: usize = 6;

/// `12`, `-3/4`, `1.25` or `2e-3`.
pub fn parse_number(text: &str) -> Option<f64> {
    let starts_like_number = |s: &str| {
        let s = s.strip_prefix('-').unwrap_or(s);
        s.starts_with(|c: char| c.is_ascii_digit())
    };
    if !starts_like_number(text) {
        return None;
    }
    let value = match text.split_once('/') {
        Some((n, d)) => {
            let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
            if d == 0.0 {
                return None;
            }
            n / d
        }
        None => text.parse::<f64>().ok()?,
    };
    value.is_finite().then_some(value)
}

/// A number as short source text: `3`, `0.25`, `-1.5`.
pub fn format_number(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let rounded = format!("{x:.SHOWN_DECIMALS$}");
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The number as the reduced fraction the playground's Stack area draws,
/// and whether that fraction only approximates it.
fn as_fraction(x: f64) -> (String, String, bool) {
    let text = format_number(x);
    let exact = text.parse::<f64>().ok() == Some(x);
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let sign = if negative { "-" } else { "" };
    let parsed = format!("{int}{frac}")
        .parse::<u128>()
        .ok()
        .zip(10u128.checked_pow(frac.len() as u32));
    match parsed {
        Some((num, den)) => {
            let g = gcd(num, den).max(1);
            (format!("{sign}{}", num / g), (den / g).to_string(), !exact)
        }
        // Too large for the fraction view: the integer part stands for it.
        None => (format!("{x:.0}"), "1".to_string(), true),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    /// A word written inside `[ ... ]`: data until something runs it.
    Symbol(String),
    Vector(Vec<Value>),
    Nil,
}

impl Value {
    /// The value as source text that reads back as (nearly) the same value.
    pub fn display(&self) -> String {
        match self {
            Value::Number(x) => format_number(*x),
            Value::Str(s) => format!("'{s}'"),
            Value::Bool(true) => "TRUE".to_string(),
            Value::Bool(false) => "FALSE".to_string(),
            Value::Symbol(name) => name.clone(),
            Value::Vector(items) if items.is_empty() => "[ ]".to_string(),
            Value::Vector(items) => {
                let inner: Vec<String> = items.iter().map(Value::display).collect();
                format!("[ {} ]", inner.join(" "))
            }
            Value::Nil => "NIL".to_string(),
        }
    }

    /// The node the playground's stack area draws.
    pub fn protocol(&self) -> serde_json::Value {
        match self {
            Value::Number(x) => {
                let (numerator, denominator, approximate) = as_fraction(*x);
                let mut node = json!({
                    "type": "number",
                    "value": { "numerator": numerator, "denominator": denominator },
                });
                if approximate {
                    node["semantics"] = json!({ "approximate": true });
                }
                node
            }
            Value::Str(s) => json!({ "type": "string", "value": s }),
            Value::Bool(b) => json!({ "type": "boolean", "value": b }),
            Value::Symbol(name) => json!({ "type": "symbol", "value": name }),
            Value::Vector(items) => json!({
                "type": "vector",
                "value": items.iter().map(Value::protocol).collect::<Vec<_>>(),
            }),
            Value::Nil => json!({ "type": "nil", "value": null }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integers_fractions_and_decimals() {
        assert_eq!(parse_number("42"), Some(42.0));
        assert_eq!(parse_number("-2/4"), Some(-0.5));
        assert_eq!(parse_number("0.25"), Some(0.25));
        assert_eq!(parse_number("2e-3"), Some(0.002));
        assert_eq!(parse_number("1/0"), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn numbers_display_as_short_decimals() {
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(-1.5), "-1.5");
        assert_eq!(format_number(1.0 / 3.0), "0.333333");
    }

    #[test]
    fn the_stack_view_marks_rounded_numbers() {
        assert_eq!(as_fraction(0.25), ("1".into(), "4".into(), false));
        assert_eq!(as_fraction(-3.0), ("-3".into(), "1".into(), false));
        assert_eq!(
            as_fraction(1.0 / 3.0),
            ("333333".into(), "1000000".into(), true)
        );
    }
}
