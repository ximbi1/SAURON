//! Bounded Boolean parser with three-valued evaluation: absent is not zero.
use crate::resources::Object;
use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};

#[derive(Clone, Debug)]
pub enum Expr {
    All,
    Text(String),
    Exact(String),
    Regex(Regex),
    Compare(String, Op, String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Truth {
    Yes,
    No,
    Unknown,
}
impl Truth {
    fn not(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
            Self::Unknown => Self::Unknown,
        }
    }
    fn and(self, b: Self) -> Self {
        if self == Self::No || b == Self::No {
            Self::No
        } else if self == Self::Unknown || b == Self::Unknown {
            Self::Unknown
        } else {
            Self::Yes
        }
    }
    fn or(self, b: Self) -> Self {
        if self == Self::Yes || b == Self::Yes {
            Self::Yes
        } else if self == Self::Unknown || b == Self::Unknown {
            Self::Unknown
        } else {
            Self::No
        }
    }
    fn from_bool(b: bool) -> Self {
        if b { Self::Yes } else { Self::No }
    }
}

pub fn fuzzy(pattern: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| chars.any(|c| c == p))
}

impl Expr {
    pub fn parse(s: &str) -> Result<Self> {
        ensure!(s.len() <= 4096, "filter exceeds 4096 bytes");
        let tokens = lex(s)?;
        if tokens.is_empty() {
            return Ok(Self::All);
        }
        ensure!(tokens.len() <= 512, "too many filter terms");
        let mut parser = Parser { tokens, index: 0 };
        let result = parser.or(0)?;
        ensure!(
            parser.index == parser.tokens.len(),
            "unexpected filter token"
        );
        Ok(result)
    }
    pub fn matches(&self, obj: &Object, now: DateTime<Utc>) -> bool {
        self.evaluate(obj, now) == Truth::Yes
    }
    pub fn evaluate(&self, obj: &Object, now: DateTime<Utc>) -> Truth {
        match self {
            Self::All => Truth::Yes,
            Self::Text(s) => Truth::from_bool(fuzzy(s, &obj.search_text())),
            Self::Exact(s) => {
                Truth::from_bool(obj.search_text().to_lowercase().contains(&s.to_lowercase()))
            }
            Self::Regex(r) => Truth::from_bool(
                r.is_match(&obj.name)
                    || r.is_match(&obj.namespace)
                    || obj.cells.iter().any(|(_, value)| r.is_match(value)),
            ),
            Self::Not(a) => a.evaluate(obj, now).not(),
            Self::And(a, b) => a.evaluate(obj, now).and(b.evaluate(obj, now)),
            Self::Or(a, b) => a.evaluate(obj, now).or(b.evaluate(obj, now)),
            Self::Compare(key, op, want) => {
                let actual = if let Some(label) = key.strip_prefix("label.") {
                    obj.value
                        .pointer("/metadata/labels")
                        .and_then(|v| v.get(label))
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                } else {
                    obj.field(key, now)
                };
                let Some(actual) = actual else {
                    return Truth::Unknown;
                };
                let numeric = matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge)
                    || [
                        "age",
                        "cpu",
                        "memory",
                        "mem",
                        "restarts",
                        "failed",
                        "succeeded",
                    ]
                    .contains(&key.as_str());
                let ordering = if numeric {
                    let parse = |s: &str| {
                        if key == "age" {
                            duration(s).or_else(|| s.parse::<f64>().ok().filter(|n| n.is_finite()))
                        } else {
                            quantity(s)
                        }
                    };
                    let (Some(a), Some(b)) = (parse(&actual), parse(want)) else {
                        return Truth::Unknown;
                    };
                    a.total_cmp(&b)
                } else {
                    actual.to_lowercase().cmp(&want.to_lowercase())
                };
                Truth::from_bool(match op {
                    Op::Eq => ordering.is_eq(),
                    Op::Ne => !ordering.is_eq(),
                    Op::Lt => ordering.is_lt(),
                    Op::Le => !ordering.is_gt(),
                    Op::Gt => ordering.is_gt(),
                    Op::Ge => !ordering.is_lt(),
                })
            }
        }
    }
}

