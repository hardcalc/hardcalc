//! Formula syntax: lexer and recursive-descent parser.
//!
//! ```text
//! expr    := compare
//! compare := concat (("=" | "<>" | "<" | "<=" | ">" | ">=") concat)?
//! concat  := add ("&" add)*
//! add     := mul (("+" | "-") mul)*
//! mul     := unary (("*" | "/") unary)*
//! unary   := "-" unary | power
//! power   := postfix ("^" unary)?
//! postfix := primary "%"*
//! primary := number ["{" unit "}"] | string | "(" expr ")"
//!          | name "(" args ")"          function call
//!          | ref layer*
//! ref     := name                       column, cell (A1, also B2b with a layer) or boolean
//!          | name ":" end               range B2:D5, also short: B2:4, B2a:c, B2:4c
//!          | name "[" args "]"          Table[row, col], Table[Col=value], Column[Label]
//!          | name "." name              Table.Column (old form of Table!Column)
//!          | table "!" ref              cell, range or column of another table:
//!                                       Expenses!B2, Expenses!B2:B4, Expenses!Amount,
//!                                       "My sheet"!B2
//! layer   := "." name                   data layer by name: B2.February
//!          | "[" ("+"|"-") number "]"   data layer relative to the formula's: B2[-1]
//! ```

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    /// Number with unit literal: `5{m/s}`.
    Quantity(f64, String),
    Str(String),
    Bool(bool),
    Name(String),
    Range(String, String),
    Member(String, String),
    /// A reference in another data layer: `B2.February`, `Amount[-1]`.
    Layer(Box<Expr>, LayerSel),
    Index(String, Vec<Expr>),
    /// `Table!B2`, `Table!B2:B4`, `Table!B`: a reference evaluated in another table.
    Remote(String, Box<Expr>),
    /// `@Amount`, `[@Amount]`, `@Table.Col`, `Table[@Col]`: the value in
    /// the formula's own row (Excel's implicit intersection / `[@Col]`).
    At(Box<Expr>),
    Neg(Box<Expr>),
    Percent(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

/// Which data layer a reference reads.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerSel {
    Name(String),
    /// Relative to the layer the formula is evaluated in.
    Offset(i64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Unit(String),
    Op(&'static str),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() && toks.last() == Some(&Tok::Op(":")) {
            // The end of a short range: `B2:4`, `B2:4c`.
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '$') {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = chars[start..i].iter().collect();
            toks.push(Tok::Num(s.parse().map_err(|_| format!("bad number '{s}'"))?));
            // Optional unit literal directly after the number (spaces allowed).
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if chars.get(j) == Some(&'{') {
                let end = chars[j..].iter().position(|&c| c == '}').ok_or("missing '}' after unit")? + j;
                toks.push(Tok::Unit(chars[j + 1..end].iter().collect::<String>().trim().to_string()));
                i = end + 1;
            }
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') if chars.get(i + 1) == Some(&'"') => {
                        s.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            toks.push(Tok::Str(s));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            // `$` anchors a reference for filling: `$B$1`.
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            let op = match two.as_str() {
                "<=" => Some("<="),
                ">=" => Some(">="),
                "<>" | "!=" => Some("<>"),
                _ => None,
            };
            if let Some(op) = op {
                toks.push(Tok::Op(op));
                i += 2;
                continue;
            }
            let op = match c {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '^' => "^",
                '&' => "&",
                '%' => "%",
                '=' => "=",
                '<' => "<",
                '>' => ">",
                '(' => "(",
                ')' => ")",
                '[' => "[",
                ']' => "]",
                ',' | ';' => ",",
                ':' => ":",
                '.' => ".",
                '!' => "!",
                '@' => "@",
                other => return Err(format!("unexpected character '{other}'")),
            };
            toks.push(Tok::Op(op));
            i += 1;
        }
    }
    Ok(toks)
}

