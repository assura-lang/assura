//! Level 2: LLM-generated intermediate lemma chains verified by Z3.

use sha2::{Digest, Sha256};

use crate::cache::{self, LlmCache};
use crate::prompt;
use crate::provider::LlmProvider;
use crate::types::*;

// ---------------------------------------------------------------------------
// Prompt template for lemma generation
// ---------------------------------------------------------------------------

pub fn lemma_system_prompt() -> String {
    r#"You are a formal verification assistant. Given a Rust function with contract annotations (#[requires], #[ensures]), produce a chain of intermediate logical assertions (lemmas) that bridge the preconditions to the postconditions.

You must respond ONLY with a JSON object (no markdown fences, no prose):

{
  "lemmas": [
    {
      "label": "<short_name>",
      "assertion": "<logical assertion in the constrained format below>",
      "justification": "<one sentence explaining why this holds, referencing the code>",
      "depends_on": ["<labels of prior lemmas this depends on>"]
    }
  ],
  "chain_complete": true | false
}

ASSERTION FORMAT (strict subset, must be parseable):
- Variables: parameter names, old_<field> (value before body), new_<field> (value after body), result (return value)
- Arithmetic: +, -, *, /, mod
- Comparison: ==, !=, <, <=, >, >=
- Logic: and, or, not, implies
- Quantifiers: forall x in range(a, b): P(x)
- Functions: len(x) for collection length

RULES:
- Each lemma must be independently verifiable by an SMT solver.
- The chain must form a logical proof: requires + lemma_1 + ... + lemma_n implies ensures.
- Label each lemma with a descriptive name (e.g., "guard_unreachable", "subtraction_effect").
- Order lemmas so each depends only on prior lemmas.
- Set chain_complete to true only if you believe the chain covers ALL control-flow paths.
- Do NOT produce Rust code. Do NOT suggest fixes. Only produce logical assertions."#.to_string()
}

pub fn lemma_user_prompt(
    function_source: &str,
    function_signature: &str,
    contracts: &[ContractClauseInfo],
    level1_verdict: &str,
    level1_paths: &[PathAnalysis],
) -> String {
    let mut p = String::with_capacity(4096);

    p.push_str("## Function\n\nSignature: `");
    p.push_str(function_signature);
    p.push_str("`\n\n```rust\n");
    p.push_str(function_source);
    p.push_str("\n```\n\n");

    p.push_str("## Contracts\n\n");
    for c in contracts {
        p.push_str(&format!("- #[{}({})]\n", c.kind, c.expression));
    }
    p.push('\n');

    p.push_str("## Level 1 Analysis\n\n");
    p.push_str(&format!("Verdict: {level1_verdict}\n\n"));

    if !level1_paths.is_empty() {
        p.push_str("Paths:\n");
        for path in level1_paths {
            let ok = if path.contracts_satisfied {
                "ok"
            } else {
                "FAIL"
            };
            p.push_str(&format!(
                "- {} [{}]: {}\n",
                path.description, ok, path.reasoning
            ));
        }
        p.push('\n');
    }

    p.push_str(
        "Produce a chain of intermediate lemmas that bridges the #[requires] preconditions \
         to the #[ensures] postconditions. Respond with JSON only.\n",
    );
    p
}

/// Parse a lemma chain response from the LLM.
pub fn parse_lemma_response(raw: &str) -> Result<LemmaChain, LlmError> {
    let json_str = prompt::extract_json(raw);
    serde_json::from_str(json_str).map_err(|e| LlmError::Parse(format!("{e}: {json_str}")))
}

// ---------------------------------------------------------------------------
// Lemma assertion parser (constrained format -> SMT-LIB)
// ---------------------------------------------------------------------------

