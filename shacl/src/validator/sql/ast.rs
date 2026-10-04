//! Constructors over the `sqlparser` AST.
//!
//! The engine never writes SQL text: every query is a [`Query`] value, and
//! text exists only where a host renders one (`Display`). These helpers keep
//! the many-field AST structs in one place, so the compilers read as
//! relational algebra rather than as struct literals. Binary operators are
//! always parenthesised ([`Expr::Nested`]), because the AST's `Display` does
//! not add parentheses for precedence.

use sqlparser::ast::helpers::attached_token::AttachedToken;
use sqlparser::ast::{
    BinaryOperator, CaseWhen, CastKind, Cte, DataType, Distinct, Expr, Function, FunctionArg, FunctionArgExpr,
    FunctionArgumentList, FunctionArguments, GroupByExpr, Ident, Join, JoinConstraint, JoinOperator, ObjectName,
    ObjectNamePart, Query, Select, SelectFlavor, SelectItem, SetExpr, SetOperator, SetQuantifier, TableAlias,
    TableFactor, TableWithJoins, UnaryOperator, Value, With,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Token;

/// A quoted identifier: `"name"`. Quoting keeps host table and column names
/// verbatim, whatever their case or characters.
pub fn ident(name: &str) -> Ident {
    Ident::with_quote('"', name)
}

/// `alias.column`.
pub fn col(alias: &str, column: &str) -> Expr {
    Expr::CompoundIdentifier(vec![ident(alias), ident(column)])
}

/// Reads a SQL object name — `table`, `schema.table`, `"Mixed Case"`,
/// `"a.b"` — as the SQL grammar does, so a delimited identifier keeps its case
/// and its dots. Text is never split on `.` by hand.
pub fn parse_object_name(text: &str) -> Result<ObjectName, String> {
    let dialect = GenericDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(text).map_err(|e| e.to_string())?;
    let name = parser.parse_object_name(false).map_err(|e| e.to_string())?;
    parser
        .expect_token(&Token::EOF)
        .map_err(|_| format!("'{text}' is not a SQL object name"))?;
    Ok(name)
}

/// Reads one SQL identifier, delimited (`"birth_year"`) or not (`birth_year`).
pub fn parse_identifier(text: &str) -> Result<Ident, String> {
    let dialect = GenericDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(text).map_err(|e| e.to_string())?;
    let ident = parser.parse_identifier().map_err(|e| e.to_string())?;
    parser
        .expect_token(&Token::EOF)
        .map_err(|_| format!("'{text}' is not a SQL identifier"))?;
    Ok(ident)
}

/// `name` with every part delimited, as the engine renders names: a part's
/// value is kept verbatim (case and all), whatever its quoting in the source.
pub fn delimited(name: &ObjectName) -> ObjectName {
    ObjectName(
        name.0
            .iter()
            .map(|part| match part {
                ObjectNamePart::Identifier(i) => ObjectNamePart::Identifier(ident(&i.value)),
                other => other.clone(),
            })
            .collect(),
    )
}

pub fn string(value: &str) -> Expr {
    Expr::value(Value::SingleQuotedString(value.to_owned()))
}

pub fn number(value: i64) -> Expr {
    Expr::value(Value::Number(value.to_string(), false))
}

pub fn boolean(value: bool) -> Expr {
    Expr::value(Value::Boolean(value))
}

pub fn null() -> Expr {
    Expr::value(Value::Null)
}

fn binary(left: Expr, op: BinaryOperator, right: Expr) -> Expr {
    Expr::Nested(Box::new(Expr::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
    }))
}

pub fn eq(left: Expr, right: Expr) -> Expr {
    binary(left, BinaryOperator::Eq, right)
}

pub fn not_eq(left: Expr, right: Expr) -> Expr {
    binary(left, BinaryOperator::NotEq, right)
}

/// A comparison by operator; the operator is one of `<`, `<=`, `>`, `>=`, `=`.
pub fn compare(left: Expr, op: BinaryOperator, right: Expr) -> Expr {
    binary(left, op, right)
}

/// Folds `items` pairwise into a balanced tree, so that a long conjunction,
/// disjunction or union nests `log2(n)` deep rather than `n`: the engines'
/// parsers recurse on the nesting, and a left-deep chain of a few hundred
/// operands exhausts their stack.
pub fn balanced<T>(items: Vec<T>, combine: &impl Fn(T, T) -> T) -> Option<T> {
    let mut level = items;
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            next.push(match it.next() {
                Some(b) => combine(a, b),
                None => a,
            });
        }
        level = next;
    }
    level.pop()
}

/// Conjunction of every expression; `TRUE` when there are none.
pub fn and_all(exprs: impl IntoIterator<Item = Expr>) -> Expr {
    balanced(exprs.into_iter().collect(), &|a, b| binary(a, BinaryOperator::And, b)).unwrap_or_else(|| boolean(true))
}