/// Parses a formula. A leading `=` is optional.
pub fn parse_formula(src: &str) -> Result<Expr, String> {
    let src = src.trim();
    let src = src.strip_prefix('=').unwrap_or(src);
    let mut p = Parser { toks: lex(src)?, pos: 0 };
    let e = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(format!("unexpected {:?}", p.toks[p.pos]));
    }
    Ok(e)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self, op: &str) -> bool {
        matches!(self.toks.get(self.pos), Some(Tok::Op(o)) if *o == op)
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek_op(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, op: &str) -> Result<(), String> {
        if self.eat(op) { Ok(()) } else { Err(format!("expected '{op}'")) }
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.toks.get(self.pos) {
            Some(Tok::Ident(s)) => {
                self.pos += 1;
                Ok(s.clone())
            }
            _ => Err("expected a name".into()),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let lhs = self.concat()?;
        for (sym, op) in
            [("=", BinOp::Eq), ("<>", BinOp::Ne), ("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)]
        {
            if self.eat(sym) {
                return Ok(Expr::Binary(op, Box::new(lhs), Box::new(self.concat()?)));
            }
        }
        Ok(lhs)
    }

    fn concat(&mut self) -> Result<Expr, String> {
        let mut lhs = self.add()?;
        while self.eat("&") {
            lhs = Expr::Binary(BinOp::Concat, Box::new(lhs), Box::new(self.add()?));
        }
        Ok(lhs)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.mul()?;
        loop {
            let op = if self.eat("+") {
                BinOp::Add
            } else if self.eat("-") {
                BinOp::Sub
            } else {
                return Ok(lhs);
            };
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.mul()?));
        }
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = if self.eat("*") {
                BinOp::Mul
            } else if self.eat("/") {
                BinOp::Div
            } else {
                return Ok(lhs);
            };
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, String> {
        let base = self.postfix()?;
        if self.eat("^") {
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while self.eat("%") {
            e = Expr::Percent(Box::new(e));
        }
        Ok(e)
    }

    fn args(&mut self, close: &str) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.eat(close) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if self.eat(close) {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    /// A name or a quoted name.
    fn name_or_str(&mut self) -> Result<String, String> {
        match self.toks.get(self.pos) {
            Some(Tok::Str(s)) => {
                let s = s.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => self.ident(),
        }
    }

    /// `[+1]` / `[-1]` ahead: a layer offset.
    fn offset_ahead(&self) -> Option<i64> {
        let sign = match self.toks.get(self.pos + 1) {
            Some(Tok::Op("+")) => 1,
            Some(Tok::Op("-")) => -1,
            _ => return None,
        };
        match (self.toks.get(self.pos), self.toks.get(self.pos + 2), self.toks.get(self.pos + 3)) {
            (Some(Tok::Op("[")), Some(Tok::Num(n)), Some(Tok::Op("]"))) if n.fract() == 0.0 => Some(sign * *n as i64),
            _ => None,
        }
    }

    /// `.Name` and `[±n]` after a reference: the data layer it reads.
    fn layers(&mut self, mut e: Expr) -> Result<Expr, String> {
        loop {
            if let Some(n) = self.offset_ahead() {
                self.pos += 4;
                e = Expr::Layer(Box::new(e), LayerSel::Offset(n));
            } else if self.eat(".") {
                e = Expr::Layer(Box::new(e), LayerSel::Name(self.name_or_str()?));
            } else {
                return Ok(e);
            }
        }
    }

    /// The end of a range after `:` – a full reference or its changed part.
    fn range_end(&mut self) -> Result<String, String> {
        match self.toks.get(self.pos) {
            Some(Tok::Num(n)) if n.fract() == 0.0 && *n >= 1.0 => {
                let n = *n as usize;
                self.pos += 1;
                Ok(n.to_string())
            }
            _ => self.ident(),
        }
    }

    /// What follows a name: `[…]`, `:end` or nothing.
    fn reference(&mut self, name: String) -> Result<Expr, String> {
        if self.peek_op("[") && self.offset_ahead().is_none() {
            self.pos += 1;
            // `Table[@Col]`: the same row of another table.
            if self.eat("@") {
                let col = self.ident()?;
                self.expect("]")?;
                return Ok(Expr::At(Box::new(Expr::Member(name, col))));
            }
            return Ok(Expr::Index(name, self.args("]")?));
        }
        if self.eat(":") {
            return Ok(Expr::Range(name, self.range_end()?));
        }
        Ok(Expr::Name(name))
    }

    /// The part after `table!`: a cell, a range, a column or a row of a column.
    fn remote(&mut self, table: String) -> Result<Expr, String> {
        let a = self.ident()?;
        let inner = self.reference(a)?;
        Ok(Expr::Remote(table, Box::new(inner)))
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.toks.get(self.pos).cloned().ok_or("unexpected end of formula")?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => match self.toks.get(self.pos) {
                Some(Tok::Unit(u)) => {
                    let u = u.clone();
                    self.pos += 1;
                    Ok(Expr::Quantity(n, u))
                }
                _ => Ok(Expr::Num(n)),
            },
            // A quoted table name ("My sheet"!B2, "My sheet".Amount, "My sheet"[…]).
            Tok::Str(s) if self.eat("!") => {
                let e = self.remote(s)?;
                self.layers(e)
            }
            Tok::Str(s) if self.eat(".") => {
                let e = Expr::Member(s, self.name_or_str()?);
                self.layers(e)
            }
            Tok::Str(s) if self.peek_op("[") => {
                let e = self.reference(s)?;
                self.layers(e)
            }
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Op("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Op("@") => match self.primary()? {
                e @ (Expr::Name(_) | Expr::Member(..) | Expr::Remote(..)) => Ok(Expr::At(Box::new(e))),
                // `@Amount.February`: this row in another layer.
                Expr::Layer(inner, sel) if matches!(*inner, Expr::Name(_) | Expr::Member(..) | Expr::Remote(..)) => {
                    Ok(Expr::Layer(Box::new(Expr::At(inner)), sel))
                }
                _ => Err("@ must be followed by a column".into()),
            },
            // `[@Col]` as in Excel tables.
            Tok::Op("[") if self.eat("@") => {
                let col = self.ident()?;
                self.expect("]")?;
                Ok(Expr::At(Box::new(Expr::Name(col))))
            }
            Tok::Ident(name) => {
                if self.eat("!") {
                    let e = self.remote(name)?;
                    self.layers(e)
                } else if self.eat("(") {
                    Ok(Expr::Call(name.to_lowercase(), self.args(")")?))
                } else if self.peek_op(".") && matches!(self.toks.get(self.pos + 1), Some(Tok::Ident(_) | Tok::Str(_))) {
                    // `X.Y`: Table.Column (old form), else Y is a data layer of X.
                    self.pos += 1;
                    let e = Expr::Member(name, self.name_or_str()?);
                    self.layers(e)
                } else {
                    match name.to_lowercase().as_str() {
                        "true" | "wahr" if !self.peek_op("[") && !self.peek_op(":") => Ok(Expr::Bool(true)),
                        "false" | "falsch" if !self.peek_op("[") && !self.peek_op(":") => Ok(Expr::Bool(false)),
                        _ => {
                            let e = self.reference(name)?;
                            self.layers(e)
                        }
                    }
                }
            }
            Tok::Unit(u) => Err(format!("unexpected unit '{{{u}}}'")),
            Tok::Op(op) => Err(format!("unexpected '{op}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Expr::*;

    fn b(op: BinOp, l: Expr, r: Expr) -> Expr {
        Binary(op, Box::new(l), Box::new(r))
    }

    #[test]
    fn precedence() {
        assert_eq!(
            parse_formula("=1+2*3^2").unwrap(),
            b(BinOp::Add, Num(1.0), b(BinOp::Mul, Num(2.0), b(BinOp::Pow, Num(3.0), Num(2.0))))
        );
    }

    #[test]
    fn references() {
        assert_eq!(
            parse_formula("=B1/sum(B)").unwrap(),
            b(BinOp::Div, Name("B1".into()), Call("sum".into(), vec![Name("B".into())]))
        );
        assert_eq!(parse_formula("Expenses.Amount").unwrap(), Member("Expenses".into(), "Amount".into()));
        assert_eq!(parse_formula("B2:D5").unwrap(), Range("B2".into(), "D5".into()));
        assert_eq!(parse_formula("Amount[Total]").unwrap(), Index("Amount".into(), vec![Name("Total".into())]));
        assert_eq!(
            parse_formula("Expenses!Amount[\"Ø Price\"]").unwrap(),
            Remote("Expenses".into(), Box::new(Index("Amount".into(), vec![Str("Ø Price".into())])))
        );
        assert_eq!(
            parse_formula("sum(Sales[Region=North])").unwrap(),
            Call("sum".into(), vec![Index("Sales".into(), vec![b(BinOp::Eq, Name("Region".into()), Name("North".into()))])])
        );
    }

    #[test]
    fn remote_references() {
        assert_eq!(parse_formula("=Expenses!B2").unwrap(), Remote("Expenses".into(), Box::new(Name("B2".into()))));
        assert_eq!(
            parse_formula("=sum(\"My sheet\"!B2:B4)").unwrap(),
            Call("sum".into(), vec![Remote("My sheet".into(), Box::new(Range("B2".into(), "B4".into())))])
        );
        assert_eq!(parse_formula("=1 != 2").unwrap(), b(BinOp::Ne, Num(1.0), Num(2.0)));
        assert_eq!(parse_formula("=\"My sheet\".Amount").unwrap(), Member("My sheet".into(), "Amount".into()));
        assert_eq!(parse_formula("=\"My sheet\"[1, 2]").unwrap(), Index("My sheet".into(), vec![Num(1.0), Num(2.0)]));
    }

    #[test]
    fn layers() {
        let layer = |e: Expr, n: &str| Layer(Box::new(e), LayerSel::Name(n.into()));
        let offset = |e: Expr, n: i64| Layer(Box::new(e), LayerSel::Offset(n));
        assert_eq!(parse_formula("B2b").unwrap(), Name("B2b".into()));
        assert_eq!(parse_formula("B2.February").unwrap(), Member("B2".into(), "February".into()));
        assert_eq!(parse_formula("B2[-1]").unwrap(), offset(Name("B2".into()), -1));
        assert_eq!(parse_formula("Amount[+2] * 2").unwrap(), b(BinOp::Mul, offset(Name("Amount".into()), 2), Num(2.0)));
        assert_eq!(parse_formula("Amount[Total].February").unwrap(), layer(Index("Amount".into(), vec![Name("Total".into())]), "February"));
        assert_eq!(
            parse_formula("Expenses!B2.February").unwrap(),
            layer(Remote("Expenses".into(), Box::new(Name("B2".into()))), "February")
        );
        assert_eq!(parse_formula("@Amount[-1]").unwrap(), offset(At(Box::new(Name("Amount".into()))), -1));
        // Short ranges: only what changes follows the colon.
        assert_eq!(parse_formula("sum(B2a:c)").unwrap(), Call("sum".into(), vec![Range("B2a".into(), "c".into())]));
        assert_eq!(parse_formula("B2:4c").unwrap(), Range("B2".into(), "4c".into()));
        assert_eq!(parse_formula("B2:4").unwrap(), Range("B2".into(), "4".into()));
        assert_eq!(parse_formula("Expenses!B2a:D4c").unwrap(), Remote("Expenses".into(), Box::new(Range("B2a".into(), "D4c".into()))));
        // Arithmetic with numbers is unchanged.
        assert!(parse_formula("2*4c").is_err());
        assert_eq!(parse_formula("(1)+1").unwrap(), b(BinOp::Add, Num(1.0), Num(1.0)));
    }

    #[test]
    fn unit_literals() {
        assert_eq!(parse_formula("=5{m/s} * 2").unwrap(), b(BinOp::Mul, Quantity(5.0, "m/s".into()), Num(2.0)));
    }

    #[test]
    fn errors() {
        assert!(parse_formula("=1+").is_err());
        assert!(parse_formula("={m}").is_err());
        assert!(parse_formula("=(1").is_err());
        assert!(parse_formula("=\"abc").is_err());
    }
}
