//! Flags variables used in a `SELECT` expression (`SELECT (<expr> AS ?x)`)
//! that are neither bound in the query body nor by an earlier alias in the
//! same `SELECT` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by SELECT in expression "<expr>", but is not defined
//! in the query body."
//!
//! Variables that only occur inside `EXISTS`/`NOT EXISTS` are neither flagged
//! nor count as bound. Variables of the trailing `VALUES` clause count as
//! bound.

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
    LazyLock::new(|| DiagnosticCode::String("unbound-select-expr-var".to_string()));

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
        let text = "SELECT (STR(?undef) AS ?x) WHERE { ?s ?p ?o }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_reference_to_later_alias() {
        let text = "SELECT (?b + 1 AS ?a) (?o AS ?b) WHERE { ?s ?p ?o }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?b", 0));
    }

    #[test]
    fn triggered_next_to_exists() {
        let text = "SELECT (EXISTS { ?a <p> ?x } || ?unbound AS ?e) WHERE { ?a <q> ?b }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?unbound", 0));
    }

    #[test]
    fn not_triggered_by_reference_to_earlier_alias() {
        let result = run(
            diagnostics,
            "SELECT (?o AS ?b) (?b + 1 AS ?a) WHERE { ?s ?p ?o }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_inside_exists() {
        let result = run(
            diagnostics,
            "SELECT (EXISTS { ?a <p> ?x } AS ?e) WHERE { ?a <q> ?b }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run(
            diagnostics,
            "SELECT ?p ?v (STR(?v) AS ?w) WHERE { ?s ?p ?o } GROUP BY ?p VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }
}
