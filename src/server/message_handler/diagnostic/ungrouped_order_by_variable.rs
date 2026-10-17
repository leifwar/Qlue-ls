//! Flags variables in `ORDER BY` of a grouped query that are neither grouped,
//! nor introduced by an alias in the `SELECT` clause, nor bound by the
//! trailing `VALUES` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used in an ORDER BY clause, but is neither grouped nor
//! created as an alias in the SELECT clause."
//!
//! A query is also grouped if it has no `GROUP BY` but uses an aggregate in
//! the `SELECT` clause. In that case the message gets the note:
//! "Note: The GROUP BY in this query is implicit because an aggregate
//! expression was used in the SELECT clause"
//!
//! A variable wrapped in `ASC(...)`/`DESC(...)` counts as a plain variable,
//! expressions are handled by `unbound_order_by_expression_variable`.

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
    LazyLock::new(|| DiagnosticCode::String("ungrouped-order-by-var".to_string()));

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
    fn triggered_by_ungrouped_variable() {
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?s";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?s", 1));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(!diagnostic.message.contains("implicit"));
    }

    #[test]
    fn triggered_with_implicit_grouping() {
        let text = "SELECT (COUNT(?o) AS ?c) WHERE { ?s ?p ?o } ORDER BY ?s";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?s", 1));
        assert!(result[0].message.contains("implicit"));
    }

    #[test]
    fn triggered_despite_unrelated_trailing_values() {
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?s VALUES ?v { 1 }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?s", 1));
    }

    #[test]
    fn triggered_next_to_unbound_group_by_variable() {
        // INFO: The unbound `?x` in GROUP BY is reported by `unbound_group_by_variable`.
        let text = "SELECT ?x WHERE { } GROUP BY ?x ORDER BY ?y";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?y", 0));
    }

    #[test]
    fn not_triggered_by_grouped_variable() {
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?p",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run(
            diagnostics,
            "SELECT ?p (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY DESC(?c)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY ?v VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_without_grouping() {
        // INFO: Ungrouped queries are handled by `unbound_order_by_variable`.
        let result = run(diagnostics, "SELECT ?p WHERE { ?s ?p ?o } ORDER BY ?s");
        assert!(result.is_empty());
    }
}