/// Disjunction of every expression; `FALSE` when there are none.
pub fn or_all(exprs: impl IntoIterator<Item = Expr>) -> Expr {
    balanced(exprs.into_iter().collect(), &|a, b| binary(a, BinaryOperator::Or, b)).unwrap_or_else(|| boolean(false))
}

pub fn and(a: Expr, b: Expr) -> Expr {
    binary(a, BinaryOperator::And, b)
}

pub fn or(a: Expr, b: Expr) -> Expr {
    binary(a, BinaryOperator::Or, b)
}

pub fn not(expr: Expr) -> Expr {
    Expr::Nested(Box::new(Expr::UnaryOp {
        op: UnaryOperator::Not,
        expr: Box::new(expr),
    }))
}

pub fn is_not_null(expr: Expr) -> Expr {
    Expr::IsNotNull(Box::new(expr))
}

/// `COALESCE(expr, FALSE)`: an unknown truth value read as false.
pub fn known_true(expr: Expr) -> Expr {
    function("COALESCE", vec![expr, boolean(false)])
}

pub fn in_list(expr: Expr, list: Vec<Expr>) -> Expr {
    if list.is_empty() {
        return boolean(false);
    }
    Expr::InList {
        expr: Box::new(expr),
        list,
        negated: false,
    }
}

pub fn exists(query: Query) -> Expr {
    Expr::Exists {
        subquery: Box::new(query),
        negated: false,
    }
}

pub fn not_exists(query: Query) -> Expr {
    Expr::Exists {
        subquery: Box::new(query),
        negated: true,
    }
}

/// `CASE WHEN c1 THEN r1 ... ELSE e END`; just `e` without branches.
pub fn case(branches: Vec<(Expr, Expr)>, otherwise: Expr) -> Expr {
    if branches.is_empty() {
        return otherwise;
    }
    Expr::Case {
        case_token: AttachedToken::empty(),
        end_token: AttachedToken::empty(),
        operand: None,
        conditions: branches
            .into_iter()
            .map(|(condition, result)| CaseWhen { condition, result })
            .collect(),
        else_result: Some(Box::new(otherwise)),
    }
}

pub fn cast(expr: Expr, data_type: DataType, kind: CastKind) -> Expr {
    Expr::Cast {
        kind,
        expr: Box::new(expr),
        data_type,
        format: None,
    }
}

/// A call `name(args...)`; `count_star` for `COUNT(*)` is separate.
pub fn function(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Function(Function {
        name: ObjectName::from(vec![Ident::new(name)]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: args
                .into_iter()
                .map(|e| FunctionArg::Unnamed(FunctionArgExpr::Expr(e)))
                .collect(),
            clauses: Vec::new(),
        }),
        within_group: Vec::new(),
        filter: None,
        null_treatment: None,
        over: None,
    })
}

pub fn count_star() -> Expr {
    Expr::Function(Function {
        name: ObjectName::from(vec![Ident::new("COUNT")]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: vec![FunctionArg::Unnamed(FunctionArgExpr::Wildcard)],
            clauses: Vec::new(),
        }),
        within_group: Vec::new(),
        filter: None,
        null_treatment: None,
        over: None,
    })
}

/// `expr AS alias`.
pub fn item(expr: Expr, alias: &str) -> SelectItem {
    SelectItem::ExprWithAlias {
        expr,
        alias: ident(alias),
    }
}

fn alias(name: &str) -> TableAlias {
    TableAlias {
        explicit: true,
        name: ident(name),
        columns: Vec::new(),
        at: None,
    }
}

/// A table, under an alias.
pub fn table(name: &ObjectName, as_alias: &str) -> TableFactor {
    TableFactor::Table {
        name: delimited(name),
        alias: Some(alias(as_alias)),
        args: None,
        with_hints: Vec::new(),
        version: None,
        with_ordinality: false,
        partitions: Vec::new(),
        json_path: None,
        sample: None,
        index_hints: Vec::new(),
    }
}

/// A CTE by name; unlike [`table`] the name is a single identifier.
pub fn cte_ref(name: &str, as_alias: &str) -> TableFactor {
    TableFactor::Table {
        name: ObjectName::from(vec![ident(name)]),
        alias: Some(alias(as_alias)),
        args: None,
        with_hints: Vec::new(),
        version: None,
        with_ordinality: false,
        partitions: Vec::new(),
        json_path: None,
        sample: None,
        index_hints: Vec::new(),
    }
}

/// A subquery in `FROM`, under an alias.
pub fn derived(query: Query, as_alias: &str) -> TableFactor {
    TableFactor::Derived {
        lateral: false,
        subquery: Box::new(query),
        alias: Some(alias(as_alias)),
        sample: None,
    }
}

