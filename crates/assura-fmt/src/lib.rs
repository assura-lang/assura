//! Source code formatter for the Assura contract language.
//!
//! Takes a parsed `SourceFile` AST and produces well-formatted source text.

use assura_ast::ExprFolder;
use assura_parser::ast::{
    BinOp, BindDecl, BlockKind, Clause, ClauseKind, CodecRegistryDecl, ContractDecl, Decl, EnumDef,
    ExternDecl, FnDef, Literal, MagicPattern, Pattern, ProphecyDecl, ServiceDecl, ServiceItem,
    SourceFile, SpExpr, TypeBody, TypeDef, UnaryOp, extract_clause_params,
};
// Re-exported for test module (format_tests.rs uses Expr via `use crate::*`).
#[cfg(test)]
use assura_parser::ast::Expr;

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

pub(crate) fn format_decl(decl: &Decl, out: &mut String) {
    match decl {
        Decl::Contract(c) => format_contract(c, out),
        Decl::Service(s) => format_service(s, out),
        Decl::TypeDef(t) => format_typedef(t, out),
        Decl::EnumDef(e) => format_enumdef(e, out),
        Decl::Extern(e) => format_extern(e, out),
        Decl::Bind(b) => format_bind(b, out),
        Decl::Prophecy(p) => format_prophecy(p, out),
        Decl::CodecRegistry(cr) => format_codec_registry(cr, out),
        Decl::FnDef(f) => format_fndef(f, out),
        Decl::Block {
            kind,
            name,
            value,
            body,
        } => format_block(kind, name, value, body, out),
    }
}

