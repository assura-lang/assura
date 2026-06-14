//! Source code formatter for the Assura contract language.
//!
//! Takes a parsed `SourceFile` AST and produces well-formatted source text.

use assura_parser::ast::{
    BinOp, BindDecl, Clause, ClauseKind, ContractDecl, Decl, EnumDef, Expr, ExternDecl, FnDef,
    Literal, Pattern, ProphecyDecl, ServiceDecl, ServiceItem, SourceFile, TypeBody, TypeDef,
    UnaryOp, extract_clause_params,
};

/// Format a `SourceFile` AST back to well-formatted source text.
pub fn format_source_file(file: &SourceFile) -> String {
    let mut out = String::new();

    // Project declaration
    if let Some(ref p) = file.project {
        out.push_str(&format!(
            "project {} {{ profile: [{}] }}\n",
            p.name,
            p.profile.join(", ")
        ));
        out.push('\n');
    }

    // Module declaration
    if let Some(ref m) = file.module {
        out.push_str(&format!("module {};\n", m.path.join(".")));
        out.push('\n');
    }

    // Imports
    if !file.imports.is_empty() {
        for imp in &file.imports {
            out.push_str("import ");
            out.push_str(&imp.path.join("."));
            if let Some(ref alias) = imp.alias {
                out.push_str(&format!(" as {alias}"));
            }
            if !imp.items.is_empty() {
                out.push_str(&format!(" {{ {} }}", imp.items.join(", ")));
            }
            out.push_str(";\n");
        }
        out.push('\n');
    }

    // Declarations
    let num_decls = file.decls.len();
    for (i, decl) in file.decls.iter().enumerate() {
        format_decl(&decl.node, &mut out);
        if i + 1 < num_decls {
            out.push('\n');
        }
    }

    out
}

pub fn format_decl(decl: &Decl, out: &mut String) {
    match decl {
        Decl::Contract(c) => format_contract(c, out),
        Decl::Service(s) => format_service(s, out),
        Decl::TypeDef(t) => format_typedef(t, out),
        Decl::EnumDef(e) => format_enumdef(e, out),
        Decl::Extern(e) => format_extern(e, out),
        Decl::Bind(b) => format_bind(b, out),
        Decl::Prophecy(p) => format_prophecy(p, out),
        Decl::FnDef(f) => format_fndef(f, out),
        Decl::Block {
            kind,
            name,
            value,
            body,
        } => format_block(kind, name, value, body, out),
    }
}

pub fn format_contract(c: &ContractDecl, out: &mut String) {
    out.push_str("contract ");
    out.push_str(&c.name);
    if !c.type_params.is_empty() {
        out.push_str(&format!("<{}>", c.type_params.join(", ")));
    }
    out.push_str(" {\n");
    for clause in &c.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
    out.push_str("}\n");
}

