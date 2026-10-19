//! Flags variables used in an `ORDER BY` expression that are neither bound in
//! the query body nor introduced by an alias in the `SELECT` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by ORDER BY in expression "<expr>", but is not
//! defined in the query body and also not bound inside the SELECT clause."
//!
//! Unlike plain variables, expressions are checked in grouped and ungrouped
//! queries alike. Plain variables (also inside `ASC(...)`/`DESC(...)`) are
//! handled by `unbound_order_by_variable` and `ungrouped_order_by_variable`.
//! Variables of the trailing `VALUES` clause count as bound.

use super::{
    query_scope::{OrderKey, QueryScope, used_variables},
    warning,
};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-order-by-expr-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        .flat_map(|scope| {
            let visible_variables: HashSet<String> = scope
                .body_variables()
                .iter()
                .chain(scope.group_by_aliases())
                .chain(scope.values_variables())
                .chain(scope.select_aliases())
                .cloned()
                .collect();
            let select_hint = scope.select_hint();
            scope.order_keys().iter().flat_map(move |key| {
                let OrderKey::Expression(expression) = key else {
                    return Vec::new();
                };
                used_variables(expression)
                    .into_iter()
                    .filter(|var| !visible_variables.contains(&var.var_name()))
                    .map(|var| {
                        let message = format!(
                            "Variable {} was used by ORDER BY in expression \"{}\", \
                             but is not defined in the query body{}.",
                            var.syntax().text(),
                            expression.text(),
                            select_hint
                        );
                        (var.syntax().clone(), message)
                    })
                    .collect()
            })
        })
        .map(|(node, message)| warning(document, &node, &CODE, message))
        .collect();
    (!diagnostics.is_empty()).then_some(diagnostics)
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run_scoped},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_unbound_variable() {
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?undef)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_inside_order_direction() {
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY DESC(?o + ?undef)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?undef", 0));
    }

    #[test]
    fn triggered_in_grouped_query() {
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY STR(?undef)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?undef", 0));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run_scoped(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?o)");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?x (STR(?x) AS ?y) WHERE { } ORDER BY ?x STR(?x) VALUES ?x { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_plain_variable() {
        // INFO: Plain variables are handled by `unbound_order_by_variable`.
        let result = run_scoped(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?undef");
        assert!(result.is_empty());
    }
}
