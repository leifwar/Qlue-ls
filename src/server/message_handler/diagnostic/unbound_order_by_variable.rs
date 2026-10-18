//! Flags variables in `ORDER BY` of a query without grouping that are neither
//! bound in the query body nor introduced by an alias in the `SELECT` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by ORDER BY, but is not defined in the query body and
//! also not bound inside the SELECT clause."
//!
//! A variable wrapped in `ASC(...)`/`DESC(...)` counts as a plain variable.
//! Expressions are handled by `unbound_order_by_expression_variable`, grouped
//! queries by `ungrouped_order_by_variable`. Variables of the trailing `VALUES`
//! clause count as bound.

use super::{
    query_scope::{OrderKey, QueryScope},
    warning,
};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-order-by-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        // INFO: Grouped queries are handled by `ungrouped_order_by_variable`.
        .filter(|scope| !scope.is_grouped())
        .flat_map(|scope| {
            let visible_variables: HashSet<String> = scope
                .body_variables()
                .iter()
                .chain(scope.values_variables())
                .chain(scope.select_aliases())
                .cloned()
                .collect();
            let select_hint = scope.select_hint();
            scope.order_keys().iter().filter_map(move |key| match key {
                OrderKey::Variable(var) if !visible_variables.contains(&var.var_name()) => {
                    let message = format!(
                        "Variable {} was used by ORDER BY, but is not defined in the query body{}.",
                        var.syntax().text(),
                        select_hint
                    );
                    Some((var.syntax().clone(), message))
                }
                _ => None,
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
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?undef";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_empty_query_body() {
        let text = "SELECT * WHERE { } ORDER BY ?s";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?s", 0));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run_scoped(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?o");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_graph_variable() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?g WHERE { GRAPH ?g { ?s ?p ?o } } ORDER BY ?g",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?s (STRLEN(STR(?o)) AS ?len) WHERE { ?s ?p ?o } ORDER BY ?len",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?x WHERE { } ORDER BY ?x VALUES ?x { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_bound_variable_with_order_direction() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?s WHERE { ?s ?p ?o } ORDER BY DESC(?o)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_expression() {
        // INFO: Expressions are handled by `unbound_order_by_expression_variable`.
        let result = run_scoped(
            diagnostics,
            "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?undef)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_grouped_query() {
        // INFO: Grouped queries are handled by `ungrouped_order_by_variable`.
        let result = run_scoped(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?undef",
        );
        assert!(result.is_empty());
    }
}
