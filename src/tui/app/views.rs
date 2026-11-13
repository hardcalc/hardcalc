//! Named views: the filter and the column settings of a table saved under
//! a name (`:view save open items`) and put back with `:view open items`.
//!
//! They live in a layer of their own, `views`, one rule per view:
//! `open items: filter Status: = open | view Note: hide`.

use super::{App, ViewKind, FILTER, VIEW};
use crate::model::Rule;

/// The layer that keeps the named views of a table.
pub const VIEWS: &str = "views";

/// The rules a saved view holds: (layer, target, body).
fn parse_view(body: &str) -> Vec<(String, String, String)> {
    body.split(" | ")
        .filter_map(|part| {
            let (layer, rule) = part.trim().split_once(' ')?;
            let (target, body) = rule.split_once(':')?;
            matches!(layer, FILTER | VIEW).then(|| (layer.to_string(), target.trim().to_string(), body.trim().to_string()))
        })
        .collect()
}

impl App {
    /// The rules of `layer` for the shown table, as written.
    fn rules_of(&self, layer: &str) -> Vec<(String, String)> {
        self.attached
            .iter()
            .filter(|a| a.sheet == self.view.sheet && a.layer.name == layer)
            .flat_map(|a| &a.layer.rules)
            .filter_map(|r| match r {
                Rule::Rule { target, body } => Some((target.clone(), body.clone())),
                Rule::Raw(_) => None,
            })
            .collect()
    }

    /// Names of the views saved for the shown table.
    pub fn view_names(&self) -> Vec<String> {
        self.rules_of(VIEWS).into_iter().map(|(name, _)| name).collect()
    }

    /// `:view` – see the command list.
    pub(super) fn view_command(&mut self, args: &[&str]) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":view works in tables");
        }
        let sheet = self.view.sheet;
        let saved = self.rules_of(VIEWS);
        match args {
            [] if saved.is_empty() => self.info("no view saved yet – set a filter or hide columns, then :view save Name"),
            [] => self.info(format!("views: {} · :view Name shows one · :view off all rows and columns", saved.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))),
            ["save"] | ["delete"] => self.error("usage: :view save Name · :view delete Name"),
            ["save", name @ ..] => {
                let name = name.join(" ");
                if name.contains([':', '|', '[', ']']) || matches!(name.as_str(), "off" | "save" | "delete") {
                    return self.error("a view name has no : | [ ] and is not off, save or delete");
                }
                let parts: Vec<String> = [FILTER, VIEW].iter().flat_map(|layer| self.rules_of(layer).into_iter().map(move |(t, b)| format!("{layer} {t}: {b}"))).collect();
                if parts.is_empty() {
                    return self.error("nothing to save: no filter and no column setting – :view off is the view without any");
                }
                let n = parts.len();
                self.set_layer_rules(VIEWS, sheet, &[(name.clone(), parts.join(" | "))]);
                self.info(format!("view {name} saved ({n} rule(s)) · :view {name} shows it again"));
            }
            ["delete", name @ ..] => {
                let name = name.join(" ");
                if !saved.iter().any(|(n, _)| *n == name) {
                    return self.error(format!("no view '{name}'"));
                }
                self.set_layer_rules(VIEWS, sheet, &[(name.clone(), String::new())]);
                self.info(format!("view {name} deleted"));
            }
            ["off"] => self.apply_view("all rows and columns", Vec::new()),
            name => {
                let name = name.join(" ");
                match saved.iter().find(|(n, _)| *n == name) {
                    Some((_, body)) => self.apply_view(&name, parse_view(body)),
                    None => self.error(format!("no view '{name}' – :view lists them, :view save {name} saves the current one")),
                }
            }
        }
    }

    /// Replaces the filter and view rules of the shown table by `rules`
    /// (one undo step) and switches both layers on.
    fn apply_view(&mut self, name: &str, rules: Vec<(String, String, String)>) {
        let sheet = self.view.sheet;
        let steps = self.undo.len();
        for layer in [FILTER, VIEW] {
            let wanted: Vec<(String, String)> = rules.iter().filter(|(l, ..)| l == layer).map(|(_, t, b)| (t.clone(), b.clone())).collect();
            let mut changes: Vec<(String, String)> =
                self.rules_of(layer).into_iter().filter(|(t, _)| !wanted.iter().any(|(w, _)| w == t)).map(|(t, _)| (t, String::new())).collect();
            changes.extend(wanted.into_iter().filter(|(t, b)| self.layer_rule(layer, sheet, t).as_deref() != Some(b.as_str())));
            self.set_layer_rules(layer, sheet, &changes);
            self.hidden_layers.remove(layer);
            self.layer_overrides.remove(&(sheet, layer.to_string()));
        }
        // Both layers changed in one step.
        self.undo.truncate(steps + 1);
        self.revalidate();
        self.view.anchor = None;
        self.snap_to_shown();
        self.snap_to_shown_col();
        let rows = self.dims().0;
        let shown = rows - self.hidden_count().map_or(0, |(n, _)| n);
        let hidden = self.hidden_cols();
        let columns = if hidden > 0 { format!(", {hidden} column(s) hidden") } else { String::new() };
        self.info(format!("view {name}: {shown} of {rows} rows{columns}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_rules() {
        assert_eq!(
            parse_view("filter Status: in(open, paid) | view Note: hide | filter Amount: >= 100"),
            [
                ("filter".to_string(), "Status".to_string(), "in(open, paid)".to_string()),
                ("view".to_string(), "Note".to_string(), "hide".to_string()),
                ("filter".to_string(), "Amount".to_string(), ">= 100".to_string()),
            ]
        );
        assert!(parse_view("something else").is_empty());
    }
}
