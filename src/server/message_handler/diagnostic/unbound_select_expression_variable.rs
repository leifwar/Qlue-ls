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

use super::{
    query_scope::{QueryScope, used_variables},
    warning,
};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-select-expr-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        .flat_map(|scope| {
            let mut visible_variables: HashSet<String> = scope
                .body_variables()
                .iter()
                .chain(scope.group_by_aliases())
                .chain(scope.values_variables())
                .cloned()
                .collect();
            // NOTE: In a grouped top level query, unaggregated variables that are not
            // grouped are reported as errors by `ungrouped_select_variable`, so they are
            // skipped here to avoid two diagnostics for the same variable.
            let grouped_variables =
                (!scope.is_sub_select() && scope.is_grouped()).then(|| scope.grouped_variables());
            scope
                .select_clause()
                .map(|select_clause| select_clause.assignments())
                .unwrap_or_default()
                .into_iter()
                .flat_map(move |assignment| {
                    let expression = assignment.expression.syntax();
                    let ungrouped_variables: Vec<_> = grouped_variables
                        .as_ref()
                        .map(|grouped_variables| {
                            assignment
                                .expression
                                .unaggregated_variables()
                                .into_iter()
                                .filter(|var| !grouped_variables.contains(&var.var_name()))
                                .map(|var| var.syntax().clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    let unbound: Vec<_> = used_variables(expression)
                        .into_iter()
                        .filter(|var| !visible_variables.contains(&var.var_name()))
                        .filter(|var| !ungrouped_variables.contains(var.syntax()))
                        .map(|var| {
                            let message = format!(
                                "Variable {} was used by SELECT in expression \"{}\", \
                                 but is not defined in the query body.",
                                var.syntax().text(),
                                expression.text()
                            );
                            (var.syntax().clone(), message)
                        })
                        .collect();
                    // NOTE: The target of an alias is visible to the following aliases.
                    visible_variables.insert(assignment.variable.var_name());
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
    fn triggered_by_unbound_variable() {
        let text = "SELECT (STR(?undef) AS ?x) WHERE { ?s ?p ?o }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?undef", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_reference_to_later_alias() {
        let text = "SELECT (?b + 1 AS ?a) (?o AS ?b) WHERE { ?s ?p ?o }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?b", 0));
    }

    #[test]
    fn triggered_next_to_exists() {
        let text = "SELECT (EXISTS { ?a <p> ?x } || ?unbound AS ?e) WHERE { ?a <q> ?b }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?unbound", 0));
    }

    #[test]
    fn not_triggered_by_reference_to_earlier_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT (?o AS ?b) (?b + 1 AS ?a) WHERE { ?s ?p ?o }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_inside_exists() {
        let result = run_scoped(
            diagnostics,
            "SELECT (EXISTS { ?a <p> ?x } AS ?e) WHERE { ?a <q> ?b }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?p ?v (STR(?v) AS ?w) WHERE { ?s ?p ?o } GROUP BY ?p VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_ungrouped_variable() {
        // INFO: Unaggregated, ungrouped variables are reported by `ungrouped_select_variable`.
        let result = run_scoped(
            diagnostics,
            "SELECT (STR(?x) AS ?y) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn triggered_by_aggregated_variable_in_grouped_query() {
        let text = "SELECT (COUNT(?x) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?s";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn triggered_by_ungrouped_variable_in_sub_select() {
        // NOTE: `ungrouped_select_variable` only checks the top level query.
        let text = "SELECT * WHERE { { SELECT (STR(?x) AS ?y) WHERE { ?s ?p ?o } GROUP BY ?s } }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }
}
