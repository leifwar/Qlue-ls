use rowan::TextRange;

use crate::{
    ast::{
        AstNode, BlankPropertyList, Expression, GraphGraphPattern, GroupGraphPattern, QueryUnit,
        SelectQuery, Triple, TriplesBlock, UpdateUnit, WhereClause,
    },
    parse, parse_query, print_full_tree,
    syntax_kind::SyntaxKind,
    SyntaxNode,
};

fn walk(node: SyntaxNode, mut path: Vec<usize>) -> Option<SyntaxNode> {
    if path.is_empty() {
        return Some(node);
    }
    let head = path.remove(0);
    if let Some(child) = node.children().nth(head) {
        return walk(child, path);
    }
    None
}

#[test]
fn expression() {
    let input = "SELECT (3 as ?b)  (Min(?c) + (?d*10) as ?d) { }";
    let query = QueryUnit::cast(parse(input).0).unwrap();
    let expressions: Vec<_> = query
        .select_query()
        .unwrap()
        .select_clause()
        .unwrap()
        .assignments()
        .into_iter()
        .map(|assignment| assignment.expression)
        .collect();
    assert_eq!(
        expressions
            .iter()
            .flat_map(|ex| ex
                .unaggregated_variables()
                .into_iter()
                .map(|var| var.text()))
            .collect::<Vec<_>>(),
        vec!["?d"]
    );
}

/// The outermost expression of each `GroupCondition` and `SelectClause` assignment
/// of the top level query.
fn top_level_expressions(input: &str) -> Vec<Expression> {
    parse_query(input)
        .0
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::GroupCondition | SyntaxKind::SelectClause
            ) && !node
                .ancestors()
                .any(|ancestor| ancestor.kind() == SyntaxKind::SubSelect)
        })
        .flat_map(|node| node.children().filter_map(Expression::cast))
        .collect()
}

#[test]
fn expression_as_var() {
    let expressions = top_level_expressions(
        "SELECT * { } GROUP BY (?a) ((?b)) (((?c))) (?d + 1) ((?e) + 1) (STR(?f))",
    );
    assert_eq!(
        expressions
            .iter()
            .map(|expression| expression.as_var().map(|var| var.text()))
            .collect::<Vec<_>>(),
        vec![
            Some("?a".to_string()),
            Some("?b".to_string()),
            Some("?c".to_string()),
            None,
            None,
            None
        ]
    );
}

#[test]
fn expression_as_var_with_unclosed_bracket() {
    let expressions = top_level_expressions("SELECT * { } GROUP BY ((?a)");
    assert!(expressions
        .iter()
        .all(|expression| expression.as_var().is_none_or(|var| var.text() == "?a")));
}