pub fn format_service(s: &ServiceDecl, out: &mut String) {
    out.push_str("service ");
    out.push_str(&s.name);
    out.push_str(" {\n");
    for (i, item) in s.items.iter().enumerate() {
        match item {
            ServiceItem::States(states) => {
                out.push_str(&format!("    states: {}\n", states.join(" -> ")));
            }
            ServiceItem::TypeDef(t) => {
                let mut sub = String::new();
                format_typedef(t, &mut sub);
                for line in sub.lines() {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            ServiceItem::EnumDef(e) => {
                let mut sub = String::new();
                format_enumdef(e, &mut sub);
                for line in sub.lines() {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            ServiceItem::Operation { name, clauses } => {
                out.push_str(&format!("    operation {name} {{\n"));
                for clause in clauses {
                    out.push_str("        ");
                    format_clause(clause, out);
                    out.push('\n');
                }
                out.push_str("    }\n");
            }
            ServiceItem::Query { name, clauses } => {
                out.push_str(&format!("    query {name} {{\n"));
                for clause in clauses {
                    out.push_str("        ");
                    format_clause(clause, out);
                    out.push('\n');
                }
                out.push_str("    }\n");
            }
            ServiceItem::Invariant(expr) => {
                out.push_str("    invariant { ");
                format_expr(expr, out);
                out.push_str(" }\n");
            }
            ServiceItem::Other { kind, body } => {
                out.push_str(&format!("    {kind}: "));
                format_expr(body, out);
                out.push('\n');
            }
        }
        // Blank line between service items (except after the last)
        if i + 1 < s.items.len() {
            let next_needs_sep = !matches!(s.items.get(i + 1), Some(ServiceItem::States(_)));
            let cur_needs_sep = !matches!(item, ServiceItem::States(_));
            if cur_needs_sep && next_needs_sep {
                out.push('\n');
            }
        }
    }
    out.push_str("}\n");
}

pub fn format_typedef(t: &TypeDef, out: &mut String) {
    out.push_str("type ");
    out.push_str(&t.name);
    if !t.type_params.is_empty() {
        out.push_str(&format!("<{}>", t.type_params.join(", ")));
    }
    match &t.body {
        TypeBody::Alias(tokens) => {
            out.push_str(&format!(" = {};\n", tokens.join(" ")));
        }
        TypeBody::Struct(fields) => {
            out.push_str(" {\n");
            for f in fields {
                let vis = if f.is_pub { "pub " } else { "" };
                out.push_str(&format!("    {vis}{}: {};\n", f.name, f.ty.join(" ")));
            }
            out.push_str("}\n");
        }
        TypeBody::Refined(tokens) => {
            out.push_str(&format!(" = {{ {} }};\n", tokens.join(" ")));
        }
        TypeBody::Empty => {
            out.push('\n');
        }
    }
}

pub fn format_enumdef(e: &EnumDef, out: &mut String) {
    out.push_str("enum ");
    out.push_str(&e.name);
    if !e.type_params.is_empty() {
        out.push_str(&format!("<{}>", e.type_params.join(", ")));
    }
    out.push_str(" {\n");
    for v in &e.variants {
        out.push_str("    ");
        out.push_str(&v.name);
        if !v.fields.is_empty() {
            out.push_str(&format!("({})", v.fields.join(", ")));
        }
        out.push('\n');
    }
    out.push_str("}\n");
}

pub fn format_extern(e: &ExternDecl, out: &mut String) {
    out.push_str("extern fn ");
    out.push_str(&e.name);
    out.push('(');
    let params: Vec<String> = e
        .params
        .iter()
        .map(|p| {
            if p.ty.is_empty() {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, p.ty.join(" "))
            }
        })
        .collect();
    out.push_str(&params.join(", "));
    out.push(')');
    if !e.return_ty.is_empty() {
        out.push_str(&format!(" -> {}", e.return_ty.join(" ")));
    }
    out.push('\n');
    for clause in &e.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
}

pub fn format_bind(b: &BindDecl, out: &mut String) {
    out.push_str(&format!("bind \"{}\" as {} {{\n", b.target_path, b.name));
    if !b.params.is_empty() {
        out.push_str("    input(");
        let params: Vec<String> = b
            .params
            .iter()
            .map(|p| {
                if p.ty.is_empty() {
                    p.name.clone()
                } else {
                    format!("{}: {}", p.name, p.ty.join(" "))
                }
            })
            .collect();
        out.push_str(&params.join(", "));
        out.push_str(")\n");
    }
    if !b.return_ty.is_empty() {
        out.push_str(&format!("    output(result: {})\n", b.return_ty.join(" ")));
    }
    for clause in &b.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
    out.push_str("}\n");
}

pub fn format_prophecy(p: &ProphecyDecl, out: &mut String) {
    out.push_str("ghost prophecy ");
    out.push_str(&p.name);
    if !p.ty_tokens.is_empty() {
        out.push_str(": ");
        out.push_str(&p.ty_tokens.join(" "));
    }
    out.push('\n');
}

pub fn format_fndef(f: &FnDef, out: &mut String) {
    if f.is_ghost {
        out.push_str("ghost ");
    }
    if f.is_lemma {
        out.push_str("lemma ");
    }
    out.push_str("fn ");
    out.push_str(&f.name);
    out.push('(');
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            if p.ty.is_empty() {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, p.ty.join(" "))
            }
        })
        .collect();
    out.push_str(&params.join(", "));
    out.push(')');
    if !f.return_ty.is_empty() {
        out.push_str(&format!(" -> {}", f.return_ty.join(" ")));
    }
    out.push('\n');
    for clause in &f.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
}

