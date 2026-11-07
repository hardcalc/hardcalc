//! User-defined functions (like Excel's LAMBDA), declared in a ```functions block:
//!
//! ```text
//! gross(x) = x * 1.19                   # gross amount with 19 % VAT
//! share(part, total) = part / total
//! ```
//!
//! Parameters are local names in the body; everything else resolves as in
//! the calling cell's table. Functions may call other functions (and
//! themselves, up to a depth limit).

use crate::formula::{parse_formula, Expr};

/// Names that cannot be used for own functions.
pub fn is_builtin(name: &str) -> bool {
    crate::builtins::find(name).is_some() || matches!(name, "true" | "false" | "wahr" | "falsch")
}

fn is_identifier(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// One definition line as written.
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: String,
    pub description: Option<String>,
}

impl FnDef {
    /// `name(a, b) = body   # description`
    pub fn parse(line: &str) -> Result<FnDef, String> {
        let (def, description) = match line.split_once(" #") {
            Some((d, c)) => (d, Some(c.trim().to_string()).filter(|c| !c.is_empty())),
            None => (line, None),
        };
        let (head, body) = def.split_once('=').ok_or("expected `name(params) = formula`")?;
        let head = head.trim();
        let (name, params) = match head.split_once('(') {
            Some((n, p)) => (n.trim(), p.strip_suffix(')').ok_or("missing ')' after the parameters")?),
            None => (head, ""),
        };
        let name = name.to_lowercase();
        if !is_identifier(&name) {
            return Err(format!("'{name}' is not a valid function name"));
        }
        let params: Vec<String> = params.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        if let Some(bad) = params.iter().find(|p| !is_identifier(p)) {
            return Err(format!("'{bad}' is not a valid parameter name"));
        }
        Ok(FnDef { name, params, body: body.trim().to_string(), description })
    }

    pub fn to_line(&self) -> String {
        let mut line = format!("{}({}) = {}", self.name, self.params.join(", "), self.body);
        if let Some(d) = &self.description {
            line.push_str(&format!("   # {d}"));
        }
        line
    }

    /// The parsed body, checked against built-in names.
    pub fn compile(&self) -> Result<UserFn, String> {
        if is_builtin(&self.name) {
            return Err(format!("'{}' is a built-in function", self.name));
        }
        let body = parse_formula(&self.body).map_err(|e| format!("{}: {e}", self.name))?;
        Ok(UserFn { params: self.params.clone(), body })
    }
}

/// A compiled user function.
#[derive(Debug, Clone, PartialEq)]
pub struct UserFn {
    pub params: Vec<String>,
    pub body: Expr,
}

/// Definitions of a ```functions block: (line index, definition), plus errors.
pub fn parse_block(lines: &[String]) -> (Vec<(usize, FnDef)>, Vec<String>) {
    let mut defs = Vec::new();
    let mut errors = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        match FnDef::parse(t) {
            Ok(d) => defs.push((i, d)),
            Err(e) => errors.push(format!("line {}: {e}", i + 1)),
        }
    }
    (defs, errors)
}

/// Replaces parameter names in `body` by argument expressions (for exports
/// that inline user functions).
pub fn substitute(body: &Expr, params: &[String], args: &[Expr]) -> Expr {
    let sub = |e: &Expr| substitute(e, params, args);
    match body {
        Expr::Name(n) => match params.iter().position(|p| p == n) {
            Some(i) => args.get(i).cloned().unwrap_or(Expr::Name(n.clone())),
            None => body.clone(),
        },
        Expr::Neg(x) => Expr::Neg(Box::new(sub(x))),
        Expr::At(x) => Expr::At(Box::new(sub(x))),
        Expr::Percent(x) => Expr::Percent(Box::new(sub(x))),
        Expr::Binary(op, l, r) => Expr::Binary(*op, Box::new(sub(l)), Box::new(sub(r))),
        Expr::Call(f, a) => Expr::Call(f.clone(), a.iter().map(sub).collect()),
        Expr::Index(t, a) => Expr::Index(t.clone(), a.iter().map(sub).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions() {
        let d = FnDef::parse("Gross(x) = x * 1.19   # with VAT").unwrap();
        assert_eq!((d.name.as_str(), d.params.clone(), d.body.as_str()), ("gross", vec!["x".to_string()], "x * 1.19"));
        assert_eq!(d.description.as_deref(), Some("with VAT"));
        assert_eq!(d.to_line(), "gross(x) = x * 1.19   # with VAT");
        assert_eq!(FnDef::parse("pi() = 3.14159").unwrap().params.len(), 0);
        assert!(FnDef::parse("sum(x) = x").unwrap().compile().is_err(), "built-in name");
        assert!(FnDef::parse("f(1x) = 1").is_err());
        assert!(FnDef::parse("f(x) = x +").unwrap().compile().is_err());
        let (defs, errors) = parse_block(&["a(x) = x".into(), "# comment".into(), "kaputt".into()]);
        assert_eq!((defs.len(), errors.len()), (1, 1));
    }

    #[test]
    fn inlining() {
        let f = FnDef::parse("share(t, g) = t / g").unwrap().compile().unwrap();
        let args = [Expr::Name("B1".into()), Expr::Call("sum".into(), vec![Expr::Name("B".into())])];
        assert_eq!(substitute(&f.body, &f.params, &args), parse_formula("B1 / sum(B)").unwrap());
    }
}