#[test]
fn expression_unaggregated_variables_skip_nested_patterns() {
    let expressions = top_level_expressions(
        "SELECT (IF(?a, EXISTS { ?s ?p ?o }, NOT EXISTS { ?s ?p ?a }) AS ?x) \
         (EXISTS { SELECT ?s { ?s ?p ?o } } && ?b AS ?y) { }",
    );
    assert_eq!(
        expressions
            .iter()
            .map(|expression| {
                expression
                    .unaggregated_variables()
                    .into_iter()
                    .map(|var| var.text())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        vec![vec!["?a"], vec!["?b"]]
    );
}

#[test]
fn expression_has_aggregate() {
    let expressions = top_level_expressions(
        "SELECT (COUNT(*) AS ?a) (1 + SUM(?x) AS ?b) (STR(?x) AS ?c) \
         (EXISTS { SELECT (COUNT(*) AS ?n) { ?s ?p ?o } } AS ?d) \
         (NOT EXISTS { { SELECT (MAX(?o) AS ?m) { ?s ?p ?o } } } AS ?e) \
         (COUNT(*) + IF(EXISTS { ?s ?p ?o }, 1, 0) AS ?f) { }",
    );
    assert_eq!(
        expressions
            .iter()
            .map(|expression| expression.has_aggregate())
            .collect::<Vec<_>>(),
        vec![true, true, false, false, false, true]
    );
}

#[test]
fn group_clause_visible_variables() {
    // NOTE: A GroupCondition binds a variable if it is a plain variable ("?a"),
    //       an alias ("(... AS ?c)"), or a bracketted variable ("(?b)").
    let input = "SELECT ?a { } GROUP BY ?a (?b) (1 + ?x AS ?c) (1 + ?y) SUM(?z)";
    let query = QueryUnit::cast(parse(input).0).unwrap();
    let group_clause = query
        .select_query()
        .unwrap()
        .soulution_modifier()
        .unwrap()
        .group_clause()
        .unwrap();
    assert_eq!(
        group_clause
            .visible_variables()
            .into_iter()
            .map(|var| var.text())
            .collect::<Vec<_>>(),
        vec!["?a", "?b", "?c"]
    );
}

#[test]
fn graph_graph_pattern_visible_variables() {
    let input = "SELECT * { GRAPH ?g { ?s ?p ?o } GRAPH <x> { ?a ?b ?c } }";
    let root = parse_query(input).0;
    let visible: Vec<Vec<String>> = root
        .descendants()
        .filter_map(GraphGraphPattern::cast)
        .map(|pattern| {
            pattern
                .visible_variables()
                .into_iter()
                .map(|var| var.text())
                .collect()
        })
        .collect();
    assert_eq!(
        visible,
        vec![vec!["?g", "?s", "?p", "?o"], vec!["?a", "?b", "?c"]]
    );
}

#[test]
fn select_clause() {
    let input = "SELECT ?a (3 as ?b) ?e (?c as ?d) { }";
    let query = QueryUnit::cast(parse(input).0).unwrap();
    let select_clause = query.select_query().unwrap().select_clause().unwrap();
    assert_eq!(
        select_clause
            .variables()
            .into_iter()
            .map(|var| var.text())
            .collect::<Vec<_>>(),
        vec!["?a", "?e"]
    );
    assert_eq!(
        select_clause
            .projected_variables()
            .into_iter()
            .map(|var| var.text())
            .collect::<Vec<_>>(),
        vec!["?a", "?b", "?e", "?d"]
    );
    assert_eq!(
        select_clause
            .assignments()
            .into_iter()
            .map(|assignment| format!(
                "{} as {}",
                assignment.expression.text(),
                assignment.variable.text()
            ))
            .collect::<Vec<_>>(),
        vec!["3 as ?b", "?c as ?d"]
    );
}

#[test]
fn values_clause() {
    let input = "SELECT * WHERE { VALUES ?x {} }";
    let root = parse(input).0;
    let query = QueryUnit::cast(root).unwrap();
    let gpnt = query
        .select_query()
        .unwrap()
        .where_clause()
        .unwrap()
        .group_graph_pattern()
        .unwrap()
        .group_pattern_not_triples();
    assert_eq!(gpnt.len(), 1)
}

fn trailing_values_variables(select_query: &SelectQuery) -> Vec<String> {
    select_query
        .values_clause()
        .map(|values_clause| values_clause.visible_variables())
        .unwrap_or_default()
        .into_iter()
        .map(|var| var.text())
        .collect()
}

#[test]
fn trailing_values_clause() {
    for (input, expected) in [
        ("SELECT * { }", vec![]),
        ("SELECT * { } VALUES ?a { 1 2 }", vec!["?a"]),
        (
            "SELECT * { } VALUES (?a ?b) { (1 2) (3 UNDEF) }",
            vec!["?a", "?b"],
        ),
        ("SELECT * { } VALUES () { () }", vec![]),
        // INFO: The `VALUES` inside the query body is `InlineData`, not a `ValuesClause`.
        ("SELECT * { VALUES ?a { 1 } }", vec![]),
    ] {
        let query = QueryUnit::cast(parse_query(input).0).unwrap();
        assert_eq!(
            trailing_values_variables(&query.select_query().unwrap()),
            expected,
            "{input}"
        );
    }
}

#[test]
fn trailing_values_clause_of_sub_select() {
    let input = "SELECT * { { SELECT ?a { } GROUP BY ?a VALUES ?a { 1 } } } VALUES ?b { 2 }";
    let root = parse_query(input).0;
    let sub_select = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::SubSelect)
        .and_then(SelectQuery::cast)
        .unwrap();
    assert_eq!(trailing_values_variables(&sub_select), vec!["?a"]);
    assert!(sub_select
        .soulution_modifier()
        .is_some_and(|modifier| modifier.group_clause().is_some()));
    let query = QueryUnit::cast(root).unwrap();
    assert_eq!(
        trailing_values_variables(&query.select_query().unwrap()),
        vec!["?b"]
    );
}

#[test]
fn property_list() {
    let input = "SELECT * WHERE { ?a ?b ?c ;  ?x }";
    let root = parse(input).0;
    let query = QueryUnit::cast(root).unwrap();
    let triples = query
        .select_query()
        .unwrap()
        .where_clause()
        .unwrap()
        .group_graph_pattern()
        .unwrap()
        .triple_blocks()
        .first()
        .unwrap()
        .triples();
    let property_list = triples
        .first()
        .unwrap()
        .properties_list_path()
        .unwrap()
        .properties();
    assert_eq!(property_list.len(), 2);
}

#[test]
fn blank_prop_list() {
    let input = "SELECT * WHERE { ?s ?p []}";
    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0]).unwrap();
    let ast_node = BlankPropertyList::cast(node).unwrap();
    assert!(ast_node.property_list().is_none());

    let input = "SELECT * WHERE { ?s ?p [?a ]}";
    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0]).unwrap();
    let ast_node = BlankPropertyList::cast(node).unwrap();
    assert!(ast_node.property_list().is_some());
}