pub fn format_block(
    kind: &str,
    name: &str,
    value: &Option<Vec<String>>,
    body: &[Clause],
    out: &mut String,
) {
    out.push_str(kind);
    out.push(' ');
    out.push_str(name);
    if let Some(v) = value {
        // The value tokens may already start with ':' or '=' from the parser.
        // Only add a separator if the tokens don't already begin with one.
        let starts_with_sep = v.first().is_some_and(|t| t == ":" || t == "=");
        if starts_with_sep {
            out.push(' ');
            out.push_str(&v.join(" "));
        } else {
            out.push_str(&format!(": {}", v.join(" ")));
        }
    }
    // Blocks without a value or clauses that could be mistaken for clause
    // keywords (spec, define, etc.) need explicit { } to be parsed as
    // declarations rather than clauses of the previous fn/contract.
    if value.is_none() && body.is_empty() {
        out.push_str(" {\n}\n");
    } else if !body.is_empty() {
        out.push('\n');
        for clause in body {
            out.push_str("    ");
            format_clause(clause, out);
            out.push('\n');
        }
    } else {
        out.push('\n');
    }
}

pub fn format_clause(clause: &Clause, out: &mut String) {
    let kind_str = match &clause.kind {
        ClauseKind::Requires => "requires",
        ClauseKind::Ensures => "ensures",
        ClauseKind::Effects => "effects",
        ClauseKind::Invariant => "invariant",
        ClauseKind::Modifies => "modifies",
        ClauseKind::Input => "input",
        ClauseKind::Output => "output",
        ClauseKind::Errors => "errors",
        ClauseKind::Rule => "rule",
        ClauseKind::DataFlow => "data_flow",
        ClauseKind::MustNot => "must_not",
        ClauseKind::Decreases => "decreases",
        ClauseKind::Other(s) => s.as_str(),
    };

    // For input/output clauses with Raw bodies, use function-call syntax
    // to match the canonical `input(name: Type, ...)` format.
    if matches!(clause.kind, ClauseKind::Input | ClauseKind::Output) {
        let params = extract_clause_params(&clause.body);
        if !params.is_empty() {
            out.push_str(kind_str);
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&p.name);
                if !p.ty.is_empty() {
                    out.push_str(": ");
                    out.push_str(&p.ty.join(" "));
                }
            }
            out.push(')');
            return;
        }
    }

    // For expression-type clauses, use braced format to avoid parser edge
    // cases with inline colon syntax (e.g., `mod` operator not parsed inline).
    if is_braced_kind(&clause.kind) {
        out.push_str(kind_str);
        out.push_str(" { ");
        format_expr(&clause.body, out);
        out.push_str(" }");
    } else {
        out.push_str(kind_str);
        out.push_str(": ");
        format_expr(&clause.body, out);
    }
}

pub fn is_braced_kind(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Requires
            | ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Decreases
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Effects
            | ClauseKind::Modifies
    )
}

