//! Fuzzer crash artifact parsing and crash-guided contract suggestion.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cache::{self, LlmCache};
use crate::provider::LlmProvider;
use crate::types::*;

// ---------------------------------------------------------------------------
// Crash artifact types
// ---------------------------------------------------------------------------

/// A parsed crash artifact from `cargo-fuzz`.
#[derive(Debug, Clone)]
pub struct CrashArtifact {
    pub path: PathBuf,
    pub input_bytes: Vec<u8>,
    pub crash_kind: CrashKind,
    pub input_summary: String,
}

/// Type of crash detected by the fuzzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    Crash,
    Oom,
    Timeout,
}

impl std::fmt::Display for CrashKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrashKind::Crash => write!(f, "crash"),
            CrashKind::Oom => write!(f, "oom"),
            CrashKind::Timeout => write!(f, "timeout"),
        }
    }
}

impl CrashArtifact {
    /// Parse a crash artifact file written by `cargo-fuzz`.
    pub fn from_file(path: &Path) -> Result<Self, LlmError> {
        let input_bytes = std::fs::read(path).map_err(LlmError::Io)?;
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let crash_kind = if filename.starts_with("oom-") {
            CrashKind::Oom
        } else if filename.starts_with("timeout-") {
            CrashKind::Timeout
        } else {
            CrashKind::Crash
        };
        let input_summary = summarize_input(&input_bytes);
        Ok(Self {
            path: path.to_owned(),
            input_bytes,
            crash_kind,
            input_summary,
        })
    }

    /// Load all crash artifacts from a directory.
    pub fn from_directory(dir: &Path) -> Result<Vec<Self>, LlmError> {
        let mut artifacts = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(LlmError::Io)?;
        for entry in entries {
            let entry = entry.map_err(LlmError::Io)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // cargo-fuzz artifacts: crash-*, oom-*, timeout-*
            if name.starts_with("crash-")
                || name.starts_with("oom-")
                || name.starts_with("timeout-")
            {
                artifacts.push(Self::from_file(&path)?);
            }
        }
        // Sort by file size (smallest first, most readable for LLM)
        artifacts.sort_by_key(|a| a.input_bytes.len());
        Ok(artifacts)
    }
}

/// Summarize crash input for the LLM prompt.
fn summarize_input(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "Empty input (0 bytes)".to_string();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.len() <= 500 {
            format!("UTF-8 string ({} bytes): {:?}", bytes.len(), s)
        } else {
            format!("UTF-8 string ({} bytes): {:?}...", bytes.len(), &s[..500])
        }
    } else {
        let preview_len = bytes.len().min(64);
        let hex: Vec<String> = bytes[..preview_len]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!(
            "Binary data ({} bytes): [{}]{}",
            bytes.len(),
            hex.join(" "),
            if bytes.len() > 64 { "..." } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// Stack trace parsing
// ---------------------------------------------------------------------------

/// A parsed stack trace from a crash.
#[derive(Debug, Clone, Default)]
pub struct StackTrace {
    pub panic_message: Option<String>,
    pub panic_location: Option<String>,
    pub frames: Vec<StackFrame>,
}

/// A single frame from a stack trace.
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub is_user_code: bool,
}

impl StackTrace {
    /// Parse a Rust backtrace from text (RUST_BACKTRACE=1 format).
    pub fn parse(text: &str) -> Self {
        let mut trace = Self::default();
        let mut lines = text.lines().peekable();

        // Look for panic message
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("thread '") && trimmed.contains("panicked at") {
                // thread 'main' panicked at 'index out of bounds: ...'
                if let Some(msg_start) = trimmed.find("panicked at") {
                    let after = &trimmed[msg_start + "panicked at".len()..].trim();
                    // Strip surrounding quotes if present
                    let msg = after
                        .trim_start_matches('\'')
                        .trim_end_matches('\'')
                        .trim_start_matches('"')
                        .trim_end_matches('"');
                    // May have ", src/file.rs:42:10" appended
                    if let Some(comma_idx) = msg.rfind(", ") {
                        let maybe_loc = &msg[comma_idx + 2..];
                        if maybe_loc.contains(':') && !maybe_loc.starts_with(' ') {
                            let cleaned = msg[..comma_idx]
                                .trim_end_matches('\'')
                                .trim_end_matches('"');
                            trace.panic_message = Some(cleaned.to_string());
                            trace.panic_location = Some(maybe_loc.to_string());
                            continue;
                        }
                    }
                    trace.panic_message =
                        Some(msg.trim_end_matches('\'').trim_end_matches('"').to_string());
                }
            }
        }

        // Parse stack frames
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            // Format: "  N: package::module::function"
            if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
                let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                if let Some(func) = rest.strip_prefix(": ") {
                    let func = func.trim();
                    let is_user = !func.starts_with("std::")
                        && !func.starts_with("core::")
                        && !func.starts_with("alloc::")
                        && !func.starts_with("test::")
                        && !func.starts_with("__rust_")
                        && !func.starts_with("rust_begin_unwind")
                        && !func.starts_with("panic_");

                    let mut frame = StackFrame {
                        function_name: func.to_string(),
                        file: None,
                        line: None,
                        is_user_code: is_user,
                    };

                    // Next line may have "at src/file.rs:42:10"
                    if let Some(next) = lines.peek() {
                        let next_trimmed = next.trim();
                        if let Some(loc) = next_trimmed.strip_prefix("at ") {
                            let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
                            if parts.len() >= 2 {
                                frame.line = parts[1].parse().ok();
                                frame.file = Some(parts.last().unwrap_or(&loc).to_string());
                            }
                            lines.next(); // consume the "at" line
                        }
                    }

                    trace.frames.push(frame);
                }
            }
        }

        trace
    }

    /// Get the first user-code frame (most likely crash location).
    pub fn crash_function(&self) -> Option<&StackFrame> {
        self.frames.iter().find(|f| f.is_user_code)
    }
}

