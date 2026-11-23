//! A small spreadsheet formula engine: arithmetic, comparisons, text
//! concatenation, cell references, ranges and the common functions.
//! Both `,` and `;` separate arguments, so formulas typed the European way
//! work as they do in other spreadsheet apps.

use crate::model::{cell_ref, parse_ref, SheetBody};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(&'static str),
}

impl Value {
    pub fn display(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Num(n) => fmt_num(*n),
            Value::Text(s) => s.clone(),
            Value::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
            Value::Err(e) => (*e).into(),
        }
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Num(_))
    }
    fn num(&self) -> Result<f64, &'static str> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => parse_number(s).ok_or("#VALUE!"),
            Value::Err(e) => Err(e),
        }
    }
    fn text(&self) -> Result<String, &'static str> {
        match self {
            Value::Err(e) => Err(e),
            v => Ok(v.display()),
        }
    }
    fn truthy(&self) -> Result<bool, &'static str> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Text(s) if s.eq_ignore_ascii_case("true") => Ok(true),
            Value::Text(s) if s.eq_ignore_ascii_case("false") => Ok(false),
            v => Ok(v.num()? != 0.0),
        }
    }
}

pub fn fmt_num(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{:.10}", n);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Plain numbers as typed in a cell: "12", "-3.5", "3,5", "1 200", "15%".
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (t, pct) = match t.strip_suffix('%') {
        Some(rest) => (rest.trim(), true),
        None => (t, false),
    };
    let cleaned: String = t.chars().filter(|c| *c != ' ' && *c != '\u{a0}' && *c != '\u{202f}').collect();
    let cleaned = if cleaned.contains(',') && !cleaned.contains('.') { cleaned.replace(',', ".") } else { cleaned.replace(',', "") };
    if !cleaned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: f64 = cleaned.parse().ok()?;
    Some(if pct { v / 100.0 } else { v })
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(char),
    Cmp(&'static str),
    LParen,
    RParen,
    Sep,
    Colon,
}

fn lex(src: &str) -> Result<Vec<Tok>, &'static str> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' => {
                let st = i;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                    i += 1;
                }
                if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                    i += 1;
                    if i < cs.len() && (cs[i] == '+' || cs[i] == '-') {
                        i += 1;
                    }
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let s: String = cs[st..i].iter().collect();
                out.push(Tok::Num(s.parse().map_err(|_| "#VALUE!")?));
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= cs.len() {
                        return Err("#VALUE!");
                    }
                    if cs[i] == '"' {
                        if i + 1 < cs.len() && cs[i + 1] == '"' {
                            s.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    s.push(cs[i]);
                    i += 1;
                }
                out.push(Tok::Str(s));
            }
            'A'..='Z' | 'a'..='z' | '$' | '_' => {
                let st = i;
                while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '$' || cs[i] == '_' || cs[i] == '.') {
                    i += 1;
                }
                out.push(Tok::Ident(cs[st..i].iter().collect::<String>().to_ascii_uppercase()));
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '%' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '<' | '>' | '=' => {
                let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
                let op = match two.as_str() {
                    "<=" => "<=",
                    ">=" => ">=",
                    "<>" => "<>",
                    _ => match c {
                        '<' => "<",
                        '>' => ">",
                        _ => "=",
                    },
                };
                i += op.len();
                out.push(Tok::Cmp(op));
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' | ';' => {
                out.push(Tok::Sep);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            _ => return Err("#NAME?"),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum Expr {
    Lit(Value),
    Ref(u32, u32),
    Range((u32, u32), (u32, u32)),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn expr(&mut self) -> Result<Expr, &'static str> {
        self.comparison()
    }
    fn comparison(&mut self) -> Result<Expr, &'static str> {
        let mut l = self.concat()?;
        while let Some(Tok::Cmp(op)) = self.peek().cloned() {
            self.i += 1;
            let r = self.concat()?;
            l = Expr::Cmp(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn concat(&mut self) -> Result<Expr, &'static str> {
        let mut l = self.additive()?;
        while self.peek() == Some(&Tok::Op('&')) {
            self.i += 1;
            let r = self.additive()?;
            l = Expr::Bin('&', Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn additive(&mut self) -> Result<Expr, &'static str> {
        let mut l = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.term()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn term(&mut self) -> Result<Expr, &'static str> {
        let mut l = self.power()?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.power()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn power(&mut self) -> Result<Expr, &'static str> {
        let l = self.unary()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.i += 1;
            let r = self.power()?;
            return Ok(Expr::Bin('^', Box::new(l), Box::new(r)));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr, &'static str> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.i += 1;
                self.unary()
            }
            _ => {
                let mut e = self.atom()?;
                while self.peek() == Some(&Tok::Op('%')) {
                    self.i += 1;
                    e = Expr::Pct(Box::new(e));
                }
                Ok(e)
            }
        }
    }
    fn atom(&mut self) -> Result<Expr, &'static str> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Lit(Value::Num(n))),
            Some(Tok::Str(s)) => Ok(Expr::Lit(Value::Text(s))),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                if self.next() != Some(Tok::RParen) {
                    return Err("#VALUE!");
                }
                Ok(e)
            }
            Some(Tok::Ident(id)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.i += 1;
                        return Ok(Expr::Call(id, args));
                    }
                    loop {
                        args.push(self.expr()?);
                        match self.next() {
                            Some(Tok::Sep) => continue,
                            Some(Tok::RParen) => break,
                            _ => return Err("#VALUE!"),
                        }
                    }
                    return Ok(Expr::Call(id, args));
                }
                if id == "TRUE" {
                    return Ok(Expr::Lit(Value::Bool(true)));
                }
                if id == "FALSE" {
                    return Ok(Expr::Lit(Value::Bool(false)));
                }
                let a = parse_ref(&id).ok_or("#NAME?")?;
                if self.peek() == Some(&Tok::Colon) {
                    self.i += 1;
                    match self.next() {
                        Some(Tok::Ident(id2)) => {
                            let b = parse_ref(&id2).ok_or("#REF!")?;
                            Ok(Expr::Range(a, b))
                        }
                        _ => Err("#REF!"),
                    }
                } else {
                    Ok(Expr::Ref(a.0, a.1))
                }
            }
            _ => Err("#VALUE!"),
        }
    }
}