pub(crate) fn format_contract(c: &ContractDecl, out: &mut String) {
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

pub(crate) fn format_service(s: &ServiceDecl, out: &mut String) {
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

pub(crate) fn format_typedef(t: &TypeDef, out: &mut String) {
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
                let ty_s = f.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
                out.push_str(&format!("    {vis}{}: {ty_s};\n", f.name));
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

pub(crate) fn format_enumdef(e: &EnumDef, out: &mut String) {
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

pub(crate) fn format_extern(e: &ExternDecl, out: &mut String) {
    out.push_str("extern fn ");
    out.push_str(&e.name);
    out.push('(');
    let params: Vec<String> = e
        .params
        .iter()
        .map(|p| match &p.ty {
            None => p.name.clone(),
            Some(te) => format!("{}: {te}", p.name),
        })
        .collect();
    out.push_str(&params.join(", "));
    out.push(')');
    if let Some(ret) = &e.return_ty {
        out.push_str(&format!(" -> {ret}"));
    }
    out.push('\n');
    for clause in &e.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
}

pub(crate) fn format_bind(b: &BindDecl, out: &mut String) {
    out.push_str(&format!("bind \"{}\" as {} {{\n", b.target_path, b.name));
    if !b.params.is_empty() {
        out.push_str("    input(");
        let params: Vec<String> = b
            .params
            .iter()
            .map(|p| match &p.ty {
                None => p.name.clone(),
                Some(te) => format!("{}: {te}", p.name),
            })
            .collect();
        out.push_str(&params.join(", "));
        out.push_str(")\n");
    }
    if let Some(ret) = &b.return_ty {
        out.push_str(&format!("    output(result: {ret})\n"));
    }
    for clause in &b.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
    out.push_str("}\n");
}

pub(crate) fn format_prophecy(p: &ProphecyDecl, out: &mut String) {
    out.push_str("ghost prophecy ");
    out.push_str(&p.name);
    if let Some(te) = &p.ty {
        out.push_str(": ");
        out.push_str(&te.to_string());
    }
    out.push('\n');
}

pub(crate) fn format_codec_registry(cr: &CodecRegistryDecl, out: &mut String) {
    out.push_str("codec_registry ");
    out.push_str(&cr.name);
    out.push_str(" {\n");
    out.push_str("    output: ");
    out.push_str(&cr.output_type.join(" "));
    out.push_str(",\n");
    for codec in &cr.codecs {
        out.push_str("\n    codec ");
        out.push_str(&codec.name);
        out.push_str(" {\n");
        out.push_str("        magic: ");
        match &codec.magic {
            MagicPattern::Bytes { bytes, prefix } => {
                out.push('[');
                for (i, b) in bytes.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("0x{b:02X}"));
                }
                if *prefix {
                    out.push_str(", ..");
                }
                out.push(']');
            }
            MagicPattern::Extension(exts) => {
                out.push_str("extension(");
                for (i, e) in exts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("\"{e}\""));
                }
                out.push(')');
            }
            MagicPattern::Probe(fn_name) => {
                out.push_str(&format!("probe({fn_name})"));
            }
        }
        out.push_str(",\n");
        out.push_str("        decoder: ");
        out.push_str(&codec.decoder);
        if codec.contracts.is_empty() {
            out.push('\n');
        } else {
            out.push_str(",\n        contracts: {\n");
            for clause in &codec.contracts {
                out.push_str("            ");
                format_clause(clause, out);
            }
            out.push_str("        }\n");
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
}

pub(crate) fn format_fndef(f: &FnDef, out: &mut String) {
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
        .map(|p| match &p.ty {
            None => p.name.clone(),
            Some(te) => format!("{}: {te}", p.name),
        })
        .collect();
    out.push_str(&params.join(", "));
    out.push(')');
    if let Some(ret) = &f.return_ty {
        out.push_str(&format!(" -> {ret}"));
    }
    out.push('\n');
    for clause in &f.clauses {
        out.push_str("    ");
        format_clause(clause, out);
        out.push('\n');
    }
}

pub(crate) fn format_block(
    kind: &BlockKind,
    name: &str,
    value: &Option<Vec<String>>,
    body: &[Clause],
    out: &mut String,
) {
    out.push_str(&kind.to_string());
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

/// Format a single clause (requires/ensures/invariant/effects/etc.) to text.
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
        ClauseKind::Ordering => "ordering",
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
                if let Some(te) = &p.ty {
                    out.push_str(": ");
                    out.push_str(&te.to_string());
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

pub(crate) fn is_braced_kind(kind: &ClauseKind) -> bool {
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

pub(crate) fn format_expr(expr: &SpExpr, out: &mut String) {
    FmtExprFolder { out }.fold_expr(expr);
}

struct FmtExprFolder<'a> {
    out: &'a mut String,
}

impl<'a> ExprFolder for FmtExprFolder<'a> {
    type Output = ();

    fn fold_literal(&mut self, lit: &Literal) {
        format_literal(lit, self.out);
    }

    fn fold_ident(&mut self, name: &str) {
        self.out.push_str(name);
    }

    fn fold_field(&mut self, base: &SpExpr, field: &str) {
        self.fold_expr(base);
        self.out.push('.');
        self.out.push_str(field);
    }

    fn fold_method_call(&mut self, receiver: &SpExpr, method: &str, args: &[SpExpr]) {
        self.fold_expr(receiver);
        self.out.push('.');
        self.out.push_str(method);
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.fold_expr(a);
        }
        self.out.push(')');
    }

    fn fold_call(&mut self, func: &SpExpr, args: &[SpExpr]) {
        self.fold_expr(func);
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.fold_expr(a);
        }
        self.out.push(')');
    }

    fn fold_index(&mut self, base: &SpExpr, index: &SpExpr) {
        self.fold_expr(base);
        self.out.push('[');
        self.fold_expr(index);
        self.out.push(']');
    }

    fn fold_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) {
        // Use binop_str for correct Assura syntax (&& vs and, etc.)
        self.fold_expr(lhs);
        self.out.push(' ');
        self.out.push_str(binop_str(op));
        self.out.push(' ');
        self.fold_expr(rhs);
    }

    fn fold_unary_op(&mut self, op: &UnaryOp, inner: &SpExpr) {
        self.out.push_str(op.as_rust_str());
        self.fold_expr(inner);
    }

    fn fold_old(&mut self, inner: &SpExpr) {
        self.out.push_str("old(");
        self.fold_expr(inner);
        self.out.push(')');
    }

    fn fold_forall(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) {
        self.out.push_str("forall ");
        self.out.push_str(var);
        self.out.push_str(" in ");
        self.fold_expr(domain);
        self.out.push_str(": ");
        self.fold_expr(body);
    }

    fn fold_exists(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) {
        self.out.push_str("exists ");
        self.out.push_str(var);
        self.out.push_str(" in ");
        self.fold_expr(domain);
        self.out.push_str(": ");
        self.fold_expr(body);
    }

    fn fold_if(&mut self, cond: &SpExpr, then_br: &SpExpr, else_br: Option<&SpExpr>) {
        self.out.push_str("if ");
        self.fold_expr(cond);
        self.out.push_str(" then ");
        self.fold_expr(then_br);
        if let Some(eb) = else_br {
            self.out.push_str(" else ");
            self.fold_expr(eb);
        }
    }

    fn fold_list(&mut self, items: &[SpExpr]) {
        self.out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.fold_expr(item);
        }
        self.out.push(']');
    }

    fn fold_cast(&mut self, inner: &SpExpr, ty: &str) {
        self.fold_expr(inner);
        self.out.push_str(" as ");
        self.out.push_str(ty);
    }

    fn fold_block(&mut self, exprs: &[SpExpr]) {
        for (i, item) in exprs.iter().enumerate() {
            if i > 0 {
                self.out.push(' ');
            }
            self.fold_expr(item);
        }
    }

    fn fold_ghost(&mut self, inner: &SpExpr) {
        self.out.push_str("ghost { ");
        self.fold_expr(inner);
        self.out.push_str(" }");
    }

    fn fold_apply(&mut self, name: &str, args: &[SpExpr]) {
        self.out.push_str("apply ");
        self.out.push_str(name);
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.fold_expr(a);
        }
        self.out.push(')');
    }

    fn fold_let(&mut self, name: &str, value: &SpExpr, body: &SpExpr) {
        self.out.push_str("let ");
        self.out.push_str(name);
        self.out.push_str(" = ");
        self.fold_expr(value);
        self.out.push_str(" in ");
        self.fold_expr(body);
    }

    fn fold_match(&mut self, scrutinee: &SpExpr, arms: &[assura_ast::MatchArm]) {
        self.out.push_str("match ");
        self.fold_expr(scrutinee);
        self.out.push_str(" { ");
        for (i, arm) in arms.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            format_pattern(&arm.pattern, self.out);
            self.out.push_str(" => ");
            self.fold_expr(&arm.body);
        }
        self.out.push_str(" }");
    }

    fn fold_tuple(&mut self, items: &[SpExpr]) {
        self.out.push('(');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.fold_expr(item);
        }
        self.out.push(')');
    }

    fn fold_raw(&mut self, tokens: &[String]) {
        self.out.push_str(&join_raw_tokens(tokens));
    }
}

pub(crate) fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Implies => "==>",
        BinOp::In | BinOp::NotIn | BinOp::Concat | BinOp::Range => op.as_str(),
        _ => op.as_rust_str(),
    }
}

pub(crate) fn format_literal(lit: &Literal, out: &mut String) {
    out.push_str(&assura_ast::literal_to_string(lit));
}

pub(crate) fn format_pattern(pat: &Pattern, out: &mut String) {
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
pub(crate) fn join_raw_tokens(tokens: &[String]) -> String {
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

#[cfg(test)]
mod format_tests;
