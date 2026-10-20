//! Flags plainly selected variables (`SELECT ?x`) that are not bound in the
//! query body.
//!
//! The selected column of such a variable stays empty, even for solutions
//! where the query body matches. Variables of the trailing `VALUES` clause and
//! aliases of the `GROUP BY` and `SELECT` clauses count as bound.
//!
//! NOTE: QLever has no such warning, the message follows the style of its
//! warning for `SELECT` expressions.

use super::{query_scope::QueryScope, warning};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-select-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        .flat_map(|scope| {
            // NOTE: A grouped variable that is unbound is reported by `unbound_group_by_variable`.
            let grouped_variables = scope.grouped_variables();
            let visible_variables: HashSet<&String> = scope
                .body_variables()
                .iter()
                .chain(scope.values_variables())
                .chain(scope.group_by_aliases())
                .chain(scope.select_aliases())
                .chain(&grouped_variables)
                .collect();
            // NOTE: In a top level query with GROUP BY, ungrouped variables are reported as
            // errors by `ungrouped_select_variable`, so every selected variable is skipped.
            let checked = !scope.is_explicitly_grouped() || scope.is_sub_select();
            scope
                .select_clause()
                .filter(|_| checked)
                .map(|select_clause| select_clause.variables())
                .unwrap_or_default()
                .into_iter()
                .filter(|var| !visible_variables.contains(&var.var_name()))
                .collect::<Vec<_>>()
        })
        .map(|var| {
            let message = format!(
                "Variable {} was selected, but is not defined in the query body.",
                var.syntax().text()
            );
            warning(document, var.syntax(), &CODE, message)
        })
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
    fn triggered_by_unbound_variable() {
        let text = "SELECT ?lable WHERE { ?s <label> ?label }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?lable", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("?lable"));
    }

    #[test]
    fn triggered_only_for_unbound_variables() {
        let text = "SELECT ?s ?x ?o ?y WHERE { ?s ?p ?o }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
        assert_eq!(result[1].range, range_of(text, "?y", 0));
    }

    #[test]
    fn triggered_in_sub_select() {
        let text = "SELECT * WHERE { { SELECT ?x WHERE { ?s ?p ?o } } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn triggered_by_variable_bound_outside_sub_select() {
        // NOTE: A sub select does not see the variables of the surrounding query.
        let text = "SELECT * WHERE { ?s ?p ?o { SELECT ?o WHERE { ?s ?q ?v } } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_by_variable_bound_in_minus() {
        let text = "SELECT ?v WHERE { ?s ?p ?o MINUS { ?s ?q ?v } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn triggered_by_variable_only_in_filter() {
        let text = "SELECT ?x WHERE { ?s ?p ?o FILTER (?x > 1) }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn triggered_by_variable_only_in_exists() {
        let text = "SELECT ?v WHERE { ?s ?p ?o FILTER EXISTS { ?s ?q ?v } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn triggered_in_implicitly_grouped_query() {
        let text = "SELECT ?x (COUNT(?s) AS ?c) WHERE { ?s ?p ?o }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn triggered_in_grouped_sub_select() {
        // NOTE: `ungrouped_select_variable` only checks the top level query.
        let text = "SELECT * WHERE { { SELECT ?x WHERE { ?s ?p ?o } GROUP BY ?s } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn not_triggered_by_bound_variables() {
        let result = run_scoped(diagnostics, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_star_selection() {
        let result = run_scoped(diagnostics, "SELECT * WHERE { ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_optional_bind_and_values() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?v ?b ?w WHERE { ?s ?p ?o OPTIONAL { ?o ?q ?v } BIND (1 AS ?b) VALUES ?w { 1 } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_union_and_nested_group() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?a ?b ?c WHERE { { ?s <p> ?a } UNION { ?s <q> ?b } { ?s <r> ?c } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_selected_by_sub_select() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?s WHERE { { SELECT ?s WHERE { ?s ?p ?o } } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?s ?v WHERE { ?s ?p ?o } VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run_scoped(diagnostics, "SELECT (STR(?o) AS ?x) ?x WHERE { ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_group_by_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?k WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_grouped_query() {
        // INFO: Ungrouped variables are reported by `ungrouped_select_variable`,
        // unbound grouped variables by `unbound_group_by_variable`.
        let result = run_scoped(diagnostics, "SELECT ?x ?y WHERE { ?s ?p ?o } GROUP BY ?y");
        assert!(result.is_empty());
    }
}