// ---------------------------------------------------------------------------
// Crash-specific LLM prompt
// ---------------------------------------------------------------------------

/// Build a system prompt for crash-guided contract suggestion.
pub fn crash_system_prompt() -> String {
    r#"You are a contract suggestion assistant that analyzes fuzzer crashes in Rust code. A fuzzer found a crash in a function, and you must propose #[requires] preconditions that would prevent the crash.

You must respond ONLY with a JSON object (no markdown fences, no prose before or after):

{
  "suggestions": [
    {
      "kind": "requires",
      "expression": "<Rust boolean expression using the function's parameter names>",
      "confidence": <float 0.0 to 1.0>,
      "reasoning": "<why this precondition prevents the crash>",
      "prevents": "<which crash scenario this guards against>"
    }
  ]
}

Rules:
- Focus on #[requires] preconditions that prevent the crash, not general contracts.
- Use the function's parameter names in expressions.
- Prefer precise contracts over overly broad ones.
- Consider whether the contract is too restrictive (rejects valid inputs) or too permissive (allows other crashes).
- Common crash-to-contract patterns:
  * Index out of bounds -> length/size check on the input
  * Integer overflow -> range check or wrapping arithmetic guard
  * Unwrap on None -> is_some() precondition
  * Division by zero -> non-zero divisor check
  * Slice range panic -> start <= end && end <= len
  * OOM -> size limit on allocation
  * Timeout -> input size limit (heuristic)
- Only propose contracts you are confident about (>= 0.8 confidence)."#.to_string()
}

/// Build a user prompt for crash-guided contract suggestion.
pub fn crash_user_prompt(
    function_source: &str,
    function_name: &str,
    crash: &CrashArtifact,
    stack_trace: Option<&StackTrace>,
    existing_contracts: &[String],
) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("## Function\n\n```rust\n");
    prompt.push_str(function_source);
    prompt.push_str("\n```\n\n");

    prompt.push_str("## Crash Details\n\n");
    prompt.push_str(&format!("- **Function**: `{function_name}`\n"));
    prompt.push_str(&format!("- **Crash type**: {}\n", crash.crash_kind));
    prompt.push_str(&format!("- **Input**: {}\n", crash.input_summary));

    if let Some(trace) = stack_trace {
        if let Some(msg) = &trace.panic_message {
            prompt.push_str(&format!("- **Panic message**: {msg}\n"));
        }
        if let Some(loc) = &trace.panic_location {
            prompt.push_str(&format!("- **Location**: {loc}\n"));
        }
        if let Some(frame) = trace.crash_function() {
            prompt.push_str(&format!(
                "- **Crashing function**: `{}`\n",
                frame.function_name
            ));
            if let Some(file) = &frame.file {
                prompt.push_str(&format!("- **File**: {file}"));
                if let Some(line) = frame.line {
                    prompt.push_str(&format!(":{line}"));
                }
                prompt.push('\n');
            }
        }
    }

    prompt.push('\n');

    if !existing_contracts.is_empty() {
        prompt.push_str("## Existing Contracts\n\n");
        for c in existing_contracts {
            prompt.push_str(&format!("- {c}\n"));
        }
        prompt.push_str("\nThe function already has these contracts. Propose ADDITIONAL preconditions that would prevent THIS specific crash.\n\n");
    } else {
        prompt.push_str("## Existing Contracts\n\n(none)\n\n");
    }

    prompt.push_str("Propose `#[requires]` preconditions that would prevent this crash. Respond with JSON only.\n");
    prompt
}