/// Kubernetes decimal/binary quantities. Base unit is cores or bytes as applicable.
pub fn quantity(input: &str) -> Option<f64> {
    let s = input.strip_suffix('%').unwrap_or(input);
    let suffixes = [
        ("Ki", 1024.0),
        ("Mi", 1024.0_f64.powi(2)),
        ("Gi", 1024.0_f64.powi(3)),
        ("Ti", 1024.0_f64.powi(4)),
        ("Pi", 1024.0_f64.powi(5)),
        ("Ei", 1024.0_f64.powi(6)),
        ("n", 1e-9),
        ("u", 1e-6),
        ("m", 1e-3),
        ("k", 1e3),
        ("K", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    let (digits, mult) = suffixes
        .iter()
        .find_map(|(suffix, mult)| s.strip_suffix(suffix).map(|d| (d, *mult)))
        .unwrap_or((s, 1.0));
    let value = digits.parse::<f64>().ok()? * mult;
    (value.is_finite() && value >= 0.0).then_some(value)
}
pub fn duration(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let mut start = 0;
    let mut total = 0.0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || c == '.' {
            continue;
        }
        let n = s.get(start..i)?.parse::<f64>().ok()?;
        let factor = match c {
            's' => 1.0,
            'm' => 60.0,
            'h' => 3600.0,
            'd' => 86400.0,
            'w' => 604800.0,
            _ => return None,
        };
        total += n * factor;
        start = i + c.len_utf8();
    }
    (start == s.len() && total.is_finite() && total >= 0.0).then_some(total)
}

#[derive(Clone, Debug)]
enum Token {
    Word(String),
    Quoted(String),
    Pattern(String),
    Op(Op),
    Not,
    And,
    Or,
    Open,
    Close,
}
fn lex(s: &str) -> Result<Vec<Token>> {
    let mut chars = s.chars().peekable();
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        out.push(match c {
            '(' => Token::Open,
            ')' => Token::Close,
            '&' | '|' => {
                ensure!(
                    chars.next() == Some(c),
                    "use && or || for Boolean operators"
                );
                if c == '&' { Token::And } else { Token::Or }
            }
            '!' | '=' | '<' | '>' => {
                let equals = chars.peek() == Some(&'=');
                if equals {
                    chars.next();
                }
                match (c, equals) {
                    ('!', false) => Token::Not,
                    ('!', true) => Token::Op(Op::Ne),
                    ('=', _) => Token::Op(Op::Eq),
                    ('<', false) => Token::Op(Op::Lt),
                    ('<', true) => Token::Op(Op::Le),
                    ('>', false) => Token::Op(Op::Gt),
                    _ => Token::Op(Op::Ge),
                }
            }
            '\'' | '"' | '/' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some(next) = chars.next() {
                    if next == c {
                        closed = true;
                        break;
                    }
                    if next == '\\' {
                        let escaped = chars
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("unfinished escape"))?;
                        if c == '/' && escaped != '/' {
                            text.push('\\');
                        }
                        text.push(escaped);
                    } else {
                        text.push(next);
                    }
                }
                ensure!(closed, "unterminated quote or regex");
                if c == '/' {
                    Token::Pattern(text)
                } else {
                    Token::Quoted(text)
                }
            }
            _ => {
                let mut word = c.to_string();
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || "()!<>=&|".contains(next) {
                        break;
                    }
                    word.push(next);
                    chars.next();
                }
                Token::Word(word)
            }
        });
    }
    Ok(out)
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
}
impl Parser {
    fn or(&mut self, depth: usize) -> Result<Expr> {
        let mut e = self.and(depth)?;
        while matches!(self.tokens.get(self.index), Some(Token::Or)) {
            self.index += 1;
            e = Expr::Or(Box::new(e), Box::new(self.and(depth)?));
        }
        Ok(e)
    }
    fn and(&mut self, depth: usize) -> Result<Expr> {
        let mut e = self.atom(depth)?;
        while self.index < self.tokens.len()
            && !matches!(self.tokens.get(self.index), Some(Token::Or | Token::Close))
        {
            if matches!(self.tokens.get(self.index), Some(Token::And)) {
                self.index += 1;
            }
            e = Expr::And(Box::new(e), Box::new(self.atom(depth)?));
        }
        Ok(e)
    }
    fn atom(&mut self, depth: usize) -> Result<Expr> {
        ensure!(depth < 32, "filter nesting exceeds 32");
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("expected filter expression"))?;
        self.index += 1;
        Ok(match token {
            Token::Not => Expr::Not(Box::new(self.atom(depth + 1)?)),
            Token::Open => {
                let e = self.or(depth + 1)?;
                ensure!(
                    matches!(self.tokens.get(self.index), Some(Token::Close)),
                    "missing closing parenthesis"
                );
                self.index += 1;
                e
            }
            Token::Word(key) => {
                ensure!(
                    !["-l", "-f"].contains(&key.as_str()),
                    "server selectors belong in :resource -l SELECTOR -f SELECTOR before /filter"
                );
                if let Some(Token::Op(op)) = self.tokens.get(self.index).cloned() {
                    self.index += 1;
                    let value = match self.tokens.get(self.index) {
                        Some(Token::Word(s) | Token::Quoted(s)) => s.clone(),
                        _ => bail!("comparison needs a value"),
                    };
                    self.index += 1;
                    let key = key.to_lowercase();
                    if key == "age" {
                        ensure!(
                            duration(&value).is_some(),
                            "age needs a duration such as 1h30m"
                        );
                    }
                    Expr::Compare(key, op, value)
                } else {
                    Expr::Text(key)
                }
            }
            Token::Quoted(s) => Expr::Exact(s),
            Token::Pattern(s) => Expr::Regex(
                RegexBuilder::new(&s)
                    .case_insensitive(true)
                    .size_limit(1_048_576)
                    .build()
                    .map_err(|_| anyhow::anyhow!("invalid or oversized regex"))?,
            ),
            _ => bail!("expected filter term"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precedence_and_unknown_survive_negation() {
        let p = Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"api-1"},"status":{"phase":"Pending"}}),
        );
        for query in [
            "status=Pending || name=other && name=none",
            "(status=Pending || name=other) && !canary",
            "/^api/",
            "\"api-1\"",
        ] {
            assert!(
                Expr::parse(query).expect("valid").matches(&p, Utc::now()),
                "{query}"
            );
        }
        for query in ["cpu>0", "!(cpu>0)", "cpu!=0"] {
            assert_eq!(
                Expr::parse(query).expect("valid").evaluate(&p, Utc::now()),
                Truth::Unknown
            );
        }
        for query in ["(x", "x ||", "/[/", "age>oops", "-l app=api"] {
            assert!(Expr::parse(query).is_err(), "{query}");
        }
    }
    #[test]
    fn quantities_and_durations() {
        assert_eq!(quantity("500m"), Some(0.5));
        assert_eq!(quantity("1Gi"), Some(1073741824.0));
        assert_eq!(quantity("1e3"), Some(1000.0));
        assert_eq!(quantity("NaN"), None);
        assert_eq!(duration("1h30m"), Some(5400.0));
        assert_eq!(duration("1h30"), None);
    }
}
