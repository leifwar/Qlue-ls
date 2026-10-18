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

use super::{
    query_scope::{GroupKey, QueryScope, used_variables},
    warning,
};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::sync::LazyLock;

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-group-by-expr-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        .flat_map(|scope| {
            let mut visible_variables = scope.body_variables().clone();
            scope.group_keys().iter().flat_map(move |key| {
                let (expression, alias) = match key {
                    GroupKey::Expression(expression) => (expression, None),
                    GroupKey::Alias(expression, var) => (expression, Some(var)),
                    GroupKey::Variable(_) => return Vec::new(),
                };
                let unbound: Vec<_> = used_variables(expression)
                    .into_iter()
                    .filter(|var| !visible_variables.contains(&var.var_name()))
                    .map(|var| {
                        let message = format!(
                            "Variable {} was used by GROUP BY in expression \"{}\", \
                             but is not defined in the query body or previously in the same GROUP BY.",
                            var.syntax().text(),
                            expression.text()
                        );
                        (var.syntax().clone(), message)
                    })
                    .collect();
                // NOTE: The target of an alias is visible to the following GROUP BY conditions.
                if let Some(alias) = alias {
                    visible_variables.insert(alias.var_name());
                }
                unbound
            })
        })
        .map(|(node, message)| warning(document, &node, &CODE, message))
        .collect();
    (!diagnostics.is_empty()).then_some(diagnostics)
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run_scoped},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_unbound_variable_in_expression() {
        let text = "SELECT (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?v))";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?v", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_unbound_variable_in_alias() {
        let text = "SELECT ?k (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?v) AS ?k)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn triggered_by_reference_to_later_alias() {
        let text = "SELECT ?k ?k2 (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (UCASE(?k2) AS ?k) (STR(?o) AS ?k2)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?k2", 1));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?k (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_reference_to_earlier_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?k ?k2 (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k) (UCASE(?k) AS ?k2)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_plain_variable() {
        // INFO: Plain variables are handled by `unbound_group_by_variable`.
        let result = run_scoped(
            diagnostics,
            "SELECT (MAX(?o) AS ?max) WHERE { ?s ?p ?o } GROUP BY ?undef",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_parenthesized_variable() {
        // INFO: Parenthesized variables are handled by `unbound_group_by_variable`.
        let result = run_scoped(diagnostics, "SELECT ?x WHERE { ?s ?p ?o } GROUP BY (?x)");
        assert!(result.is_empty());
    }
}
