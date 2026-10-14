//! Constructors over the `sqlparser` AST, as `shacl::validator::sql` has its own:
//! the query is a value and text exists only where the host renders it. Binary
//! operators are parenthesised, because the AST's `Display` adds no parentheses
//! for precedence.

use sqlparser::ast::helpers::attached_token::AttachedToken;
use sqlparser::ast::{
    BinaryOperator, CaseWhen, CastKind, DataType, Distinct, Expr, Function, FunctionArg, FunctionArgExpr,
    FunctionArgumentList, FunctionArguments, GroupByExpr, Ident, Join, JoinConstraint, JoinOperator, LambdaFunction,
    LambdaFunctionParameter, LambdaSyntax, ObjectName, OneOrManyWithParens, Query, Select, SelectFlavor, SelectItem,
    SetExpr, SetOperator, SetQuantifier, TableAlias, TableFactor, TableWithJoins, Value,
};

/// A delimited identifier: `"name"`, so that case and characters are kept.
pub fn ident(name: &str) -> Ident {
    Ident::with_quote('"', name)
}

/// `alias."column"`.
pub fn col(alias: &str, column: &str) -> Expr {
    Expr::CompoundIdentifier(vec![ident(alias), ident(column)])
}

pub fn string(value: &str) -> Expr {
    Expr::value(Value::SingleQuotedString(value.to_owned()))
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

pub fn and(left: Expr, right: Expr) -> Expr {
    binary(left, BinaryOperator::And, right)
}

/// `a || b || …`, `''` for none.
pub fn concat(parts: Vec<Expr>) -> Expr {
    parts
        .into_iter()
        .reduce(|a, b| binary(a, BinaryOperator::StringConcat, b))
        .unwrap_or_else(|| string(""))
}

pub fn is_not_null(expr: Expr) -> Expr {
    Expr::IsNotNull(Box::new(expr))
}

pub fn in_list(expr: Expr, list: &[&str]) -> Expr {
    Expr::InList {
        expr: Box::new(expr),
        list: list.iter().map(|s| string(s)).collect(),
        negated: false,
    }
}

/// `CASE WHEN c1 THEN r1 … ELSE otherwise END`; `otherwise` alone without branches.
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

/// `CAST(expr AS VARCHAR)`.
pub fn varchar(expr: Expr) -> Expr {
    Expr::Cast {
        kind: CastKind::Cast,
        expr: Box::new(expr),
        data_type: DataType::Varchar(None),
        format: None,
    }
}

/// `TRY_CAST(expr AS BLOB)`: NULL for a value that is not a blob. It binds for a value of
/// every type, which `hex(expr)` does not, so a `CASE` that reaches `hex` only for a blob can
/// still be planned over a column of any type.
pub fn try_cast_blob(expr: Expr) -> Expr {
    Expr::Cast {
        kind: CastKind::TryCast,
        expr: Box::new(expr),
        data_type: DataType::Blob(None),
        format: None,
    }
}

/// `CAST(NULL AS VARCHAR)`.
pub fn null_varchar() -> Expr {
    varchar(null())
}

/// A call `name(args…)`.
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

/// `lambda parameter: body`, DuckDB's recommended lambda syntax.
pub fn lambda(parameter: &str, body: Expr) -> Expr {
    Expr::Lambda(LambdaFunction {
        params: OneOrManyWithParens::One(LambdaFunctionParameter {
            name: Ident::new(parameter),
            data_type: None,
        }),
        body: Box::new(body),
        syntax: LambdaSyntax::LambdaKeyword,
    })
}

/// `expr AS "alias"`.
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

/// A table under an alias, its name as the host gave it.
pub fn table(name: ObjectName, as_alias: &str) -> TableFactor {
    TableFactor::Table {
        name,
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

/// A `SELECT` of `projection` from `relation`, joined as `joins` say, where
/// every condition holds.
pub fn select(projection: Vec<SelectItem>, relation: TableFactor, joins: Vec<Join>, conditions: Vec<Expr>) -> SetExpr {
    SetExpr::Select(Box::new(Select {
        select_token: AttachedToken::empty(),
        optimizer_hints: Vec::new(),
        distinct: None::<Distinct>,
        select_modifiers: None,
        top: None,
        top_before_distinct: false,
        projection,
        exclude: None,
        into: None,
        from: vec![TableWithJoins { relation, joins }],
        lateral_views: Vec::new(),
        prewhere: None,
        selection: conditions.into_iter().reduce(and),
        connect_by: Vec::new(),
        group_by: GroupByExpr::Expressions(Vec::new(), Vec::new()),
        cluster_by: Vec::new(),
        distribute_by: Vec::new(),
        sort_by: Vec::new(),
        having: None,
        named_window: Vec::new(),
        qualify: None,
        window_before_qualify: false,
        value_table_mode: None,
        flavor: SelectFlavor::Standard,
    }))
}

/// `SELECT projection WHERE false`: no relation, no rows.
pub fn none(projection: Vec<SelectItem>) -> SetExpr {
    SetExpr::Select(Box::new(Select {
        select_token: AttachedToken::empty(),
        optimizer_hints: Vec::new(),
        distinct: None::<Distinct>,
        select_modifiers: None,
        top: None,
        top_before_distinct: false,
        projection,
        exclude: None,
        into: None,
        from: Vec::new(),
        lateral_views: Vec::new(),
        prewhere: None,
        selection: Some(Expr::value(Value::Boolean(false))),
        connect_by: Vec::new(),
        group_by: GroupByExpr::Expressions(Vec::new(), Vec::new()),
        cluster_by: Vec::new(),
        distribute_by: Vec::new(),
        sort_by: Vec::new(),
        having: None,
        named_window: Vec::new(),
        qualify: None,
        window_before_qualify: false,
        value_table_mode: None,
        flavor: SelectFlavor::Standard,
    }))
}

/// A query whose body is `body`.
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

/// The `UNION ALL` of the bodies, folded into a balanced tree: an engine's
/// parser recurses on the nesting, and a left-deep chain of a few hundred
/// branches (one per mapped column) exhausts its stack.
pub fn union_all(mut bodies: Vec<SetExpr>) -> Option<SetExpr> {
    while bodies.len() > 1 {
        let mut next = Vec::with_capacity(bodies.len().div_ceil(2));
        let mut it = bodies.into_iter();
        while let Some(left) = it.next() {
            next.push(match it.next() {
                Some(right) => SetExpr::SetOperation {
                    left: Box::new(left),
                    op: SetOperator::Union,
                    set_quantifier: SetQuantifier::All,
                    right: Box::new(right),
                },
                None => left,
            });
        }
        bodies = next;
    }
    bodies.pop()
}