/// An inner join on `on`.
pub fn join(relation: TableFactor, on: Expr) -> Join {
    Join {
        relation,
        global: false,
        join_operator: JoinOperator::Join(JoinConstraint::On(on)),
    }
}

/// A left outer join on `on`.
pub fn left_join(relation: TableFactor, on: Expr) -> Join {
    Join {
        relation,
        global: false,
        join_operator: JoinOperator::LeftOuter(JoinConstraint::On(on)),
    }
}

/// A `SELECT` under construction.
#[derive(Debug, Clone)]
pub struct SelectBuilder {
    distinct: bool,
    projection: Vec<SelectItem>,
    from: Vec<TableWithJoins>,
    selection: Option<Expr>,
    group_by: Vec<Expr>,
    having: Option<Expr>,
}

impl SelectBuilder {
    pub fn new(projection: Vec<SelectItem>) -> Self {
        Self {
            distinct: false,
            projection,
            from: Vec::new(),
            selection: None,
            group_by: Vec::new(),
            having: None,
        }
    }

    /// Replaces the projection.
    #[must_use]
    pub fn with_projection(mut self, projection: Vec<SelectItem>) -> Self {
        self.projection = projection;
        self
    }

    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    #[must_use]
    pub fn from(mut self, relation: TableFactor) -> Self {
        self.from.push(TableWithJoins {
            relation,
            joins: Vec::new(),
        });
        self
    }

    /// Adds a join to the last relation of `FROM`.
    #[must_use]
    pub fn join(mut self, join: Join) -> Self {
        if let Some(last) = self.from.last_mut() {
            last.joins.push(join);
        }
        self
    }

    /// Adds a conjunct to `WHERE`.
    #[must_use]
    pub fn filter(mut self, condition: Expr) -> Self {
        self.selection = Some(match self.selection.take() {
            None => condition,
            Some(prev) => and(prev, condition),
        });
        self
    }

    #[must_use]
    pub fn group_by(mut self, exprs: Vec<Expr>) -> Self {
        self.group_by = exprs;
        self
    }

    #[must_use]
    pub fn having(mut self, condition: Expr) -> Self {
        self.having = Some(condition);
        self
    }

    pub fn into_set_expr(self) -> SetExpr {
        SetExpr::Select(Box::new(Select {
            select_token: AttachedToken::empty(),
            optimizer_hints: Vec::new(),
            distinct: self.distinct.then_some(Distinct::Distinct),
            select_modifiers: None,
            top: None,
            top_before_distinct: false,
            projection: self.projection,
            exclude: None,
            into: None,
            from: self.from,
            lateral_views: Vec::new(),
            prewhere: None,
            selection: self.selection,
            connect_by: Vec::new(),
            group_by: GroupByExpr::Expressions(self.group_by, Vec::new()),
            cluster_by: Vec::new(),
            distribute_by: Vec::new(),
            sort_by: Vec::new(),
            having: self.having,
            named_window: Vec::new(),
            qualify: None,
            window_before_qualify: false,
            value_table_mode: None,
            flavor: SelectFlavor::Standard,
        }))
    }

    pub fn into_query(self) -> Query {
        query(self.into_set_expr())
    }
}

/// A query whose body is `body`, with no `WITH`, ordering or limit.
pub fn query(body: SetExpr) -> Query {
    Query {
        with: None,
        body: Box::new(body),
        order_by: None,
        limit_clause: None,
        fetch: None,
        locks: Vec::new(),
        for_clause: None,
        settings: None,
        format_clause: None,
        pipe_operators: Vec::new(),
    }
}

/// `left UNION [ALL] right`.
pub fn union(left: SetExpr, right: SetExpr, all: bool) -> SetExpr {
    SetExpr::SetOperation {
        left: Box::new(left),
        op: SetOperator::Union,
        set_quantifier: if all { SetQuantifier::All } else { SetQuantifier::None },
        right: Box::new(right),
    }
}

/// The union of every body (balanced, see [`balanced`]); `None` when there
/// are none.
pub fn union_all_of(bodies: Vec<SetExpr>, all: bool) -> Option<SetExpr> {
    balanced(bodies, &|a, b| union(a, b, all))
}

/// A CTE `name AS (query)`.
pub fn cte(name: &str, body: Query) -> Cte {
    Cte {
        alias: TableAlias {
            explicit: false,
            name: ident(name),
            columns: Vec::new(),
            at: None,
        },
        query: Box::new(body),
        from: None,
        materialized: None,
        closing_paren_token: AttachedToken::empty(),
    }
}

/// `WITH [RECURSIVE] ctes body`.
pub fn with(ctes: Vec<Cte>, recursive: bool, body: Query) -> Query {
    if ctes.is_empty() {
        return body;
    }
    let mut out = body;
    out.with = Some(With {
        with_token: AttachedToken::empty(),
        recursive,
        cte_tables: ctes,
    });
    out
}
