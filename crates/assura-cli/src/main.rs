#![allow(dead_code)]

use std::env;
use std::fs;
use std::process;

use ariadne::{Color, Label, Report, ReportKind, Source};
use assura_parser::ast::*;
use assura_parser::lexer::Token;
use assura_parser::parser;
use chumsky::Stream;
use chumsky::prelude::*;
use logos::Logos;

fn main() {
    let args: Vec<String> = env::args().collect();

    let show_ast = args.contains(&"--ast".to_string());
    let show_tokens = args.contains(&"--tokens".to_string());

    let filename = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: assura [--ast|--tokens] <file.assura>");
            process::exit(2);
        });

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Lex ---
    let lex = Token::lexer(&source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();
    let mut lex_errors = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => lex_errors.push(span),
        }
    }

    if show_tokens {
        for (tok, span) in &tokens {
            let line = source[..span.start].lines().count();
            let col = span.start - source[..span.start].rfind('\n').map_or(0, |p| p + 1) + 1;
            println!("{line}:{col}  {tok:?}");
        }
        return;
    }

    for span in &lex_errors {
        let snippet = &source[span.clone()];
        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(format!("unexpected character: {snippet:?}"))
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message("invalid token")
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    if !lex_errors.is_empty() {
        process::exit(1);
    }

    // --- Parse ---
    let len = source.len();
    let token_stream = Stream::from_iter(len..len + 1, tokens.into_iter());

    let (file, parse_errors) = parser::source_file().parse_recovery(token_stream);

    for e in &parse_errors {
        let span = e.span();
        let found = e
            .found()
            .map(|t| format!("{t}"))
            .unwrap_or_else(|| "end of file".to_string());
        let expected: Vec<String> = e
            .expected()
            .map(|ex| match ex {
                Some(t) => format!("{t}"),
                None => "end of input".to_string(),
            })
            .collect();

        let msg = if expected.is_empty() {
            format!("unexpected {found}")
        } else {
            format!("expected {}, found {found}", expected.join(" or "))
        };

        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(&msg)
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message(&msg)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    let Some(file) = file else {
        eprintln!("{filename}: parse failed");
        process::exit(1);
    };

    if !parse_errors.is_empty() {
        process::exit(1);
    }

    // --- Resolve ---
    let resolved = match assura_resolve::resolve(&file) {
        Ok(r) => r,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} resolution error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Type check ---
    let typed = match assura_types::type_check(&resolved) {
        Ok(t) => t,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} type error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Output ---
    if show_ast {
        print_ast(&file);
    } else {
        print_summary(filename, &file, &resolved.symbols, &typed.type_env);
    }
}

fn print_summary(
    filename: &str,
    file: &SourceFile,
    symbols: &assura_resolve::SymbolTable,
    type_env: &assura_types::TypeEnv,
) {
    let mut contracts = 0u32;
    let mut types = 0u32;
    let mut enums = 0u32;
    let mut externs = 0u32;
    let mut fns = 0u32;
    let mut services = 0u32;
    let mut other = 0u32;

    for d in &file.decls {
        match &d.node {
            Decl::Contract(_) => contracts += 1,
            Decl::TypeDef(_) => types += 1,
            Decl::EnumDef(_) => enums += 1,
            Decl::Extern(_) => externs += 1,
            Decl::FnDef(_) => fns += 1,
            Decl::Service(_) => services += 1,
            Decl::Block { .. } => other += 1,
        }
    }

    println!("OK  {filename}");
    if let Some(p) = &file.project {
        println!(
            "    project:   {}  profile: [{}]",
            p.name,
            p.profile.join(", ")
        );
    }
    if let Some(m) = &file.module {
        println!("    module:    {}", m.path.join("."));
    }
    println!("    imports:   {}", file.imports.len());

    let mut parts = Vec::new();
    if contracts > 0 {
        parts.push(format!("{contracts} contract(s)"));
    }
    if types > 0 {
        parts.push(format!("{types} type(s)"));
    }
    if enums > 0 {
        parts.push(format!("{enums} enum(s)"));
    }
    if externs > 0 {
        parts.push(format!("{externs} extern(s)"));
    }
    if fns > 0 {
        parts.push(format!("{fns} fn(s)"));
    }
    if services > 0 {
        parts.push(format!("{services} service(s)"));
    }
    if other > 0 {
        parts.push(format!("{other} other"));
    }
    println!(
        "    declares:  {}",
        if parts.is_empty() {
            "(empty)".to_string()
        } else {
            parts.join(", ")
        }
    );
    let user_symbols = symbols
        .symbols
        .iter()
        .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
        .count();
    println!("    resolve:   OK ({user_symbols} symbols)");
    println!("    typecheck: OK ({} bindings)", type_env.len());
}

fn print_ast(file: &SourceFile) {
    if let Some(p) = &file.project {
        println!("Project: {} [{}]", p.name, p.profile.join(", "));
    }
    if let Some(m) = &file.module {
        println!("Module: {}", m.path.join("."));
    }
    for imp in &file.imports {
        let alias = imp
            .alias
            .as_deref()
            .map(|a| format!(" as {a}"))
            .unwrap_or_default();
        let items = if imp.items.is_empty() {
            String::new()
        } else {
            format!(" {{{}}}", imp.items.join(", "))
        };
        println!("Import: {}{alias}{items}", imp.path.join("."));
    }
    for d in &file.decls {
        print_decl(&d.node, 0);
    }
}

fn print_decl(decl: &Decl, indent: usize) {
    let pad = "  ".repeat(indent);
    match decl {
        Decl::Contract(c) => {
            let tps = if c.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", c.type_params.join(", "))
            };
            println!("{pad}Contract: {}{tps}", c.name);
            for cl in &c.clauses {
                let body = truncate(&expr_to_string(&cl.body), 60);
                println!("{pad}  {:?}: {body}", cl.kind);
            }
        }
        Decl::TypeDef(t) => {
            let tps = if t.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", t.type_params.join(", "))
            };
            match &t.body {
                TypeBody::Refined(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {{{}}}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Alias(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Struct(fields) => {
                    println!("{pad}Type: {}{tps}", t.name);
                    for f in fields {
                        let pub_str = if f.is_pub { "pub " } else { "" };
                        println!("{pad}  {pub_str}{}: {}", f.name, f.ty.join(" "));
                    }
                }
                TypeBody::Empty => println!("{pad}Type: {}{tps}", t.name),
            }
        }
        Decl::EnumDef(e) => {
            let tps = if e.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", e.type_params.join(", "))
            };
            println!("{pad}Enum: {}{tps}", e.name);
            for v in &e.variants {
                if v.fields.is_empty() {
                    println!("{pad}  {}", v.name);
                } else {
                    println!("{pad}  {}({})", v.name, v.fields.join(" "));
                }
            }
        }
        Decl::Extern(ex) => {
            let params = ex
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{pad}Extern: fn {}({params}) -> {}",
                ex.name,
                ex.return_ty.join(" ")
            );
            for cl in &ex.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::FnDef(f) => {
            let params = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = if f.return_ty.is_empty() {
                String::new()
            } else {
                format!(" -> {}", f.return_ty.join(" "))
            };
            println!("{pad}Fn: {}({params}){ret}", f.name);
            for cl in &f.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::Service(s) => {
            println!("{pad}Service: {}", s.name);
            for item in &s.items {
                match item {
                    ServiceItem::TypeDef(t) => {
                        println!("{pad}  type: {}", t.name);
                    }
                    ServiceItem::States(states) => {
                        println!("{pad}  states: {}", states.join(" -> "));
                    }
                    ServiceItem::Operation { name, clauses } => {
                        println!("{pad}  operation: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Query { name, clauses } => {
                        println!("{pad}  query: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Invariant(expr) => {
                        println!("{pad}  invariant: {}", truncate(&expr_to_string(expr), 50));
                    }
                    _ => {}
                }
            }
        }
        Decl::Block { kind, name, body } => {
            println!("{pad}{kind}: {name} ({} clause(s))", body.len());
        }
    }
}

fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => s.clone(),
        Expr::Field(e, f) => format!("{}.{f}", expr_to_string(e)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!(
                "{}.{method}({})",
                expr_to_string(receiver),
                args_s.join(", ")
            )
        }
        Expr::Call { func, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("{}({})", expr_to_string(func), args_s.join(", "))
        }
        Expr::Index { expr: e, index } => {
            format!("{}[{}]", expr_to_string(e), expr_to_string(index))
        }
        Expr::BinOp { lhs, op, rhs } => {
            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "mod",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                BinOp::In => "in",
                BinOp::NotIn => "not in",
                BinOp::Concat => "++",
                BinOp::Range => "..",
            };
            format!("{} {op_s} {}", expr_to_string(lhs), expr_to_string(rhs))
        }
        Expr::UnaryOp { op, expr: e } => {
            let op_s = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "not",
            };
            format!("{op_s} {}", expr_to_string(e))
        }
        Expr::Old(e) => format!("old({})", expr_to_string(e)),
        Expr::Forall { var, domain, body } => {
            format!(
                "forall {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "exists {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => match else_branch {
            Some(eb) => format!(
                "if {} then {} else {}",
                expr_to_string(cond),
                expr_to_string(then_branch),
                expr_to_string(eb)
            ),
            None => format!(
                "if {} then {}",
                expr_to_string(cond),
                expr_to_string(then_branch)
            ),
        },
        Expr::Paren(e) => format!("({})", expr_to_string(e)),
        Expr::List(elems) => {
            let elems_s: Vec<String> = elems.iter().map(expr_to_string).collect();
            format!("[{}]", elems_s.join(", "))
        }
        Expr::Cast { expr: e, ty } => format!("{} as {ty}", expr_to_string(e)),
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_string).collect();
            strs.join(" ")
        }
        Expr::Raw(tokens) => tokens.join(" "),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}
