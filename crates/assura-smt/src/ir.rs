// ===========================================================================
// T112: IR format parser
// ===========================================================================

/// Implementation IR: the intermediate format that AI agents generate.
#[derive(Debug, Clone)]
pub struct IrParser {
    nodes: Vec<IrNode>,
}

#[derive(Debug, Clone)]
pub enum IrNode {
    FnDecl {
        name: String,
        params: Vec<(String, String)>,
        body: Vec<IrNode>,
    },
    VarDecl {
        name: String,
        ty: String,
        value: Option<Box<IrNode>>,
    },
    Call {
        target: String,
        args: Vec<IrNode>,
    },
    Literal(IrLiteral),
    BinOp {
        op: String,
        left: Box<IrNode>,
        right: Box<IrNode>,
    },
    Return(Box<IrNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrLiteral {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

impl IrParser {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Parse a text IR into nodes.
    pub fn parse_text(&mut self, source: &str) -> Result<(), String> {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with("fn ") {
                let name = trimmed
                    .strip_prefix("fn ")
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.nodes.push(IrNode::FnDecl {
                    name,
                    params: Vec::new(),
                    body: Vec::new(),
                });
            } else if trimmed.starts_with("let ") {
                let rest = trimmed.strip_prefix("let ").unwrap_or("");
                let name = rest.split(':').next().unwrap_or("").trim().to_string();
                self.nodes.push(IrNode::VarDecl {
                    name,
                    ty: "auto".into(),
                    value: None,
                });
            } else if trimmed.starts_with("return ") {
                let val = trimmed.strip_prefix("return ").unwrap_or("").trim();
                if let Ok(n) = val.parse::<i64>() {
                    self.nodes
                        .push(IrNode::Return(Box::new(IrNode::Literal(IrLiteral::Int(n)))));
                }
            }
        }
        Ok(())
    }

    /// Serialize nodes to a compact binary format.
    pub fn serialize_binary(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for node in &self.nodes {
            match node {
                IrNode::FnDecl { name, .. } => {
                    buf.push(0x01);
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                }
                IrNode::VarDecl { name, .. } => {
                    buf.push(0x02);
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                }
                IrNode::Return(_) => {
                    buf.push(0x03);
                }
                _ => {
                    buf.push(0xFF);
                }
            }
        }
        buf
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl Default for IrParser {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// P005: Implementation IR (Section 4 of the spec)
// ===========================================================================

/// A parsed IR module (Section 4.2 of the spec).
///
/// The Implementation IR is the flat, numbered-slot format that AI agents
/// generate. Slots are `$0`, `$1`, etc. No variable names. Every
/// expression is explicitly typed. One canonical representation.
#[derive(Debug, Clone, PartialEq)]
pub struct IrModule {
    pub name: String,
    pub functions: Vec<IrFunction>,
}

/// A function declaration in the IR.
#[derive(Debug, Clone, PartialEq)]
pub struct IrFunction {
    /// Function ID (e.g., "#0", "#1")
    pub id: String,
    /// Parameter slots
    pub params: Vec<IrSlotDecl>,
    /// Return type
    pub return_type: String,
    /// Effect row (e.g., "pure", "io")
    pub effects: String,
    /// Precondition (optional)
    pub pre: Option<IrPred>,
    /// Postcondition (optional)
    pub post: Option<IrPred>,
    /// Instruction body
    pub body: Vec<IrInstr>,
}

/// A slot declaration: `$N : Type`
#[derive(Debug, Clone, PartialEq)]
pub struct IrSlotDecl {
    pub slot: usize,
    pub ty: String,
}

/// An instruction: `$N = <expr> : Type`
#[derive(Debug, Clone, PartialEq)]
pub struct IrInstr {
    pub target: usize,
    pub expr: IrExprKind,
    pub ty: String,
}

/// IR expression forms (Section 4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum IrExprKind {
    /// `const <literal>`
    Const(IrLiteral),
    /// `load $N`
    Load(usize),
    /// `call <fn> ($N, $M, ...)`
    Call { func: String, args: Vec<usize> },
    /// `field $N .M`
    Field { slot: usize, index: usize },
    /// `construct TypeId { .0 = $N, .1 = $M, ... }`
    Construct {
        type_id: String,
        fields: Vec<(usize, usize)>,
    },
    /// `arith <op> $N $M`
    Arith {
        op: IrArithOp,
        lhs: usize,
        rhs: usize,
    },
    /// `cmp <op> $N $M`
    Cmp { op: IrCmpOp, lhs: usize, rhs: usize },
    /// `cast $N as Type`
    Cast { slot: usize, target_type: String },
    /// `if $N then #B1 else #B2`
    If {
        cond: usize,
        then_block: usize,
        else_block: usize,
    },
    /// `transition $N to StateId`
    Transition { slot: usize, state: String },
    /// `match $N { 0 => #B0, 1 => #B1, _ => #Bdef }`
    Match {
        scrutinee: usize,
        arms: Vec<(IrMatchPattern, usize)>,
    },
    /// `loop #body_block $cond`
    Loop { body_block: usize, cond: usize },
}

/// Pattern in an IR match arm.
#[derive(Debug, Clone, PartialEq)]
pub enum IrMatchPattern {
    /// Integer literal
    Int(i64),
    /// Boolean literal
    Bool(bool),
    /// String literal
    Str(String),
    /// Wildcard `_`
    Wildcard,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrCmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// IR predicate for pre/post conditions (Section 4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum IrPred {
    True,
    False,
    Cmp {
        op: IrCmpOp,
        lhs: IrPredArg,
        rhs: IrPredArg,
    },
    And(Box<IrPred>, Box<IrPred>),
    Or(Box<IrPred>, Box<IrPred>),
    Not(Box<IrPred>),
}

/// Argument inside an IR predicate (slot ref or literal).
#[derive(Debug, Clone, PartialEq)]
pub enum IrPredArg {
    Slot(usize),
    SlotResult,
    Lit(IrLiteral),
    Arith {
        op: IrArithOp,
        lhs: Box<IrPredArg>,
        rhs: Box<IrPredArg>,
    },
}

/// Result of validating IR against a contract.
#[derive(Debug, Clone)]
pub struct IrValidation {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Parse an IR text module from source.
///
/// The text format follows Section 4.2 of the spec:
/// ```text
/// module <name> {
///   fn #0 : ($0: Int, $1: Int) -> Int ! pure
///     pre: cmp ne $1 (const 0)
///     post: cmp eq ... $0
///   {
///     $2 = arith div $0 $1 : Int
///     $result = load $2 : Int
///   }
/// }
/// ```
pub fn parse_ir_module(source: &str) -> Result<IrModule, Vec<String>> {
    let mut errors = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    // Skip blanks / comments
    while i < lines.len() {
        let t = lines[i].trim();
        if !t.is_empty() && !t.starts_with("//") {
            break;
        }
        i += 1;
    }

    // Parse module header
    if i >= lines.len() {
        errors.push("expected 'module <name> {', got end of input".into());
        return Err(errors);
    }
    let header = lines[i].trim();
    let module_name = if let Some(rest) = header.strip_prefix("module ") {
        let rest = rest.trim();
        let name = rest.trim_end_matches('{').trim().to_string();
        if name.is_empty() {
            errors.push(format!("line {}: empty module name", i + 1));
            return Err(errors);
        }
        name
    } else {
        errors.push(format!(
            "line {}: expected 'module <name> {{', got: {}",
            i + 1,
            header
        ));
        return Err(errors);
    };
    i += 1;

    let mut functions = Vec::new();

    // Parse declarations until closing '}'
    while i < lines.len() {
        let t = lines[i].trim();
        if t == "}" {
            break;
        }
        if t.is_empty() || t.starts_with("//") {
            i += 1;
            continue;
        }
        if t.starts_with("fn ") {
            match parse_ir_function(&lines, &mut i) {
                Ok(f) => functions.push(f),
                Err(e) => errors.extend(e),
            }
        } else {
            errors.push(format!("line {}: unexpected: {}", i + 1, t));
            i += 1;
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(IrModule {
        name: module_name,
        functions,
    })
}

fn parse_ir_function(lines: &[&str], pos: &mut usize) -> Result<IrFunction, Vec<String>> {
    let mut errors = Vec::new();
    let header = lines[*pos].trim();

    // Parse: fn #0 : ($0: Int, $1: Int) -> Int ! pure
    let rest = header.strip_prefix("fn ").unwrap_or("");
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    let id = parts[0].trim().to_string();

    let sig_str = if parts.len() > 1 { parts[1].trim() } else { "" };

    // Parse params from signature: ($0: Type, $1: Type) -> RetType ! Effects
    let (params, return_type, effects) = parse_ir_sig(sig_str);

    *pos += 1;

    // Parse optional pre/post conditions
    let mut pre = None;
    let mut post = None;
    while *pos < lines.len() {
        let t = lines[*pos].trim();
        if t.starts_with("pre:") {
            let pred_str = t.strip_prefix("pre:").unwrap_or("").trim();
            pre = parse_ir_pred_str(pred_str);
            *pos += 1;
        } else if t.starts_with("post:") {
            let pred_str = t.strip_prefix("post:").unwrap_or("").trim();
            post = parse_ir_pred_str(pred_str);
            *pos += 1;
        } else {
            break;
        }
    }

    // Parse body: { ... }
    let mut body = Vec::new();
    if *pos < lines.len() && lines[*pos].trim() == "{" {
        *pos += 1;
    }
    while *pos < lines.len() {
        let t = lines[*pos].trim();
        if t == "}" {
            *pos += 1;
            break;
        }
        if t.is_empty() || t.starts_with("//") {
            *pos += 1;
            continue;
        }
        match parse_ir_instr(t) {
            Ok(instr) => body.push(instr),
            Err(e) => errors.push(format!("line {}: {}", *pos + 1, e)),
        }
        *pos += 1;
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(IrFunction {
        id,
        params,
        return_type,
        effects,
        pre,
        post,
        body,
    })
}

fn parse_ir_sig(sig: &str) -> (Vec<IrSlotDecl>, String, String) {
    let mut params = Vec::new();
    let mut return_type = String::new();
    let mut effects = String::new();

    // Find the param list between ( and )
    if let Some(paren_start) = sig.find('(')
        && let Some(paren_end) = sig.find(')')
    {
        let param_str = &sig[paren_start + 1..paren_end];
        for part in param_str.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            // Parse $N: Type or $N: Type @grade
            let slot_and_ty: Vec<&str> = part.splitn(2, ':').collect();
            if slot_and_ty.len() == 2 {
                let slot_str = slot_and_ty[0].trim().trim_start_matches('$');
                let ty_str = slot_and_ty[1].trim().split('@').next().unwrap_or("").trim();
                if let Ok(slot) = slot_str.parse::<usize>() {
                    params.push(IrSlotDecl {
                        slot,
                        ty: ty_str.to_string(),
                    });
                }
            }
        }

        // Parse: -> RetType ! Effects
        let after_params = &sig[paren_end + 1..];
        if let Some(arrow_pos) = after_params.find("->") {
            let after_arrow = &after_params[arrow_pos + 2..];
            if let Some(bang_pos) = after_arrow.find('!') {
                return_type = after_arrow[..bang_pos].trim().to_string();
                effects = after_arrow[bang_pos + 1..].trim().to_string();
            } else {
                return_type = after_arrow.trim().to_string();
            }
        }
    }

    (params, return_type, effects)
}

fn parse_slot(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if s == "$result" {
        return Ok(usize::MAX); // sentinel for $result
    }
    s.strip_prefix('$')
        .and_then(|n| n.parse::<usize>().ok())
        .ok_or_else(|| format!("expected slot ($N), got: {s}"))
}

fn parse_ir_instr(line: &str) -> Result<IrInstr, String> {
    // Format: $N = <expr> : Type
    // or: $result = <expr> : Type
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("expected '$N = <expr> : Type', got: {line}"));
    }
    let target = parse_slot(parts[0].trim())?;

    // Split on last ' : ' to get expr and type
    let rhs = parts[1].trim();
    let (expr_str, ty) = if let Some(colon_pos) = rhs.rfind(" : ") {
        (&rhs[..colon_pos], rhs[colon_pos + 3..].trim().to_string())
    } else {
        (rhs, String::new())
    };
    let expr_str = expr_str.trim();

    let expr = parse_ir_expr(expr_str)?;

    Ok(IrInstr { target, expr, ty })
}

fn parse_ir_expr(s: &str) -> Result<IrExprKind, String> {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix("const ") {
        // const <literal>
        let lit = parse_ir_literal(rest.trim())?;
        return Ok(IrExprKind::Const(lit));
    }

    if let Some(rest) = s.strip_prefix("load ") {
        let slot = parse_slot(rest.trim())?;
        return Ok(IrExprKind::Load(slot));
    }

    if let Some(rest) = s.strip_prefix("call ") {
        // call <fn> ($N, $M, ...)
        let rest = rest.trim();
        if let Some(paren_start) = rest.find('(') {
            let func = rest[..paren_start].trim().to_string();
            let paren_end = rest.rfind(')').unwrap_or(rest.len());
            let args_str = &rest[paren_start + 1..paren_end];
            let mut args = Vec::new();
            for a in args_str.split(',') {
                let a = a.trim();
                if !a.is_empty() {
                    args.push(parse_slot(a)?);
                }
            }
            return Ok(IrExprKind::Call { func, args });
        }
        return Err(format!("malformed call: {s}"));
    }

    if let Some(rest) = s.strip_prefix("field ") {
        // field $N .M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            let slot = parse_slot(parts[0])?;
            let index = parts[1]
                .trim_start_matches('.')
                .parse::<usize>()
                .map_err(|_| format!("bad field index: {}", parts[1]))?;
            return Ok(IrExprKind::Field { slot, index });
        }
        return Err(format!("malformed field: {s}"));
    }

    if let Some(rest) = s.strip_prefix("arith ") {
        // arith <op> $N $M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let op = parse_arith_op(parts[0])?;
            let lhs = parse_slot(parts[1])?;
            let rhs = parse_slot(parts[2])?;
            return Ok(IrExprKind::Arith { op, lhs, rhs });
        }
        return Err(format!("malformed arith: {s}"));
    }

    if let Some(rest) = s.strip_prefix("cmp ") {
        // cmp <op> $N $M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let op = parse_cmp_op(parts[0])?;
            let lhs = parse_slot(parts[1])?;
            let rhs = parse_slot(parts[2])?;
            return Ok(IrExprKind::Cmp { op, lhs, rhs });
        }
        return Err(format!("malformed cmp: {s}"));
    }

    if let Some(rest) = s.strip_prefix("cast ") {
        // cast $N as Type
        if let Some(as_pos) = rest.find(" as ") {
            let slot = parse_slot(rest[..as_pos].trim())?;
            let target_type = rest[as_pos + 4..].trim().to_string();
            return Ok(IrExprKind::Cast { slot, target_type });
        }
        return Err(format!("malformed cast: {s}"));
    }

    if let Some(rest) = s.strip_prefix("construct ") {
        // construct TypeId { .0 = $N, .1 = $M }
        let rest = rest.trim();
        if let Some(brace_start) = rest.find('{') {
            let type_id = rest[..brace_start].trim().to_string();
            let brace_end = rest.rfind('}').unwrap_or(rest.len());
            let fields_str = &rest[brace_start + 1..brace_end];
            let mut fields = Vec::new();
            for f in fields_str.split(',') {
                let f = f.trim();
                if f.is_empty() {
                    continue;
                }
                let kv: Vec<&str> = f.splitn(2, '=').collect();
                if kv.len() == 2 {
                    let idx = kv[0]
                        .trim()
                        .trim_start_matches('.')
                        .parse::<usize>()
                        .unwrap_or(0);
                    let slot = parse_slot(kv[1].trim())?;
                    fields.push((idx, slot));
                }
            }
            return Ok(IrExprKind::Construct { type_id, fields });
        }
        return Err(format!("malformed construct: {s}"));
    }

    if let Some(rest) = s.strip_prefix("if ") {
        // if $N then #B1 else #B2
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 5 && parts[1] == "then" && parts[3] == "else" {
            let cond = parse_slot(parts[0])?;
            let then_block = parts[2]
                .trim_start_matches('#')
                .parse::<usize>()
                .map_err(|_| format!("bad block id: {}", parts[2]))?;
            let else_block = parts[4]
                .trim_start_matches('#')
                .parse::<usize>()
                .map_err(|_| format!("bad block id: {}", parts[4]))?;
            return Ok(IrExprKind::If {
                cond,
                then_block,
                else_block,
            });
        }
        return Err(format!("malformed if: {s}"));
    }

    if let Some(rest) = s.strip_prefix("transition ") {
        // transition $N to StateId
        if let Some(to_pos) = rest.find(" to ") {
            let slot = parse_slot(rest[..to_pos].trim())?;
            let state = rest[to_pos + 4..].trim().to_string();
            return Ok(IrExprKind::Transition { slot, state });
        }
        return Err(format!("malformed transition: {s}"));
    }

    if let Some(rest) = s.strip_prefix("match ") {
        // match $N { 0 => #B0, 1 => #B1, _ => #Bdef }
        let rest = rest.trim();
        if let Some(brace_start) = rest.find('{') {
            let scrutinee = parse_slot(rest[..brace_start].trim())?;
            let brace_end = rest.rfind('}').unwrap_or(rest.len());
            let arms_str = &rest[brace_start + 1..brace_end];
            let mut arms = Vec::new();
            for arm in arms_str.split(',') {
                let arm = arm.trim();
                if arm.is_empty() {
                    continue;
                }
                if let Some(arrow) = arm.find("=>") {
                    let pat_str = arm[..arrow].trim();
                    let target_str = arm[arrow + 2..].trim();
                    let pat = parse_match_pattern(pat_str)?;
                    let block = target_str
                        .trim_start_matches('#')
                        .parse::<usize>()
                        .map_err(|_| format!("bad block id in match arm: {target_str}"))?;
                    arms.push((pat, block));
                }
            }
            return Ok(IrExprKind::Match { scrutinee, arms });
        }
        return Err(format!("malformed match: {s}"));
    }

    if let Some(rest) = s.strip_prefix("loop ") {
        // loop #body_block $cond
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            let body_block = parts[0]
                .trim_start_matches('#')
                .parse::<usize>()
                .map_err(|_| format!("bad block id in loop: {}", parts[0]))?;
            let cond = parse_slot(parts[1])?;
            return Ok(IrExprKind::Loop { body_block, cond });
        }
        return Err(format!("malformed loop: {s}"));
    }

    Err(format!("unknown IR expression: {s}"))
}

fn parse_ir_literal(s: &str) -> Result<IrLiteral, String> {
    let s = s.trim();
    if s == "true" {
        return Ok(IrLiteral::Bool(true));
    }
    if s == "false" {
        return Ok(IrLiteral::Bool(false));
    }
    if s.starts_with('"') && s.ends_with('"') {
        return Ok(IrLiteral::Str(s[1..s.len() - 1].to_string()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(IrLiteral::Int(n));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(IrLiteral::Float(f));
    }
    Err(format!("cannot parse IR literal: {s}"))
}

pub(crate) fn parse_arith_op(s: &str) -> Result<IrArithOp, String> {
    match s {
        "add" => Ok(IrArithOp::Add),
        "sub" => Ok(IrArithOp::Sub),
        "mul" => Ok(IrArithOp::Mul),
        "div" => Ok(IrArithOp::Div),
        "mod" => Ok(IrArithOp::Mod),
        _ => Err(format!("unknown arith op: {s}")),
    }
}

pub(crate) fn parse_cmp_op(s: &str) -> Result<IrCmpOp, String> {
    match s {
        "eq" => Ok(IrCmpOp::Eq),
        "ne" => Ok(IrCmpOp::Ne),
        "lt" => Ok(IrCmpOp::Lt),
        "le" => Ok(IrCmpOp::Le),
        "gt" => Ok(IrCmpOp::Gt),
        "ge" => Ok(IrCmpOp::Ge),
        _ => Err(format!("unknown cmp op: {s}")),
    }
}

pub(crate) fn parse_ir_pred_str(s: &str) -> Option<IrPred> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s == "true" {
        return Some(IrPred::True);
    }
    if s == "false" {
        return Some(IrPred::False);
    }
    // cmp <op> <arg> <arg>
    if let Some(rest) = s.strip_prefix("cmp ") {
        let tokens = tokenize_pred(rest);
        if tokens.len() >= 3
            && let Ok(op) = parse_cmp_op(&tokens[0])
            && let Some((lhs_arg, consumed)) = parse_pred_arg_tokens(&tokens[1..])
            && let Some((rhs_arg, _)) = parse_pred_arg_tokens(&tokens[1 + consumed..])
        {
            return Some(IrPred::Cmp {
                op,
                lhs: lhs_arg,
                rhs: rhs_arg,
            });
        }
    }
    // not <pred>
    if let Some(rest) = s.strip_prefix("not ")
        && let Some(inner) = parse_ir_pred_str(rest)
    {
        return Some(IrPred::Not(Box::new(inner)));
    }
    None
}

fn tokenize_pred(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth == 1 {
                    if !current.trim().is_empty() {
                        tokens.push(current.trim().to_string());
                        current.clear();
                    }
                    continue;
                }
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if !current.trim().is_empty() {
                        tokens.push(format!("({})", current.trim()));
                        current.clear();
                    }
                    continue;
                }
                current.push(ch);
            }
            ' ' | '\t' if depth == 0 => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn parse_pred_arg_tokens(tokens: &[String]) -> Option<(IrPredArg, usize)> {
    if tokens.is_empty() {
        return None;
    }
    let first = &tokens[0];
    if first == "$result" {
        return Some((IrPredArg::SlotResult, 1));
    }
    if let Some(stripped) = first.strip_prefix('$')
        && let Ok(n) = stripped.parse::<usize>()
    {
        return Some((IrPredArg::Slot(n), 1));
    }
    if first.starts_with("(arith ") || first.starts_with("(cmp ") {
        // Nested expression in parens
        let inner = &first[1..first.len() - 1]; // strip outer parens
        if let Some(rest) = inner.strip_prefix("arith ") {
            let sub = tokenize_pred(rest);
            if sub.len() >= 3
                && let Ok(op) = parse_arith_op(&sub[0])
                && let Some((lhs, lc)) = parse_pred_arg_tokens(&sub[1..])
                && let Some((rhs, _)) = parse_pred_arg_tokens(&sub[1 + lc..])
            {
                return Some((
                    IrPredArg::Arith {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    1,
                ));
            }
        }
    }
    // Try as literal
    if let Ok(n) = first.parse::<i64>() {
        return Some((IrPredArg::Lit(IrLiteral::Int(n)), 1));
    }
    if let Ok(f) = first.parse::<f64>() {
        return Some((IrPredArg::Lit(IrLiteral::Float(f)), 1));
    }
    // Parenthesized constant: (const 0)
    if first.starts_with("(const ") {
        let inner = &first[7..first.len() - 1];
        if let Ok(lit) = parse_ir_literal(inner) {
            return Some((IrPredArg::Lit(lit), 1));
        }
    }
    None
}

/// Validate an IR module against the contract it claims to implement.
///
/// Checks:
/// 1. Function parameter count matches contract input count
/// 2. Parameter types match contract input types
/// 3. Return type matches contract output type
/// 4. Effect annotations are compatible
/// 5. Slot numbering is sequential (no gaps)
/// 6. All slot references are defined before use
pub fn validate_ir_against_contract(
    ir: &IrModule,
    contract: &assura_ast::ContractDecl,
) -> IrValidation {
    let mut errors = Vec::new();

    for func in &ir.functions {
        // Check that slot numbering starts from 0 and uses no gaps
        let mut max_defined = func.params.iter().map(|p| p.slot).max().unwrap_or(0);

        for instr in &func.body {
            // $result uses sentinel usize::MAX
            if instr.target != usize::MAX {
                if instr.target > max_defined + 1 {
                    errors.push(format!(
                        "fn {}: slot ${} skips slot ${}",
                        func.id,
                        instr.target,
                        max_defined + 1
                    ));
                }
                max_defined = max_defined.max(instr.target);
            }

            // Check all slot references are defined
            for referenced in referenced_slots(&instr.expr) {
                if referenced != usize::MAX
                    && referenced > max_defined
                    && !func.params.iter().any(|p| p.slot == referenced)
                {
                    errors.push(format!(
                        "fn {}: instruction uses undefined slot ${}",
                        func.id, referenced
                    ));
                }
            }
        }

        // Check parameter count against contract inputs
        let contract_inputs: Vec<_> = contract
            .clauses
            .iter()
            .filter(|c| c.kind == assura_ast::ClauseKind::Input)
            .collect();
        if !contract_inputs.is_empty() {
            // Count params in the first input clause
            let input_count = count_input_params(&contract_inputs[0].body);
            if func.params.len() != input_count {
                errors.push(format!(
                    "fn {}: has {} params, contract expects {}",
                    func.id,
                    func.params.len(),
                    input_count
                ));
            }
        }
    }

    IrValidation {
        valid: errors.is_empty(),
        errors,
    }
}

fn parse_match_pattern(s: &str) -> Result<IrMatchPattern, String> {
    let s = s.trim();
    if s == "_" {
        return Ok(IrMatchPattern::Wildcard);
    }
    if s == "true" {
        return Ok(IrMatchPattern::Bool(true));
    }
    if s == "false" {
        return Ok(IrMatchPattern::Bool(false));
    }
    if s.starts_with('"') && s.ends_with('"') {
        return Ok(IrMatchPattern::Str(s[1..s.len() - 1].to_string()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(IrMatchPattern::Int(n));
    }
    Err(format!("cannot parse match pattern: {s}"))
}

fn referenced_slots(expr: &IrExprKind) -> Vec<usize> {
    match expr {
        IrExprKind::Const(_) => vec![],
        IrExprKind::Load(s) => vec![*s],
        IrExprKind::Call { args, .. } => args.clone(),
        IrExprKind::Field { slot, .. } => vec![*slot],
        IrExprKind::Construct { fields, .. } => fields.iter().map(|(_, s)| *s).collect(),
        IrExprKind::Arith { lhs, rhs, .. } => vec![*lhs, *rhs],
        IrExprKind::Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        IrExprKind::Cast { slot, .. } => vec![*slot],
        IrExprKind::If { cond, .. } => vec![*cond],
        IrExprKind::Transition { slot, .. } => vec![*slot],
        IrExprKind::Match { scrutinee, .. } => vec![*scrutinee],
        IrExprKind::Loop { cond, .. } => vec![*cond],
    }
}

fn count_input_params(body: &assura_ast::SpExpr) -> usize {
    match &body.node {
        assura_ast::Expr::Tuple(items) => items.len(),
        assura_ast::Expr::Call { args, .. } => args.len(),
        _ => 1,
    }
}

/// Placeholder `.ir` sidecar text for a declaration (AI replaces with real IR).
///
/// Uses identity `load` from the first parameter when present so SMT havoc+assume
/// has a minimal implementation constraint to refine.
pub fn stub_ir_sidecar_text(
    name: &str,
    params: &[(usize, String)],
    return_ty: &str,
    requires_count: usize,
    ensures_count: usize,
) -> String {
    let module = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let param_list = params
        .iter()
        .map(|(slot, ty)| format!("${slot}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let body = if let Some((slot, _)) = params.first() {
        format!("    $result = load ${slot} : {return_ty}\n")
    } else {
        format!("    $result = const 0 : {return_ty}\n")
    };
    format!(
        "// Stub IR for {name} — AI replaces body to satisfy contract ensures\n\
         // Contract: {requires_count} requires, {ensures_count} ensures\n\
         module {module} {{\n\
           fn #0 : ({param_list}) -> {return_ty} ! pure\n\
           pre: true\n\
           {{\n\
         {body}\
           }}\n\
         }}\n"
    )
}

/// Generate Rust source code from a validated IR module.
///
/// Each IR function becomes a Rust function with debug_assert!
/// for pre/post conditions.
pub fn ir_to_rust(module: &IrModule) -> String {
    let mut code = String::new();
    code.push_str(&format!("// Generated from IR module: {}\n\n", module.name));

    for func in &module.functions {
        // Function signature
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| format!("slot_{}: {}", p.slot, ir_type_to_rust(&p.ty)))
            .collect();

        let ret_type = ir_type_to_rust(&func.return_type);
        code.push_str(&format!(
            "fn ir_{}({}) -> {} {{\n",
            func.id.trim_start_matches('#'),
            params.join(", "),
            ret_type
        ));

        // Pre-condition
        if let Some(ref pre) = func.pre {
            let pre_rust = pred_to_rust(pre);
            code.push_str(&format!("    debug_assert!({pre_rust});\n"));
        }

        // Body instructions
        for instr in &func.body {
            let target = if instr.target == usize::MAX {
                crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
            } else {
                format!("slot_{}", instr.target)
            };
            let ty = ir_type_to_rust(&instr.ty);
            let expr_code = ir_expr_to_rust(&instr.expr);
            code.push_str(&format!("    let {target}: {ty} = {expr_code};\n"));
        }

        // Post-condition
        if let Some(ref post) = func.post {
            let post_rust = pred_to_rust(post);
            code.push_str(&format!("    debug_assert!({post_rust});\n"));
        }

        // Return $result if it was assigned, otherwise use a default value
        if func.body.iter().any(|i| i.target == usize::MAX) {
            code.push_str("    __result\n");
        } else {
            // Generate a type-appropriate default return value
            let default_val = ir_type_default(&func.return_type);
            code.push_str(&format!("    {default_val}\n"));
        }

        code.push_str("}\n\n");
    }

    code
}

/// Generate only the function body (instructions + postcondition) from an IR function.
///
/// Unlike `ir_to_rust` which generates complete Rust functions, this returns
/// the body code suitable for embedding into codegen-produced contract/fn/service
/// bodies in place of `todo!()` placeholders. The code uses slot variables
/// (`slot_0`, `slot_1`, etc.) and assumes the caller maps contract input params
/// to the corresponding slot bindings.
pub fn ir_function_body_to_rust(func: &IrFunction) -> String {
    let mut code = String::new();

    // Pre-condition
    if let Some(ref pre) = func.pre {
        let pre_rust = pred_to_rust(pre);
        if pre_rust != "true" {
            code.push_str(&format!(
                "    debug_assert!({pre_rust}, \"IR pre-condition\");\n"
            ));
        }
    }

    // Body instructions
    for instr in &func.body {
        let target = if instr.target == usize::MAX {
            crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
        } else {
            format!("slot_{}", instr.target)
        };
        let ty = ir_type_to_rust(&instr.ty);
        let expr_code = ir_expr_to_rust(&instr.expr);
        code.push_str(&format!("    let {target}: {ty} = {expr_code};\n"));
    }

    // Post-condition
    if let Some(ref post) = func.post {
        let post_rust = pred_to_rust(post);
        if post_rust != "true" {
            code.push_str(&format!(
                "    debug_assert!({post_rust}, \"IR post-condition\");\n"
            ));
        }
    }

    // Return __result if it was assigned
    if func.body.iter().any(|i| i.target == usize::MAX) {
        code.push_str("    __result\n");
    } else {
        let default_val = ir_type_default(&func.return_type);
        code.push_str(&format!("    {default_val}\n"));
    }

    code
}

/// Build a map from contract/function names to their IR-generated Rust body code.
///
/// For each function in the module, the first function is mapped to the module name,
/// and subsequent functions are mapped to their function ID.
pub fn ir_module_to_body_map(module: &IrModule) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for (i, func) in module.functions.iter().enumerate() {
        let key = if i == 0 {
            module.name.clone()
        } else {
            func.id.trim_start_matches('#').to_string()
        };
        map.insert(key, ir_function_body_to_rust(func));
    }
    map
}

pub(crate) fn ir_type_to_rust(ty: &str) -> String {
    match ty {
        "Int" => "i64".to_string(),
        "Nat" => "u64".to_string(),
        "Float" => "f64".to_string(),
        "Bool" => "bool".to_string(),
        "String" => "String".to_string(),
        "Bytes" => "Vec<u8>".to_string(),
        "Unit" => "()".to_string(),
        "" => "_".to_string(),
        other => other.to_string(),
    }
}

/// Generate a default value for an IR return type.
fn ir_type_default(ty: &str) -> String {
    match ty {
        "Int" => "0_i64".to_string(),
        "Nat" => "0_u64".to_string(),
        "Float" => "0.0_f64".to_string(),
        "Bool" => "false".to_string(),
        "String" => "String::new()".to_string(),
        "Bytes" => "Vec::new()".to_string(),
        "Unit" | "" => "()".to_string(),
        _ => "Default::default()".to_string(),
    }
}

fn ir_expr_to_rust(expr: &IrExprKind) -> String {
    match expr {
        IrExprKind::Const(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\".to_string()"),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrExprKind::Load(s) => {
            if *s == usize::MAX {
                crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
            } else {
                format!("slot_{s}")
            }
        }
        IrExprKind::Call { func, args } => {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    if *a == usize::MAX {
                        crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
                    } else {
                        format!("slot_{a}")
                    }
                })
                .collect();
            format!("{func}({})", arg_strs.join(", "))
        }
        IrExprKind::Field { slot, index } => format!("slot_{slot}.{index}"),
        IrExprKind::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cast { slot, target_type } => {
            format!("slot_{slot} as {}", ir_type_to_rust(target_type))
        }
        IrExprKind::Construct {
            type_id, fields, ..
        } => {
            let field_strs: Vec<String> = fields.iter().map(|(_, s)| format!("slot_{s}")).collect();
            format!("{type_id}::new({})", field_strs.join(", "))
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            format!("if slot_{cond} {{ block_{then_block}() }} else {{ block_{else_block}() }}")
        }
        IrExprKind::Transition { slot, state } => {
            format!("slot_{slot}.transition_to_{state}()")
        }
        IrExprKind::Match { scrutinee, arms } => {
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|(pat, block)| {
                    let pat_str = match pat {
                        IrMatchPattern::Int(n) => format!("{n}"),
                        IrMatchPattern::Bool(b) => format!("{b}"),
                        IrMatchPattern::Str(s) => format!("\"{s}\""),
                        IrMatchPattern::Wildcard => "_".to_string(),
                    };
                    format!("{pat_str} => block_{block}()")
                })
                .collect();
            format!("match slot_{scrutinee} {{ {} }}", arm_strs.join(", "))
        }
        IrExprKind::Loop { body_block, cond } => {
            format!("loop {{ block_{body_block}(); if !slot_{cond} {{ break; }} }}")
        }
    }
}

