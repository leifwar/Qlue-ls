//! Flags `SELECT` variables that are illegal in a grouped query.
//!
//! When a query groups its results, every projected variable must either appear
//! in the `GROUP BY` clause or be used inside an aggregate. This reports both
//! plainly selected variables and unaggregated variables in `SELECT`
//! expressions that violate that rule.
//!
//! Without `GROUP BY`, an aggregate in the `SELECT` clause groups the query
//! implicitly into a single group, so the same rule applies. Like QLever, the
//! variables of the trailing `VALUES` clause count as grouped, since they are
//! joined after the grouping.

use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
};
use ll_sparql_parser::{
    SyntaxNode,
    ast::{AstNode, SelectClause, Unit},
    syntax_kind::SyntaxKind,
};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("ungrouped-select-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let query_unit = ast.as_query()?;
    let select_query = query_unit.select_query()?;
    let group_clause = select_query
        .soulution_modifier()
        .and_then(|solution_modifier| solution_modifier.group_clause());
    // NOTE: Without GROUP BY, the first aggregate in the SELECT clause groups the query implicitly.
    let implicit_aggregate = match group_clause {
        Some(_) => None,
        None => Some(first_aggregate(&select_query.select_clause()?)?),
    };
    // NOTE: The variables specified in the GROUP BY and the trailing VALUES clause
    let group_vars_str: HashSet<String> = group_clause
        .map(|group_clause| group_clause.visible_variables())
        .unwrap_or_default()
        .into_iter()
        .chain(
            select_query
                .values_clause()
                .map(|values_clause| values_clause.visible_variables())
                .unwrap_or_default(),
        )
        .map(|var| var.text())
        .collect();
    let message = |var: String, fallback: &str| match &implicit_aggregate {
        Some(aggregate) => format!(
            "{var} is not aggregated, but the aggregate {} groups the query implicitly",
            aggregate.text()
        ),
        None => format!("{var} {fallback}"),
    };

    // NOTE: ALL variables used SELECT CLAUSE
    let selected_variables: Vec<_> = select_query.select_clause()?.variables();

    // NOTE: ALL variables used in assignments in the SELECT CLAUSE that are not part of an aggregation
    let unaggregated_variables = select_query
        .select_clause()?
        .assignments()
        .into_iter()
        .enumerate()
        .flat_map(|(idx, assignment)| {
            assignment
                .expression
                .unaggregated_variables()
                .into_iter()
                .map(move |var| (idx, var))
        });

    // NOTE: The variables derived in assignments in the SELECT CLAUSE
    let derived_variables: HashMap<String, usize> = HashMap::from_iter(
        select_query
            .select_clause()?
            .assignments()
            .into_iter()
            .enumerate()
            .map(|(idx, assignment)| (assignment.variable.text(), idx)),
    );

    Some(
        selected_variables
            .into_iter()
            .filter_map(|var| {
                (!group_vars_str.contains(&var.text())).then_some(Diagnostic {
                    code: Some((*CODE).clone()),
                    code_description: None,
                    range: Range::from_byte_offset_range(var.syntax().text_range(), &document.text)
                        .unwrap(),
                    severity: DiagnosticSeverity::Error,
                    message: message(var.text(), "is not part of the Group by Clause"),
                    data: None,
                    source: None,
                })
            })
            .chain(
                unaggregated_variables
                    .into_iter()
                    .filter_map(|(assignment_idx, var)| {
                        (!group_vars_str.contains(&var.text())
                            && derived_variables
                                .get(&var.text())
                                .is_none_or(|derived_idx| *derived_idx > assignment_idx))
                        .then_some(Diagnostic {
                            code: Some((*CODE).clone()),
                            code_description: None,
                            range: Range::from_byte_offset_range(
                                var.syntax().text_range(),
                                &document.text,
                            )
                            .unwrap(),
                            severity: DiagnosticSeverity::Error,
                            message: message(
                                var.text(),
                                "is not aggregated or part of the Group by Clause",
                            ),
                            data: None,
                            source: None,
                        })
                    }),
            )
            .collect(),
    )
}

