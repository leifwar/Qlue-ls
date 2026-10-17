//! Flags variables used in a `GROUP BY` expression or alias that are neither
//! bound in the query body nor by an earlier alias in the same `GROUP BY`.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by GROUP BY in expression "<expr>", but is not
//! defined in the query body or previously in the same GROUP BY."
//!
//! Plain (and parenthesized) variables are handled by
//! `unbound_group_by_variable`. Variables of the trailing `VALUES` clause do
//! not count as bound.

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
    LazyLock::new(|| DiagnosticCode::String("unbound-group-by-expr-var".to_string()));

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
    fn triggered_by_unbound_variable_in_expression() {
        let text = "SELECT (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?v))";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?v", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_unbound_variable_in_alias() {
        let text = "SELECT ?k (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?v) AS ?k)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn triggered_by_reference_to_later_alias() {
        let text = "SELECT ?k ?k2 (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (UCASE(?k2) AS ?k) (STR(?o) AS ?k2)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?k2", 1));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run(
            diagnostics,
            "SELECT ?k (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_reference_to_earlier_alias() {
        let result = run(
            diagnostics,
            "SELECT ?k ?k2 (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k) (UCASE(?k) AS ?k2)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_plain_variable() {
        // INFO: Plain variables are handled by `unbound_group_by_variable`.
        let result = run(
            diagnostics,
            "SELECT (MAX(?o) AS ?max) WHERE { ?s ?p ?o } GROUP BY ?undef",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_parenthesized_variable() {
        // INFO: Parenthesized variables are handled by `unbound_group_by_variable`.
        let result = run(diagnostics, "SELECT ?x WHERE { ?s ?p ?o } GROUP BY (?x)");
        assert!(result.is_empty());
    }
}
