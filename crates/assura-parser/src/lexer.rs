use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    // --- Keywords (alphabetical) ---
    #[token("and")]
    And,
    #[token("as")]
    As,
    #[token("axiom")]
    Axiom,
    #[token("bind")]
    Bind,
    #[token("compliance")]
    Compliance,
    #[token("concurrency")]
    Concurrency,
    #[token("contract")]
    Contract,
    #[token("data-flow")]
    DataFlow,
    #[token("effects")]
    Effects,
    #[token("else")]
    Else,
    #[token("enum")]
    Enum,
    #[token("ensures")]
    Ensures,
    #[token("eventually")]
    Eventually,
    #[token("evolution")]
    Evolution,
    #[token("exists")]
    Exists,
    #[token("extern")]
    Extern,
    #[token("fair")]
    Fair,
    #[token("false")]
    False,
    #[token("fn")]
    Fn,
    #[token("forall")]
    Forall,
    #[token("ghost")]
    Ghost,
    #[token("idempotent")]
    Idempotent,
    #[token("if")]
    If,
    #[token("import")]
    Import,
    #[token("in")]
    In,
    #[token("incremental")]
    Incremental,
    #[token("input")]
    Input,
    #[token("invariant")]
    Invariant,
    #[token("is")]
    Is,
    #[token("leads_to")]
    LeadsTo,
    #[token("lemma")]
    Lemma,
    #[token("liveness")]
    Liveness,
    #[token("modifies")]
    Modifies,
    #[token("module")]
    Module,
    #[token("must-not")]
    MustNot,
    #[token("not")]
    Not,
    #[token("old")]
    Old,
    #[token("opaque")]
    Opaque,
    #[token("operation")]
    Operation,
    #[token("or")]
    Or,
    #[token("ordering")]
    Ordering,
    #[token("output")]
    Output,
    #[token("performance")]
    Performance,
    #[token("privacy")]
    Privacy,
    #[token("profile")]
    Profile,
    #[token("project")]
    Project,
    #[token("prophecy")]
    Prophecy,
    #[token("protocol")]
    Protocol,
    #[token("pub")]
    Pub,
    #[token("pure")]
    Pure,
    #[token("query")]
    Query,
    #[token("requires")]
    Requires,
    #[token("resolve")]
    Resolve,
    #[token("result")]
    Result_,
    #[token("retention")]
    Retention,
    #[token("rule")]
    Rule,
    #[token("self")]
    Self_,
    #[token("service")]
    Service,
    #[token("states")]
    States,
    #[token("then")]
    Then,
    #[token("transaction")]
    Transaction,
    #[token("true")]
    True,
    #[token("type")]
    Type,
    #[token("where")]
    Where,

    // --- Literals ---
    #[regex(r"-?[0-9][0-9_]*\.[0-9][0-9_]*", |lex| lex.slice().to_string())]
    Float(String),

    #[regex(r"-?[0-9][0-9_]*", |lex| lex.slice().to_string(), priority = 3)]
    Int(String),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    String(String),

    // --- Identifiers ---
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 2)]
    Ident(String),

    // --- Punctuation ---
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    LAngle,
    #[token(">")]
    RAngle,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
    #[token("?")]
    Question,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("#")]
    Hash,
    #[token("@")]
    At,
    #[token("=")]
    Equals,

    // --- Operators ---
    #[token("++")]
    Concat,
    #[token("+")]
    Plus,
    #[token("-", priority = 1)]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("==")]
    Eq,
    #[token("!=")]
    Neq,
    #[token("<=")]
    Lte,
    #[token(">=")]
    Gte,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("!")]
    Bang,
    #[token("&mut")]
    AmpMut,
    #[token("&")]
    Amp,
    #[token("..")]
    DotDot,
    #[token("^")]
    Caret,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
