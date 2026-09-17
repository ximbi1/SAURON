//! Typed scalar access shared by querying and ordering. None is unknown, never zero.
use crate::resources::Object;
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Auto,
    Text,
    Integer,
    Number,
    Count,
    Duration,
    Cpu,
    Memory,
    Percent,
    Bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub kind: Kind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Text(String),
    Integer(i128),
    Number(f64),
    Bool(bool),
}
impl Scalar {
    /// Total order for sorting heterogeneous generic JSON scalars: numbers, booleans,
    /// then text. Numeric integer/float comparisons never round the integer to f64.
    pub fn order(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Integer(a), Self::Number(b)) => integer_float_order(*a, *b),
            (Self::Number(a), Self::Integer(b)) => integer_float_order(*b, *a).reverse(),
            _ => self
                .compare(other)
                .unwrap_or_else(|| self.rank().cmp(&other.rank())),
        }
    }
    fn rank(&self) -> u8 {
        match self {
            Self::Integer(_) | Self::Number(_) => 0,
            Self::Bool(_) => 1,
            Self::Text(_) => 2,
        }
    }
    pub fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => Some(a.cmp(b)),
            (Self::Integer(a), Self::Integer(b)) => Some(a.cmp(b)),
            (Self::Number(a), Self::Number(b)) => a.partial_cmp(b),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }
    pub fn literal_like(&self, text: &str) -> Option<Self> {
        match self {
            Self::Text(_) => Some(Self::Text(text.into())),
            Self::Integer(_) => text.parse().ok().map(Self::Integer),
            Self::Number(_) => finite(text).map(Self::Number),
            Self::Bool(_) => text.parse().ok().map(Self::Bool),
        }
    }
    /// Table-cell display text. Never rounds an exact integer through float
    /// formatting; a `Number` prints with its natural precision, trimming a
    /// trailing `.0` only, never adding false precision.
    pub fn display(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Integer(n) => n.to_string(),
            Self::Number(n) if *n == n.trunc() && n.abs() < 1e15 => format!("{n:.0}"),
            Self::Number(n) => n.to_string(),
            Self::Bool(b) => b.to_string(),
        }
    }
}
fn integer_float_order(integer: i128, float: f64) -> Ordering {
    if float >= -(i128::MIN as f64) {
        return Ordering::Less;
    }
    if float < i128::MIN as f64 {
        return Ordering::Greater;
    }
    let truncated = float as i128;
    integer.cmp(&truncated).then_with(|| {
        if float.fract() > 0.0 {
            Ordering::Less
        } else if float.fract() < 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    })
}
fn finite(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}
impl Kind {
    pub fn parse(self, text: &str) -> Option<Scalar> {
        match self {
            Self::Auto | Self::Text => Some(Scalar::Text(text.into())),
            Self::Integer => text.parse().ok().map(Scalar::Integer),
            Self::Count => text.parse::<u64>().ok().map(|n| Scalar::Integer(n.into())),
            Self::Number => finite(text).map(Scalar::Number),
            Self::Duration => super::duration(text).map(Scalar::Number),
            Self::Cpu | Self::Memory => (!text.ends_with('%'))
                .then(|| super::quantity(text))
                .flatten()
                .map(Scalar::Number),
            Self::Percent => finite(text.strip_suffix('%').unwrap_or(text))
                .filter(|n| *n >= 0.0)
                .map(Scalar::Number),
            Self::Bool => text.parse().ok().map(Scalar::Bool),
        }
    }
}
impl Field {
    pub fn parse(input: &str) -> Result<Self> {
        let (kind, key) = match input.split_once(':') {
            Some(("integer", key)) => (Kind::Integer, key),
            Some(("number" | "float", key)) => (Kind::Number, key),
            Some(("count", key)) => (Kind::Count, key),
            Some(("duration", key)) => (Kind::Duration, key),
            Some(("cpu", key)) => (Kind::Cpu, key),
            Some(("memory", key)) => (Kind::Memory, key),
            Some(("percent", key)) => (Kind::Percent, key),
            Some(("bool", key)) => (Kind::Bool, key),
            _ => (Kind::Auto, input),
        };
        ensure!(!key.is_empty(), "comparison needs a field name");
        if let Some(pointer) = key.strip_prefix("field:") {
            ensure!(
                pointer.starts_with('/'),
                "field needs a JSON Pointer: field:/spec/path"
            );
            for part in pointer.split('~').skip(1) {
                ensure!(
                    part.starts_with(['0', '1']),
                    "JSON Pointer escapes must be ~0 or ~1"
                );
            }
        } else {
            ensure!(!key.contains(':'), "unknown field type or syntax");
        }
        let key = if key.starts_with("label.") || key.starts_with("field:") {
            key.to_owned()
        } else {
            key.to_ascii_lowercase()
        };
        ensure!(key != "label.", "label key cannot be empty");
        let kind = if kind != Kind::Auto {
            kind
        } else {
            match key.as_str() {
                "age" => Kind::Duration,
                "restarts" | "updated" | "available" | "failed" | "succeeded" | "active"
                | "data" | "subsets" => Kind::Count,
                "cpu" | "cpu/a" | "cpu/c" | "cpu/r" | "cpu/l" => Kind::Cpu,
                "memory" | "mem" | "mem/a" | "mem/c" | "mem/r" | "mem/l" | "capacity" => {
                    Kind::Memory
                }
                "cpu/%r" | "cpu/%l" | "mem/%r" | "mem/%l" => Kind::Percent,
                "suspend" => Kind::Bool,
                _ => Kind::Auto,
            }
        };
        Ok(Self { key, kind })
    }
    pub fn validate_literal(&self, text: &str) -> Result<()> {
        ensure!(
            self.kind.parse(text).is_some(),
            "invalid {:?} comparison value",
            self.kind
        );
        Ok(())
    }
    pub fn read(
        &self,
        obj: &Object,
        now: DateTime<Utc>,
        metrics: Option<&dyn crate::resources::Metrics>,
    ) -> Option<Scalar> {
        if self.key == "age" {
            return obj.age(now).map(Scalar::Number);
        }
        // Bare "cpu"/"memory" is live sampled usage, distinct from the "cpu/r",
        // "cpu/l", "cpu/a" accounting/allocatable keys, which stay object-only.
        if matches!(self.key.as_str(), "cpu" | "memory") {
            return metrics.and_then(|m| m.usage(obj, self.key == "cpu").ok().map(Scalar::Number));
        }
        if let Some((cpu, of_limit)) = match self.key.as_str() {
            "cpu/%r" => Some((true, false)),
            "mem/%r" => Some((false, false)),
            "cpu/%l" => Some((true, true)),
            "mem/%l" => Some((false, true)),
            _ => None,
        } {
            return metrics.and_then(|m| m.percentage(obj, cpu, of_limit).ok().map(Scalar::Number));
        }
        let json = if let Some(label) = self.key.strip_prefix("label.") {
            Some(obj.value.pointer("/metadata/labels")?.get(label)?)
        } else if let Some(pointer) = self.key.strip_prefix("field:") {
            Some(obj.value.pointer(pointer)?)
        } else {
            None
        };
        if let Some(json) = json {
            if self.kind == Kind::Auto {
                return match json {
                    Value::String(s) => Some(Scalar::Text(s.clone())),
                    Value::Bool(b) => Some(Scalar::Bool(*b)),
                    Value::Number(n) if n.is_i64() || n.is_u64() => {
                        n.to_string().parse().ok().map(Scalar::Integer)
                    }
                    Value::Number(n) => n.as_f64().filter(|n| n.is_finite()).map(Scalar::Number),
                    _ => None,
                };
            }
            return match json {
                Value::String(s) => self.kind.parse(s),
                Value::Number(n) => self.kind.parse(&n.to_string()),
                Value::Bool(b) if self.kind == Kind::Bool => Some(Scalar::Bool(*b)),
                _ => None,
            };
        }
        // A display fallback must never manufacture metrics or counts. Object::field
        // returns None for absent projection cells and unknown derived statuses.
        let text = obj.field(&self.key, now)?;
        self.kind.parse(&text)
    }
    pub fn compare(
        &self,
        obj: &Object,
        now: DateTime<Utc>,
        want: &str,
        metrics: Option<&dyn crate::resources::Metrics>,
    ) -> Option<Ordering> {
        let actual = self.read(obj, now, metrics)?;
        let expected = if self.kind == Kind::Auto {
            actual.literal_like(want)?
        } else {
            self.kind.parse(want)?
        };
        actual.compare(&expected)
    }
}
