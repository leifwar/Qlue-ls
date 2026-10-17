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
    LazyLock::new(|| DiagnosticCode::String("unbound-order-by-expr-var".to_string()));

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
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?undef)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_inside_order_direction() {
        let text = "SELECT ?s WHERE { ?s ?p ?o } ORDER BY DESC(?o + ?undef)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?undef", 0));
    }

    #[test]
    fn triggered_in_grouped_query() {
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY STR(?undef)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?undef", 0));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY STR(?o)");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run(
            diagnostics,
            "SELECT ?x (STR(?x) AS ?y) WHERE { } ORDER BY ?x STR(?x) VALUES ?x { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_plain_variable() {
        // INFO: Plain variables are handled by `unbound_order_by_variable`.
        let result = run(diagnostics, "SELECT ?s WHERE { ?s ?p ?o } ORDER BY ?undef");
        assert!(result.is_empty());
    }
}