#[test]
fn prologue() {
    let input = "PREFIX a: <dings>\n Prefix b: <foo> SELECT ?a WHERE { ?s ?p ?o}";

    let root = parse_query(input).0;
    let query_unit = QueryUnit::cast(root).unwrap();
    let prologue = query_unit.prologue().unwrap();
    assert_eq!(prologue.prefix_declarations()[0].prefix().unwrap(), "a");
    assert_eq!(
        prologue.prefix_declarations()[0].uri_prefix().unwrap(),
        "<dings>"
    );

    assert_eq!(prologue.prefix_declarations()[1].prefix().unwrap(), "b");
    assert_eq!(
        prologue.prefix_declarations()[1].uri_prefix().unwrap(),
        "<foo>"
    );

    let query_unit2 = QueryUnit::cast(parse_query("SELECT * {}").0).unwrap();
    assert_eq!(query_unit2.prologue(), None);
}

#[test]
fn where_clause() {
    let input = "SELECT ?a WHERE { ?s ?p ?o}";

    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1]).unwrap();
    let where_clause = WhereClause::cast(node).unwrap();
    assert_eq!(
        where_clause.where_token().unwrap().text_range(),
        TextRange::new(10.into(), 15.into())
    );
    assert_eq!(
        where_clause
            .group_graph_pattern()
            .unwrap()
            .syntax
            .to_string(),
        "{ ?s ?p ?o}"
    );
}

#[test]
fn group_graph_pattern() {
    let input = "SELECT * { ?s ?p ?o . {} ?a  ?b ?c .     ?x ?y  ?z}";
    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1, 0]).unwrap();
    let ggp = GroupGraphPattern::cast(node).unwrap();
    let triples_blocks = ggp.triple_blocks();
    assert_eq!(triples_blocks.len(), 2);
    assert_eq!(
        triples_blocks[1]
            .group_graph_pattern()
            .unwrap()
            .syntax
            .to_string(),
        "{ ?s ?p ?o . {} ?a  ?b ?c .     ?x ?y  ?z}"
    );
    assert_eq!(
        ggp.l_paren_token().unwrap().text_range(),
        TextRange::new(9.into(), 10.into())
    );
    assert_eq!(
        ggp.r_paren_token().unwrap().text_range(),
        TextRange::new(50.into(), 51.into())
    );
}

#[test]
fn triples_block() {
    let input = "SELECT * { ?s ?p ?o . ?a  ?b ?c .     ?x ?y  ?z}";
    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1, 0, 0, 0]).unwrap();
    let triples_block = TriplesBlock::cast(node).unwrap();
    let triples = triples_block.triples();
    assert_eq!(triples.len(), 3);
    assert_eq!(triples_block.triples()[0].syntax.to_string(), "?s ?p ?o");
    assert_eq!(triples_block.triples()[1].syntax.to_string(), "?a  ?b ?c");
    assert_eq!(triples_block.triples()[2].syntax.to_string(), "?x ?y  ?z");
    assert_eq!(
        triples[2].triples_block().unwrap().syntax().to_string(),
        "?s ?p ?o . ?a  ?b ?c .     ?x ?y  ?z"
    );
}

#[test]
fn ast_triple() {
    let input = "SELECT * { ?s ?p ?o ; ?p2 ?o2 ; ?p3 ?o3}";
    let root = parse_query(input).0;
    let node = walk(root, vec![0, 0, 1, 0, 0, 0, 0]).unwrap();
    let triple = Triple::cast(node).unwrap();
}

/// WARNING: A non-empty `Prologue` precedes the `SelectQuery` inside the
///          `Query` node, so these accessors must not just take the first
///          child. Only an empty `Prologue` is absent from the tree, which is
///          why the prefix-less variants alone would not catch a regression.
#[test]
fn accessors_look_past_the_prologue() {
    for input in [
        "SELECT ?a ?b { } GROUP BY ?a",
        "PREFIX ex: <http://example.org/> SELECT ?a ?b { } GROUP BY ?a",
        "BASE <http://example.org/> SELECT ?a ?b { } GROUP BY ?a",
    ] {
        let query = QueryUnit::cast(parse(input).0).unwrap();
        let select_query = query
            .select_query()
            .unwrap_or_else(|| panic!("no SelectQuery in {input:?}"));
        assert_eq!(
            select_query
                .select_clause()
                .unwrap()
                .variables()
                .into_iter()
                .map(|var| var.text())
                .collect::<Vec<_>>(),
            vec!["?a", "?b"],
            "select clause of {input:?}"
        );
        assert!(
            select_query
                .soulution_modifier()
                .and_then(|modifier| modifier.group_clause())
                .is_some(),
            "group clause of {input:?}"
        );
    }
}

#[test]
fn update_prologue() {
    let update = UpdateUnit::cast(parse("PREFIX ex: <http://example.org/> CLEAR DEFAULT").0)
        .expect("CLEAR is an update");
    assert_eq!(
        update
            .prologue()
            .unwrap()
            .prefix_declarations()
            .into_iter()
            .filter_map(|declaration| declaration.prefix())
            .collect::<Vec<_>>(),
        vec!["ex"]
    );
}
