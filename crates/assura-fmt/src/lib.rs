//! Source code formatter for the Assura contract language.
//!
//! Takes a parsed `SourceFile` AST and produces well-formatted source text.

use assura_parser::ast::{
    BinOp, BindDecl, Clause, ClauseKind, ContractDecl, Decl, EnumDef, Expr, ExternDecl, FnDef,
    Literal, Pattern, ServiceDecl, ServiceItem, SourceFile, TypeBody, TypeDef, UnaryOp,
    extract_clause_params,
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
        out.push_str(&format!(": {}", v.join(" ")));
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
