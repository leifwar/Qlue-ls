//! Flags variables used in a `HAVING` expression that are neither bound in the
//! query body nor introduced by an alias in the `SELECT` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by HAVING in expression "<expr>", but is not defined
//! in the query body and also not bound inside the SELECT clause."
//!
//! The part about the `SELECT` clause is only part of the message if the query
//! has one (i.e. not for `CONSTRUCT`, `ASK` or `DESCRIBE`). Variables of the
//! trailing `VALUES` clause count as bound, variables that only occur inside
//! `EXISTS`/`NOT EXISTS` do not.

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
    LazyLock::new(|| DiagnosticCode::String("unbound-having-var".to_string()));

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
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?x) > 1)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?x", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("SELECT clause"));
    }

    #[test]
    fn triggered_in_query_without_select_clause() {
        let text = "CONSTRUCT { ?p ?p ?p } WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?x) > 1)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
        assert!(!result[0].message.contains("SELECT clause"));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?s) > 1)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run(
            diagnostics,
            "SELECT ?p (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?p HAVING (?c > 1)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (?v > 1) VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }
}