/// The first aggregate of the `SELECT` expressions.
fn first_aggregate(select_clause: &SelectClause) -> Option<SyntaxNode> {
    select_clause
        .assignments()
        .into_iter()
        .find_map(|assignment| {
            // NOTE: Aggregates inside a nested pattern, as in
            // "EXISTS { SELECT (COUNT(*) AS ?c) { ... } }", belong to the sub select.
            let mut stack = vec![assignment.expression.syntax().clone()];
            while let Some(node) = stack.pop() {
                match node.kind() {
                    SyntaxKind::Aggregate => return Some(node),
                    SyntaxKind::GroupGraphPattern => {}
                    // INFO: Reversed, so the children are visited from left to right.
                    _ => stack.extend(node.children().collect::<Vec<_>>().into_iter().rev()),
                }
            }
            None
        })
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::{diagnostic::DiagnosticSeverity, textdocument::Range},
        message_handler::diagnostic::test_utils::{range_of, run},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_ungrouped_selected_variable() {
        let result = run(diagnostics, "SELECT ?s ?o WHERE { ?s ?p ?o } GROUP BY ?s");
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, Range::new(0, 10, 0, 12));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_unaggregated_variable_in_assignment() {
        let result = run(
            diagnostics,
            "SELECT ?s (?o AS ?x) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, Range::new(0, 11, 0, 13));
    }

    #[test]
    fn triggered_by_reference_to_later_assignment() {
        // NOTE: `?d` is derived after it is used, so it is not yet available.
        let result = run(
            diagnostics,
            "SELECT ?s (?d + 1 AS ?c) (COUNT(?o) AS ?d) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, Range::new(0, 11, 0, 13));
    }

    #[test]
    fn not_triggered_by_grouped_or_aggregated_variables() {
        let result = run(
            diagnostics,
            "SELECT ?s (COUNT(?o) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_reference_to_earlier_assignment() {
        let result = run(
            diagnostics,
            "SELECT ?s (COUNT(?o) AS ?c) (?c + 1 AS ?d) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variables_local_to_exists() {
        let result = run(
            diagnostics,
            "SELECT ?s (EXISTS { ?s ?b ?c } AS ?e) WHERE { ?s ?p ?o } GROUP BY ?s",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_without_group_by() {
        let result = run(diagnostics, "SELECT ?s ?o WHERE { ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values_variable() {
        // NOTE: The trailing VALUES clause is joined after the grouping.
        let result = run(
            diagnostics,
            "SELECT ?s ?v WHERE { ?s ?p ?o } GROUP BY ?s VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn triggered_by_implicit_grouping() {
        let text = "SELECT ?subject (COUNT(*) AS ?count) WHERE { ?subject ?predicate ?object }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?subject", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("COUNT(*)"));
    }

    #[test]
    fn triggered_by_unaggregated_variable_in_implicitly_grouped_assignment() {
        let text = "SELECT (COUNT(*) + ?o AS ?x) WHERE { ?s ?p ?o }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 0));
    }

    #[test]
    fn implicit_grouping_names_first_aggregate() {
        let text = "SELECT ?s (SUM(?o) AS ?sum) (COUNT(*) AS ?count) WHERE { ?s ?p ?o }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert!(result[0].message.contains("SUM(?o)"));
    }

    #[test]
    fn not_triggered_by_aggregates_only() {
        let result = run(
            diagnostics,
            "SELECT (COUNT(?o) AS ?c) (?c + 1 AS ?d) WHERE { ?s ?p ?o }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_trailing_values_variable_in_implicitly_grouped_query() {
        let result = run(
            diagnostics,
            "SELECT ?v (COUNT(*) AS ?c) WHERE { ?s ?p ?o } VALUES ?v { 1 }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_aggregate_in_exists() {
        // NOTE: The aggregate belongs to the sub select inside EXISTS.
        let result = run(
            diagnostics,
            "SELECT ?s (EXISTS { SELECT (COUNT(*) AS ?c) WHERE { ?a ?b ?d } } AS ?e) WHERE { ?s ?p ?o }",
        );
        assert!(result.is_empty());
    }
}
