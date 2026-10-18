//! Flags variables in `GROUP BY` that are not bound in the query body.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by GROUP BY, but is not defined in the query body."
//!
//! A parenthesized variable (`GROUP BY (?v)`) counts as a plain variable,
//! other expressions and aliases are handled by
//! `unbound_group_by_expression_variable`. Variables that only occur inside
//! `MINUS`, inside `EXISTS`/`NOT EXISTS` or in the trailing `VALUES` clause do
//! not count as bound.

use super::bound_variables;
use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
};
use ll_sparql_parser::{
    ast::{AstNode, SelectQuery, Unit},
    syntax_kind::SyntaxKind,
};
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-group-by-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let result: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter_map(SelectQuery::cast)
        .map(|select_query| {
            let available_variables = body_variables(&select_query);
            select_query
                .soulution_modifier()
                .and_then(|solution_modifier| solution_modifier.group_clause())
                .iter()
                .flat_map(|group_clause| {
                    let group_variables = group_clause.visible_variables();
                    group_variables
                        .into_iter()
                        .filter(|var| !available_variables.contains(&var.var_name()))
                })
                .map(|var| Diagnostic {
                    range: Range::from_byte_offset_range(var.syntax().text_range(), &document.text)
                        .unwrap(),
                    severity: DiagnosticSeverity::Warning,
                    code: Some((*CODE).clone()),
                    code_description: None,
                    source: Some("qlue-ls".to_string()),
                    message: format!(
                        "Variable {} was used by GROUP BY, but is not defined in the query body.",
                        var.syntax().text()
                    ),
                    data: None,
                })
                .collect::<Vec<_>>()
        })
        .flatten()
        .collect();
    (!result.is_empty()).then_some(result)
}

/// The names of the variables bound by the query body (`WHERE` clause) of `select_query`.
fn body_variables(select_query: &SelectQuery) -> HashSet<String> {
    select_query
        .where_clause()
        .and_then(|where_clause| where_clause.group_graph_pattern())
        .map(|ggp| {
            match ggp
                .syntax()
                .children()
                .find(|child| child.kind() == SyntaxKind::GroupGraphPatternSub)
            {
                // NOTE: Collected per element, since MINUS and FILTER do not bind variables.
                Some(group) => group.children().flat_map(bound_variables).collect(),
                // INFO: The body is a sub select.
                None => ggp.visible_variables(),
            }
        })
        .unwrap_or_default()
        .into_iter()
        .map(|var| var.var_name())
        .collect()
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
        let text = "SELECT (MAX(?o) AS ?max) WHERE { ?s ?p ?o } GROUP BY ?undef";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_parenthesized_variable() {
        let text = "SELECT ?x WHERE { ?s ?p ?o } GROUP BY (?x)";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_despite_trailing_values() {
        // NOTE: Variables of the trailing VALUES are not visible to GROUP BY.
        let text = "SELECT ?v WHERE { ?s ?p ?o } GROUP BY ?v VALUES ?v { 1 }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 1));
    }

    #[test]
    fn triggered_by_variable_only_bound_in_minus() {
        let text = "SELECT ?p WHERE { ?s ?q ?o MINUS { ?s ?p ?x } } GROUP BY ?p";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?p", 2));
    }

    #[test]
    fn triggered_only_for_group_by_variable() {
        // INFO: The unbound `?y` in ORDER BY is reported by `ungrouped_order_by_variable`.
        let text = "SELECT ?x WHERE { } GROUP BY ?x ORDER BY ?y";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_select_alias() {
        // NOTE: GROUP BY is evaluated before the SELECT clause, aliases are not visible to it.
        let text = "SELECT (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?c";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?c", 1));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run(
            diagnostics,
            "SELECT ?p (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?p",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_bound_in_nested_group() {
        let result = run(diagnostics, "SELECT ?p WHERE { { ?s ?p ?o } } GROUP BY ?p");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_bound_in_optional() {
        let result = run(
            diagnostics,
            "SELECT ?p WHERE { ?s ?q ?o OPTIONAL { ?s ?p ?x } } GROUP BY ?p",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_expression() {
        // INFO: Expressions are handled by `unbound_group_by_expression_variable`.
        let result = run(
            diagnostics,
            "SELECT (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY (STR(?v))",
        );
        assert!(result.is_empty());
    }
}
