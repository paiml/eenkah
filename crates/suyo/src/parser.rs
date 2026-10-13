//! Parser for Graphite-style target expressions, e.g.
//! `alias(movingAverage(health.daily.{rhr,hr_min}, "7d"), "RHR")`.

use anyhow::{anyhow, bail, Result};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Path(String),
    Call(String, Vec<Arg>),
    Num(f64),
    Str(String),
    Bool(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub key: Option<String>,
    pub val: Expr,
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Path(p) => write!(f, "{p}"),
            Expr::Num(n) => write!(f, "{n}"),
            Expr::Str(s) => write!(f, "\"{s}\""),
            Expr::Bool(b) => write!(f, "{b}"),
            Expr::Call(name, args) => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    if let Some(k) = &a.key {
                        write!(f, "{k}=")?;
                    }
                    write!(f, "{}", a.val)?;
                }
                write!(f, ")")
            }
        }
    }
}

pub fn parse(src: &str) -> Result<Expr> {
    let mut p = P {
        s: src.as_bytes(),
        i: 0,
        src,
    };
    let e = p.expr()?;
    p.ws();
    if p.i != p.s.len() {
        bail!("unexpected '{}' at position {} in target", &src[p.i..], p.i);
    }
    Ok(e)
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    src: &'a str,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Result<Expr> {
        self.ws();
        match self.peek() {
            None => bail!("unexpected end of target"),
            Some(q @ (b'"' | b'\'')) => {
                self.i += 1;
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i] != q {
                    if self.s[self.i] == b'\\' {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                if self.i >= self.s.len() {
                    bail!("unterminated string in target");
                }
                let raw = &self.src[start..self.i];
                self.i += 1;
                Ok(Expr::Str(raw.replace("\\\"", "\"").replace("\\'", "'")))
            }
            Some(_) => {
                let tok = self.token()?;
                self.ws();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let args = self.args()?;
                    return Ok(Expr::Call(tok, args));
                }
                if tok == "true" || tok == "false" {
                    return Ok(Expr::Bool(tok == "true"));
                }
                let numeric_start =
                    tok.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'));
                if let (true, Ok(n)) = (numeric_start, tok.parse::<f64>()) {
                    return Ok(Expr::Num(n));
                }
                Ok(Expr::Path(tok))
            }
        }
    }

    /// A path or identifier. Commas are allowed inside `{a,b}` braces.
    fn token(&mut self) -> Result<String> {
        let start = self.i;
        let mut depth = 0i32;
        while let Some(c) = self.peek() {
            match c {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                b'(' | b')' | b'=' if depth == 0 => break,
                b',' if depth == 0 => break,
                c if c.is_ascii_whitespace() && depth == 0 => break,
                _ => {}
            }
            self.i += 1;
        }
        if self.i == start {
            bail!("expected a series path or function at position {}", start);
        }
        Ok(self.src[start..self.i].to_string())
    }

    fn args(&mut self) -> Result<Vec<Arg>> {
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(out);
        }
        loop {
            self.ws();
            // keyword argument?
            let save = self.i;
            let mut key = None;
            if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                let k_start = self.i;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    self.i += 1;
                }
                let k = self.src[k_start..self.i].to_string();
                self.ws();
                if self.peek() == Some(b'=') {
                    self.i += 1;
                    key = Some(k);
                } else {
                    self.i = save;
                }
            }
            let val = self.expr()?;
            out.push(Arg { key, val });
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return Err(anyhow!("expected ',' or ')' at position {}", self.i)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_calls_and_globs() {
        let e =
            parse("alias(movingAverage(health.daily.{rhr,hr_min}, \"7d\"), 'Resting HR')").unwrap();
        let Expr::Call(n, args) = e else { panic!() };
        assert_eq!(n, "alias");
        assert_eq!(args[1].val, Expr::Str("Resting HR".into()));
        let Expr::Call(n2, a2) = &args[0].val else {
            panic!()
        };
        assert_eq!(n2, "movingAverage");
        assert_eq!(a2[0].val, Expr::Path("health.daily.{rhr,hr_min}".into()));
    }

    #[test]
    fn numbers_bools_keywords() {
        let e = parse("summarize(a.b, \"1d\", func=\"sum\", alignToFrom=false) ").unwrap();
        let Expr::Call(_, args) = e else { panic!() };
        assert_eq!(args[2].key.as_deref(), Some("func"));
        assert_eq!(args[3].val, Expr::Bool(false));
        assert_eq!(
            parse("scale(x.y, -0.5)").unwrap().to_string(),
            "scale(x.y,-0.5)"
        );
        assert_eq!(
            parse("a.*.c[0-9]").unwrap(),
            Expr::Path("a.*.c[0-9]".into())
        );
        assert_eq!(
            parse("constantLine(10000)").unwrap().to_string(),
            "constantLine(10000)"
        );
    }

    #[test]
    fn errors() {
        assert!(parse("alias(a.b").is_err());
        assert!(parse("alias(a.b, 'x)").is_err());
    }
}