fn parse_formula(src: &str) -> Result<Expr, &'static str> {
    let mut p = Parser { t: lex(src)?, i: 0 };
    let e = p.expr()?;
    if p.i != p.t.len() {
        return Err("#VALUE!");
    }
    Ok(e)
}

// ---------------------------------------------------------------- evaluation

pub struct Evaluator<'a> {
    sheet: &'a SheetBody,
    cache: HashMap<(u32, u32), Value>,
    visiting: HashSet<(u32, u32)>,
}

impl<'a> Evaluator<'a> {
    pub fn new(sheet: &'a SheetBody) -> Self {
        Evaluator { sheet, cache: HashMap::new(), visiting: HashSet::new() }
    }

    pub fn cell(&mut self, r: u32, c: u32) -> Value {
        if let Some(v) = self.cache.get(&(r, c)) {
            return v.clone();
        }
        if !self.visiting.insert((r, c)) {
            return Value::Err("#CYCLE!");
        }
        let input = self.sheet.cells.get(&cell_ref(r, c)).map(|c| c.input.clone()).unwrap_or_default();
        let v = self.eval_input(&input);
        self.visiting.remove(&(r, c));
        self.cache.insert((r, c), v.clone());
        v
    }

    pub fn eval_input(&mut self, input: &str) -> Value {
        let t = input.trim();
        if t.is_empty() {
            return Value::Empty;
        }
        if let Some(f) = t.strip_prefix('=') {
            return match parse_formula(f) {
                Ok(e) => self.eval(&e).unwrap_or_else(Value::Err),
                Err(e) => Value::Err(e),
            };
        }
        if let Some(n) = parse_number(t) {
            return Value::Num(n);
        }
        Value::Text(input.to_string())
    }

    fn range(&mut self, a: (u32, u32), b: (u32, u32)) -> Vec<Value> {
        let (r0, r1) = (a.0.min(b.0), a.0.max(b.0));
        let (c0, c1) = (a.1.min(b.1), a.1.max(b.1));
        let mut v = Vec::new();
        for r in r0..=r1.min(r0 + 10_000) {
            for c in c0..=c1.min(c0 + 1_000) {
                v.push(self.cell(r, c));
            }
        }
        v
    }

