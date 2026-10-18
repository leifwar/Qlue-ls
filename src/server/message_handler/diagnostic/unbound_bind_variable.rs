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

use super::bound_variables;
use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
};
use ll_sparql_parser::{
    ast::{AstNode, Bind, Expression, Unit, Var},
    syntax_kind::SyntaxKind,
};
use std::{collections::HashSet, iter::successors, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-bind-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter_map(Bind::cast)
        .flat_map(|bind| {
            // INFO: The BIND is wrapped in a `GraphPatternNotTriples`, whose siblings are the
            // elements of the enclosing group.
            let preceding_variables: HashSet<String> = successors(
                bind.syntax()
                    .parent()
                    .and_then(|pattern| pattern.prev_sibling()),
                |node| node.prev_sibling(),
            )
            .flat_map(bound_variables)
            .map(|var| var.var_name())
            .collect();
            bind.syntax()
                .first_child()
                .and_then(Expression::cast)
                .map(|expression| {
                    expression
                        .visible_variables()
                        .into_iter()
                        .filter(|variable| !inside_exists(variable, &expression))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
                .into_iter()
                .filter(move |variable| !preceding_variables.contains(&variable.var_name()))
        })
        .map(|variable| Diagnostic {
            range: Range::from_byte_offset_range(variable.syntax().text_range(), &document.text)
                .expect("Every variable should be within the document"),
            severity: DiagnosticSeverity::Warning,
            code: Some((*CODE).clone()),
            code_description: None,
            source: Some("qlue-ls".to_string()),
            message: format!(
                "The variable {} was used in the expression of a BIND clause \
                 but was not previously bound in the query",
                variable.syntax().text()
            ),
            data: None,
        })
        .collect();

    (!diagnostics.is_empty()).then_some(diagnostics)
}

/// Whether `variable` occurs inside an `EXISTS`/`NOT EXISTS` of `expression`.
///
/// NOTE: Such variables live in the scope of the EXISTS pattern and need not
/// be bound by the surrounding query.
fn inside_exists(variable: &Var, expression: &Expression) -> bool {
    variable
        .syntax()
        .ancestors()
        // INFO: Only look within the expression, the BIND itself may be nested in an EXISTS.
        .take_while(|node| node != expression.syntax())
        .any(|node| {
            matches!(
                node.kind(),
                SyntaxKind::ExistsFunc | SyntaxKind::NotExistsFunc
            )
        })
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
    fn triggered_despite_preceding_filter() {
        // NOTE: FILTER never binds variables.
        let text = "SELECT * WHERE { FILTER (?o > 1) BIND (?o AS ?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_next_to_exists() {
        let text = "SELECT * WHERE { ?a <q> ?b BIND (EXISTS { ?a <p> ?x } || ?unbound AS ?e) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?unbound", 0));
    }

    #[test]
    fn triggered_in_bind_nested_in_exists() {
        let text = "SELECT * WHERE { ?s ?p ?o FILTER EXISTS { BIND (?a AS ?b) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?a", 0));
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