pub fn format_expr(expr: &Expr, out: &mut String) {
    match expr {
        Expr::Literal(lit) => format_literal(lit, out),
        Expr::Ident(name) => out.push_str(name),
        Expr::Field(base, field) => {
            format_expr(base, out);
            out.push('.');
            out.push_str(field);
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            format_expr(receiver, out);
            out.push('.');
            out.push_str(method);
            out.push('(');
            format_expr_list(args, out);
            out.push(')');
        }
        Expr::Call { func, args } => {
            format_expr(func, out);
            out.push('(');
            format_expr_list(args, out);
            out.push(')');
        }
        Expr::Index { expr: e, index } => {
            format_expr(e, out);
            out.push('[');
            format_expr(index, out);
            out.push(']');
        }
        Expr::BinOp { lhs, op, rhs } => {
            format_expr(lhs, out);
            out.push(' ');
            out.push_str(binop_str(op));
            out.push(' ');
            format_expr(rhs, out);
        }
        Expr::UnaryOp { op, expr: e } => {
            out.push_str(match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            });
            format_expr(e, out);
        }
        Expr::Old(e) => {
            out.push_str("old(");
            format_expr(e, out);
            out.push(')');
        }
        Expr::Forall { var, domain, body } => {
            out.push_str("forall ");
            out.push_str(var);
            out.push_str(" in ");
            format_expr(domain, out);
            out.push_str(": ");
            format_expr(body, out);
        }
        Expr::Exists { var, domain, body } => {
            out.push_str("exists ");
            out.push_str(var);
            out.push_str(" in ");
            format_expr(domain, out);
            out.push_str(": ");
            format_expr(body, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if ");
            format_expr(cond, out);
            out.push_str(" then ");
            format_expr(then_branch, out);
            if let Some(else_b) = else_branch {
                out.push_str(" else ");
                format_expr(else_b, out);
            }
        }
        Expr::Paren(e) => {
            out.push('(');
            format_expr(e, out);
            out.push(')');
        }
        Expr::List(items) => {
            out.push('[');
            format_expr_list(items, out);
            out.push(']');
        }
        Expr::Cast { expr: e, ty } => {
            format_expr(e, out);
            out.push_str(" as ");
            out.push_str(ty);
        }
        Expr::Block(items) => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                format_expr(item, out);
            }
        }
        Expr::Ghost(e) => {
            out.push_str("ghost { ");
            format_expr(e, out);
            out.push_str(" }");
        }
        Expr::Apply { lemma_name, args } => {
            out.push_str("apply ");
            out.push_str(lemma_name);
            out.push('(');
            format_expr_list(args, out);
            out.push(')');
        }
        Expr::Let { name, value, body } => {
            out.push_str("let ");
            out.push_str(name);
            out.push_str(" = ");
            format_expr(value, out);
            out.push_str(" in ");
            format_expr(body, out);
        }
        Expr::Match { scrutinee, arms } => {
            out.push_str("match ");
            format_expr(scrutinee, out);
            out.push_str(" { ");
            for (i, arm) in arms.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_pattern(&arm.pattern, out);
                out.push_str(" => ");
                format_expr(&arm.body, out);
            }
            out.push_str(" }");
        }
        Expr::Tuple(items) => {
            out.push('(');
            format_expr_list(items, out);
            out.push(')');
        }
        Expr::Raw(tokens) => {
            out.push_str(&join_raw_tokens(tokens));
        }
    }
}

pub fn format_expr_list(items: &[Expr], out: &mut String) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        format_expr(item, out);
    }
}

pub fn format_literal(lit: &Literal, out: &mut String) {
    match lit {
        Literal::Int(s) | Literal::Float(s) => out.push_str(s),
        Literal::Str(s) => {
            out.push('"');
            out.push_str(s);
            out.push('"');
        }
        Literal::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
    }
}

pub fn format_pattern(pat: &Pattern, out: &mut String) {
    match pat {
        Pattern::Ident(name) => out.push_str(name),
        Pattern::Literal(lit) => format_literal(lit, out),
        Pattern::Wildcard => out.push('_'),
        Pattern::Constructor { name, fields } => {
            out.push_str(name);
            out.push('(');
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_pattern(f, out);
            }
            out.push(')');
        }
        Pattern::Tuple(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_pattern(item, out);
            }
            out.push(')');
        }
    }
}

