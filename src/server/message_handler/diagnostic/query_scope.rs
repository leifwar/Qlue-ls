//! Shared helpers for the diagnostics that check which variables are visible
//! to the solution modifiers (`GROUP BY`, `HAVING`, `ORDER BY`) and the
//! `SELECT` expressions of a query.
//!
//! NOTE: The visibility rules mirror QLever (`ParsedQuery::addSolutionModifiers`),
//! which processes the parts of a query in this order:
//! 1. `GROUP BY`: sees the query body and the aliases defined earlier in the
//!    same `GROUP BY`.
//! 2. `HAVING`: additionally sees all `GROUP BY` aliases and `SELECT` aliases.
//! 3. The trailing `VALUES` clause is joined.
//! 4. `SELECT` expressions and `ORDER BY`: additionally see the variables of the
//!    trailing `VALUES` clause.
//!
//! NOTE: The scopes are computed once per diagnostic request and shared by all
//! providers, the derived variable sets are computed lazily and cached.

use super::bound_variables;
use ll_sparql_parser::{
    SyntaxNode,
    ast::{
        AstNode, Expression, GroupGraphPattern, SelectClause, SelectQuery, Unit, Var, WhereClause,
    },
    syntax_kind::SyntaxKind,
};
use std::{cell::OnceCell, collections::HashSet};

/// A query (or sub select) together with its solution modifiers.
pub(super) struct QueryScope {
    syntax: SyntaxNode,
    body_variables: OnceCell<HashSet<String>>,
    group_keys: OnceCell<Vec<GroupKey>>,
    order_keys: OnceCell<Vec<OrderKey>>,
    group_by_aliases: OnceCell<HashSet<String>>,
    select_aliases: OnceCell<HashSet<String>>,
    values_variables: OnceCell<HashSet<String>>,
}

/// A single condition of a `GROUP BY` clause.
pub(super) enum GroupKey {
    /// `GROUP BY ?x` or `GROUP BY (?x)`
    Variable(Var),
    /// `GROUP BY STR(?x)` or `GROUP BY (STR(?x))`
    Expression(SyntaxNode),
    /// `GROUP BY (STR(?x) AS ?y)`
    Alias(SyntaxNode, Var),
}

/// A single condition of an `ORDER BY` clause.
pub(super) enum OrderKey {
    /// `ORDER BY ?x`, `ORDER BY (?x)` or `ORDER BY DESC(?x)`
    Variable(Var),
    /// `ORDER BY STR(?x)` or `ORDER BY DESC(?x + 1)`
    Expression(SyntaxNode),
}

