//! The small expression grammar of `ratio` formulas:
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := factor (('*' | '/') factor)*
//! factor := number | name | '(' expr ')' | '-' factor
//! name   := [a-z_][a-z0-9_]*
//! ```
//!
//! Names are the step's `terms`. Division by zero yields `None` (the row
//! becomes `missing`, never infinity).

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(usize),
    Neg(Box<Self>),
    Bin(char, Box<Self>, Box<Self>),
}

impl Expr {
    /// Parses `src` with `names` as the only variables (index = position).
    pub fn parse(src: &str, names: &[String]) -> Result<Self> {
        let toks = lex(src)?;
        let mut p = Parser {
            toks: &toks,
            pos: 0,
            names,
        };
        let e = p.expr()?;
        if p.pos != toks.len() {
            bail!("formula {src:?}: unexpected {:?}", toks[p.pos]);
        }
        Ok(e)
    }

    /// Variables used, ascending.
    #[must_use]
    pub fn vars(&self) -> Vec<usize> {
        fn walk(e: &Expr, out: &mut Vec<usize>) {
            match e {
                Expr::Num(_) => {}
                Expr::Var(i) => out.push(*i),
                Expr::Neg(a) => walk(a, out),
                Expr::Bin(_, a, b) => {
                    walk(a, out);
                    walk(b, out);
                }
            }
        }
        let mut v = Vec::new();
        walk(self, &mut v);
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The leftmost variable of the formula (the default key term).
    #[must_use]
    pub fn first_var(&self) -> Option<usize> {
        match self {
            Self::Num(_) => None,
            Self::Var(i) => Some(*i),
            Self::Neg(a) => a.first_var(),
            Self::Bin(_, a, b) => a.first_var().or_else(|| b.first_var()),
        }
    }

    #[must_use]
    pub fn eval(&self, x: &[f64]) -> Option<f64> {
        let v = match self {
            Self::Num(n) => *n,
            Self::Var(i) => *x.get(*i)?,
            Self::Neg(a) => -a.eval(x)?,
            Self::Bin(op, a, b) => {
                let (a, b) = (a.eval(x)?, b.eval(x)?);
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    _ => {
                        if b == 0.0 {
                            return None;
                        }
                        a / b
                    }
                }
            }
        };
        v.is_finite().then_some(v)
    }

    /// Standard deviation of the result by the delta method from per-input
    /// standard deviations (independence assumed): `sd² ≈ Σ (∂f/∂xᵢ)² sdᵢ²`,
    /// partial derivatives by central differences.
    #[must_use]
    pub fn delta_sd(&self, x: &[f64], sd: &[f64]) -> Option<f64> {
        let mut var = 0.0;
        for (i, s) in sd.iter().enumerate() {
            if *s == 0.0 {
                continue;
            }
            let h = (x[i].abs() * 1e-6).max(1e-9);
            let mut up = x.to_vec();
            let mut dn = x.to_vec();
            up[i] += h;
            dn[i] -= h;
            let g = (self.eval(&up)? - self.eval(&dn)?) / (2.0 * h);
            var += g * g * s * s;
        }
        Some(var.sqrt())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Op(char),
}

fn lex(src: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
        } else if "+-*/()".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let s = i;
            while i < b.len()
                && ((b[i] as char).is_ascii_digit()
                    || b[i] == b'.'
                    || b[i] == b'e'
                    || ((b[i] == b'-' || b[i] == b'+') && i > s && b[i - 1] == b'e'))
            {
                i += 1;
            }
            let t = &src[s..i];
            out.push(Tok::Num(t.parse().map_err(|_| {
                anyhow::anyhow!("formula {src:?}: bad number {t:?}")
            })?));
        } else if c.is_ascii_lowercase() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Name(src[s..i].to_owned()));
        } else {
            bail!("formula {src:?}: unexpected character {c:?}");
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    names: &'a [String],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn expr(&mut self) -> Result<Expr> {
        let mut e = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.term()?));
        }
        Ok(e)
    }

    fn term(&mut self) -> Result<Expr> {
        let mut e = self.factor()?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.factor()?));
        }
        Ok(e)
    }

    fn factor(&mut self) -> Result<Expr> {
        let t = self.peek().cloned();
        self.pos += 1;
        match t {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Name(n)) => match self.names.iter().position(|x| *x == n) {
                Some(i) => Ok(Expr::Var(i)),
                None => bail!("formula: {n:?} is not a declared term"),
            },
            Some(Tok::Op('-')) => Ok(Expr::Neg(Box::new(self.factor()?))),
            Some(Tok::Op('(')) => {
                let e = self.expr()?;
                if self.peek() != Some(&Tok::Op(')')) {
                    bail!("formula: missing ')'");
                }
                self.pos += 1;
                Ok(e)
            }
            other => bail!("formula: unexpected {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["a".into(), "b".into()]
    }

    #[test]
    fn parses_and_evaluates() {
        let e = Expr::parse("a / b * 10000", &names()).expect("parse");
        assert_eq!(e.eval(&[3.0, 6.0]), Some(5000.0));
        let e = Expr::parse("(a - b) / b * 100", &names()).expect("parse");
        assert_eq!(e.eval(&[110.0, 100.0]), Some(10.0));
        assert_eq!(
            Expr::parse("-a + 2e1", &names())
                .expect("p")
                .eval(&[1.0, 0.0]),
            Some(19.0)
        );
        assert_eq!(
            Expr::parse("a / b", &names()).expect("p").eval(&[1.0, 0.0]),
            None
        );
        assert_eq!(
            Expr::parse("a / b", &names()).expect("p").vars(),
            vec![0, 1]
        );
        assert_eq!(
            Expr::parse("2 * b / a", &names()).expect("p").first_var(),
            Some(1)
        );
        assert!(Expr::parse("a / c", &names()).is_err());
        assert!(Expr::parse("a / (b", &names()).is_err());
        assert!(Expr::parse("a $ b", &names()).is_err());
    }

    #[test]
    fn delta_method_matches_the_ratio_formula() {
        // r = a/b: var(r) ≈ r²(var(a)/a² + var(b)/b²)
        let e = Expr::parse("a / b", &names()).expect("parse");
        let (a, b, sa, sb) = (50.0, 200.0, 2.0, 5.0);
        let r: f64 = a / b;
        let want = (r * r * ((sa * sa) / (a * a) + (sb * sb) / (b * b))).sqrt();
        let got = e.delta_sd(&[a, b], &[sa, sb]).expect("sd");
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
    }
}