/// Convert a constrained-format assertion into an SMT-LIB expression.
///
/// The parser handles the subset defined in the system prompt:
/// arithmetic, comparison, logic, simple quantifiers, and len().
pub fn assertion_to_smtlib(assertion: &str) -> Result<String, String> {
    let tokens = tokenize(assertion)?;
    let (expr, rest) = parse_expr(&tokens, 0)?;
    if rest < tokens.len() {
        return Err(format!(
            "unexpected token after expression: {:?}",
            tokens[rest]
        ));
    }
    Ok(expr)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Number(String),
    Op(String),
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            ',' => {
                tokens.push(Token::Comma);
                chars.next();
            }
            '+' | '-' | '*' | '/' | '%' => {
                tokens.push(Token::Op(c.to_string()));
                chars.next();
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Op("==".to_string()));
                } else {
                    tokens.push(Token::Op("=".to_string()));
                }
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Op("!=".to_string()));
                } else {
                    tokens.push(Token::Op("not".to_string()));
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Op("<=".to_string()));
                } else {
                    tokens.push(Token::Op("<".to_string()));
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Op(">=".to_string()));
                } else {
                    tokens.push(Token::Op(">".to_string()));
                }
            }
            _ if c.is_ascii_digit() => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Number(num));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let mut ident = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_alphanumeric() || d == '_' {
                        ident.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Keywords that are operators
                match ident.as_str() {
                    "and" | "or" | "not" | "implies" | "mod" => {
                        tokens.push(Token::Op(ident));
                    }
                    _ => tokens.push(Token::Ident(ident)),
                }
            }
            _ => return Err(format!("unexpected character: {c}")),
        }
    }

    Ok(tokens)
}

/// Operator precedence (lower = binds tighter last, i.e. lower precedence).
fn precedence(op: &str) -> u8 {
    match op {
        "implies" => 1,
        "or" => 2,
        "and" => 3,
        "==" | "!=" => 4,
        "<" | "<=" | ">" | ">=" => 5,
        "+" | "-" => 6,
        "*" | "/" | "%" | "mod" => 7,
        _ => 0,
    }
}

fn is_binary_op(op: &str) -> bool {
    matches!(
        op,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "mod"
            | "=="
            | "!="
            | "<"
            | "<="
            | ">"
            | ">="
            | "and"
            | "or"
            | "implies"
    )
}

fn op_to_smtlib(op: &str) -> &str {
    match op {
        "+" => "+",
        "-" => "-",
        "*" => "*",
        "/" => "div",
        "%" | "mod" => "mod",
        "==" | "=" => "=",
        "!=" => "distinct",
        "<" => "<",
        "<=" => "<=",
        ">" => ">",
        ">=" => ">=",
        "and" => "and",
        "or" => "or",
        "implies" => "=>",
        _ => op,
    }
}