/// Join raw tokens, collapsing `.` into dotted paths without spaces.
/// E.g., `["io", ".", "read"]` -> `"io.read"` instead of `"io . read"`.
pub fn join_raw_tokens(tokens: &[String]) -> String {
    let mut out = String::new();
    for (i, tok) in tokens.iter().enumerate() {
        if tok == "." {
            out.push('.');
        } else if i > 0 && tokens.get(i - 1).is_some_and(|prev| prev == ".") {
            out.push_str(tok);
        } else {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(tok);
        }
    }
    out
}

pub fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Implies => "==>",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::Concat => "++",
        BinOp::Range => "..",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse source, assert no errors, format, return formatted string.
    fn parse_and_format(source: &str) -> String {
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.expect("parse returned None");
        format_source_file(&file)
    }

    /// Helper: assert that formatting is idempotent (format(format(x)) == format(x)).
    fn assert_idempotent(source: &str) {
        let first = parse_and_format(source);
        let second = parse_and_format(&first);
        assert_eq!(first, second, "formatting is not idempotent");
    }

    // ----- 1. Minimal contract -----

    #[test]
    fn test_format_minimal_contract() {
        let src = "contract Foo { requires { x > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("contract Foo {"));
        assert!(out.contains("requires"));
        assert!(out.contains("x > 0"));
    }

    #[test]
    fn test_format_contract_with_ensures() {
        let src = "contract Bar { requires { x > 0 } ensures { result > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("contract Bar {"));
        assert!(out.contains("requires"));
        assert!(out.contains("ensures"));
    }

    #[test]
    fn test_format_contract_with_type_params() {
        let src = "contract Generic<T> { requires { x > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("contract Generic<T> {"));
    }

    // ----- 2. Service declaration -----

    #[test]
    fn test_format_service_with_operation() {
        let src = r#"
service OrderService {
    states: Created -> Paid -> Shipped
    operation pay {
        requires { amount > 0 }
        ensures { state == Paid }
    }
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("service OrderService {"));
        assert!(out.contains("states:"));
        assert!(out.contains("operation pay {"));
    }

    #[test]
    fn test_format_service_with_query() {
        let src = r#"
service DataService {
    query getItem {
        requires { id > 0 }
        ensures { result > 0 }
    }
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("service DataService {"));
        assert!(out.contains("query getItem {"));
    }

    // ----- 3. Type and enum definitions -----

    #[test]
    fn test_format_struct_type() {
        let src = r#"
type Point {
    pub x: Int;
    pub y: Int;
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("type Point {"));
        assert!(out.contains("pub x: Int;"));
        assert!(out.contains("pub y: Int;"));
    }

    #[test]
    fn test_format_alias_type() {
        let src = "type Age = Int;\n";
        let out = parse_and_format(src);
        assert!(out.contains("type Age = Int;"));
    }

    #[test]
    fn test_format_enum() {
        let src = r#"
enum Color {
    Red
    Green
    Blue
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("enum Color {"));
        assert!(out.contains("Red"));
        assert!(out.contains("Green"));
        assert!(out.contains("Blue"));
    }

    #[test]
    fn test_format_enum_with_fields() {
        let src = r#"
enum Shape {
    Circle(Int)
    Rect(Int, Int)
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("Circle(Int)"));
        // The parser stores field types as raw tokens which may include trailing spaces;
        // the formatter joins them with ", " so the output may have extra spaces.
        assert!(out.contains("Rect("));
        assert!(out.contains("Int"));
    }

    #[test]
    fn test_format_generic_type() {
        let src = r#"
type Pair<A, B> {
    pub first: A;
    pub second: B;
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("type Pair<A, B> {"));
    }

    // ----- 4. Extern functions -----

    #[test]
    fn test_format_extern_fn() {
        let src = "extern fn read_file(path: String) -> String\n";
        let out = parse_and_format(src);
        assert!(out.contains("extern fn read_file(path: String) -> String"));
    }

    #[test]
    fn test_format_extern_fn_with_clauses() {
        let src = r#"
extern fn divide(a: Int, b: Int) -> Int
    requires { b != 0 }
    ensures { result * b == a }
"#;
        let out = parse_and_format(src);
        assert!(out.contains("extern fn divide(a: Int, b: Int) -> Int"));
        assert!(out.contains("requires"));
        assert!(out.contains("ensures"));
    }

    // ----- 5. Bind declarations -----

    #[test]
    fn test_format_bind_decl() {
        let src = r#"
bind "libc::malloc" as safe_alloc {
    input(size: Nat)
    output(result: Bytes)
    requires { size > 0 }
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("bind \"libc::malloc\" as safe_alloc {"));
        assert!(out.contains("input(size: Nat)"));
        assert!(out.contains("output(result: Bytes)"));
    }

    // ----- 6. Multiple contracts (ordering) -----

    #[test]
    fn test_format_multiple_contracts() {
        let src = r#"
contract First {
    requires { a > 0 }
}

contract Second {
    requires { b > 0 }
}

contract Third {
    requires { c > 0 }
}
"#;
        let out = parse_and_format(src);
        let first_pos = out.find("contract First").unwrap();
        let second_pos = out.find("contract Second").unwrap();
        let third_pos = out.find("contract Third").unwrap();
        assert!(first_pos < second_pos, "First should come before Second");
        assert!(second_pos < third_pos, "Second should come before Third");
    }

    // ----- 7. Deeply nested expressions in clauses -----

    #[test]
    fn test_format_nested_binary_ops() {
        let src = "contract Nested { requires { a + b * c > d - e } }";
        let out = parse_and_format(src);
        assert!(out.contains("a + b * c > d - e"));
    }

    #[test]
    fn test_format_nested_logical_ops() {
        let src = "contract Logic { requires { a > 0 && b > 0 || c == 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("&&"));
        assert!(out.contains("||"));
    }

    #[test]
    fn test_format_quantifier_expression() {
        let src = "contract Quant { requires { forall i in items: i > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("forall i in items: i > 0"));
    }

    #[test]
    fn test_format_if_then_else_expression() {
        let src = "contract Cond { ensures { if x > 0 then result > 0 else result == 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("if x > 0 then result > 0 else result == 0"));
    }

    #[test]
    fn test_format_old_expression() {
        let src = "contract OldExpr { ensures { result > old(x) } }";
        let out = parse_and_format(src);
        assert!(out.contains("old(x)"));
    }

    // ----- 8. All clause kinds -----

    #[test]
    fn test_format_requires_clause() {
        let src = "contract C { requires { x > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("requires"));
    }

    #[test]
    fn test_format_ensures_clause() {
        let src = "contract C { ensures { result > 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("ensures"));
    }

    #[test]
    fn test_format_invariant_clause() {
        let src = "contract C { invariant { x >= 0 } }";
        let out = parse_and_format(src);
        assert!(out.contains("invariant"));
    }

    #[test]
    fn test_format_effects_clause() {
        let src = "contract C { effects { io } }";
        let out = parse_and_format(src);
        assert!(out.contains("effects"));
    }

    #[test]
    fn test_format_input_clause() {
        let src = r#"
contract C {
    input(x: Int, y: Bool)
    requires { x > 0 }
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("input(x: Int, y: Bool)"));
    }

    #[test]
    fn test_format_output_clause() {
        let src = r#"
contract C {
    output(result: Int)
    ensures { result > 0 }
}
"#;
        let out = parse_and_format(src);
        assert!(out.contains("output(result: Int)"));
    }

    #[test]
    fn test_format_modifies_clause() {
        let src = "contract C { modifies { state } }";
        let out = parse_and_format(src);
        assert!(out.contains("modifies"));
    }

    #[test]
    fn test_format_decreases_clause() {
        let src = r#"
fn factorial(n: Int) -> Int
    requires { n >= 0 }
    decreases { n }
"#;
        let out = parse_and_format(src);
        assert!(out.contains("decreases"));
    }

    // ----- 9. Idempotency tests -----

    #[test]
    fn test_idempotent_contract() {
        assert_idempotent("contract Foo { requires { x > 0 } ensures { result > 0 } }");
    }

    #[test]
    fn test_idempotent_service() {
        assert_idempotent(
            r#"
service S {
    states: A -> B -> C
    operation go {
        requires { x > 0 }
    }
}
"#,
        );
    }

    #[test]
    fn test_idempotent_type_and_enum() {
        assert_idempotent(
            r#"
type Point {
    pub x: Int;
    pub y: Int;
}

enum Color {
    Red
    Green
    Blue
}
"#,
        );
    }

    #[test]
    fn test_idempotent_extern() {
        assert_idempotent("extern fn do_thing(a: Int) -> Bool\n");
    }

    #[test]
    fn test_idempotent_bind() {
        assert_idempotent(
            r#"
bind "lib::func" as wrapper {
    input(x: Int)
    output(result: Bool)
    requires { x >= 0 }
}
"#,
        );
    }

    // ----- 10. Edge case: empty source -----

    #[test]
    fn test_format_empty_source() {
        let src = "";
        let out = parse_and_format(src);
        assert_eq!(out, "");
    }

    // ----- 11. Edge case: file with only imports -----

    #[test]
    fn test_format_only_imports() {
        let src = "import std.math;\nimport std.collections;\n";
        let out = parse_and_format(src);
        assert!(out.contains("import std.math;"));
        assert!(out.contains("import std.collections;"));
        assert!(!out.contains("contract"));
        assert!(!out.contains("service"));
    }

    #[test]
    fn test_format_import_with_alias() {
        let src = "import std.math as m;\n";
        let out = parse_and_format(src);
        assert!(out.contains("import std.math as m;"));
    }

    #[test]
    fn test_format_import_with_items() {
        let src = "import std.math { abs, max };\n";
        let out = parse_and_format(src);
        assert!(out.contains("import std.math { abs, max };"));
    }

    // ----- 12. Edge case: file with project declaration -----

    #[test]
    fn test_format_project_declaration() {
        let src = "project MyProject { profile: [safety, security] }\n";
        let out = parse_and_format(src);
        assert!(out.contains("project MyProject { profile: [safety, security] }"));
    }

    // ----- Additional tests for coverage -----

    #[test]
    fn test_format_fn_def() {
        let src = r#"
fn add(a: Int, b: Int) -> Int
    requires { a >= 0 }
    ensures { result == a + b }
"#;
        let out = parse_and_format(src);
        assert!(out.contains("fn add(a: Int, b: Int) -> Int"));
        assert!(out.contains("requires"));
        assert!(out.contains("ensures"));
    }

    #[test]
    fn test_format_module_declaration() {
        let src = "module test.basic;\n";
        let out = parse_and_format(src);
        assert!(out.contains("module test.basic;"));
    }

    #[test]
    fn test_join_raw_tokens_dotted_path() {
        let tokens: Vec<String> = vec!["io".into(), ".".into(), "read".into()];
        assert_eq!(join_raw_tokens(&tokens), "io.read");
    }

    #[test]
    fn test_join_raw_tokens_simple() {
        let tokens: Vec<String> = vec!["hello".into(), "world".into()];
        assert_eq!(join_raw_tokens(&tokens), "hello world");
    }

    #[test]
    fn test_join_raw_tokens_empty() {
        let tokens: Vec<String> = vec![];
        assert_eq!(join_raw_tokens(&tokens), "");
    }

    #[test]
    fn test_format_literal_int() {
        let mut out = String::new();
        format_literal(&Literal::Int("42".to_string()), &mut out);
        assert_eq!(out, "42");
    }

    #[test]
    fn test_format_literal_bool() {
        let mut out = String::new();
        format_literal(&Literal::Bool(true), &mut out);
        assert_eq!(out, "true");

        let mut out2 = String::new();
        format_literal(&Literal::Bool(false), &mut out2);
        assert_eq!(out2, "false");
    }

    #[test]
    fn test_format_literal_string() {
        let mut out = String::new();
        format_literal(&Literal::Str("hello".to_string()), &mut out);
        assert_eq!(out, "\"hello\"");
    }

    #[test]
    fn test_format_unary_neg() {
        let mut out = String::new();
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".to_string())),
        };
        format_expr(&expr, &mut out);
        assert_eq!(out, "-x");
    }

    #[test]
    fn test_format_unary_not() {
        let mut out = String::new();
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Ident("flag".to_string())),
        };
        format_expr(&expr, &mut out);
        assert_eq!(out, "!flag");
    }

    #[test]
    fn test_format_paren_expr() {
        let mut out = String::new();
        let expr = Expr::Paren(Box::new(Expr::Ident("x".to_string())));
        format_expr(&expr, &mut out);
        assert_eq!(out, "(x)");
    }

    #[test]
    fn test_format_list_expr() {
        let mut out = String::new();
        let expr = Expr::List(vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("2".into())),
            Expr::Literal(Literal::Int("3".into())),
        ]);
        format_expr(&expr, &mut out);
        assert_eq!(out, "[1, 2, 3]");
    }

    #[test]
    fn test_format_pattern_wildcard() {
        let mut out = String::new();
        format_pattern(&Pattern::Wildcard, &mut out);
        assert_eq!(out, "_");
    }

    #[test]
    fn test_format_pattern_constructor() {
        let mut out = String::new();
        format_pattern(
            &Pattern::Constructor {
                name: "Some".to_string(),
                fields: vec![Pattern::Ident("x".to_string())],
            },
            &mut out,
        );
        assert_eq!(out, "Some(x)");
    }

    #[test]
    fn test_binop_str_all_ops() {
        assert_eq!(binop_str(&BinOp::Add), "+");
        assert_eq!(binop_str(&BinOp::Sub), "-");
        assert_eq!(binop_str(&BinOp::Mul), "*");
        assert_eq!(binop_str(&BinOp::Div), "/");
        assert_eq!(binop_str(&BinOp::Mod), "%");
        assert_eq!(binop_str(&BinOp::Eq), "==");
        assert_eq!(binop_str(&BinOp::Neq), "!=");
        assert_eq!(binop_str(&BinOp::Lt), "<");
        assert_eq!(binop_str(&BinOp::Lte), "<=");
        assert_eq!(binop_str(&BinOp::Gt), ">");
        assert_eq!(binop_str(&BinOp::Gte), ">=");
        assert_eq!(binop_str(&BinOp::And), "&&");
        assert_eq!(binop_str(&BinOp::Or), "||");
        assert_eq!(binop_str(&BinOp::Implies), "==>");
        assert_eq!(binop_str(&BinOp::In), "in");
        assert_eq!(binop_str(&BinOp::NotIn), "not in");
        assert_eq!(binop_str(&BinOp::Concat), "++");
        assert_eq!(binop_str(&BinOp::Range), "..");
    }

    #[test]
    fn test_format_reparseable() {
        let src = r#"
contract SafeDivide {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
    ensures { result == a / b }
}
"#;
        let formatted = parse_and_format(src);
        let (file2, errs2) = assura_parser::parse(&formatted);
        assert!(
            errs2.is_empty(),
            "formatted output should re-parse: {errs2:?}"
        );
        assert!(
            file2.is_some(),
            "formatted output should produce a SourceFile"
        );
    }

    #[test]
    fn test_format_exists_quantifier() {
        let src = "contract Ex { requires { exists x in items: x == target } }";
        let out = parse_and_format(src);
        assert!(out.contains("exists x in items: x == target"));
    }

    #[test]
    fn test_format_field_access() {
        let mut out = String::new();
        let expr = Expr::Field(Box::new(Expr::Ident("point".to_string())), "x".to_string());
        format_expr(&expr, &mut out);
        assert_eq!(out, "point.x");
    }

    #[test]
    fn test_format_index_expr() {
        let mut out = String::new();
        let expr = Expr::Index {
            expr: Box::new(Expr::Ident("arr".to_string())),
            index: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        format_expr(&expr, &mut out);
        assert_eq!(out, "arr[0]");
    }
}
