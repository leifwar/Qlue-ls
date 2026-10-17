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

use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode},
        textdocument::TextDocumentItem,
    },
};
use ll_sparql_parser::ast::Unit;
use std::sync::LazyLock;

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-order-by-var".to_string()));

pub(super) fn diagnostics(
    _document: &TextDocumentItem,
    _ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    todo!()
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_unbound_variable() {
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?undef";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_empty_query_body() {
        let text = "SELECT * WHERE { } ORDER BY ?s";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?s", 0));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?o");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run(
            diagnostics,
            "SELECT ?s (STRLEN(STR(?o)) AS ?len) WHERE { ?s ?p ?o } ORDER BY ?len",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run(
            diagnostics,
            "SELECT ?x WHERE { } ORDER BY ?x VALUES ?x { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_bound_variable_with_order_direction() {
        let result = run(
            diagnostics,
            "SELECT ?s WHERE { ?s ?p ?o } ORDER BY DESC(?o)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_expression() {
        // INFO: Expressions are handled by `unbound_order_by_expression_variable`.
        let result = run(
            diagnostics,
            "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?undef)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_grouped_query() {
        // INFO: Grouped queries are handled by `ungrouped_order_by_variable`.
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?undef",
        );
        assert!(result.is_empty());
    }
}
