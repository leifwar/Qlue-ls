//! Flags `BIND` targets that are already bound earlier in the enclosing group
//! graph pattern.
//!
//! SPARQL forbids this: "The variable assigned in a BIND clause must not be
//! already in-scope from the preceding elements in the group graph pattern in
//! which it is used." QLever rejects such a query with the error
//! (`SparqlQleverVisitor.cpp`):
//! "The target variable ?v of an AS clause was already used before in the
//! query body."
//!
//! Like for `unbound_bind_variable`, variables bound inside a preceding nested
//! group, `OPTIONAL`, `SERVICE` or (selected by) a subquery are in scope,
//! variables inside `MINUS` or `FILTER` are not. Variables bound after the
//! `BIND` are fine, they are joined with its result.

use super::{bound_variables, warning};
use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::TextDocumentItem,
    },
};
use ll_sparql_parser::ast::{AstNode, Bind, Unit};
use std::{iter::successors, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("invalid-bind-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter_map(Bind::cast)
        .filter_map(|bind| {
            let target = bind.visible_variables().pop()?;
            // INFO: The BIND is wrapped in a `GraphPatternNotTriples`, whose siblings are the
            // elements of the enclosing group.
            successors(
                bind.syntax()
                    .parent()
                    .and_then(|pattern| pattern.prev_sibling()),
                |node| node.prev_sibling(),
            )
            .flat_map(bound_variables)
            .any(|var| var.var_name() == target.var_name())
            .then_some(target)
        })
        .map(|target| {
            let message = format!(
                "The target variable {} of an AS clause was already used before in the query body.",
                target.syntax().text()
            );
            Diagnostic {
                severity: DiagnosticSeverity::Error,
                ..warning(document, target.syntax(), &CODE, message)
            }
        })
        .collect();

    (!diagnostics.is_empty()).then_some(diagnostics)
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
    fn triggered_by_variable_bound_by_triple() {
        let text = "SELECT * WHERE { ?item <price> ?price BIND (?price * 2 AS ?price) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?price", 2));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("?price"));
    }

    #[test]
    fn triggered_by_earlier_bind() {
        let text = "SELECT * WHERE { BIND (1 AS ?x) BIND (2 AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_values() {
        let text = "SELECT * WHERE { VALUES ?x { 1 } BIND (2 AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_optional() {
        let text = "SELECT * WHERE { ?s ?p ?o OPTIONAL { ?o ?q ?x } BIND (2 AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_union() {
        let text = "SELECT * WHERE { { ?s <p> ?x } UNION { ?s <q> ?o } BIND (2 AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_graph_variable() {
        let text = "SELECT * WHERE { GRAPH ?g { ?s ?p ?o } BIND (<g> AS ?g) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?g", 1));
    }

    #[test]
    fn triggered_by_variable_selected_by_subquery() {
        let text = "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } BIND (1 AS ?s) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?s", 2));
    }

    #[test]
    fn triggered_in_nested_group() {
        let text = "SELECT * WHERE { { ?s ?p ?o BIND (1 AS ?o) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn not_triggered_by_fresh_variable() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?item <price> ?price BIND (?price * 2 AS ?double) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_bound_after_bind() {
        let result = run(diagnostics, "SELECT * WHERE { BIND (1 AS ?o) ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_of_outer_group() {
        // NOTE: The nested group starts without variables in scope.
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o { BIND (1 AS ?o) } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_minus_or_filter() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o MINUS { ?s ?q ?x } FILTER (?y > 1) BIND (1 AS ?x) BIND (2 AS ?y) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_minus_or_filter_in_nested_group() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { ?s ?p ?o MINUS { ?s ?q ?x } FILTER (?y > 1) } BIND (1 AS ?x) BIND (2 AS ?y) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_exists() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o FILTER EXISTS { ?s ?q ?x } BIND (1 AS ?x) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_not_selected_by_subquery() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } BIND (1 AS ?o) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_bind_in_expression() {
        // INFO: Using the target in the expression without binding it is `unbound_bind_variable`.
        let result = run(diagnostics, "SELECT * WHERE { BIND (?x AS ?x) }");
        assert!(result.is_empty());
    }
}