impl QueryScope {
    /// All queries and sub selects in `ast`.
    pub(super) fn all(ast: &Unit) -> Vec<QueryScope> {
        ast.syntax()
            .descendants()
            .filter(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::SelectQuery
                        | SyntaxKind::SubSelect
                        | SyntaxKind::ConstructQuery
                        | SyntaxKind::DescribeQuery
                        | SyntaxKind::AskQuery
                )
            })
            .map(|syntax| QueryScope {
                syntax,
                body_variables: OnceCell::new(),
                group_keys: OnceCell::new(),
                order_keys: OnceCell::new(),
                group_by_aliases: OnceCell::new(),
                select_aliases: OnceCell::new(),
                values_variables: OnceCell::new(),
            })
            .collect()
    }

    /// Whether this is a sub select (as opposed to a top level query).
    pub(super) fn is_sub_select(&self) -> bool {
        self.syntax.kind() == SyntaxKind::SubSelect
    }

    pub(super) fn select_clause(&self) -> Option<SelectClause> {
        self.syntax.children().find_map(SelectClause::cast)
    }

    /// The names of the variables bound by the query body (`WHERE` clause).
    ///
    /// NOTE: Variables that only occur inside `MINUS`, `FILTER` or
    /// `EXISTS`/`NOT EXISTS` do not count as bound.
    pub(super) fn body_variables(&self) -> &HashSet<String> {
        self.body_variables
            .get_or_init(|| self.compute_body_variables())
    }

    fn compute_body_variables(&self) -> HashSet<String> {
        let Some(where_clause) = self.syntax.children().find_map(WhereClause::cast) else {
            // INFO: The short form `CONSTRUCT WHERE { <triples> }` has no `WhereClause`.
            return self
                .syntax
                .children()
                .filter(|child| child.kind() == SyntaxKind::TriplesTemplate)
                .flat_map(|triples| triples.descendants().filter_map(Var::cast))
                .map(|var| var.var_name())
                .collect();
        };
        where_clause
            .group_graph_pattern()
            .map(|ggp| match group_graph_pattern_sub(&ggp) {
                // NOTE: Collected per element, since MINUS and FILTER do not bind variables.
                Some(group) => group.children().flat_map(bound_variables).collect(),
                // INFO: The body is a sub select (or empty).
                None => ggp.visible_variables(),
            })
            .unwrap_or_default()
            .into_iter()
            .map(|var| var.var_name())
            .collect()
    }

    /// The `GROUP BY` conditions, in order.
    pub(super) fn group_keys(&self) -> &[GroupKey] {
        self.group_keys.get_or_init(|| {
            self.solution_modifier_clause(SyntaxKind::GroupClause)
                .map(|group_clause| {
                    group_clause
                        .children()
                        .filter_map(|condition| group_key(&condition))
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// The constraints of the `HAVING` clause, in order.
    pub(super) fn having_conditions(&self) -> Vec<SyntaxNode> {
        self.solution_modifier_clause(SyntaxKind::HavingClause)
            .map(|having_clause| {
                having_clause
                    .children()
                    .filter_map(|condition| condition.first_child())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `ORDER BY` conditions, in order.
    pub(super) fn order_keys(&self) -> &[OrderKey] {
        self.order_keys.get_or_init(|| {
            self.solution_modifier_clause(SyntaxKind::OrderClause)
                .map(|order_clause| {
                    order_clause
                        .children()
                        .filter_map(|condition| order_key(&condition))
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// The names of the variables introduced by aliases in the `GROUP BY` clause.
    pub(super) fn group_by_aliases(&self) -> &HashSet<String> {
        self.group_by_aliases.get_or_init(|| {
            self.group_keys()
                .iter()
                .filter_map(|key| match key {
                    GroupKey::Alias(_, var) => Some(var.var_name()),
                    _ => None,
                })
                .collect()
        })
    }

    /// The names of the variables the query is grouped by: plain (or
    /// bracketted) variables and alias targets of the `GROUP BY` clause.
    pub(super) fn grouped_variables(&self) -> HashSet<String> {
        self.group_keys()
            .iter()
            .filter_map(|key| match key {
                GroupKey::Variable(var) | GroupKey::Alias(_, var) => Some(var.var_name()),
                GroupKey::Expression(_) => None,
            })
            .collect()
    }

    /// The names of the variables introduced by aliases in the `SELECT` clause.
    pub(super) fn select_aliases(&self) -> &HashSet<String> {
        self.select_aliases.get_or_init(|| {
            self.select_clause()
                .map(|select_clause| {
                    select_clause
                        .assignments()
                        .into_iter()
                        .map(|assignment| assignment.variable.var_name())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// The part of QLever's warning messages that points to the `SELECT` clause.
    ///
    /// INFO: The hint only makes sense if the query has a `SELECT` clause.
    pub(super) fn select_hint(&self) -> &'static str {
        if self.select_clause().is_some() {
            " and also not bound inside the SELECT clause"
        } else {
            ""
        }
    }

    /// The names of the variables bound by the trailing `VALUES` clause.
    pub(super) fn values_variables(&self) -> &HashSet<String> {
        self.values_variables
            .get_or_init(|| self.compute_values_variables())
    }

    fn compute_values_variables(&self) -> HashSet<String> {
        SelectQuery::cast(self.syntax.clone())
            .and_then(|select_query| select_query.values_clause())
            .map(|values_clause| {
                values_clause
                    .visible_variables()
                    .into_iter()
                    .map(|var| var.var_name())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the query is grouped, explicitly by `GROUP BY` or implicitly by
    /// an aggregate in the `SELECT` clause.
    pub(super) fn is_grouped(&self) -> bool {
        self.is_explicitly_grouped() || self.is_implicitly_grouped()
    }

    /// Whether the query has a `GROUP BY` clause.
    pub(super) fn is_explicitly_grouped(&self) -> bool {
        !self.group_keys().is_empty()
    }

    /// Whether the query has no `GROUP BY`, but an aggregate in the `SELECT` clause.
    pub(super) fn is_implicitly_grouped(&self) -> bool {
        !self.is_explicitly_grouped()
            && self.select_clause().is_some_and(|select_clause| {
                select_clause
                    .assignments()
                    .iter()
                    .any(|assignment| assignment.expression.has_aggregate())
            })
    }

    fn solution_modifier_clause(&self, kind: SyntaxKind) -> Option<SyntaxNode> {
        self.syntax
            .children()
            .find(|child| child.kind() == SyntaxKind::SolutionModifier)?
            .children()
            .find(|child| child.kind() == kind)
    }
}

/// The variables used in `node` (usually an expression).
///
/// NOTE: Variables that only occur inside an `EXISTS`/`NOT EXISTS` live in the
/// scope of the EXISTS pattern and need not be bound by the surrounding query.
pub(super) fn used_variables(node: &SyntaxNode) -> Vec<Var> {
    node.descendants()
        .filter_map(Var::cast)
        .filter(|var| {
            !var.syntax()
                .ancestors()
                // INFO: Only look within `node`, it may itself be nested in an EXISTS.
                .take_while(|ancestor| ancestor != node)
                .any(|ancestor| {
                    matches!(
                        ancestor.kind(),
                        SyntaxKind::ExistsFunc | SyntaxKind::NotExistsFunc
                    )
                })
        })
        .collect()
}

fn group_graph_pattern_sub(ggp: &GroupGraphPattern) -> Option<SyntaxNode> {
    ggp.syntax()
        .children()
        .find(|child| child.kind() == SyntaxKind::GroupGraphPatternSub)
}

fn group_key(condition: &SyntaxNode) -> Option<GroupKey> {
    // INFO: GroupCondition = BuiltInCall | FunctionCall | '(' Expression ( 'AS' Var )? ')' | Var
    let mut children = condition.children();
    let first = children.next()?;
    match (first.kind(), children.next()) {
        (SyntaxKind::Var, _) => Var::cast(first).map(GroupKey::Variable),
        (SyntaxKind::Expression, Some(alias)) => {
            Var::cast(alias).map(|var| GroupKey::Alias(first, var))
        }
        (SyntaxKind::Expression, None) => Some(
            match Expression::cast(first.clone()).and_then(|expression| expression.as_var()) {
                Some(var) => GroupKey::Variable(var),
                None => GroupKey::Expression(first),
            },
        ),
        _ => Some(GroupKey::Expression(first)),
    }
}

fn order_key(condition: &SyntaxNode) -> Option<OrderKey> {
    // INFO: OrderCondition = ( ( 'ASC' | 'DESC' ) BrackettedExpression ) | ( Constraint | Var )
    let first = condition.first_child()?;
    if let Some(var) = Var::cast(first.clone()) {
        return Some(OrderKey::Variable(var));
    }
    // NOTE: Like QLever, a bracketted variable (also inside `ASC`/`DESC`) counts as plain variable.
    let bracketted = match first.kind() {
        SyntaxKind::Constraint => first.first_child(),
        _ => Some(first.clone()),
    }
    .filter(|node| node.kind() == SyntaxKind::BrackettedExpression);
    match bracketted
        .and_then(|bracketted| bracketted.children().find_map(Expression::cast))
        .and_then(|expression| expression.as_var())
    {
        Some(var) => Some(OrderKey::Variable(var)),
        None => Some(OrderKey::Expression(first)),
    }
}