/// Parse a crash suggestion response from the LLM.
pub fn parse_crash_response(raw: &str) -> Result<CrashSuggestionResponse, LlmError> {
    let json_str = crate::prompt::extract_json(raw);
    serde_json::from_str(json_str).map_err(|e| LlmError::Parse(format!("{e}: {json_str}")))
}

/// Response from the crash suggestion LLM call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrashSuggestionResponse {
    pub suggestions: Vec<CrashSuggestion>,
}

/// A single crash-guided contract suggestion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrashSuggestion {
    pub kind: String,
    pub expression: String,
    pub confidence: f64,
    pub reasoning: String,
    pub prevents: String,
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

/// A crash analysis result with associated function info.
#[derive(Debug, Clone)]
pub struct CrashAnalysis {
    pub artifact: CrashArtifact,
    pub function_name: String,
    pub panic_line: Option<usize>,
    pub suggestions: Vec<CrashSuggestion>,
}

/// Deduplicate crashes by (function_name, panic_line).
/// Keeps the crash with the smallest input (most readable for LLM).
pub fn deduplicate_crashes(crashes: &[CrashAnalysis]) -> Vec<&CrashAnalysis> {
    let mut seen = HashSet::new();
    crashes
        .iter()
        .filter(|c| {
            let key = format!("{}:{}", c.function_name, c.panic_line.unwrap_or(0));
            seen.insert(key)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Compute cache key for crash suggestion.
/// Keyed by (function + panic class), not specific input.
pub fn crash_cache_key(
    function_name: &str,
    function_body: &str,
    crash_kind: &str,
    panic_message: &str,
    existing_contracts: &[String],
    model: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"crash-v1:");
    hasher.update(function_name.as_bytes());
    hasher.update(function_body.as_bytes());
    hasher.update(crash_kind.as_bytes());
    hasher.update(panic_message.as_bytes());
    for c in existing_contracts {
        hasher.update(c.as_bytes());
    }
    hasher.update(model.as_bytes());
    hasher.update(crate::prompt::prompt_version().as_bytes());
    cache::hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Run crash-guided suggestion for a single crash + function.
pub fn suggest_from_crash(
    provider: &dyn LlmProvider,
    cache: &LlmCache,
    function_source: &str,
    function_name: &str,
    crash: &CrashArtifact,
    stack_trace: Option<&StackTrace>,
    existing_contracts: &[String],
) -> Result<CrashSuggestionResponse, LlmError> {
    let key = crash_cache_key(
        function_name,
        function_source,
        &crash.crash_kind.to_string(),
        stack_trace
            .and_then(|t| t.panic_message.as_deref())
            .unwrap_or(""),
        existing_contracts,
        provider.model_id(),
    );

    // Check fuzz sub-cache
    let fuzz_dir = cache.cache_dir().join("fuzz");
    let cache_path = fuzz_dir.join(format!("{key}.json"));
    if let Ok(data) = std::fs::read_to_string(&cache_path)
        && let Ok(resp) = serde_json::from_str::<CrashSuggestionResponse>(&data)
    {
        return Ok(resp);
    }

    let system = crash_system_prompt();
    let user = crash_user_prompt(
        function_source,
        function_name,
        crash,
        stack_trace,
        existing_contracts,
    );
    let raw = provider.call_raw(&system, &user)?;
    let response = parse_crash_response(&raw)?;

    // Cache the result
    let _ = std::fs::create_dir_all(&fuzz_dir);
    if let Ok(data) = serde_json::to_string_pretty(&response) {
        let _ = std::fs::write(cache_path, data);
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_kind_from_filename() {
        let dir = std::env::temp_dir().join("assura-fuzz-test-kind");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Write test artifacts
        std::fs::write(dir.join("crash-abc123"), b"hello").unwrap();
        std::fs::write(dir.join("oom-def456"), b"big").unwrap();
        std::fs::write(dir.join("timeout-ghi789"), b"slow").unwrap();

        let c = CrashArtifact::from_file(&dir.join("crash-abc123")).unwrap();
        assert_eq!(c.crash_kind, CrashKind::Crash);

        let o = CrashArtifact::from_file(&dir.join("oom-def456")).unwrap();
        assert_eq!(o.crash_kind, CrashKind::Oom);

        let t = CrashArtifact::from_file(&dir.join("timeout-ghi789")).unwrap();
        assert_eq!(t.crash_kind, CrashKind::Timeout);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn from_directory_filters_and_sorts() {
        let dir = std::env::temp_dir().join("assura-fuzz-test-dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Large crash
        std::fs::write(dir.join("crash-large"), vec![0u8; 100]).unwrap();
        // Small crash
        std::fs::write(dir.join("crash-small"), b"hi").unwrap();
        // Not a crash artifact
        std::fs::write(dir.join("README.md"), b"ignore me").unwrap();

        let artifacts = CrashArtifact::from_directory(&dir).unwrap();
        assert_eq!(artifacts.len(), 2); // README filtered out
        assert!(artifacts[0].input_bytes.len() <= artifacts[1].input_bytes.len());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn input_summary_utf8() {
        let s = summarize_input(b"hello world");
        assert!(s.contains("UTF-8"));
        assert!(s.contains("11 bytes"));
    }

    #[test]
    fn input_summary_binary() {
        let s = summarize_input(&[0xff, 0x00, 0x42]);
        assert!(s.contains("Binary data"));
        assert!(s.contains("3 bytes"));
    }

    #[test]
    fn input_summary_empty() {
        let s = summarize_input(&[]);
        assert!(s.contains("Empty"));
    }

    #[test]
    fn stack_trace_parse() {
        let bt = r#"thread 'main' panicked at 'index out of bounds: the len is 2 but the index is 5', src/parser.rs:42:10
stack backtrace:
   0: rust_begin_unwind
   1: core::panicking::panic
   2: myapp::parser::parse_header
             at src/parser.rs:42:10
   3: myapp::main
             at src/main.rs:10:5
"#;
        let trace = StackTrace::parse(bt);
        assert_eq!(
            trace.panic_message.as_deref(),
            Some("index out of bounds: the len is 2 but the index is 5")
        );
        assert_eq!(trace.panic_location.as_deref(), Some("src/parser.rs:42:10"));

        let user_frame = trace.crash_function().unwrap();
        assert_eq!(user_frame.function_name, "myapp::parser::parse_header");
        assert_eq!(user_frame.file.as_deref(), Some("src/parser.rs"));
        assert_eq!(user_frame.line, Some(42));
    }

    #[test]
    fn dedup_keeps_first() {
        let make = |name: &str, line: Option<usize>, size: usize| CrashAnalysis {
            artifact: CrashArtifact {
                path: PathBuf::from("x"),
                input_bytes: vec![0; size],
                crash_kind: CrashKind::Crash,
                input_summary: String::new(),
            },
            function_name: name.to_string(),
            panic_line: line,
            suggestions: vec![],
        };

        let crashes = vec![
            make("parse", Some(42), 10),
            make("parse", Some(42), 100), // same function+line, larger input
            make("validate", Some(10), 50),
        ];

        let deduped = deduplicate_crashes(&crashes);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].function_name, "parse");
        assert_eq!(deduped[0].artifact.input_bytes.len(), 10); // kept smaller
        assert_eq!(deduped[1].function_name, "validate");
    }

    #[test]
    fn crash_cache_key_deterministic() {
        let k1 = crash_cache_key("foo", "body", "crash", "oob", &[], "mock");
        let k2 = crash_cache_key("foo", "body", "crash", "oob", &[], "mock");
        assert_eq!(k1, k2);
    }

    #[test]
    fn crash_cache_key_differs_on_panic() {
        let k1 = crash_cache_key("foo", "body", "crash", "oob", &[], "mock");
        let k2 = crash_cache_key("foo", "body", "crash", "div by zero", &[], "mock");
        assert_ne!(k1, k2);
    }

    #[test]
    fn crash_prompt_includes_details() {
        let crash = CrashArtifact {
            path: PathBuf::from("crash-abc"),
            input_bytes: vec![0xff, 0x42],
            crash_kind: CrashKind::Crash,
            input_summary: "Binary data (2 bytes): [ff 42]".to_string(),
        };
        let trace = StackTrace {
            panic_message: Some("index out of bounds".to_string()),
            panic_location: Some("src/lib.rs:10:5".to_string()),
            frames: vec![],
        };
        let prompt = crash_user_prompt(
            "fn parse(data: &[u8]) { data[5]; }",
            "parse",
            &crash,
            Some(&trace),
            &[],
        );
        assert!(prompt.contains("parse"));
        assert!(prompt.contains("crash"));
        assert!(prompt.contains("index out of bounds"));
        assert!(prompt.contains("ff 42"));
    }
}