fn pred_to_rust(pred: &IrPred) -> String {
    match pred {
        IrPred::True => "true".to_string(),
        IrPred::False => "false".to_string(),
        IrPred::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
        IrPred::And(a, b) => format!("({} && {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Or(a, b) => format!("({} || {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Not(p) => format!("!({})", pred_to_rust(p)),
    }
}

fn pred_arg_to_rust(arg: &IrPredArg) -> String {
    match arg {
        IrPredArg::Slot(n) => format!("slot_{n}"),
        IrPredArg::SlotResult => crate::encode_atom_policy::RESULT_VAR_NAME.to_string(),
        IrPredArg::Lit(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\""),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrPredArg::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // IrParser (text format) tests
    // -------------------------------------------------------------------

    #[test]
    fn stub_ir_sidecar_text_includes_identity_load() {
        let text = stub_ir_sidecar_text("CopyBytes", &[(0, "Bytes".into())], "Bytes", 1, 1);
        assert!(text.contains("CopyBytes"));
        assert!(text.contains("$result = load $0"));
        assert!(text.contains("pre: true"));
        assert!(text.contains("1 requires, 1 ensures"));
        parse_ir_module(&text).unwrap();
    }

    #[test]
    fn test_ir_parser_empty() {
        let mut p = IrParser::new();
        p.parse_text("").unwrap();
        assert_eq!(p.node_count(), 0);
    }

    #[test]
    fn test_ir_parser_fn_decl() {
        let mut p = IrParser::new();
        p.parse_text("fn foo(x: Int)").unwrap();
        assert_eq!(p.node_count(), 1);
    }

    #[test]
    fn test_ir_parser_var_decl() {
        let mut p = IrParser::new();
        p.parse_text("let x: Int").unwrap();
        assert_eq!(p.node_count(), 1);
    }

    #[test]
    fn test_ir_parser_return_literal() {
        let mut p = IrParser::new();
        p.parse_text("return 42").unwrap();
        assert_eq!(p.node_count(), 1);
    }

    #[test]
    fn test_ir_parser_comments_skipped() {
        let mut p = IrParser::new();
        p.parse_text("// comment\nfn bar()").unwrap();
        assert_eq!(p.node_count(), 1);
    }

    #[test]
    fn test_ir_parser_serialize_binary() {
        let mut p = IrParser::new();
        p.parse_text("fn foo()\nlet x: Int\nreturn 0").unwrap();
        let bin = p.serialize_binary();
        // 4 bytes for count (3) + 3 nodes
        assert!(bin.len() > 4);
        // First 4 bytes = 3 (little-endian u32)
        let count = u32::from_le_bytes([bin[0], bin[1], bin[2], bin[3]]);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_ir_parser_default() {
        let p = IrParser::default();
        assert_eq!(p.node_count(), 0);
    }

    // -------------------------------------------------------------------
    // IR module parser tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_ir_module_minimal() {
        let src = "module test {\n}";
        let m = parse_ir_module(src).unwrap();
        assert_eq!(m.name, "test");
        assert!(m.functions.is_empty());
    }

    #[test]
    fn test_parse_ir_module_with_function() {
        let src = "\
module math {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
  {
    $2 = arith add $0 $1 : Int
    $result = load $2 : Int
  }
}";
        let m = parse_ir_module(src).unwrap();
        assert_eq!(m.name, "math");
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].id, "#0");
        assert_eq!(m.functions[0].params.len(), 2);
        assert_eq!(m.functions[0].return_type, "Int");
        assert_eq!(m.functions[0].effects, "pure");
        assert_eq!(m.functions[0].body.len(), 2);
    }

    #[test]
    fn test_parse_ir_module_with_pre_post() {
        let src = "\
module check {
  fn #0 : ($0: Int) -> Int ! pure
  pre: cmp ne $0 (const 0)
  post: cmp gt $result (const 0)
  {
    $result = load $0 : Int
  }
}";
        let m = parse_ir_module(src).unwrap();
        m.functions[0].pre.as_ref().unwrap();
        m.functions[0].post.as_ref().unwrap();
    }

    #[test]
    fn test_parse_ir_module_error_no_header() {
        let result = parse_ir_module("not a module");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ir_module_error_empty() {
        let result = parse_ir_module("");
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------
    // IR instruction parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_instr_const_int() {
        let instr = parse_ir_instr("$0 = const 42 : Int").unwrap();
        assert_eq!(instr.target, 0);
        assert_eq!(instr.ty, "Int");
        assert!(matches!(instr.expr, IrExprKind::Const(IrLiteral::Int(42))));
    }

    #[test]
    fn test_parse_instr_load() {
        let instr = parse_ir_instr("$2 = load $1 : Int").unwrap();
        assert_eq!(instr.target, 2);
        assert!(matches!(instr.expr, IrExprKind::Load(1)));
    }

    #[test]
    fn test_parse_instr_arith() {
        let instr = parse_ir_instr("$3 = arith mul $1 $2 : Int").unwrap();
        assert!(matches!(
            instr.expr,
            IrExprKind::Arith {
                op: IrArithOp::Mul,
                lhs: 1,
                rhs: 2
            }
        ));
    }

    #[test]
    fn test_parse_instr_cmp() {
        let instr = parse_ir_instr("$3 = cmp lt $0 $1 : Bool").unwrap();
        assert!(matches!(
            instr.expr,
            IrExprKind::Cmp {
                op: IrCmpOp::Lt,
                lhs: 0,
                rhs: 1
            }
        ));
    }

    #[test]
    fn test_parse_instr_call() {
        let instr = parse_ir_instr("$2 = call foo ($0, $1) : Int").unwrap();
        match instr.expr {
            IrExprKind::Call { func, args } => {
                assert_eq!(func, "foo");
                assert_eq!(args, vec![0, 1]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_instr_field() {
        let instr = parse_ir_instr("$2 = field $0 .1 : Int").unwrap();
        assert!(matches!(
            instr.expr,
            IrExprKind::Field { slot: 0, index: 1 }
        ));
    }

    #[test]
    fn test_parse_instr_cast() {
        let instr = parse_ir_instr("$1 = cast $0 as Float : Float").unwrap();
        assert!(matches!(instr.expr, IrExprKind::Cast { slot: 0, .. }));
    }

    #[test]
    fn test_parse_instr_result_slot() {
        let instr = parse_ir_instr("$result = load $0 : Int").unwrap();
        assert_eq!(instr.target, usize::MAX);
    }

    #[test]
    fn test_parse_instr_if() {
        let instr = parse_ir_instr("$3 = if $0 then #1 else #2 : Int").unwrap();
        assert!(matches!(
            instr.expr,
            IrExprKind::If {
                cond: 0,
                then_block: 1,
                else_block: 2
            }
        ));
    }

    #[test]
    fn test_parse_instr_transition() {
        let instr = parse_ir_instr("$1 = transition $0 to Active : Unit").unwrap();
        match instr.expr {
            IrExprKind::Transition { slot: 0, ref state } => assert_eq!(state, "Active"),
            other => panic!("expected Transition, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // IR literal parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_literal_int() {
        assert_eq!(parse_ir_literal("42").unwrap(), IrLiteral::Int(42));
    }

    #[test]
    fn test_parse_literal_float() {
        assert_eq!(parse_ir_literal("3.14").unwrap(), IrLiteral::Float(3.14));
    }

    #[test]
    fn test_parse_literal_bool() {
        assert_eq!(parse_ir_literal("true").unwrap(), IrLiteral::Bool(true));
        assert_eq!(parse_ir_literal("false").unwrap(), IrLiteral::Bool(false));
    }

    #[test]
    fn test_parse_literal_string() {
        assert_eq!(
            parse_ir_literal("\"hello\"").unwrap(),
            IrLiteral::Str("hello".into())
        );
    }

    // -------------------------------------------------------------------
    // IR type mapping tests
    // -------------------------------------------------------------------

    #[test]
    fn test_ir_type_to_rust_mapping() {
        assert_eq!(ir_type_to_rust("Int"), "i64");
        assert_eq!(ir_type_to_rust("Nat"), "u64");
        assert_eq!(ir_type_to_rust("Float"), "f64");
        assert_eq!(ir_type_to_rust("Bool"), "bool");
        assert_eq!(ir_type_to_rust("String"), "String");
        assert_eq!(ir_type_to_rust("Bytes"), "Vec<u8>");
        assert_eq!(ir_type_to_rust("Unit"), "()");
        assert_eq!(ir_type_to_rust(""), "_");
        assert_eq!(ir_type_to_rust("CustomType"), "CustomType");
    }

    // -------------------------------------------------------------------
    // ir_type_default tests
    // -------------------------------------------------------------------

    #[test]
    fn ir_type_default_covers_all_base_types() {
        assert_eq!(ir_type_default("Int"), "0_i64");
        assert_eq!(ir_type_default("Nat"), "0_u64");
        assert_eq!(ir_type_default("Float"), "0.0_f64");
        assert_eq!(ir_type_default("Bool"), "false");
        assert_eq!(ir_type_default("String"), "String::new()");
        assert_eq!(ir_type_default("Bytes"), "Vec::new()");
        assert_eq!(ir_type_default("Unit"), "()");
        assert_eq!(ir_type_default(""), "()");
    }

    #[test]
    fn ir_type_default_unknown_uses_default_trait() {
        assert_eq!(ir_type_default("CustomType"), "Default::default()");
        assert_eq!(ir_type_default("List<Int>"), "Default::default()");
    }

    // -------------------------------------------------------------------
    // IR to Rust codegen tests
    // -------------------------------------------------------------------

    #[test]
    fn test_ir_to_rust_generates_function() {
        let module = IrModule {
            name: "test".into(),
            functions: vec![IrFunction {
                id: "#0".into(),
                params: vec![
                    IrSlotDecl {
                        slot: 0,
                        ty: "Int".into(),
                    },
                    IrSlotDecl {
                        slot: 1,
                        ty: "Int".into(),
                    },
                ],
                return_type: "Int".into(),
                effects: "pure".into(),
                pre: None,
                post: None,
                body: vec![
                    IrInstr {
                        target: 2,
                        expr: IrExprKind::Arith {
                            op: IrArithOp::Add,
                            lhs: 0,
                            rhs: 1,
                        },
                        ty: "Int".into(),
                    },
                    IrInstr {
                        target: usize::MAX,
                        expr: IrExprKind::Load(2),
                        ty: "Int".into(),
                    },
                ],
            }],
        };
        let code = ir_to_rust(&module);
        assert!(code.contains("fn ir_0("));
        assert!(code.contains("slot_0: i64"));
        assert!(code.contains("slot_1: i64"));
        assert!(code.contains("-> i64"));
        assert!(code.contains("(slot_0 + slot_1)"));
        assert!(code.contains("__result"));
    }

    #[test]
    fn test_ir_to_rust_with_pre_post() {
        let module = IrModule {
            name: "guarded".into(),
            functions: vec![IrFunction {
                id: "#0".into(),
                params: vec![IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                }],
                return_type: "Int".into(),
                effects: "pure".into(),
                pre: Some(IrPred::Cmp {
                    op: IrCmpOp::Gt,
                    lhs: IrPredArg::Slot(0),
                    rhs: IrPredArg::Lit(IrLiteral::Int(0)),
                }),
                post: Some(IrPred::True),
                body: vec![IrInstr {
                    target: usize::MAX,
                    expr: IrExprKind::Load(0),
                    ty: "Int".into(),
                }],
            }],
        };
        let code = ir_to_rust(&module);
        assert!(code.contains("debug_assert!"));
        assert!(code.contains("slot_0"));
    }

    // -------------------------------------------------------------------
    // Predicate parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_pred_true() {
        assert_eq!(parse_ir_pred_str("true"), Some(IrPred::True));
    }

    #[test]
    fn test_parse_pred_false() {
        assert_eq!(parse_ir_pred_str("false"), Some(IrPred::False));
    }

    #[test]
    fn test_parse_pred_empty() {
        assert_eq!(parse_ir_pred_str(""), None);
    }

    #[test]
    fn test_parse_pred_cmp() {
        let pred = parse_ir_pred_str("cmp eq $0 $1").unwrap();
        assert!(matches!(
            pred,
            IrPred::Cmp {
                op: IrCmpOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_pred_not() {
        let pred = parse_ir_pred_str("not true").unwrap();
        assert!(matches!(pred, IrPred::Not(_)));
    }

    // -------------------------------------------------------------------
    // Arith/Cmp op parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_arith_ops() {
        assert_eq!(parse_arith_op("add").unwrap(), IrArithOp::Add);
        assert_eq!(parse_arith_op("sub").unwrap(), IrArithOp::Sub);
        assert_eq!(parse_arith_op("mul").unwrap(), IrArithOp::Mul);
        assert_eq!(parse_arith_op("div").unwrap(), IrArithOp::Div);
        assert_eq!(parse_arith_op("mod").unwrap(), IrArithOp::Mod);
        assert!(parse_arith_op("bad").is_err());
    }

    #[test]
    fn test_parse_cmp_ops() {
        assert_eq!(parse_cmp_op("eq").unwrap(), IrCmpOp::Eq);
        assert_eq!(parse_cmp_op("ne").unwrap(), IrCmpOp::Ne);
        assert_eq!(parse_cmp_op("lt").unwrap(), IrCmpOp::Lt);
        assert_eq!(parse_cmp_op("le").unwrap(), IrCmpOp::Le);
        assert_eq!(parse_cmp_op("gt").unwrap(), IrCmpOp::Gt);
        assert_eq!(parse_cmp_op("ge").unwrap(), IrCmpOp::Ge);
        assert!(parse_cmp_op("bad").is_err());
    }

    // -------------------------------------------------------------------
    // ir_function_body_to_rust tests
    // -------------------------------------------------------------------

    #[test]
    fn test_ir_function_body_generates_instructions() {
        let func = IrFunction {
            id: "#0".into(),
            params: vec![
                IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                },
                IrSlotDecl {
                    slot: 1,
                    ty: "Int".into(),
                },
            ],
            return_type: "Int".into(),
            effects: "pure".into(),
            pre: None,
            post: None,
            body: vec![
                IrInstr {
                    target: 2,
                    expr: IrExprKind::Arith {
                        op: IrArithOp::Add,
                        lhs: 0,
                        rhs: 1,
                    },
                    ty: "Int".into(),
                },
                IrInstr {
                    target: usize::MAX,
                    expr: IrExprKind::Load(2),
                    ty: "Int".into(),
                },
            ],
        };
        let body = ir_function_body_to_rust(&func);
        assert!(body.contains("(slot_0 + slot_1)"), "body: {body}");
        assert!(body.contains("__result"), "body: {body}");
        // No function signature
        assert!(
            !body.contains("fn "),
            "body should not contain fn signature"
        );
    }

    #[test]
    fn test_ir_function_body_with_pre_post() {
        let func = IrFunction {
            id: "#0".into(),
            params: vec![IrSlotDecl {
                slot: 0,
                ty: "Int".into(),
            }],
            return_type: "Int".into(),
            effects: "pure".into(),
            pre: Some(IrPred::Cmp {
                op: IrCmpOp::Ge,
                lhs: IrPredArg::Slot(0),
                rhs: IrPredArg::Lit(IrLiteral::Int(0)),
            }),
            post: Some(IrPred::Cmp {
                op: IrCmpOp::Ge,
                lhs: IrPredArg::SlotResult,
                rhs: IrPredArg::Lit(IrLiteral::Int(0)),
            }),
            body: vec![IrInstr {
                target: usize::MAX,
                expr: IrExprKind::Load(0),
                ty: "Int".into(),
            }],
        };
        let body = ir_function_body_to_rust(&func);
        assert!(body.contains("debug_assert!"), "body: {body}");
        assert!(body.contains("IR pre-condition"), "body: {body}");
        assert!(body.contains("IR post-condition"), "body: {body}");
    }

    #[test]
    fn test_ir_module_to_body_map() {
        let module = IrModule {
            name: "AddOne".into(),
            functions: vec![IrFunction {
                id: "#0".into(),
                params: vec![IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                }],
                return_type: "Int".into(),
                effects: "pure".into(),
                pre: None,
                post: None,
                body: vec![IrInstr {
                    target: 1,
                    expr: IrExprKind::Arith {
                        op: IrArithOp::Add,
                        lhs: 0,
                        rhs: 0,
                    },
                    ty: "Int".into(),
                }],
            }],
        };
        let map = ir_module_to_body_map(&module);
        assert!(
            map.contains_key("AddOne"),
            "map keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        let body = &map["AddOne"];
        assert!(body.contains("slot_0 + slot_0"), "body: {body}");
    }

    // -------------------------------------------------------------------
    // Match and Loop IR instruction tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_ir_match_instruction() {
        let src = "\
module matcher {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = match $0 { 0 => #0, 1 => #1, _ => #2 } : Int
    $result = load $1 : Int
  }
}";
        let m = parse_ir_module(src).unwrap();
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].body.len(), 2);
        match &m.functions[0].body[0].expr {
            IrExprKind::Match { scrutinee, arms } => {
                assert_eq!(*scrutinee, 0);
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0], (IrMatchPattern::Int(0), 0));
                assert_eq!(arms[1], (IrMatchPattern::Int(1), 1));
                assert_eq!(arms[2], (IrMatchPattern::Wildcard, 2));
            }
            other => panic!("expected Match, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_ir_loop_instruction() {
        let src = "\
module looper {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = const 0 : Int
    $2 = loop #0 $0 : Int
    $result = load $1 : Int
  }
}";
        let m = parse_ir_module(src).unwrap();
        assert_eq!(m.functions.len(), 1);
        match &m.functions[0].body[1].expr {
            IrExprKind::Loop { body_block, cond } => {
                assert_eq!(*body_block, 0);
                assert_eq!(*cond, 0);
            }
            other => panic!("expected Loop, got: {other:?}"),
        }
    }

    #[test]
    fn test_ir_match_to_rust() {
        let expr = IrExprKind::Match {
            scrutinee: 0,
            arms: vec![(IrMatchPattern::Int(1), 0), (IrMatchPattern::Wildcard, 1)],
        };
        let rust = ir_expr_to_rust(&expr);
        assert!(rust.contains("match slot_0"), "got: {rust}");
        assert!(rust.contains("1 => block_0()"), "got: {rust}");
        assert!(rust.contains("_ => block_1()"), "got: {rust}");
    }

    #[test]
    fn test_ir_loop_to_rust() {
        let expr = IrExprKind::Loop {
            body_block: 0,
            cond: 1,
        };
        let rust = ir_expr_to_rust(&expr);
        assert!(rust.contains("loop"), "got: {rust}");
        assert!(rust.contains("block_0()"), "got: {rust}");
        assert!(rust.contains("slot_1"), "got: {rust}");
    }

    #[test]
    fn test_match_referenced_slots() {
        let expr = IrExprKind::Match {
            scrutinee: 3,
            arms: vec![(IrMatchPattern::Wildcard, 0)],
        };
        assert_eq!(referenced_slots(&expr), vec![3]);
    }

    #[test]
    fn test_loop_referenced_slots() {
        let expr = IrExprKind::Loop {
            body_block: 0,
            cond: 5,
        };
        assert_eq!(referenced_slots(&expr), vec![5]);
    }
}
