//! A tiny arithmetic expression evaluator, for plotting a function series.
//!
//! A [`crate::chart::Series::Function`] is authored as a string — `"sin(x)"`,
//! `"x^2 / 50"`, `"3*log(x + 1)"` — because that is what a person opening a
//! `.film` wants to read, and what "a function evaluated over a range" means.
//! The alternative, a list of a few thousand sampled points, would be neither
//! readable nor editable.
//!
//! The grammar is deliberately small: real numbers, one variable `x`, the four
//! operators plus `^`, unary minus, parentheses, a fixed set of unary functions
//! and the constants `pi`, `tau` and `e`. It is **not** a general expression
//! language — no user-defined names, no comparison, no calls the list below
//! does not name — because a film file is not a place to run arbitrary code and
//! a chart does not need one. An unknown name or a malformed expression is a
//! parse *error*, surfaced when the film is validated, not a silent zero.
//!
//! The string is parsed once into an [`Expr`] tree; evaluation is then a cheap
//! tree-walk per sample, so a 400-point curve costs 400 walks, not 400 parses.

use std::fmt;

/// A parsed expression, ready to evaluate at any `x`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var,
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(Func, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Exp,
    Ln,
    Log2,
    Log10,
    Sqrt,
    Cbrt,
    Abs,
    Floor,
    Ceil,
    Round,
    Sign,
}

impl Func {
    fn from_name(s: &str) -> Option<Func> {
        Some(match s {
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "asin" => Func::Asin,
            "acos" => Func::Acos,
            "atan" => Func::Atan,
            "sinh" => Func::Sinh,
            "cosh" => Func::Cosh,
            "tanh" => Func::Tanh,
            "exp" => Func::Exp,
            // `log` is the natural log, matching most calculators and every
            // maths convention this tool's audience will expect; `log10`/`log2`
            // are there when the base matters.
            "ln" | "log" => Func::Ln,
            "log2" => Func::Log2,
            "log10" => Func::Log10,
            "sqrt" => Func::Sqrt,
            "cbrt" => Func::Cbrt,
            "abs" => Func::Abs,
            "floor" => Func::Floor,
            "ceil" => Func::Ceil,
            "round" => Func::Round,
            "sign" | "signum" => Func::Sign,
            _ => return None,
        })
    }

    fn apply(self, v: f64) -> f64 {
        match self {
            Func::Sin => v.sin(),
            Func::Cos => v.cos(),
            Func::Tan => v.tan(),
            Func::Asin => v.asin(),
            Func::Acos => v.acos(),
            Func::Atan => v.atan(),
            Func::Sinh => v.sinh(),
            Func::Cosh => v.cosh(),
            Func::Tanh => v.tanh(),
            Func::Exp => v.exp(),
            Func::Ln => v.ln(),
            Func::Log2 => v.log2(),
            Func::Log10 => v.log10(),
            Func::Sqrt => v.sqrt(),
            Func::Cbrt => v.cbrt(),
            Func::Abs => v.abs(),
            Func::Floor => v.floor(),
            Func::Ceil => v.ceil(),
            Func::Round => v.round(),
            Func::Sign => v.signum(),
        }
    }
}

impl Expr {
    /// Parse an expression. The variable is always `x`.
    pub fn parse(src: &str) -> Result<Expr, ParseError> {
        let tokens = lex(src)?;
        let mut p = Parser { tokens, pos: 0 };
        let e = p.expr(0)?;
        if p.pos != p.tokens.len() {
            return Err(ParseError(format!("unexpected trailing input in {src:?}")));
        }
        Ok(e)
    }

    /// Evaluate at a given `x`. May be non-finite (e.g. `1/x` at 0, `sqrt(-1)`);
    /// the caller decides how to draw a gap — see [`crate::chart`].
    pub fn eval(&self, x: f64) -> f64 {
        match self {
            Expr::Num(n) => *n,
            Expr::Var => x,
            Expr::Neg(a) => -a.eval(x),
            Expr::Call(f, a) => f.apply(a.eval(x)),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(x), b.eval(x));
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Pow => a.powf(b),
                }
            }
        }
    }
}

/// A failure to parse an expression string.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

// ---- lexer ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
}

fn lex(src: &str) -> Result<Vec<Tok>, ParseError> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '^' => {
                out.push(Tok::Caret);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                // A trailing `e`/`E` exponent, so `1e3` and `2.5e-2` lex as one
                // number rather than a number times a variable named `e`.
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        while j < chars.len() && chars[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let s: String = chars[start..i].iter().collect();
                let n: f64 = s
                    .parse()
                    .map_err(|_| ParseError(format!("not a number: {s:?}")))?;
                out.push(Tok::Num(n));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                out.push(Tok::Ident(s));
            }
            other => return Err(ParseError(format!("unexpected character {other:?}"))),
        }
    }
    Ok(out)
}

