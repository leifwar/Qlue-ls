//! Flags variables used in the expression of a `BIND` that are not bound
//! earlier in the enclosing group graph pattern.
//!
//! Mirrors the QLever warning (`SparqlQleverVisitor.cpp`):
//! "The variable ?v was used in the expression of a BIND clause but was not
//! previously bound in the query"
//!
//! Every group `{ ... }` starts without visible variables. Variables bound
//! inside a preceding nested group, `OPTIONAL`, `SERVICE` or (selected by) a
//! subquery become visible after it, variables inside `MINUS` never do.
//! Variables that only occur inside `EXISTS`/`NOT EXISTS` are neither flagged
//! nor count as bound.
//!
//! NOTE: QLever deliberately does not warn about `FILTER`, since a filter may
//! use variables that are bound later in the same group.

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
    LazyLock::new(|| DiagnosticCode::String("unbound-bind-var".to_string()));

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
        let text = "SELECT * WHERE { BIND (?a AS ?b) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?a", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_variable_bound_after_bind() {
        let text = "SELECT * WHERE { BIND (STR(?o) AS ?x) ?s ?p ?o }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 0));
    }

    #[test]
    fn triggered_in_nested_group() {
        // NOTE: The nested group does not see `?o` of the outer group.
        let text = "SELECT * WHERE { ?s ?p ?o { BIND (STR(?o) AS ?x) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_by_variable_bound_in_minus() {
        let text = "SELECT * WHERE { ?s ?p ?o MINUS { ?s ?q ?v } BIND (?v AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 1));
    }

    #[test]
    fn triggered_by_variable_not_selected_by_subquery() {
        let text = "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } BIND (?o AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn not_triggered_by_preceding_triple() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o BIND (STR(?o) AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_earlier_bind() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o BIND (STR(?o) AS ?x) BIND (UCASE(?x) AS ?y) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_preceding_nested_group() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { ?s ?p ?o } BIND (STR(?o) AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_preceding_optional() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o OPTIONAL { ?o ?q ?v } BIND (?v AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_preceding_values() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { VALUES ?o { 1 2 } BIND (?o + 1 AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_selected_by_subquery() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } BIND (STR(?s) AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_inside_exists() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <q> ?b BIND (EXISTS { ?a <p> ?x } AS ?e) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_filter() {
        let result = run(diagnostics, "SELECT * WHERE { FILTER (?o > 1) ?s ?p ?o }");
        assert!(result.is_empty());
    }
}
