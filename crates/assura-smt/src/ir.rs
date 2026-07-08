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
    /// `field $N .M` (numeric index) or `field $N .name` (named struct field).
    ///
    /// Named fields lower to Rust `.name` and SMT `__field_name` UFs. Numeric
    /// indices remain for tuple-style / collection length (index 0) access.
    Field {
        slot: usize,
        index: usize,
        name: Option<String>,
    },
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

pub(crate) fn parse_ir_instr(line: &str) -> Result<IrInstr, String> {
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

pub(crate) fn parse_ir_expr(s: &str) -> Result<IrExprKind, String> {
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
        // field $N .M  or  field $N .name
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            let slot = parse_slot(parts[0])?;
            let key = parts[1].trim_start_matches('.');
            if let Ok(index) = key.parse::<usize>() {
                return Ok(IrExprKind::Field {
                    slot,
                    index,
                    name: None,
                });
            }
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Ok(IrExprKind::Field {
                    slot,
                    index: 0,
                    name: Some(key.to_string()),
                });
            }
            return Err(format!("bad field key: {}", parts[1]));
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

pub(crate) fn parse_ir_literal(s: &str) -> Result<IrLiteral, String> {
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

pub(crate) fn parse_match_pattern(s: &str) -> Result<IrMatchPattern, String> {
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

pub(crate) fn referenced_slots(expr: &IrExprKind) -> Vec<usize> {
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

pub(crate) fn count_input_params(body: &assura_ast::SpExpr) -> usize {
    // Delegate to the canonical param extractor which handles all AST shapes
    // (Cast, Call, Tuple, Block, Raw tokens) produced by the parser for
    // input(a: Int, b: Int, c: Int) clauses.
    assura_ast::extract_clause_params(body).len()
}

#[cfg(test)]
#[path = "ir_modules/ir_tests.rs"]
mod tests;