    /// Flatten arguments: ranges expand to their cells.
    fn values(&mut self, args: &[Expr]) -> Result<Vec<Value>, &'static str> {
        let mut out = Vec::new();
        for a in args {
            match a {
                Expr::Range(x, y) => out.extend(self.range(*x, *y)),
                e => out.push(self.eval(e)?),
            }
        }
        Ok(out)
    }

    fn numbers(&mut self, args: &[Expr]) -> Result<Vec<f64>, &'static str> {
        let mut out = Vec::new();
        for a in args {
            match a {
                Expr::Range(x, y) => {
                    for v in self.range(*x, *y) {
                        match v {
                            Value::Num(n) => out.push(n),
                            Value::Err(e) => return Err(e),
                            _ => {}
                        }
                    }
                }
                e => out.push(self.eval(e)?.num()?),
            }
        }
        Ok(out)
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, &'static str> {
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Ref(r, c) => match self.cell(*r, *c) {
                Value::Err(e) => return Err(e),
                v => v,
            },
            Expr::Range(a, _) => self.cell(a.0, a.1),
            Expr::Neg(x) => Value::Num(-self.eval(x)?.num()?),
            Expr::Pct(x) => Value::Num(self.eval(x)?.num()? / 100.0),
            Expr::Bin('&', a, b) => Value::Text(self.eval(a)?.text()? + &self.eval(b)?.text()?),
            Expr::Bin(op, a, b) => {
                let x = self.eval(a)?.num()?;
                let y = self.eval(b)?.num()?;
                Value::Num(match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    '/' => {
                        if y == 0.0 {
                            return Err("#DIV/0!");
                        }
                        x / y
                    }
                    '^' => x.powf(y),
                    _ => return Err("#VALUE!"),
                })
            }
            Expr::Cmp(op, a, b) => {
                let x = self.eval(a)?;
                let y = self.eval(b)?;
                let ord = match (&x, &y) {
                    (Value::Text(s), Value::Text(t)) => s.to_lowercase().cmp(&t.to_lowercase()),
                    (Value::Text(_), _) | (_, Value::Text(_)) => x.text()?.cmp(&y.text()?),
                    _ => x.num()?.partial_cmp(&y.num()?).ok_or("#NUM!")?,
                };
                use std::cmp::Ordering::*;
                Value::Bool(match *op {
                    "=" => ord == Equal,
                    "<>" => ord != Equal,
                    "<" => ord == Less,
                    ">" => ord == Greater,
                    "<=" => ord != Greater,
                    ">=" => ord != Less,
                    _ => return Err("#VALUE!"),
                })
            }
            Expr::Call(name, args) => self.call(name, args)?,
        })
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, &'static str> {
        let n = |v: f64| Ok(Value::Num(v));
        match name {
            "SUM" => n(self.numbers(args)?.iter().sum()),
            "PRODUCT" => n(self.numbers(args)?.iter().product()),
            "AVERAGE" | "MEAN" => {
                let v = self.numbers(args)?;
                if v.is_empty() {
                    return Err("#DIV/0!");
                }
                n(v.iter().sum::<f64>() / v.len() as f64)
            }
            "MIN" => n(self.numbers(args)?.into_iter().fold(f64::INFINITY, f64::min)).map(|v| fix_inf(v)),
            "MAX" => n(self.numbers(args)?.into_iter().fold(f64::NEG_INFINITY, f64::max)).map(|v| fix_inf(v)),
            "MEDIAN" => {
                let mut v = self.numbers(args)?;
                if v.is_empty() {
                    return Err("#NUM!");
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let m = v.len() / 2;
                n(if v.len() % 2 == 0 { (v[m - 1] + v[m]) / 2.0 } else { v[m] })
            }
            "COUNT" => n(self.values(args)?.iter().filter(|v| v.is_number()).count() as f64),
            "COUNTA" => n(self.values(args)?.iter().filter(|v| **v != Value::Empty).count() as f64),
            "ABS" => n(self.arg_num(args, 0)?.abs()),
            "SQRT" => {
                let x = self.arg_num(args, 0)?;
                if x < 0.0 {
                    return Err("#NUM!");
                }
                n(x.sqrt())
            }
            "POWER" => n(self.arg_num(args, 0)?.powf(self.arg_num(args, 1)?)),
            "ROUND" => {
                let x = self.arg_num(args, 0)?;
                let d = if args.len() > 1 { self.arg_num(args, 1)? } else { 0.0 };
                let f = 10f64.powi(d as i32);
                n((x * f).round() / f)
            }
            "INT" => n(self.arg_num(args, 0)?.floor()),
            "MOD" => {
                let b = self.arg_num(args, 1)?;
                if b == 0.0 {
                    return Err("#DIV/0!");
                }
                n(self.arg_num(args, 0)?.rem_euclid(b))
            }
            "IF" => {
                if args.is_empty() {
                    return Err("#VALUE!");
                }
                let c = self.eval(&args[0])?.truthy()?;
                match (c, args.get(1), args.get(2)) {
                    (true, Some(a), _) => self.eval(a),
                    (false, _, Some(b)) => self.eval(b),
                    (b, _, _) => Ok(Value::Bool(b)),
                }
            }
            "AND" => {
                let v = self.values(args)?;
                let mut acc = true;
                for x in v {
                    acc &= x.truthy()?;
                }
                Ok(Value::Bool(acc))
            }
            "OR" => {
                let v = self.values(args)?;
                let mut acc = false;
                for x in v {
                    acc |= x.truthy()?;
                }
                Ok(Value::Bool(acc))
            }
            "NOT" => Ok(Value::Bool(!self.arg(args, 0)?.truthy()?)),
            "CONCAT" | "CONCATENATE" => {
                let mut s = String::new();
                for v in self.values(args)? {
                    s += &v.text()?;
                }
                Ok(Value::Text(s))
            }
            "LEN" => n(self.arg(args, 0)?.text()?.chars().count() as f64),
            "UPPER" => Ok(Value::Text(self.arg(args, 0)?.text()?.to_uppercase())),
            "LOWER" => Ok(Value::Text(self.arg(args, 0)?.text()?.to_lowercase())),
            "TRIM" => Ok(Value::Text(self.arg(args, 0)?.text()?.split_whitespace().collect::<Vec<_>>().join(" "))),
            "PI" => n(std::f64::consts::PI),
            _ => Err("#NAME?"),
        }
    }

    fn arg(&mut self, args: &[Expr], i: usize) -> Result<Value, &'static str> {
        let e = args.get(i).ok_or("#VALUE!")?;
        self.eval(e)
    }
    fn arg_num(&mut self, args: &[Expr], i: usize) -> Result<f64, &'static str> {
        self.arg(args, i)?.num()
    }
}