/// Parse an expression with Pratt-style precedence climbing.
fn parse_expr(tokens: &[Token], pos: usize) -> Result<(String, usize), String> {
    let (mut left, mut pos) = parse_atom(tokens, pos)?;

    while pos < tokens.len() {
        if let Token::Op(ref op) = tokens[pos] {
            if is_binary_op(op) {
                let prec = precedence(op);
                let smt_op = op_to_smtlib(op).to_string();
                pos += 1;
                let (right, new_pos) = parse_expr_with_min_prec(tokens, pos, prec)?;
                left = format!("({smt_op} {left} {right})");
                pos = new_pos;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok((left, pos))
}

fn parse_expr_with_min_prec(
    tokens: &[Token],
    pos: usize,
    min_prec: u8,
) -> Result<(String, usize), String> {
    let (mut left, mut pos) = parse_atom(tokens, pos)?;

    while pos < tokens.len() {
        if let Token::Op(ref op) = tokens[pos] {
            if is_binary_op(op) {
                let prec = precedence(op);
                if prec < min_prec {
                    break;
                }
                let smt_op = op_to_smtlib(op).to_string();
                pos += 1;
                let (right, new_pos) = parse_expr_with_min_prec(tokens, pos, prec + 1)?;
                left = format!("({smt_op} {left} {right})");
                pos = new_pos;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok((left, pos))
}

fn parse_atom(tokens: &[Token], pos: usize) -> Result<(String, usize), String> {
    if pos >= tokens.len() {
        return Err("unexpected end of expression".to_string());
    }

    match &tokens[pos] {
        Token::Number(n) => Ok((n.clone(), pos + 1)),
        Token::Ident(name) => {
            // Check for function call: ident(args)
            if pos + 1 < tokens.len() && tokens[pos + 1] == Token::LParen {
                // Function call
                match name.as_str() {
                    "len" => {
                        let (arg, end) = parse_function_args(tokens, pos + 2)?;
                        Ok((format!("(len {arg})"), end))
                    }
                    "forall" | "exists" => {
                        // forall x in range(a, b): P(x)
                        // We'll handle a simplified version
                        Ok((format!("|{name}_unhandled|"), pos + 1))
                    }
                    "true" => Ok(("true".to_string(), pos + 1)),
                    "false" => Ok(("false".to_string(), pos + 1)),
                    _ => {
                        // Generic function call
                        let (arg, end) = parse_function_args(tokens, pos + 2)?;
                        Ok((format!("({name} {arg})"), end))
                    }
                }
            } else {
                // Plain variable
                match name.as_str() {
                    "true" => Ok(("true".to_string(), pos + 1)),
                    "false" => Ok(("false".to_string(), pos + 1)),
                    _ => Ok((name.clone(), pos + 1)),
                }
            }
        }
        Token::LParen => {
            let (expr, pos) = parse_expr(tokens, pos + 1)?;
            if pos >= tokens.len() || tokens[pos] != Token::RParen {
                return Err("expected ')'".to_string());
            }
            Ok((expr, pos + 1))
        }
        Token::Op(op) if op == "not" => {
            let (expr, pos) = parse_atom(tokens, pos + 1)?;
            Ok((format!("(not {expr})"), pos))
        }
        Token::Op(op) if op == "-" => {
            // Unary minus
            let (expr, pos) = parse_atom(tokens, pos + 1)?;
            Ok((format!("(- {expr})"), pos))
        }
        other => Err(format!("unexpected token: {other:?}")),
    }
}

fn parse_function_args(tokens: &[Token], pos: usize) -> Result<(String, usize), String> {
    if pos >= tokens.len() {
        return Err("expected function arguments".to_string());
    }
    if tokens[pos] == Token::RParen {
        return Ok(("".to_string(), pos + 1));
    }

    let (first, mut pos) = parse_expr(tokens, pos)?;
    let mut args = first;

    while pos < tokens.len() && tokens[pos] == Token::Comma {
        pos += 1;
        let (arg, new_pos) = parse_expr(tokens, pos)?;
        args = format!("{args} {arg}");
        pos = new_pos;
    }

    if pos >= tokens.len() || tokens[pos] != Token::RParen {
        return Err("expected ')'".to_string());
    }

    Ok((args, pos + 1))
}

// ---------------------------------------------------------------------------
// Z3 verification of lemma chain
// ---------------------------------------------------------------------------

/// Verify a lemma chain using the existing SMT infrastructure.
///
/// For each lemma, constructs a Z3 query:
///   (requires + prior lemmas) => this lemma
/// Then checks the final implication:
///   (requires + all lemmas) => ensures
pub fn verify_lemma_chain(
    requires: &[String],
    ensures: &[String],
    chain: &LemmaChain,
) -> LemmaChainVerification {
    let mut results = Vec::new();
    let mut all_valid = true;
    let mut valid_count = 0;

    // Verify each lemma in sequence
    let mut prior_assertions: Vec<String> = requires
        .iter()
        .filter_map(|r| assertion_to_smtlib(r).ok())
        .collect();

    for lemma in &chain.lemmas {
        let start = std::time::Instant::now();

        let lemma_result = match assertion_to_smtlib(&lemma.assertion) {
            Ok(smt_assertion) => {
                // Build the query: (and prior_assertions) => smt_assertion
                // Validity check: assert NOT (prior => lemma), check UNSAT
                let query = build_validity_query(&prior_assertions, &smt_assertion);
                let result = run_smtlib_check(&query);

                // Add this lemma to priors for next step
                prior_assertions.push(smt_assertion.clone());

                result
            }
            Err(msg) => LemmaResult::ParseError { message: msg },
        };

        let elapsed = start.elapsed().as_millis() as u64;

        if matches!(lemma_result, LemmaResult::Valid) {
            valid_count += 1;
        } else {
            all_valid = false;
        }

        results.push(LemmaVerification {
            label: lemma.label.clone(),
            assertion: lemma.assertion.clone(),
            result: lemma_result,
            time_ms: elapsed,
        });
    }

    // Final check: all lemmas imply ensures
    let ensures_follows = if all_valid && !ensures.is_empty() {
        let ensures_smtlib: Vec<String> = ensures
            .iter()
            .filter_map(|e| assertion_to_smtlib(e).ok())
            .collect();

        ensures_smtlib.iter().all(|e_smt| {
            let query = build_validity_query(&prior_assertions, e_smt);
            matches!(run_smtlib_check(&query), LemmaResult::Valid)
        })
    } else {
        all_valid
    };

    let total = results.len();
    LemmaChainVerification {
        lemmas: results,
        ensures_follows,
        chain_valid: all_valid && ensures_follows,
        valid_count,
        total_count: total,
    }
}

/// Build a validity query: assert (and assumptions), assert NOT conclusion, check-sat.
/// If UNSAT, the conclusion follows from assumptions (valid).
fn build_validity_query(assumptions: &[String], conclusion: &str) -> String {
    let mut query = String::new();
    query.push_str("(set-logic ALL)\n");

    // We don't know variable types, declare everything as Int
    // Collect all identifiers from assumptions + conclusion
    let mut vars = std::collections::HashSet::new();
    let all_text: String = assumptions
        .iter()
        .chain(std::iter::once(&conclusion.to_string()))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    // Simple identifier extraction from SMT-LIB text
    for word in all_text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if !word.is_empty()
            && !word.chars().next().unwrap().is_ascii_digit()
            && !is_smtlib_keyword(word)
        {
            vars.insert(word.to_string());
        }
    }

    for var in &vars {
        query.push_str(&format!("(declare-const {var} Int)\n"));
    }

    // Assert all assumptions
    for a in assumptions {
        query.push_str(&format!("(assert {a})\n"));
    }

    // Assert NOT conclusion (validity = negation is UNSAT)
    query.push_str(&format!("(assert (not {conclusion}))\n"));
    query.push_str("(check-sat)\n");

    query
}

fn is_smtlib_keyword(word: &str) -> bool {
    matches!(
        word,
        "and"
            | "or"
            | "not"
            | "true"
            | "false"
            | "div"
            | "mod"
            | "distinct"
            | "assert"
            | "declare"
            | "const"
            | "Int"
            | "Bool"
            | "set"
            | "logic"
            | "ALL"
            | "check"
            | "sat"
            | "len"
    )
}

/// Run an SMT-LIB query through Z3.
fn run_smtlib_check(query: &str) -> LemmaResult {
    use std::process::Command;

    // Write query to temp file
    let tmp = std::env::temp_dir().join(format!("assura-lemma-{}.smt2", std::process::id()));
    if std::fs::write(&tmp, query).is_err() {
        return LemmaResult::ParseError {
            message: "failed to write SMT query".to_string(),
        };
    }

    let output = Command::new("z3")
        .arg("-T:5") // 5 second timeout
        .arg(tmp.to_str().unwrap())
        .output();

    let _ = std::fs::remove_file(&tmp);

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let result = stdout.trim();
            if result == "unsat" {
                LemmaResult::Valid
            } else if result == "sat" {
                LemmaResult::Counterexample {
                    model: "(model extraction not implemented for lemma checks)".to_string(),
                }
            } else if result.contains("timeout") || result.contains("unknown") {
                LemmaResult::Timeout
            } else {
                LemmaResult::ParseError {
                    message: format!("unexpected Z3 output: {result}"),
                }
            }
        }
        Err(e) => LemmaResult::ParseError {
            message: format!("z3 not found or failed: {e}"),
        },
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Compute cache key for lemma chain.
pub fn lemma_cache_key(
    function_name: &str,
    function_body: &str,
    contracts: &[ContractClauseInfo],
    level1_verdict: &str,
    model: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lemma-v1:");
    hasher.update(function_name.as_bytes());
    hasher.update(function_body.as_bytes());
    for c in contracts {
        hasher.update(c.kind.as_bytes());
        hasher.update(c.expression.as_bytes());
    }
    hasher.update(level1_verdict.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(prompt::prompt_version().as_bytes());
    cache::hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Generate and verify a lemma chain for a function.
pub fn generate_and_verify_lemmas(
    provider: &dyn LlmProvider,
    cache: &LlmCache,
    function_source: &str,
    function_signature: &str,
    contracts: &[ContractClauseInfo],
    level1_verdict: &str,
    level1_paths: &[PathAnalysis],
) -> Result<(LemmaChain, LemmaChainVerification), LlmError> {
    let key = lemma_cache_key(
        function_signature,
        function_source,
        contracts,
        level1_verdict,
        provider.model_id(),
    );

    // Check cache for lemma chain
    let lemma_dir = cache.cache_dir().join("lemmas");
    let cache_path = lemma_dir.join(format!("{key}.json"));
    if let Ok(data) = std::fs::read_to_string(&cache_path)
        && let Ok(chain) = serde_json::from_str::<LemmaChain>(&data)
    {
        // Re-verify (cheap) even if cached, in case Z3 version changed
        let requires: Vec<String> = contracts
            .iter()
            .filter(|c| c.kind == "requires")
            .map(|c| c.expression.clone())
            .collect();
        let ensures: Vec<String> = contracts
            .iter()
            .filter(|c| c.kind == "ensures")
            .map(|c| c.expression.clone())
            .collect();
        let verification = verify_lemma_chain(&requires, &ensures, &chain);
        return Ok((chain, verification));
    }

    // Generate lemma chain via LLM
    let system = lemma_system_prompt();
    let user = lemma_user_prompt(
        function_source,
        function_signature,
        contracts,
        level1_verdict,
        level1_paths,
    );
    let raw = provider.call_raw(&system, &user)?;
    let chain = parse_lemma_response(&raw)?;

    // Cache the chain
    let _ = std::fs::create_dir_all(&lemma_dir);
    if let Ok(data) = serde_json::to_string_pretty(&chain) {
        let _ = std::fs::write(&cache_path, data);
    }

    // Verify with Z3
    let requires: Vec<String> = contracts
        .iter()
        .filter(|c| c.kind == "requires")
        .map(|c| c.expression.clone())
        .collect();
    let ensures: Vec<String> = contracts
        .iter()
        .filter(|c| c.kind == "ensures")
        .map(|c| c.expression.clone())
        .collect();
    let verification = verify_lemma_chain(&requires, &ensures, &chain);

    Ok((chain, verification))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("a + b == c").unwrap();
        assert_eq!(tokens.len(), 5);
        assert!(matches!(&tokens[0], Token::Ident(s) if s == "a"));
        assert!(matches!(&tokens[1], Token::Op(s) if s == "+"));
        assert!(matches!(&tokens[3], Token::Op(s) if s == "=="));
    }

    #[test]
    fn assertion_arithmetic() {
        let smt = assertion_to_smtlib("a + b").unwrap();
        assert_eq!(smt, "(+ a b)");
    }

    #[test]
    fn assertion_comparison() {
        let smt = assertion_to_smtlib("x <= y").unwrap();
        assert_eq!(smt, "(<= x y)");
    }

    #[test]
    fn assertion_logic() {
        let smt = assertion_to_smtlib("a and b").unwrap();
        assert_eq!(smt, "(and a b)");
    }

    #[test]
    fn assertion_implies() {
        let smt = assertion_to_smtlib("x > 0 implies y > 0").unwrap();
        assert_eq!(smt, "(=> (> x 0) (> y 0))");
    }

    #[test]
    fn assertion_not() {
        let smt = assertion_to_smtlib("not a").unwrap();
        assert_eq!(smt, "(not a)");
    }

    #[test]
    fn assertion_nested() {
        let smt = assertion_to_smtlib("(a + b) * c == d").unwrap();
        assert_eq!(smt, "(= (* (+ a b) c) d)");
    }

    #[test]
    fn assertion_complex() {
        let smt = assertion_to_smtlib("new_balance == old_balance - amount").unwrap();
        assert_eq!(smt, "(= new_balance (- old_balance amount))");
    }

    #[test]
    fn assertion_len() {
        let smt = assertion_to_smtlib("len(data) >= 3").unwrap();
        assert_eq!(smt, "(>= (len data) 3)");
    }

    #[test]
    fn parse_lemma_response_ok() {
        let raw = r#"{"lemmas":[{"label":"step1","assertion":"x > 0","justification":"given","depends_on":[]}],"chain_complete":true}"#;
        let chain = parse_lemma_response(raw).unwrap();
        assert_eq!(chain.lemmas.len(), 1);
        assert_eq!(chain.lemmas[0].label, "step1");
        assert!(chain.chain_complete);
    }

    #[test]
    fn verify_trivial_chain() {
        // x > 0 implies x >= 0 -- should be valid if Z3 is available
        let chain = LemmaChain {
            lemmas: vec![LlmLemma {
                label: "trivial".to_string(),
                assertion: "x >= 0".to_string(),
                justification: "follows from x > 0".to_string(),
                depends_on: vec![],
            }],
            chain_complete: true,
        };
        let verification = verify_lemma_chain(&["x > 0".to_string()], &[], &chain);
        // This test only checks structure; Z3 may not be available in CI
        assert_eq!(verification.total_count, 1);
    }

    #[test]
    fn build_query_structure() {
        let query = build_validity_query(&["(> x 0)".to_string()], "(>= x 0)");
        assert!(query.contains("(set-logic ALL)"));
        assert!(query.contains("(assert (> x 0))"));
        assert!(query.contains("(assert (not (>= x 0)))"));
        assert!(query.contains("(check-sat)"));
    }

    #[test]
    fn precedence_climbing() {
        // a + b * c should be (+ a (* b c)) due to precedence
        let smt = assertion_to_smtlib("a + b * c").unwrap();
        assert_eq!(smt, "(+ a (* b c))");
    }

    #[test]
    fn implies_is_lowest_precedence() {
        // a > 0 implies b > 0 and c > 0
        // should be (=> (> a 0) (and (> b 0) (> c 0)))
        let smt = assertion_to_smtlib("a > 0 implies b > 0 and c > 0").unwrap();
        assert_eq!(smt, "(=> (> a 0) (and (> b 0) (> c 0)))");
    }

    #[test]
    fn assertion_empty_input() {
        let result = assertion_to_smtlib("");
        assert!(result.is_err());
    }

    #[test]
    fn assertion_unbalanced_parens() {
        let result = assertion_to_smtlib("(a + b");
        assert!(result.is_err());
    }

    #[test]
    fn assertion_unicode_rejected() {
        let result = assertion_to_smtlib("α > 0");
        assert!(result.is_err());
    }

    #[test]
    fn assertion_unary_minus() {
        let smt = assertion_to_smtlib("-x + 1").unwrap();
        assert_eq!(smt, "(+ (- x) 1)");
    }

    #[test]
    fn assertion_mod_operator() {
        let smt = assertion_to_smtlib("x mod 2 == 0").unwrap();
        assert_eq!(smt, "(= (mod x 2) 0)");
    }

    #[test]
    fn assertion_not_equals() {
        let smt = assertion_to_smtlib("x != y").unwrap();
        assert_eq!(smt, "(distinct x y)");
    }

    #[test]
    fn verify_empty_lemma_chain() {
        let chain = LemmaChain {
            lemmas: vec![],
            chain_complete: true,
        };
        let v = verify_lemma_chain(&["x > 0".to_string()], &["x >= 0".to_string()], &chain);
        assert_eq!(v.total_count, 0);
        assert_eq!(v.valid_count, 0);
    }

    #[test]
    fn verify_chain_with_parse_error() {
        let chain = LemmaChain {
            lemmas: vec![LlmLemma {
                label: "bad".to_string(),
                assertion: "α ≥ 0".to_string(), // unparseable
                justification: "test".to_string(),
                depends_on: vec![],
            }],
            chain_complete: false,
        };
        let v = verify_lemma_chain(&["x > 0".to_string()], &[], &chain);
        assert_eq!(v.total_count, 1);
        assert_eq!(v.valid_count, 0);
        assert!(matches!(v.lemmas[0].result, LemmaResult::ParseError { .. }));
    }

    #[test]
    fn parse_lemma_response_invalid_json() {
        let result = parse_lemma_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn tokenize_empty() {
        let tokens = tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_operators() {
        let tokens = tokenize("!= <= >= == < >").unwrap();
        assert_eq!(tokens.len(), 6);
        assert!(matches!(&tokens[0], Token::Op(s) if s == "!="));
        assert!(matches!(&tokens[1], Token::Op(s) if s == "<="));
        assert!(matches!(&tokens[2], Token::Op(s) if s == ">="));
        assert!(matches!(&tokens[3], Token::Op(s) if s == "=="));
        assert!(matches!(&tokens[4], Token::Op(s) if s == "<"));
        assert!(matches!(&tokens[5], Token::Op(s) if s == ">"));
    }
}
