//! Flags `MINUS` clauses that share no variables with the patterns before
//! them in the same group graph pattern.
//!
//! Mirrors the QLever warning (`QueryPlanner.cpp`):
//! "A MINUS clause that shares no variables with the patterns before it has no
//! effect and is ignored. Use FILTER NOT EXISTS if you want to remove all
//! results whenever the pattern of the MINUS clause matches."
//!
//! A `MINUS` at the start of a group has no preceding variables and is always
//! flagged. Variables bound in a preceding `OPTIONAL` count, variables of a
//! preceding `MINUS` do not.

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
    LazyLock::new(|| DiagnosticCode::String("disjoint-minus".to_string()));

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
    fn triggered_by_minus_without_shared_variables() {
        let text = "SELECT * WHERE { ?a <is-a> ?b MINUS { ?x <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "MINUS", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_minus_at_start_of_group() {
        let text = "SELECT * WHERE { MINUS { ?x <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "MINUS", 0));
    }

    #[test]
    fn triggered_once_per_disjoint_minus() {
        // NOTE: The variables of the first MINUS (`?y`) do not connect the second.
        let text = "SELECT * WHERE { ?a <is-a> ?b MINUS { ?x <is-a> ?y } MINUS { ?z <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(
            result.iter().map(|d| d.range.clone()).collect::<Vec<_>>(),
            vec![range_of(text, "MINUS", 0), range_of(text, "MINUS", 1)]
        );
    }

    #[test]
    fn not_triggered_by_shared_variable() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b MINUS { ?a <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_bound_in_optional() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b OPTIONAL { ?a <p> ?c } MINUS { ?c <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_filter_not_exists() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b FILTER NOT EXISTS { ?x <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }
}