fn fix_inf(v: Value) -> Value {
    match v {
        Value::Num(n) if !n.is_finite() => Value::Num(0.0),
        v => v,
    }
}

/// Evaluate every non-empty cell. Keys are A1 references.
pub fn compute(sheet: &SheetBody) -> HashMap<String, Value> {
    let mut ev = Evaluator::new(sheet);
    let mut out = HashMap::new();
    for key in sheet.cells.keys() {
        if let Some((r, c)) = parse_ref(key) {
            out.insert(key.clone(), ev.cell(r, c));
        }
    }
    out
}

/// Short plain-text summary of a sheet, used in document lists.
pub fn excerpt(sheet: &SheetBody) -> String {
    let vals = compute(sheet);
    let mut parts: Vec<(u32, u32, String)> = vals
        .iter()
        .filter_map(|(k, v)| parse_ref(k).map(|(r, c)| (r, c, v.display())))
        .filter(|(_, _, s)| !s.is_empty())
        .collect();
    parts.sort();
    parts.into_iter().take(8).map(|p| p.2).collect::<Vec<_>>().join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Cell;

    fn sheet(cells: &[(&str, &str)]) -> SheetBody {
        let mut s = SheetBody::default();
        for (k, v) in cells {
            s.cells.insert(k.to_string(), Cell { input: v.to_string(), ..Default::default() });
        }
        s
    }

    #[test]
    fn arithmetic_and_refs() {
        let s = sheet(&[("A1", "2"), ("A2", "3,5"), ("A3", "=A1*A2+1"), ("B1", "=SUM(A1:A3)"), ("B2", "=ROUND(B1/3; 2)")]);
        let v = compute(&s);
        assert_eq!(v["A3"], Value::Num(8.0));
        assert_eq!(v["B1"], Value::Num(13.5));
        assert_eq!(v["B2"], Value::Num(4.5));
    }

    #[test]
    fn errors_and_cycles() {
        let s = sheet(&[("A1", "=B1"), ("B1", "=A1"), ("C1", "=1/0"), ("D1", "=FOO(1)")]);
        let v = compute(&s);
        assert_eq!(v["A1"], Value::Err("#CYCLE!"));
        assert_eq!(v["C1"], Value::Err("#DIV/0!"));
        assert_eq!(v["D1"], Value::Err("#NAME?"));
    }

    #[test]
    fn text_and_logic() {
        let s = sheet(&[("A1", "Naute"), ("A2", "=IF(LEN(A1)>3, UPPER(A1) & \"!\", \"no\")"), ("A3", "=AVERAGE(1,2,3,4)"), ("A4", "20%")]);
        let v = compute(&s);
        assert_eq!(v["A2"], Value::Text("NAUTE!".into()));
        assert_eq!(v["A3"], Value::Num(2.5));
        assert_eq!(v["A4"], Value::Num(0.2));
    }
}