// ---- parser (precedence climbing) ----------------------------------------

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Precedence climbing. `min_bp` is the binding power a following operator
    /// must beat to bind to the expression parsed so far.
    fn expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.prefix()?;
        loop {
            let (op, l_bp, r_bp) = match self.peek() {
                Some(Tok::Plus) => (Op::Add, 1, 2),
                Some(Tok::Minus) => (Op::Sub, 1, 2),
                Some(Tok::Star) => (Op::Mul, 3, 4),
                Some(Tok::Slash) => (Op::Div, 3, 4),
                // Right-associative, and binds tighter than the rest, so
                // `2^3^2` is `2^(3^2)` and `-x^2` is `-(x^2)`.
                Some(Tok::Caret) => (Op::Pow, 6, 5),
                _ => break,
            };
            if l_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.expr(r_bp)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr, ParseError> {
        match self.bump() {
            Some(Tok::Minus) => Ok(Expr::Neg(Box::new(self.expr(5)?))),
            Some(Tok::Plus) => self.expr(5),
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.expr(0)?;
                match self.bump() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err(ParseError("expected ')'".into())),
                }
            }
            Some(Tok::Ident(name)) => self.ident(name),
            other => Err(ParseError(format!("expected a value, found {other:?}"))),
        }
    }

    fn ident(&mut self, name: String) -> Result<Expr, ParseError> {
        // A name followed by `(` is a function call; otherwise it is the
        // variable or a named constant.
        if matches!(self.peek(), Some(Tok::LParen)) {
            let func = Func::from_name(&name)
                .ok_or_else(|| ParseError(format!("unknown function {name:?}")))?;
            self.bump(); // (
            let arg = self.expr(0)?;
            match self.bump() {
                Some(Tok::RParen) => Ok(Expr::Call(func, Box::new(arg))),
                _ => Err(ParseError(format!("expected ')' after {name}(…"))),
            }
        } else {
            match name.as_str() {
                "x" => Ok(Expr::Var),
                "pi" | "PI" => Ok(Expr::Num(std::f64::consts::PI)),
                "tau" | "TAU" => Ok(Expr::Num(std::f64::consts::TAU)),
                "e" | "E" => Ok(Expr::Num(std::f64::consts::E)),
                other => Err(ParseError(format!("unknown name {other:?}"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, x: f64) -> f64 {
        Expr::parse(src).unwrap().eval(x)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1 + 2 * 3", 0.0), 7.0);
        assert_eq!(ev("(1 + 2) * 3", 0.0), 9.0);
        assert_eq!(ev("10 - 2 - 3", 0.0), 5.0); // left-associative
        assert_eq!(ev("2 ^ 3 ^ 2", 0.0), 512.0); // right-associative
    }

    #[test]
    fn unary_minus_binds_below_pow() {
        // -x^2 is -(x^2), the maths convention, not (-x)^2.
        assert_eq!(ev("-x^2", 3.0), -9.0);
        assert_eq!(ev("(-x)^2", 3.0), 9.0);
    }

    #[test]
    fn the_variable_and_functions() {
        assert_eq!(ev("x", 4.0), 4.0);
        assert!((ev("sin(x)", std::f64::consts::FRAC_PI_2) - 1.0).abs() < 1e-12);
        assert_eq!(ev("abs(x)", -5.0), 5.0);
        assert!((ev("log(e)", 0.0) - 1.0).abs() < 1e-12);
        assert_eq!(ev("sqrt(x)", 9.0), 3.0);
    }

    #[test]
    fn constants_and_exponent_literals() {
        assert!((ev("pi", 0.0) - std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(ev("1e3", 0.0), 1000.0);
        assert_eq!(ev("2.5e-1 * x", 4.0), 1.0);
    }

    #[test]
    fn a_realistic_curve() {
        // The kind of thing a film would actually plot.
        let e = Expr::parse("40 * log(x + 1)").unwrap();
        assert_eq!(e.eval(0.0), 0.0);
        assert!(e.eval(80.0) > e.eval(40.0)); // monotonic increasing
    }

    #[test]
    fn errors_are_reported_not_swallowed() {
        assert!(Expr::parse("x +").is_err());
        assert!(Expr::parse("wobble(x)").is_err());
        assert!(Expr::parse("2 x").is_err()); // no implicit multiplication
        assert!(Expr::parse("(x").is_err());
        assert!(Expr::parse("y").is_err()); // only x is a variable
    }

    #[test]
    fn non_finite_is_returned_not_hidden() {
        assert!(ev("1 / x", 0.0).is_infinite());
        assert!(ev("sqrt(x)", -1.0).is_nan());
    }
}
