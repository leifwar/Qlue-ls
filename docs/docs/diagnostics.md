# :stethoscope: Diagnostics

Every diagnostic qlue-ls reports, with an example that triggers it and a way to fix it.
The section of each diagnostic is anchored by its code (e.g. `#unbound-bind-var`),
editors that support it link the code of a diagnostic directly to its section.

Lines starting with `#` and `^^^` mark the part the diagnostic is reported on.
Examples use short IRIs like `<population>` for brevity.

## ❌ Errors

### Syntax error { #syntax-error }

`syntax-error`

The query is not valid SPARQL.

```sparql
SELECT * WHERE {
  ?city <population>   # the object is missing
}
```

### Undeclared prefix { #undeclared-prefix }

`undeclared-prefix`

A prefix is used, but never declared.

```sparql
SELECT ?name WHERE {
  ?city rdfs:label ?name
#       ^^^^^^^^^^
}
```

**Fix:** declare the prefix.
If the prefix is known to the backend, the code action *declare prefix* does this for you,
with [`prefixes.add_missing`](03_configuration.md#prefixesadd_missing) it happens automatically.

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?name WHERE {
  ?city rdfs:label ?name
}
```

### Ungrouped select variable { #ungrouped-select-var }

`ungrouped-select-var`

A selected variable is neither grouped nor aggregated.

```sparql
SELECT ?country ?city WHERE {
#               ^^^^^
  ?city <located-in> ?country
}
GROUP BY ?country
```

**Fix:** aggregate the variable (or add it to the `GROUP BY`).

```sparql
SELECT ?country (COUNT(?city) AS ?cities) WHERE {
  ?city <located-in> ?country
}
GROUP BY ?country
```

### GROUP BY star selection { #groupby-star-selection }

`groupby-star-selection`

`SELECT *` is not allowed in a grouped query.

```sparql
SELECT * WHERE {
#      ^
  ?city <located-in> ?country
}
GROUP BY ?country
```

**Fix:** select the grouped variables and aggregates.

```sparql
SELECT ?country WHERE {
  ?city <located-in> ?country
}
GROUP BY ?country
```

### Invalid projection variable { #invalid-projection-var }

`invalid-projection-var`

The variable of a `SELECT` expression is already defined in the query body.

```sparql
SELECT (?population / 1000 AS ?population) WHERE {
#                             ^^^^^^^^^^^
  ?city <population> ?population
}
```

**Fix:** use a fresh variable.

```sparql
SELECT (?population / 1000 AS ?population_k) WHERE {
  ?city <population> ?population
}
```

## ⚠️ Warnings

### Unused prefix { #unused-prefix-declaration }

`unused-prefix-declaration`

A prefix is declared, but never used.

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>   # never used
SELECT * WHERE {
  ?city <population> ?population
}
```

**Fix:** the code action *remove prefix declaration* removes it,
with [`prefixes.remove_unused`](03_configuration.md#prefixesremove_unused) it happens automatically.

### Duplicate prefix declaration { #duplicate-prefix-declaration }

`duplicate-prefix-declaration`

The same prefix is declared more than once.

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>   # already declared
SELECT ?name WHERE {
  ?city rdfs:label ?name
}
```

**Fix:** the code action *remove prefix declaration* removes the duplicate.

### Unbound BIND variable { #unbound-bind-var }

`unbound-bind-var`

A variable in a `BIND` expression is not bound before the `BIND`.
`BIND` only sees the variables bound before it in the same group.

```sparql
SELECT ?city ?density WHERE {
  BIND(?population / ?area AS ?density)
#      ^^^^^^^^^^^   ^^^^^
  ?city <population> ?population ;
        <area> ?area .
}
```

**Fix:** move the `BIND` after the patterns that bind its variables.

```sparql
SELECT ?city ?density WHERE {
  ?city <population> ?population ;
        <area> ?area .
  BIND(?population / ?area AS ?density)
}
```

### Unbound SELECT expression variable { #unbound-select-expr-var }

`unbound-select-expr-var`

A variable in a `SELECT` expression is not bound in the query body.

```sparql
SELECT ?city (?population / ?area AS ?density) WHERE {
#                           ^^^^^
  ?city <population> ?population
}
```

**Fix:** bind the variable in the query body.

```sparql
SELECT ?city (?population / ?area AS ?density) WHERE {
  ?city <population> ?population ;
        <area> ?area .
}
```

### Unbound GROUP BY variable { #unbound-group-by-var }

`unbound-group-by-var`

A `GROUP BY` variable is not bound in the query body.

```sparql
SELECT (COUNT(?city) AS ?cities) WHERE {
  ?city <population> ?population
}
GROUP BY ?country
#        ^^^^^^^^
```

**Fix:** bind the variable in the query body.

```sparql
SELECT (COUNT(?city) AS ?cities) WHERE {
  ?city <population> ?population ;
        <located-in> ?country .
}
GROUP BY ?country
```

### Unbound GROUP BY expression variable { #unbound-group-by-expr-var }

`unbound-group-by-expr-var`

A variable in a `GROUP BY` expression is neither bound in the query body
nor by an earlier alias in the same `GROUP BY`.

```sparql
SELECT ?size (COUNT(?city) AS ?cities) WHERE {
  ?city <population> ?population
}
GROUP BY (FLOOR(?pop / 1000000) AS ?size)
#               ^^^^
```

**Fix:** use a variable of the query body.

```sparql
SELECT ?size (COUNT(?city) AS ?cities) WHERE {
  ?city <population> ?population
}
GROUP BY (FLOOR(?population / 1000000) AS ?size)
```

### Unbound HAVING variable { #unbound-having-var }

`unbound-having-var`

A variable in a `HAVING` expression is neither bound in the query body nor by an alias in the `SELECT` clause.

```sparql
SELECT ?country (COUNT(?city) AS ?cities) WHERE {
  ?city <located-in> ?country
}
GROUP BY ?country
HAVING (?count > 10)
#       ^^^^^^
```

**Fix:** use a variable of the query body or a `SELECT` alias.

```sparql
SELECT ?country (COUNT(?city) AS ?cities) WHERE {
  ?city <located-in> ?country
}
GROUP BY ?country
HAVING (?cities > 10)
```

### Unbound ORDER BY variable { #unbound-order-by-var }

`unbound-order-by-var`

An `ORDER BY` variable is neither bound in the query body nor by an alias in the `SELECT` clause.

```sparql
SELECT ?city WHERE {
  ?city <population> ?population
}
ORDER BY DESC(?pop)
#             ^^^^
```

**Fix:** use a variable of the query body or a `SELECT` alias.

```sparql
SELECT ?city WHERE {
  ?city <population> ?population
}
ORDER BY DESC(?population)
```

### Unbound ORDER BY expression variable { #unbound-order-by-expr-var }

`unbound-order-by-expr-var`

A variable in an `ORDER BY` expression is neither bound in the query body nor by an alias in the `SELECT` clause.

```sparql
SELECT ?city WHERE {
  ?city <population> ?population
}
ORDER BY DESC(?population / ?area)
#                           ^^^^^
```

**Fix:** bind the variable in the query body.

```sparql
SELECT ?city WHERE {
  ?city <population> ?population ;
        <area> ?area .
}
ORDER BY DESC(?population / ?area)
```

### Ungrouped ORDER BY variable { #ungrouped-order-by-var }

`ungrouped-order-by-var`

An `ORDER BY` variable of a grouped query is neither grouped nor an alias in the `SELECT` clause.

```sparql
SELECT ?country (COUNT(?city) AS ?cities) WHERE {
  ?city <located-in> ?country ;
        <population> ?population .
}
GROUP BY ?country
ORDER BY DESC(?population)
#             ^^^^^^^^^^^
```

**Fix:** order by a grouped variable or an aggregate.

```sparql
SELECT ?country (COUNT(?city) AS ?cities) WHERE {
  ?city <located-in> ?country ;
        <population> ?population .
}
GROUP BY ?country
ORDER BY DESC(?cities)
```

!!! note

    An aggregate in the `SELECT` clause groups the query implicitly, even without `GROUP BY`:

    ```sparql
    SELECT (COUNT(?city) AS ?cities) WHERE {
      ?city <population> ?population
    }
    ORDER BY ?population
    #        ^^^^^^^^^^^
    ```

### Disjoint MINUS { #disjoint-minus }

`disjoint-minus`

A `MINUS` that shares no variables with the patterns before it removes nothing.

```sparql
SELECT ?city WHERE {
  ?city <located-in> ?country
  MINUS { ?capital <capital-of> ?state }
# ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
}
```

**Fix:** share variables with the patterns before the `MINUS`

```sparql
SELECT ?city WHERE {
  ?city <located-in> ?country
  MINUS { ?city <capital-of> ?country }
}
```

or, to remove all results whenever the pattern matches, use `FILTER NOT EXISTS`.

```sparql
SELECT ?city WHERE {
  ?city <located-in> ?country
  FILTER NOT EXISTS { ?capital <capital-of> ?state }
}
```

### Deprecated nearest neighbors { #deprecated-nearest-neighbors }

`deprecated-nearest-neighbors` · QLever only

The special predicate `<nearest-neighbors:…>` is deprecated.
It is not reported if the backend is known to run an engine other than QLever.

```sparql
SELECT * WHERE {
  ?station <location> ?station_location .
  ?restaurant <location> ?restaurant_location .
  ?station_location <nearest-neighbors:5> ?restaurant_location .
#                   ^^^^^^^^^^^^^^^^^^^^^
}
```

**Fix:** use the spatial search `SERVICE`, see the [QLever docs](https://docs.qlever.dev/geosparql/).

```sparql
PREFIX spatialSearch: <https://qlever.cs.uni-freiburg.de/spatialSearch/>
SELECT * WHERE {
  ?station <location> ?station_location .
  SERVICE spatialSearch: {
    _:config spatialSearch:algorithm spatialSearch:s2 ;
             spatialSearch:left ?station_location ;
             spatialSearch:right ?restaurant_location ;
             spatialSearch:numNearestNeighbors 5 .
    {
      ?restaurant <location> ?restaurant_location .
    }
  }
}
```

## ℹ️ Info

### Uncompacted URI { #uncompacted-uri }

`uncompacted-uri`

A full IRI could be written as a prefixed name.
Requires a backend whose [`prefixMap`](03_configuration.md#backendprefixmap) knows the namespace.

```sparql
SELECT ?name WHERE {
  ?city <http://www.w3.org/2000/01/rdf-schema#label> ?name
#       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
}
```

**Fix:** the code action *shorten uri* (or *shorten all uri's*) compacts it.

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?name WHERE {
  ?city rdfs:label ?name
}
```

### Same subject { #same-subject }

`same-subject`

Multiple triples share the same subject.

```sparql
SELECT * WHERE {
  ?city <population> ?population .
# ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  ?city <area> ?area .
# ^^^^^^^^^^^^^^^^^^
}
```

**Fix:** the code action *contract triples* contracts them.

```sparql
SELECT * WHERE {
  ?city <population> ?population ;
        <area> ?area .
}
```
