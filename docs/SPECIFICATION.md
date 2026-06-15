# Assura Language Specification v0.1

> Implementer's reference. Every section is designed so an engineer can
> build the corresponding compiler component without ambiguity.

### Verification Scope

Assura verifies **safety and liveness properties** of single-node
programs under all Rust memory orderings (SeqCst, AcqRel, Relaxed).

**Safety** (Layer 0-2): preconditions, postconditions, invariants,
frame conditions, structural invariants. Decidable or bounded SMT.

**Liveness** (Layer 2-3): bounded model checking with k-induction.
Properties like "eventually a leader is elected" are verified up to
a configurable step bound K, with optional unbounded proof via
k-induction.

**Concurrency** (Layer 1-2): per-thread views for weak memory
ordering, prophecy variables for linearizability proofs. These
extend the verification beyond what Dafny, F*, or SPARK offer.

## Table of Contents

- [1. Contract Language Grammar](#1-contract-language-grammar)
- [2. Type System](#2-type-system)
- [3. Effect System](#3-effect-system)
- [4. Implementation IR Format](#4-implementation-ir-format)
- [5. Verification Architecture](#5-verification-architecture)
- [6. Rust Codegen Mapping](#6-rust-codegen-mapping)
- [7. Error Code Catalog](#7-error-code-catalog)
- [8. Module System](#8-module-system)
- [9. Standard Library](#9-standard-library)
- [10. CLI Interface](#10-cli-interface)
- [11. AI Agent API](#11-ai-agent-api)
- [12. Decidability Boundaries](#12-decidability-boundaries)
- [13. Type Interaction Test Cases](#13-type-interaction-test-cases)
- [14. Verification Categories](#14-verification-categories)
  - [14.CORE: Verification Infrastructure](#14core-verification-infrastructure)
  - [14.MEM: Memory Safety](#14mem-memory-safety)
  - [14.TYPE: Types and Contracts](#14type-types-and-contracts)
  - [14.SEC: Trust and Security](#14sec-trust-and-security)
  - [14.CONC: Concurrency](#14conc-concurrency)
  - [14.STOR: Storage and Durability](#14stor-storage-and-durability)
  - [14.FMT: Data Formats and Parsing](#14fmt-data-formats-and-parsing)
  - [14.NUM: Numerical and Precision](#14num-numerical-and-precision)
  - [14.PLAT: Platform and Configuration](#14plat-platform-and-configuration)
  - [14.PERF: Performance](#14perf-performance)
  - [14.TEST: Testing and Verification Workflow](#14test-testing-and-verification-workflow)
  - [14.MISC: Specialized](#14misc-specialized)

---

## 1. Contract Language Grammar

The contract language is what humans write. It is declarative,
readable, and complete. The grammar below is in EBNF notation.
Terminals are in `'quotes'` or `UPPER_CASE`. Non-terminals are in
`PascalCase`.

### 1.2 Project Profile

A project declares which verification categories are active.
Features in excluded categories are ignored by the parser
and skipped by the verifier.

```assura
project stb_image_rs {
  profile: [core, mem, sec, fmt, num, type, test]
}
```

CORE is always included and cannot be excluded.

#### Preset Profiles

| Preset | Categories | Example Projects |
|---|---|---|
| minimal | CORE, MEM, TYPE | Simple libraries, CLI tools |
| parser | + SEC, FMT | stb_image, picohttpparser, protobuf |
| database | + STOR, CONC | SQLite, RocksDB |
| embedded | + STOR, PLAT | littlefs, RTOS drivers |
| crypto | + SEC, CONC, NUM | WireGuard, libsodium |
| tls | + SEC, CONC, FMT, NUM, PLAT | mbedTLS, s2n-tls |
| systems | All categories | jemalloc, kernel modules |

Custom profiles list categories explicitly. Adding a
category mid-project is always safe (additive).

### 1.1 Lexical Grammar

```ebnf
(* Identifiers *)
Ident          = Letter (Letter | Digit | '_')* ;
TypeIdent      = UpperLetter (Letter | Digit | '_')* ;
FieldIdent     = LowerLetter (Letter | Digit | '_')* ;
EffectIdent    = LowerLetter (Letter | Digit | '.' | '_')* ;

(* Literals *)
IntLit         = ['-'] Digit+ ['_' Digit+]* ;
FloatLit       = ['-'] Digit+ '.' Digit+ ;
StringLit      = '"' { StringChar } '"' ;
BoolLit        = 'true' | 'false' ;

(* Comments *)
LineComment    = '//' { any except newline } ;
BlockComment   = '/*' { any } '*/' ;

(* Keywords *)
Keyword        = 'service' | 'contract' | 'type' | 'enum' | 'states'
               | 'transition' | 'operation' | 'query' | 'input'
               | 'output' | 'errors' | 'requires' | 'ensures'
               | 'invariant' | 'effects' | 'must-not' | 'rule'
               | 'data-flow' | 'extern' | 'bind' | 'import'
               | 'module' | 'pub' | 'where' | 'forall' | 'exists'
               | 'in' | 'not' | 'and' | 'or' | 'if' | 'then'
               | 'else' | 'old' | 'result' | 'self'
               | 'concurrency' | 'privacy' | 'retention' | 'audit'
               | 'evolution' | 'transaction' | 'serialization'
               | 'api_compat' | 'performance' | 'protocol'
               | 'observe' | 'compliance' | 'ordering'
               | 'idempotent' ;
```

### 1.2 Top-Level Declarations

```ebnf
SourceFile     = ModuleDecl? { ImportDecl } { TopDecl } ;

ModuleDecl     = 'module' QualifiedName ';' ;

ImportDecl     = 'import' QualifiedName ['as' Ident]
                 ['{' ImportList '}'] ';' ;
ImportList     = Ident { ',' Ident } ;

QualifiedName  = Ident { '.' Ident } ;

TopDecl        = ServiceDecl
               | ContractDecl
               | TypeDecl
               | EnumDecl
               | ExternDecl
               | BindDecl
               | ComplianceDecl
               | ProtocolDecl ;
```

### 1.3 Service Declarations

```ebnf
ServiceDecl    = 'service' TypeIdent '{'
                   { ServiceItem }
                 '}' ;

ServiceItem    = TypeDecl
               | EnumDecl
               | StateDecl
               | TransitionDecl
               | OperationDecl
               | QueryDecl
               | InvariantDecl
               | ConcurrencyDecl
               | PrivacyDecl
               | RetentionDecl
               | AuditDecl
               | EvolutionDecl
               | TransactionDecl
               | SerializationDecl
               | ApiCompatDecl
               | PerformanceDecl
               | ProtocolDecl
               | ObserveDecl
               | ComplianceDecl
               | OrderingDecl
               | IdempotencyDecl ;
```

### 1.4 Type Declarations

```ebnf
TypeDecl       = 'type' TypeIdent [TypeParams] ['=' TypeExpr]
                 ['{' { FieldDecl } '}']
                 [WhereClause] ;

TypeParams     = '<' TypeParam { ',' TypeParam } '>' ;
TypeParam      = TypeIdent [':' KindExpr] ;

FieldDecl      = [Visibility] FieldIdent ':' TypeExpr
                 [FieldConstraint] ';' ;

Visibility     = 'pub' ;

FieldConstraint = 'where' Predicate ;

(* Types *)
TypeExpr       = BaseType
               | RefinedType
               | FunctionType
               | GenericType
               | TupleType
               | OptionType
               | ListType
               | MapType
               | SetType ;

BaseType       = TypeIdent [TypeArgs]
               | BuiltinType ;

BuiltinType    = 'Int' | 'Nat' | 'Float' | 'Bool' | 'String'
               | 'Bytes' | 'Unit' | 'Never' ;

RefinedType    = '{' Ident ':' TypeExpr '|' Predicate '}' ;

FunctionType   = '(' [ParamList] ')' '->' EffectRow TypeExpr ;

GenericType    = TypeIdent '<' TypeExpr { ',' TypeExpr } '>' ;

TupleType      = '(' TypeExpr ',' TypeExpr { ',' TypeExpr } ')' ;
OptionType     = TypeExpr '?' ;
ListType       = 'List' '<' TypeExpr '>' ;
MapType        = 'Map' '<' TypeExpr ',' TypeExpr '>' ;
SetType        = 'Set' '<' TypeExpr '>' ;

TypeArgs       = '<' TypeExpr { ',' TypeExpr } '>' ;
```

### 1.5 Enum and State Declarations

```ebnf
EnumDecl       = 'enum' TypeIdent [TypeParams] '{'
                   EnumVariant { ',' EnumVariant } [',']
                 '}' ;
EnumVariant    = TypeIdent ['(' TypeExpr { ',' TypeExpr } ')'] ;

StateDecl      = 'states' ':' StateIdent { '->' StateIdent } ;
StateIdent     = TypeIdent ;

TransitionDecl = 'transition' StateIdent
                 ['requires' ':' Predicate]
                 ';' ;
```

### 1.6 Operations and Queries

```ebnf
OperationDecl  = 'operation' TypeIdent '{'
                   { OperationItem }
                 '}' ;

QueryDecl      = 'query' TypeIdent '{'
                   { OperationItem }
                 '}' ;

OperationItem  = InputDecl
               | OutputDecl
               | ErrorsDecl
               | RequiresClause
               | EnsuresClause
               | EffectsClause
               | MustNotClause
               | RuleClause
               | DataFlowClause ;

InputDecl      = 'input' ':' '{' { FieldDecl } '}' ;
               | 'input' '(' ParamList ')' ;

OutputDecl     = 'output' ':' '{' { FieldDecl } '}' ;
               | 'output' '(' ParamList ')' ;

ErrorsDecl     = 'errors' ':' '[' TypeExpr { ',' TypeExpr } ']' ;

ParamList      = Param { ',' Param } ;
Param          = Ident ':' TypeExpr ;
```

### 1.7 Contract Clauses

```ebnf
ContractDecl   = 'contract' TypeIdent [TypeParams] '{'
                   { ContractItem }
                 '}' ;

ContractItem   = InputDecl | OutputDecl | RequiresClause
               | EnsuresClause | EffectsClause | InvariantDecl ;

RequiresClause = 'requires' ['{'] Predicate ['}'] ;
               | 'requires' ':' Predicate ;

EnsuresClause  = 'ensures' ['{'] Predicate ['}'] ;
               | 'ensures' ':' Predicate ;

InvariantDecl  = 'invariant' ['{'] Predicate ['}'] ;
               | 'invariant' ':' Predicate ;

EffectsClause  = 'effects' ':' EffectList ;
               | 'effects' ['{'] EffectList ['}'] ;

MustNotClause  = 'must-not' ':' EffectList ;

EffectList     = EffectIdent { ',' EffectIdent } ;

RuleClause     = 'rule' ':' Predicate ;

DataFlowClause = 'data-flow' ':' Expr 'must-not-appear-in'
                 Ident { ',' Ident } ;
```

### 1.8 Predicates and Expressions

```ebnf
Predicate      = PredAtom
               | Predicate 'and' Predicate
               | Predicate 'or' Predicate
               | 'not' Predicate
               | Predicate '=>' Predicate        (* implication *)
               | Quantifier
               | '(' Predicate ')'
               | 'if' Predicate 'then' Predicate
                 ['else' Predicate] ;

PredAtom       = Expr RelOp Expr
               | Expr 'in' Expr
               | Expr 'not' 'in' Expr
               | Expr 'is' TypeIdent
               | Expr                             (* boolean expr *) ;

Quantifier     = 'forall' Ident 'in' Expr ':' Predicate
               | 'exists' Ident 'in' Expr ':' Predicate ;

RelOp          = '==' | '!=' | '<' | '<=' | '>' | '>=' ;

Expr           = Literal
               | Ident
               | 'self'
               | 'result'
               | 'old' '(' Expr ')'
               | Expr '.' Ident
               | Expr '.' Ident '(' [ArgList] ')'
               | Expr BinOp Expr
               | UnaryOp Expr
               | Expr '[' Expr ']'
               | '|' Expr '|'                    (* absolute value *)
               | '(' Expr ')'
               | 'if' Expr 'then' Expr 'else' Expr
               | ListExpr
               | SetExpr
               | MapExpr ;

BinOp          = '+' | '-' | '*' | '/' | '%' | '++' ;
UnaryOp        = '-' | 'not' ;

ArgList        = Expr { ',' Expr } ;
Literal        = IntLit | FloatLit | StringLit | BoolLit ;

ListExpr       = '[' [Expr { ',' Expr }] ']' ;
SetExpr        = '{' Expr { ',' Expr } '}' ;
MapExpr        = '{' [MapEntry { ',' MapEntry }] '}' ;
MapEntry       = Expr ':' Expr ;
```

### 1.9 Extended Contract Layers (8-27)

```ebnf
(* Layer 8: Concurrency *)
ConcurrencyDecl = 'concurrency' ':' TypeIdent 'is'
                  ConcurrencyMode ;
ConcurrencyMode = 'exclusive' | 'shared-read' | 'actor-isolated' ;

(* Layer 9: Numerical Precision -- expressed via type aliases *)
(* e.g., type Money = FixedDecimal<2, USD> *)

(* Layer 10: Temporal Ordering *)
OrderingDecl   = 'ordering' ':' TypeIdent 'must' 'be' 'processed'
                 'in' OrderingMode ;
OrderingMode   = 'chronological' 'order'
               | 'reverse' 'chronological' 'order'
               | 'dependency' 'order' ;

(* Layer 11: Idempotency *)
IdempotencyDecl = 'idempotent' ':' TypeIdent 'consumes'
                  TypeIdent '(' LinearityMode ')' ;
LinearityMode  = 'linear' | 'affine' ;

(* Layer 12: Privacy *)
PrivacyDecl    = 'privacy' ':' Expr 'purpose' '{' PurposeList '}' ;
PurposeList    = Ident { ',' Ident } ;

RetentionDecl  = 'retention' ':' Ident 'retain' Duration
                 'then' RetentionAction ;
Duration       = IntLit '_' TimeUnit ;
TimeUnit       = 'days' | 'months' | 'years' ;
RetentionAction = 'delete' | 'archive' | 'anonymize' ;

(* Layer 13: Schema Evolution *)
EvolutionDecl  = 'evolution' ':' TypeIdent 'v' IntLit
                 EvolutionAction ;
EvolutionAction = 'adds' 'field' FieldDecl
               | 'removes' 'field' Ident
               | 'renames' 'field' Ident 'to' Ident
               | 'changes' 'field' Ident 'from' TypeExpr 'to'
                 TypeExpr ['default' Expr] ;

(* Layer 14: Crash Safety *)
TransactionDecl = 'transaction' ':' TypeIdent '{'
                    { CrashHandler }
                  '}' ;
CrashHandler   = 'on_crash' ':' CrashAction ;
CrashAction    = 'rollback' Expr
               | 'mark' Expr 'as' TypeIdent
               | 'compensate' Expr ;

(* Layer 15: Audit *)
AuditDecl      = 'audit' ':' AuditRule ;
AuditRule      = 'every' 'mutation' 'to' TypeIdent
                 'requires' TypeIdent [AuditConstraint]
               | Ident 'requires' TypeIdent [AuditConstraint] ;
AuditConstraint = 'with' Predicate ;

(* Layer 16: Serialization *)
SerializationDecl = 'serialization' ':' TypeIdent
                    SerializationGuarantee ;
SerializationGuarantee = 'roundtrip' 'guarantee'
                       | 'backward' 'compatible'
                       | 'forward' 'compatible' ;

(* Layer 17: API Evolution *)
ApiCompatDecl  = 'api_compat' ':' 'v' IntLit 'of' TypeIdent
                 ApiCompatRule ;
ApiCompatRule  = 'may' 'add' ApiTarget
               | 'may' 'not' 'remove' ApiTarget
               | 'may' 'not' 'add' 'required' ApiTarget ;
ApiTarget      = 'response' 'fields'
               | 'request' 'fields'
               | 'error' 'variants' ;

(* Layer 18: Performance *)
PerformanceDecl = 'performance' ':' TypeIdent 'is'
                  ComplexityBound 'in' Ident ;
ComplexityBound = 'O' '(' ComplexityExpr ')' ;
ComplexityExpr = '1' | 'n' | 'n' '^' IntLit
               | 'log' 'n' | 'n' 'log' 'n' ;

(* Layer 19: Multi-Service Protocol *)
ProtocolDecl   = 'protocol' TypeIdent '{'
                   { ProtocolStep }
                 '}' ;
ProtocolStep   = TypeIdent '->' TypeIdent ':' TypeIdent
                 ['|' TypeIdent] ;

(* Layer 20: Observability *)
ObserveDecl    = 'observe' ':' ObserveRule ;
ObserveRule    = 'every' 'operation' 'emits' ObserveTarget
                 ['with' Ident]
               | TypeIdent 'emits' ObserveTarget ;
ObserveTarget  = 'trace_span' | 'metric' Ident | 'log' Ident ;

(* Layer 21: Regulatory Compliance *)
ComplianceDecl = 'compliance' TypeIdent '{'
                   { ComplianceRule }
                 '}' ;
ComplianceRule = 'rule' ':' Predicate ;
```

### 1.10 Extern and Bind Declarations

```ebnf
ExternDecl     = 'extern' 'fn' Ident '(' [ParamList] ')'
                 '->' TypeExpr
                 { ExternClause } ;

ExternClause   = RequiresClause | EnsuresClause | EffectsClause ;

BindDecl       = 'bind' StringLit 'as' Ident '{'
                   { BindItem }
                 '}' ;

BindItem       = InputDecl | OutputDecl | RequiresClause
               | EnsuresClause | EffectsClause ;
```

### 1.11 Where Clauses and Kind Expressions

```ebnf
WhereClause    = 'where' WhereItem { ',' WhereItem } ;
WhereItem      = TypeParam ':' KindExpr
               | Predicate ;

KindExpr       = 'Type'
               | 'Nat'
               | 'Effect'
               | 'Label'
               | KindExpr '->' KindExpr ;
```

### 1.12 Effect Rows (used in function types)

```ebnf
EffectRow      = '<' [EffectElem { ',' EffectElem }
                 ['|' EffectVar]] '>'
               | 'pure'
               | EffectVar ;

EffectElem     = EffectIdent ['<' TypeArgs '>'] ;
EffectVar      = LowerLetter Ident? ;
```

---

## 2. Type System

Assura's type system combines six features into a unified framework.
Each feature is proven in isolation by prior work; the contribution is
their composition. The core calculus is based on Quantitative Type
Theory (QTT, Idris 2) extended with refinement predicates (Liquid
Haskell), effect rows (Koka), information flow labels (Jif/FlowCaml),
and typestate (Plaid).

### 2.1 Universes and Kinds

```
Kind ::= Type              -- the kind of value types
       | Nat               -- natural number indices
       | Effect            -- effect rows
       | Label             -- security labels
       | Kind -> Kind      -- higher-order kinds
```

All types live in `Type`. Natural number indices (`Nat`) are used for
dependent typing (vector lengths, matrix dimensions). `Effect` is the
kind of effect rows. `Label` is the kind of security labels.

### 2.2 Core Judgment Forms

The central typing judgment is:

```
Gamma; Delta; Sigma |- e : T ! E
```

Where:
- `Gamma` = unrestricted context (types, constants, type aliases)
- `Delta` = graded linear context (variables with usage grades)
- `Sigma` = typestate context (variables with current state)
- `e` = expression
- `T` = type (possibly refined)
- `E` = effect row

#### Graded Linear Context (Delta)

Each entry in `Delta` has the form `x :_{r} T` where `r` is a grade
from the semiring `(N+, 0, 1, +, *)` extended with `omega` (unlimited):

```
Grade ::= 0         -- erased (compile-time only)
        | 1         -- linear (use exactly once)
        | n         -- exact count (use exactly n times)
        | omega     -- unrestricted (use any number of times)
```

Grades compose:
- **Parallel use**: `r + s` (both branches of a pair use x)
- **Sequential use**: `r * s` (f uses x r times, applied to g that
  uses x s times)
- **Subgrading**: `0 <= 1 <= omega` (a linear variable can be treated
  as unrestricted is NOT allowed; the reverse is)

#### Typestate Context (Sigma)

Each entry in `Sigma` has the form `x @ S` where `S` is the current
state of `x` from a declared state machine:

```
Sigma = { x1 @ S1, x2 @ S2, ... }
```

Operations on `x` update its state in `Sigma`. The type checker rejects
any operation that is not valid for the current state.

### 2.3 Refinement Types

A refined type `{v : T | P}` pairs a base type `T` with a predicate
`P` over the value `v`. The predicate is a term in the decidable
fragment of first-order logic (QF_UFLIA: quantifier-free uninterpreted
functions + linear integer arithmetic).

#### Subtyping Rule

```
Gamma |- {v:S | P} <: {v:T | Q}
  if  S <: T
  and Gamma, v:T, P |- Q    (checked by SMT: P => Q is valid)
```

Refinement subtyping reduces to SMT validity checking. The solver
receives `P => Q` and must prove it valid (return `unsat` for `P and
not Q`).

#### Built-in Refinement Aliases

```assura
type Nat       = {v: Int  | v >= 0}
type Pos       = {v: Int  | v > 0}
type NonEmpty<T> = {v: List<T> | len(v) > 0}
type Btwn<Lo, Hi> = {v: Int | Lo <= v and v < Hi}
type Percentage = {v: Float | 0.0 <= v and v <= 1.0}
```

#### Measures

Measures bridge data structures into the logic. They are
structurally recursive functions lifted into the refinement language:

```assura
measure len : List<T> -> Nat
  len([])     = 0
  len(_ :: t) = 1 + len(t)

measure elems : List<T> -> Set<T>
  elems([])      = {}
  elems(x :: xs) = {x} ++ elems(xs)
```

Measures appear as uninterpreted functions in SMT with axioms
generated from their definitions.

### 2.4 Dependent Types

Types may depend on values. The dependent function type (Pi type) is:

```
(x : A) -> B    where B may mention x
```

Assura supports a restricted form of dependent types: types may depend
on values of kind `Nat`, `Bool`, and finite enums. Full value-level
dependency (arbitrary terms in types) is deferred to v2.

#### Examples

```assura
type Vec<T, n: Nat> = ...

fn head(v: Vec<T, n>) -> T
  where n > 0

fn append(a: Vec<T, n>, b: Vec<T, m>) -> Vec<T, n + m>

fn replicate(x: T, n: Nat) -> Vec<T, n>
```

#### Index Erasure

All `Nat`-kinded indices are erased at runtime. They exist only for
type checking. The generated Rust code has no runtime representation
of type indices.

### 2.5 Linear and Graded Types

Following Granule and Idris 2's QTT, every binding carries a usage
grade:

```assura
fn use_once(conn :_1 DbConnection) -> Result
  -- conn must be used exactly once

fn use_twice(val :_2 Token) -> (Token, Token)
  -- val is used exactly twice

fn read_only(cfg :_omega Config) -> Settings
  -- cfg can be used any number of times

fn compile_only(phantom :_0 TypeInfo) -> Unit
  -- phantom is erased at runtime
```

#### Context Splitting

When type-checking a pair `(e1, e2)`, the linear context `Delta` is
split: `Delta = Delta1 + Delta2` where each variable's grade is
partitioned between the two sub-expressions.

```
Delta1 + Delta2 = Delta
  if for each x:  r1 + r2 = r  where Delta(x) = r, Delta1(x) = r1, Delta2(x) = r2
```

#### Linear Protocol: Resource Safety

Linear types enforce resource protocols:

```assura
type File<State> where State in {Closed, Open}

fn open(path: String) -> File<Open> :_1
  effects: filesystem.read

fn read(f: File<Open> :_1) -> (File<Open> :_1, Bytes)
  effects: filesystem.read

fn close(f: File<Open> :_1) -> Unit
  effects: filesystem.write

-- COMPILE ERROR: f used twice (grade violation)
-- fn bad(f: File<Open> :_1) = (read(f), read(f))

-- COMPILE ERROR: f not used (linear variable dropped)
-- fn leak(f: File<Open> :_1) = ()
```

### 2.6 Typestate

Typestate tracks the protocol state of objects through the type system.
State changes are reflected in the type.

#### State Machine Declaration

```assura
type Order<State>
  where State in {Created, Paid, Shipped, Delivered, Cancelled}
```

#### Transition Typing

```
Gamma; Delta; Sigma, x @ S1 |- transition(x) : T ! E -| Sigma', x @ S2
```

The judgment produces an updated typestate context where `x` moves
from state `S1` to state `S2`.

#### Transition Rules

```assura
fn pay(o: Order<Created> :_1, payment: Payment)
    -> Order<Paid> :_1
  requires { payment.amount > 0 }
  effects: payment.charge

fn ship(o: Order<Paid> :_1, tracking: NonEmpty<String>)
    -> Order<Shipped> :_1
  effects: logistics.dispatch

-- COMPILE ERROR: cannot ship from Created state
-- fn bad(o: Order<Created>) = ship(o, "TRACK123")
```

#### Typestate + Linearity Interaction

Typestate variables MUST be linear (grade 1). This prevents aliasing:
if two references point to the same object, the state becomes ambiguous.
Linearity guarantees each stateful object has exactly one owner.

### 2.7 Information Flow Types

Every type carries a security label from a lattice `(L, <=)`. Data
can only flow from lower to higher labels. The default lattice is:

```
Public < Internal < Confidential < Restricted
```

#### Label Annotation

```assura
type SSN = String @Restricted
type Email = String @Internal
type PublicName = String @Public
type LogMessage = String @Public
```

#### Flow Rules

```
Gamma |- e : T @L1,  L1 <= L2
---------------------------------
Gamma |- e : T @L2                 (subsumption: can raise label)

Gamma |- e1 : T1 @L1,  Gamma |- e2 : T2 @L2
----------------------------------------------
Gamma |- (e1, e2) : (T1, T2) @(L1 join L2)    (join: pair takes max)
```

#### Declassification

Explicit declassification is required to lower a label:

```assura
fn mask_ssn(ssn: SSN) -> String @Public
  declassify { "***-**-" ++ ssn.last(4) }
```

Declassification points are tracked and auditable. The compiler emits
a warning for every `declassify` so security reviews can inspect them.

#### Purpose Labels (GDPR/Privacy)

Beyond the security lattice, data carries purpose labels:

```assura
privacy: employee.ssn purpose {payroll, tax_reporting, w2_generation}
```

The compiler checks that `ssn` is only used in functions whose
declared purpose includes one of `{payroll, tax_reporting,
w2_generation}`. Using it in a function with purpose `{analytics}`
is a compile error.

### 2.8 Totality Checking

Every function must be total unless annotated `partial`:

1. **Coverage**: Pattern matches must be exhaustive
2. **Termination**: Recursive calls must decrease on a well-founded
   measure

#### Termination Measures

```assura
fn factorial(n: Nat) -> Nat
  decreases n
{
  if n == 0 then 1
  else n * factorial(n - 1)
}
```

The compiler checks that `n - 1 < n` on each recursive call using
the specified `decreases` measure.

#### Partial Escape Hatch

```assura
partial fn server_loop() -> Never
  effects: io
{
  -- intentionally non-terminating
}
```

`partial` functions cannot be called from total functions without an
explicit `trust` annotation.

### 2.9 Type Interaction Rules

The six type features interact. These rules govern the interactions:

#### Rule 1: Refinement + Linearity

A refined linear type `{v: T @_1 | P}` is checked by splitting: the
linear context tracks usage, the refinement predicate is verified by
SMT. They do not interfere.

#### Rule 2: Typestate + Effects

A state transition must declare its effects. The effect row is checked
independently from the state transition:

```assura
fn ship(o: Order<Paid> :_1) -> Order<Shipped> :_1
  effects: database.write, logistics.dispatch
```

Both the state transition (Paid -> Shipped) AND the effect compliance
(only database.write and logistics.dispatch) are checked.

#### Rule 3: Information Flow + Effects

Effects are labeled with the minimum security level of data they may
handle:

```assura
effects: log.info @Public          -- log may only contain public data
effects: database.write @Restricted -- database may contain restricted data
```

A function that handles `Restricted` data can call `database.write`
but NOT `log.info` unless the data is declassified first.

#### Rule 4: Refinement + Dependent Types

Refinement predicates may reference dependent indices:

```assura
fn safe_index(v: Vec<T, n>, i: {x: Nat | x < n}) -> T
```

Here `n` is a dependent index and `x < n` is a refinement predicate.
The SMT solver handles the arithmetic; the type checker handles the
index propagation.

#### Rule 5: Linearity + Information Flow

Linear types and information flow labels are orthogonal. A value can
be both linear and labeled:

```assura
fn process(key: CryptoKey @Restricted :_1) -> EncryptedData
```

The key must be used exactly once (linear) and cannot flow to public
sinks (restricted).

#### Rule 6: Totality + Effects

Total functions must be pure or use only terminating effects. A
function with `effects: io` (which may block indefinitely) cannot be
total. The compiler partitions effects into terminating (pure,
database.read) and potentially non-terminating (io, network).

---

## 3. Effect System

### 3.1 Effect Algebra

Effects use row polymorphism (Koka model). An effect row is an
unordered set of effect labels with an optional row variable tail:

```
EffectRow = <l1, l2, ..., ln>       -- closed row (exactly these effects)
          | <l1, l2, ..., ln | e>   -- open row (these effects plus whatever e contains)
          | pure                     -- empty row (no effects)
```

#### Effect Labels (Built-in)

```
pure                   -- no side effects
console.read           -- read from stdin
console.write          -- write to stdout/stderr
filesystem.read        -- read files
filesystem.write       -- write files
network.connect        -- open network connections
network.send           -- send data over network
network.receive        -- receive data from network
database.read          -- read from database
database.write         -- write to database
payment.charge         -- charge a payment method
payment.refund         -- refund a payment
log.debug              -- debug logging
log.info               -- info logging
log.warn               -- warning logging
log.error              -- error logging
time.read              -- read current time
random                 -- generate random values
state<T>               -- mutable state of type T
exception<E>           -- may throw exception of type E
diverge                -- may not terminate
```

#### Custom Effects

```assura
effect audit.write<T>            -- write audit entries of type T
effect email.send                -- send emails
effect sms.send                  -- send SMS messages
effect cache.read                -- read from cache
effect cache.write               -- write to cache
```

### 3.2 Effect Polymorphism

Functions that are generic over effects use a row variable:

```assura
fn map<T, U>(list: List<T>, f: (T) -> <e> U) -> <e> List<U>
```

`map` inherits whatever effects `f` has. If `f` is pure, `map` is
pure. If `f` does IO, `map` does IO.

### 3.3 Effect Subtyping

A function with fewer effects is a subtype of a function with more:

```
<e1> <: <e1, e2>     -- fewer effects is more specific
pure <: <e>          -- pure is subtype of any effect
```

This means a pure function can be passed where an effectful function
is expected, but not the reverse.

### 3.4 Effect Handlers

Effect handlers eliminate effects from the row. A handler for
`exception<E>` transforms `<exception<E> | e>` into `<e>`:

```assura
handle result = risky_operation()
  with exception<ParseError> -> default_value
-- result has type T with effect row <e> (exception removed)
```

### 3.5 Effect Checking Rules

```
(* A function's body must only use effects declared in its signature *)

Gamma; Delta |- body : T ! E_actual
E_actual <=_row E_declared
-----------------------------------------
Gamma; Delta |- fn f() -> <E_declared> T { body }  OK

(* Effect row inclusion *)
E1 <=_row E2   iff   every label in E1 is also in E2
                 or   E2 has an open tail variable
```

### 3.6 Effect Hierarchy

Effects form a hierarchy for convenience:

```
io = {console.read, console.write, filesystem.read, filesystem.write,
      network.connect, network.send, network.receive, time.read, random}

database = {database.read, database.write}

logging = {log.debug, log.info, log.warn, log.error}
```

Using `effects: io` is shorthand for allowing all IO sub-effects.

---

## 4. Implementation IR Format

The Implementation IR is what AI generates. It is NOT human-readable.
It is a flat, fully-annotated, canonically-serialized format optimized
for Transformer attention patterns and maximum compiler checking.

### 4.1 Design Principles

1. **No variable names.** Slots are numbered (`$0`, `$1`, `$2`).
   Eliminates naming hallucination.
2. **Flat structure.** No nesting deeper than 2 levels. Each operation
   is a single instruction. Optimized for Transformer positional
   attention.
3. **Complete annotations.** Every expression has an explicit type.
   No type inference. Every function has explicit effect and linearity
   annotations.
4. **Canonical serialization.** One way to write everything. No
   stylistic variation. Enables exact diff and caching.
5. **Stable addressing.** Instructions are addressed by index, not by
   name. Enables stable error references.

### 4.2 IR Grammar

```ebnf
IRModule       = 'module' ModuleId '{' { IRDecl } '}' ;

IRDecl         = IRFunction
               | IRType
               | IRImpl ;

IRFunction     = 'fn' FnId ':' IRFnType '{'
                   { IRInstr }
                 '}' ;

IRFnType       = '(' { SlotDecl } ')' '->' IRType '!' EffectRow
                 '@' Grade
                 ['pre' ':' IRPred]
                 ['post' ':' IRPred] ;

SlotDecl       = SlotId ':' IRType '@' Grade ['@' Label] ;
SlotId         = '$' Nat ;

IRInstr        = SlotId '=' IRExpr ':' IRType ;

IRExpr         = 'const' Literal
               | 'load' SlotId
               | 'call' FnId '(' { SlotId } ')'
               | 'field' SlotId '.' FieldIndex
               | 'construct' TypeId '{' { FieldIndex '=' SlotId } '}'
               | 'match' SlotId '{' { MatchArm } '}'
               | 'if' SlotId 'then' BlockId 'else' BlockId
               | 'arith' ArithOp SlotId SlotId
               | 'cmp' CmpOp SlotId SlotId
               | 'cast' SlotId 'as' IRType
               | 'transition' SlotId 'to' StateId
               | 'declassify' SlotId 'to' Label
               | 'handle' EffectId BlockId HandlerBlock ;

MatchArm       = Pattern '=>' BlockId ;
BlockId        = '#' Nat ;
FieldIndex     = '.' Nat ;

ArithOp        = 'add' | 'sub' | 'mul' | 'div' | 'mod' ;
CmpOp          = 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge' ;

IRPred         = 'true'
               | 'false'
               | 'cmp' CmpOp SlotId SlotId
               | 'and' IRPred IRPred
               | 'or' IRPred IRPred
               | 'not' IRPred
               | 'forall' SlotId 'in' SlotId ':' IRPred
               | 'measure' MeasureId '(' SlotId ')' CmpOp Literal
               | 'call' FnId '(' { SlotId } ')' ;
```

### 4.3 IR Example

The contract:

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  effects  { pure }
}
```

Generates IR:

```
module safe_division {
  fn #0 : ($0: Int @omega, $1: Int @omega) -> Int ! pure
    pre: cmp ne $1 (const 0)
    post: cmp eq (arith add (arith mul $result $1) (arith mod $0 $1)) $0
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
```

### 4.4 Canonical Serialization Format

The IR has two serialization formats:

1. **Text format** (`.assura-ir`): Human-inspectable, used for
   debugging. The grammar above.
2. **Binary format** (`.assura-irb`): Compact, used for caching and
   transmission. MessagePack encoding with a fixed schema.

Both formats are canonicalized:
- Slots are numbered sequentially from `$0`
- Instructions are in topological order (definition before use)
- No comments, no whitespace variation
- Identical IR always produces identical bytes

### 4.5 IR Metadata

Every IR module includes metadata for tracing:

```json
{
  "source_contract": "src/contracts/division.assura",
  "source_hash": "sha256:abc123...",
  "generator": "claude-4/2026-06",
  "generation_timestamp": "2026-06-11T20:00:00Z",
  "assura_version": "0.1.0",
  "verification_status": "unverified"
}
```

---

## 5. Verification Architecture

### 5.1 Verification Layers

The compiler verifies in layers, fastest first. Each layer runs only
if all previous layers pass.

#### Layer 0: Syntactic / Structural (No SMT, < 10ms)

Checks performed purely by the parser and type checker algorithms:

| Check | Method | Time |
|---|---|---|
| Parse validity | Recursive descent parser | < 1ms |
| Name resolution | Scope analysis | < 1ms |
| Linearity / ownership | Context splitting algorithm | < 5ms |
| Basic typestate | Finite state machine DFA | < 5ms |
| Exhaustive patterns | Coverage checker | < 2ms |
| Effect set containment | Set inclusion | < 1ms |
| Scope/lifetime | Lexical region analysis | < 2ms |

No SMT solver is invoked. These checks are decidable and deterministic.

#### Layer 1: Lightweight SMT (Decidable, < 200ms)

Checks using quantifier-free decidable SMT theories:

| Check | SMT Theory | Time |
|---|---|---|
| Refinement subtyping | QF_UFLIA (int) or QF_UFLRA (float) | < 50ms |
| Null safety | QF_UFLIA (`v != null`) | < 10ms |
| Bounds checking | QF_UFLIA (`0 <= i < len`) | < 50ms |
| Information flow (finite lattice) | QF_DT (enum sort) | < 10ms |
| Grade arithmetic | QF_LIA (semiring) | < 10ms |
| Typestate with data guards | QF_DT + QF_LIA | < 20ms |
| Numerical unit checking | QF_DT (unit compatibility) | < 10ms |
| API compatibility rules | QF_DT (variance) | < 10ms |

All queries in this layer are decidable. The solver always terminates.

#### Layer 2: Heavy SMT (Undecidable, < 10s, budgeted)

Checks using quantified or nonlinear SMT theories with timeouts:

| Check | SMT Theory | Budget | Fallback |
|---|---|---|---|
| Quantified invariants | AUFLIA | 5s | Property-based test |
| Functional correctness | AUFLIA + UF | 10s | Lemma hint required |
| Termination (complex) | LIA + fuel | 5s | `decreases` annotation required |
| Serialization roundtrip | AUFLIA + DT | 5s | Runtime check |
| Complexity bounds (AARA) | QF_LIA + LP | 5s | Annotation required |
| Multi-service protocol | HORN | 5s | Bounded model check |
| Crash safety compensation | DT + LIA | 5s | Runtime check |

Each query has a timeout. If the solver times out, the compiler does
NOT reject the code. Instead:

1. It emits a warning: `W0801: Unable to verify invariant within 5s`
2. It generates a property-based test for the unverified property
3. The property is re-checked at Layer 2 if the user runs
   `assura verify --deep`

### 5.2 Z3 Encoding Strategy

#### Refinement Types -> SMT

Each refinement subtyping check `{v:S | P} <: {v:T | Q}` generates:

```smt2
; Declare the value variable
(declare-const v Int)

; Assert the antecedent (what we know)
(assert P_encoded)

; Assert the negation of the consequent (what we need to prove)
(assert (not Q_encoded))

; Check satisfiability
(check-sat)
; unsat => P implies Q => subtyping holds
; sat   => counterexample exists => type error
```

#### Measures -> Uninterpreted Functions

```assura
measure len : List<T> -> Nat
  len([])     = 0
  len(_ :: t) = 1 + len(t)
```

Encodes as:

```smt2
(declare-fun len (List) Int)
(assert (= (len nil) 0))
(assert (forall ((x T) (xs List))
  (= (len (cons x xs)) (+ 1 (len xs)))))
(assert (forall ((xs List)) (>= (len xs) 0)))
```

#### Typestate -> Enumeration Datatypes

```smt2
(declare-datatypes () ((OrderState Created Paid Shipped Delivered Cancelled Error)))

(define-fun valid_transition ((from OrderState) (action Int)) OrderState
  (ite (and (= from Created) (= action 0)) Paid        ; pay
  (ite (and (= from Paid)    (= action 1)) Shipped     ; ship
  (ite (and (= from Shipped) (= action 2)) Delivered   ; deliver
       Error))))

; Verify sequence: pay then ship
(declare-const s0 OrderState)
(declare-const s1 OrderState)
(assert (= s0 (valid_transition Created 0)))
(assert (= s1 (valid_transition s0 1)))
(assert (= s1 Error))
(check-sat)  ; unsat => sequence is valid
```

#### Information Flow -> Lattice Constraints

```smt2
(declare-datatypes () ((Label Public Internal Confidential Restricted)))

(define-fun flows ((from Label) (to Label)) Bool
  (or (= from to)
      (and (= from Public) (not (= to Public)))  ; Public flows anywhere
      (and (= from Internal) (or (= to Confidential) (= to Restricted)))
      (and (= from Confidential) (= to Restricted))))

; Check: can Restricted data flow to Public log?
(assert (flows Restricted Public))
(check-sat)  ; sat would mean it's allowed; expect unsat
```

#### Business Invariants -> Quantified Assertions

```assura
invariant { forall u in users.values(): u.email.is_valid() }
```

Encodes as:

```smt2
(declare-fun users (Int) User)
(declare-fun user_count () Int)
(declare-fun is_valid_email (String) Bool)

(assert (not
  (forall ((i Int))
    (=> (and (>= i 0) (< i user_count))
        (is_valid_email (email (users i)))))))
(check-sat)
; sat => counterexample: specific i where email is invalid
; unsat => invariant holds
```

### 5.3 Counterexample Extraction

When Z3 returns `sat` (property violated), the compiler extracts a
model (concrete values for each variable) and formats it as a
structured counterexample:

```json
{
  "error_code": "E0412",
  "category": "invariant_violation",
  "constraint": "order.total == sum(item.price * item.qty)",
  "counterexample": {
    "items": [{"price": 10, "qty": 2}, {"price": 5, "qty": 3}],
    "expected_total": 35,
    "actual_total": 0
  },
  "model_values": {
    "v": 0,
    "items.len": 2,
    "items[0].price": 10,
    "items[0].qty": 2
  }
}
```

The counterexample includes concrete inputs that violate the property,
making it actionable for AI to fix.

---

## 6. Rust Codegen Mapping

The Assura compiler generates Rust source code. This section defines
how each Assura construct maps to Rust.

### 6.1 Types

| Assura Type | Rust Type | Notes |
|---|---|---|
| `Int` | `i64` | |
| `Nat` | `u64` | Runtime bounds check on construction |
| `Float` | `f64` | |
| `Bool` | `bool` | |
| `String` | `String` | |
| `Bytes` | `Vec<u8>` | |
| `Unit` | `()` | |
| `Never` | `!` (never type) | |
| `List&lt;T&gt;` | `Vec&lt;T&gt;` | |
| `Map&lt;K, V&gt;` | `BTreeMap&lt;K, V&gt;` | Deterministic ordering |
| `Set&lt;T&gt;` | `BTreeSet&lt;T&gt;` | Deterministic ordering |
| `T?` | `Option&lt;T&gt;` | |
| `(T, U)` | `(T, U)` | |

### 6.2 Refinement Types

Refinement types erase to their base type at runtime. The predicate
is checked at construction in debug mode:

```assura
type Pos = {v: Int | v > 0}
```

Generates:

```rust
/// Assura refinement type: {v: Int | v > 0}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos(i64);

impl Pos {
    pub fn new(v: i64) -> Result<Self, AssuraRefinementError> {
        #[cfg(debug_assertions)]
        if !(v > 0) {
            return Err(AssuraRefinementError {
                type_name: "Pos",
                predicate: "v > 0",
                actual_value: format!("{}", v),
            });
        }
        Ok(Self(v))
    }

    pub fn value(&self) -> i64 { self.0 }
}
```

In release mode, the check is elided. The Assura compiler has already
proven the predicate holds at every construction site.

### 6.3 Typestate

Typestate maps to Rust phantom types + the typestate pattern:

```assura
type Order<State> where State in {Created, Paid, Shipped}
```

Generates:

```rust
pub mod order_states {
    pub struct Created;
    pub struct Paid;
    pub struct Shipped;
}

pub struct Order<State> {
    // ... fields ...
    _state: std::marker::PhantomData<State>,
}

impl Order<order_states::Created> {
    pub fn pay(self, payment: Payment) -> Order<order_states::Paid> {
        Order {
            // ... fields ...
            _state: std::marker::PhantomData,
        }
    }
}

impl Order<order_states::Paid> {
    pub fn ship(self, tracking: String) -> Order<order_states::Shipped> {
        // ...
    }
}

// pay() on Order<Shipped> does not exist -> compile error in Rust too
```

The `self` parameter consumes the old state (Rust ownership = linear).
The return type has the new state. Rust's type checker enforces the
state machine as a second safety net.

### 6.4 Effects

Effects map to Rust traits that bound what capabilities a function
receives:

```assura
fn save(order: Order) -> OrderId
  effects: database.write, log.info
```

Generates:

```rust
pub fn save<Db: DatabaseWrite, Log: LogInfo>(
    db: &mut Db,
    log: &Log,
    order: Order,
) -> OrderId {
    // implementation
}
```

Capability traits are passed explicitly. A function that declares
`effects: pure` receives no capability parameters and cannot perform
any side effects.

### 6.5 Linear Types

Linear types map to Rust ownership. A linear parameter is passed by
value (moved), ensuring single use:

```assura
fn close(f: File<Open> :_1) -> Unit
```

Generates:

```rust
pub fn close(f: File<Open>) {
    // f is moved in, cannot be used after this call
    drop(f);
}
```

Rust's move semantics naturally enforce linearity. The Assura compiler
additionally checks exact usage counts (Rust allows unused variables
with `#[allow(unused)]`; Assura does not).

### 6.6 Information Flow Labels

Labels erase at runtime. They exist only in the Assura type checker.
The generated Rust code has no label representation:

```assura
type SSN = String @Restricted
```

Generates:

```rust
pub type SSN = String;  // label erased
```

The Assura compiler has already verified that no `Restricted` data
flows to `Public` sinks. The Rust code does not need to re-check.

### 6.7 Contracts (requires/ensures)

In debug mode, contracts become runtime assertions. In release mode,
they are elided:

```assura
fn divide(a: Int, b: Int) -> Int
  requires { b != 0 }
  ensures  { result * b + (a % b) == a }
```

Generates:

```rust
pub fn divide(a: i64, b: i64) -> i64 {
    debug_assert!(b != 0, "Assura requires: b != 0");

    let result = a / b;

    debug_assert!(
        result * b + (a % b) == a,
        "Assura ensures: result * b + (a % b) == a"
    );

    result
}
```

### 6.8 Measures

Measures generate pure Rust functions used only in debug assertions:

```assura
measure len : List<T> -> Nat
```

Generates:

```rust
#[cfg(debug_assertions)]
fn assura_measure_len<T>(list: &[T]) -> u64 {
    list.len() as u64
}
```

### 6.9 Dependent Indices

Dependent indices erase completely. `Vec<T, n>` becomes `Vec<T>`:

```assura
fn append(a: Vec<T, n>, b: Vec<T, m>) -> Vec<T, n + m>
```

Generates:

```rust
pub fn append<T>(mut a: Vec<T>, b: Vec<T>) -> Vec<T> {
    a.extend(b);
    a
}
```

The index arithmetic (`n + m`) was verified at compile time. The Rust
code is a plain `Vec` with no runtime size tracking.

### 6.10 Extern and Bind

`extern` functions generate a trait that the hand-written Rust code
must implement:

```assura
extern fn render_template(name: String, data: Map<String, Value>) -> Html
  effects: filesystem.read
```

Generates:

```rust
pub trait RenderTemplateExtern {
    fn render_template(
        &self,
        name: String,
        data: BTreeMap<String, Value>,
    ) -> Html;
}
```

`bind` functions generate wrapper functions with debug assertions
around the bound Rust function:

```rust
pub fn render_page_checked(template: String, user: User) -> Html {
    debug_assert!(!template.is_empty(), "Assura requires: template.length > 0");

    let result = app::renderer::render_page(template, user.clone());

    debug_assert!(
        result.contains(&user.name),
        "Assura ensures: result.contains(user.name)"
    );
    debug_assert!(
        !result.contains(&user.password),
        "Assura ensures: not result.contains(user.password)"
    );

    result
}
```

---

## 7. Error Code Catalog

Error codes are stable identifiers for AI fix-pattern databases. The
format is `ANNSSS` where `A` = Assura, `NN` = category (2 digits),
`SSS` = specific error (3 digits).

### 7.1 Category Index

| Code Range | Category | Verification Layer |
|---|---|---|
| A01xxx | Syntax errors | 0 (parser) |
| A02xxx | Name resolution | 0 (scope) |
| A03xxx | Type mismatch | 0 (type checker) |
| A04xxx | Refinement violation | 1 (SMT) |
| A05xxx | Linearity / ownership | 0 (algorithmic) |
| A06xxx | Typestate violation | 0 (DFA) |
| A07xxx | Effect violation | 0 (set inclusion) |
| A08xxx | Information flow | 1 (SMT lattice) |
| A09xxx | Totality / termination | 0-2 |
| A10xxx | Pattern exhaustiveness | 0 (coverage) |
| A11xxx | Business invariant | 1-2 (SMT) |
| A12xxx | Concurrency | 0 (algorithmic) |
| A13xxx | Numerical precision | 0-1 |
| A14xxx | Temporal ordering | 0 (typestate) |
| A15xxx | Idempotency | 0 (linearity) |
| A16xxx | Privacy / purpose | 0-1 |
| A17xxx | Schema evolution | 0 (structural) |
| A18xxx | Crash safety | 1 (SMT) |
| A19xxx | Audit trail | 0 (effect pairing) |
| A20xxx | Serialization | 2 (SMT) |
| A21xxx | API evolution | 0 (variance) |
| A22xxx | Complexity bounds | 2 (AARA/LP) |
| A23xxx | Protocol violation | 1-2 (session types) |
| A24xxx | Observability | 0 (effect pairing) |
| A25xxx | Regulatory compliance | 1-2 (SMT) |
| A26xxx | i18n completeness | 0 (structural) |
| A27xxx | Module / import | 0 (scope) |

### 7.2 Specific Error Codes

#### Syntax (A01xxx)

| Code | Message | Cause |
|---|---|---|
| A01001 | Unexpected token | Parser error |
| A01002 | Unterminated string literal | Missing closing quote |
| A01003 | Invalid numeric literal | Malformed number |
| A01004 | Reserved keyword used as identifier | Naming conflict |
| A01005 | Mismatched braces | Unbalanced `{}` |

#### Name Resolution (A02xxx)

| Code | Message | Cause |
|---|---|---|
| A02001 | Undefined identifier `X` | Name not in scope |
| A02002 | Undefined type `X` | Type not declared |
| A02003 | Duplicate definition of `X` | Name collision |
| A02004 | Ambiguous import `X` | Multiple modules export same name |
| A02005 | Circular import | Module A imports B imports A |

#### Type Mismatch (A03xxx)

| Code | Message | Cause |
|---|---|---|
| A03001 | Expected `T1`, found `T2` | Incompatible types |
| A03002 | Type parameter count mismatch | Wrong number of generics |
| A03003 | Cannot unify `T1` with `T2` | Failed unification |
| A03004 | Missing field `F` in struct | Incomplete construction |
| A03005 | Unknown field `F` in type `T` | Field does not exist |
| A03006 | Dependent index mismatch | `Vec<T, 3>` vs `Vec<T, 5>` |

#### Refinement Violation (A04xxx)

| Code | Message | Cause |
|---|---|---|
| A04001 | Precondition may not hold | `requires` clause violated |
| A04002 | Postcondition may not hold | `ensures` clause violated |
| A04003 | Refinement subtype check failed | `{v: T \| P}` not subtype |
| A04004 | Division by zero possible | Divisor may be 0 |
| A04005 | Index out of bounds possible | Index may exceed length |
| A04006 | Arithmetic overflow possible | Result may exceed bounds |
| A04007 | Refinement timeout | SMT solver timed out |

#### Linearity (A05xxx)

| Code | Message | Cause |
|---|---|---|
| A05001 | Linear variable `X` used twice | Grade 1, used 2+ times |
| A05002 | Linear variable `X` not used | Grade 1, never consumed |
| A05003 | Grade mismatch: expected `N`, used `M` | Exact count violated |
| A05004 | Cannot copy linear value | Tried to duplicate |
| A05005 | Linear value dropped without consuming | Resource leak |

#### Typestate (A06xxx)

| Code | Message | Cause |
|---|---|---|
| A06001 | Invalid transition: `S1` -> `S2` | Not in state machine |
| A06002 | Operation requires state `S`, found `S'` | Wrong current state |
| A06003 | Object not in final state at end of scope | Protocol incomplete |
| A06004 | Ambiguous state after branch | Different states in if/else |
| A06005 | Missing transition guard | Required predicate missing |

#### Effect Violation (A07xxx)

| Code | Message | Cause |
|---|---|---|
| A07001 | Undeclared effect `E` | Effect not in function signature |
| A07002 | Pure function performs effect `E` | Side effect in pure context |
| A07003 | Effect `E` in must-not list | Explicitly forbidden effect |
| A07004 | Effect handler missing for `E` | Unhandled effect |
| A07005 | Effect hierarchy violation | Sub-effect used but parent not declared |

#### Information Flow (A08xxx)

| Code | Message | Cause |
|---|---|---|
| A08001 | Data flow violation: `L1` to `L2` | High to low flow |
| A08002 | PII leaked to logs | Restricted data in Public sink |
| A08003 | Implicit flow via branch | Secret in branch condition |
| A08004 | Purpose violation | Data used for undeclared purpose |
| A08005 | Missing declassification | Label downgrade without `declassify` |

#### Totality (A09xxx)

| Code | Message | Cause |
|---|---|---|
| A09001 | Non-exhaustive pattern match | Missing cases |
| A09002 | Recursion may not terminate | No decreasing measure |
| A09003 | Decreasing measure not well-founded | Measure does not decrease |
| A09004 | Partial function called from total context | Missing `trust` |

#### Business Invariant (A11xxx)

| Code | Message | Cause |
|---|---|---|
| A11001 | Invariant violated | SMT found counterexample |
| A11002 | Invariant not preserved by operation | Mutation breaks invariant |
| A11003 | Invariant verification timeout | SMT solver timed out |
| A11004 | Rule clause violated | Business rule not satisfied |

#### Concurrency (A12xxx)

| Code | Message | Cause |
|---|---|---|
| A12001 | Exclusive resource accessed concurrently | Data race possible |
| A12002 | Actor isolation violated | Cross-actor mutable access |
| A12003 | Shared-read resource modified | Write in shared-read context |

#### Numerical Precision (A13xxx)

| Code | Message | Cause |
|---|---|---|
| A13001 | Unit mismatch: `U1` vs `U2` | e.g., USD + EUR |
| A13002 | Dimensionally invalid operation | e.g., Money * Money |
| A13003 | Float used where fixed-point required | Precision loss |
| A13004 | Integer overflow possible | Arithmetic exceeds bounds |

#### Privacy (A16xxx)

| Code | Message | Cause |
|---|---|---|
| A16001 | Purpose violation | Data used outside declared purposes |
| A16002 | Retention policy missing | No retention declared for PII |
| A16003 | Anonymization required | Retention period expired |

#### Schema Evolution (A17xxx)

| Code | Message | Cause |
|---|---|---|
| A17001 | Breaking field removal | Required field removed |
| A17002 | Missing default for new field | Non-optional field added |
| A17003 | Type change without migration | Incompatible field type change |

#### API Evolution (A21xxx)

| Code | Message | Cause |
|---|---|---|
| A21001 | Breaking response field removal | Client may depend on field |
| A21002 | New required request field | Existing clients will fail |
| A21003 | Error variant removed | Client handlers break |

#### Complexity Bounds (A22xxx)

| Code | Message | Cause |
|---|---|---|
| A22001 | Exceeds declared complexity | O(n^2) found, O(n) declared |
| A22002 | Complexity analysis timeout | AARA solver timed out |
| A22003 | Unbounded allocation detected | No allocation bound proved |

### 7.3 Error Output Format

Every error is emitted as structured JSON:

```json
{
  "error_code": "A05001",
  "severity": "error",
  "category": "linearity",
  "message": "Linear variable `conn` used twice",
  "primary_location": {
    "file": "src/db.assura-impl",
    "line": 23,
    "col": 5,
    "instruction_index": 14
  },
  "secondary_locations": [
    {
      "file": "src/db.assura-impl",
      "line": 27,
      "col": 5,
      "instruction_index": 18,
      "label": "second use here"
    }
  ],
  "contract_reference": {
    "file": "contracts/db.assura",
    "line": 8,
    "clause": "conn :_1 DbConnection"
  },
  "suggested_fixes": [
    {
      "description": "Clone conn before second use",
      "confidence": 0.4,
      "note": "Cloning a linear resource defeats its purpose"
    },
    {
      "description": "Split operations into separate functions",
      "confidence": 0.85,
      "replacement_hint": "extract second use into a new function"
    }
  ],
  "related_errors": [],
  "documentation_url": "https://assura.dev/errors/A05001"
}
```

---

## 8. Module System

### 8.1 Module Declaration

Every `.assura` file is a module. The module name matches the file
path:

```
contracts/
  payment/
    order.assura     -> module payment.order
    refund.assura    -> module payment.refund
  user.assura        -> module user
```

### 8.2 Import Rules

```assura
import payment.order                    -- import entire module
import payment.order { Order, Item }    -- import specific names
import payment.order as po              -- aliased import
```

#### Visibility

By default, all declarations are module-private. Use `pub` to export:

```assura
pub type Order { ... }          -- exported
type InternalHelper { ... }     -- private to module

pub contract PlaceOrder { ... } -- exported
```

### 8.3 Contract Composition

Contracts can extend other contracts:

```assura
contract BaseEntity {
  ensures { self.id != "" }
  ensures { self.created_at <= self.updated_at }
}

contract Order extends BaseEntity {
  -- inherits BaseEntity's ensures clauses
  ensures { self.total >= 0 }
}
```

### 8.4 Service Composition

Services can depend on other services:

```assura
service OrderService {
  depends: PaymentService, InventoryService

  operation PlaceOrder {
    -- can reference PaymentService.charge and InventoryService.reserve
  }
}
```

### 8.5 Contract Libraries

Reusable contract patterns are published as packages:

```assura
import assura.std.crud { CrudService }
import assura.std.auth { Authenticated, Authorized }

service UserService extends CrudService<User> {
  -- inherits Create, Read, Update, Delete operations
  -- with standard contracts for each
}
```

---

## 9. Standard Library

### 9.1 Core Types

```assura
module assura.core

-- Primitive types are built-in: Int, Nat, Float, Bool, String,
-- Bytes, Unit, Never

-- Standard refinement aliases
pub type Pos       = {v: Int   | v > 0}
pub type NonNeg    = {v: Int   | v >= 0}
pub type NonZero   = {v: Int   | v != 0}
pub type Percentage = {v: Float | 0.0 <= v and v <= 1.0}

-- Constrained strings
pub type NonEmpty  = {v: String | len(v) > 0}
pub type Email     = {v: String | is_email(v)}
pub type Url       = {v: String | is_url(v)}
pub type Uuid      = {v: String | is_uuid(v)}

-- Time
pub type Timestamp = {v: Int | v > 0}    -- Unix epoch millis
pub type Duration  = {v: Int | v >= 0}   -- milliseconds
pub type Date      = { year: Int, month: Btwn<1,13>, day: Btwn<1,32> }
```

### 9.2 Collection Contracts

```assura
module assura.collections

pub measure len<T> : List<T> -> Nat
pub measure elems<T> : List<T> -> Set<T>
pub measure keys<K,V> : Map<K,V> -> Set<K>
pub measure values<K,V> : Map<K,V> -> List<V>
pub measure size<T> : Set<T> -> Nat

pub contract ListOps<T> {
  operation head {
    input(list: NonEmpty<List<T>>)
    output(result: T)
    ensures { result in elems(list) }
    effects { pure }
  }

  operation append {
    input(a: List<T>, b: List<T>)
    output(result: List<T>)
    ensures { len(result) == len(a) + len(b) }
    ensures { elems(result) == elems(a) ++ elems(b) }
    effects { pure }
  }

  operation filter {
    input(list: List<T>, pred: (T) -> Bool)
    output(result: List<T>)
    ensures { len(result) <= len(list) }
    ensures { forall x in result: x in elems(list) }
    ensures { forall x in result: pred(x) == true }
    effects { pure }
  }

  operation sort {
    input(list: List<T>)
    output(result: List<T>)
    requires { T has Ord }
    ensures { len(result) == len(list) }
    ensures { elems(result) == elems(list) }
    ensures { forall i in 0..len(result)-1:
                result[i] <= result[i+1] }
    effects { pure }
  }
}
```

### 9.3 Numerical Types

```assura
module assura.numeric

pub type FixedDecimal<Scale: Nat, Unit: Label>

pub type USD
pub type EUR
pub type GBP
pub type Percent

pub type Money<C> = FixedDecimal<2, C>

pub contract NumericOps {
  operation add<S, U> {
    input(a: FixedDecimal<S, U>, b: FixedDecimal<S, U>)
    output(result: FixedDecimal<S, U>)
    ensures { result == a + b }
    effects { pure }
  }

  operation scale<S, U> {
    input(amount: FixedDecimal<S, U>, factor: Float)
    output(result: FixedDecimal<S, U>)
    effects { pure }
  }
}
```

### 9.4 Common Contract Patterns

```assura
module assura.std.crud

pub service CrudService<Entity> {
  type Id = Uuid

  states: Draft -> Active -> Archived -> Deleted

  operation Create {
    input(data: Entity)
    output(id: Id)
    ensures { self.store.contains(id) }
    ensures { self.store[id] == data }
    effects { database.write }
  }

  operation Read {
    input(id: Id)
    output(entity: Entity)
    requires { self.store.contains(id) }
    effects { database.read }
  }

  operation Update {
    input(id: Id, data: Entity)
    requires { self.store.contains(id) }
    ensures  { self.store[id] == data }
    effects  { database.write }
  }

  operation Delete {
    input(id: Id)
    requires { self.store.contains(id) }
    ensures  { not self.store.contains(id) }
    effects  { database.write }
  }

  invariant { forall id in self.store.keys():
                self.store[id].is_valid() }
}
```

### 9.5 Auth Contracts

```assura
module assura.std.auth

pub type Role = enum { Admin, User, ReadOnly, Service }
pub type Principal = { id: Uuid, roles: Set<Role> }

pub contract Authenticated {
  requires { self.principal != null }
  requires { self.principal.is_authenticated() }
}

pub contract Authorized<R: Role> {
  extends Authenticated
  requires { R in self.principal.roles }
}
```

---

## 10. CLI Interface

### 10.1 Commands

```
assura <command> [options] [files...]

COMMANDS:
  check       Type-check contracts and implementations (layers 0-1)
  verify      Full verification including SMT (layers 0-2)
  build       Check + generate Rust code + compile
  run         Build + execute
  init        Create a new Assura project
  fmt         Format contract files
  lsp         Start the language server
  ir          Inspect generated IR
  explain     Show detailed explanation of an error code

OPTIONS (global):
  --json              Output diagnostics as JSON (default for AI)
  --human             Output diagnostics as human-readable text
  --color             Force color output
  --no-color          Disable color output
  -q, --quiet         Only output errors
  -v, --verbose       Show verification details
  --threads <N>       Number of parallel verification threads
```

### 10.2 Command Details

#### `assura check`

```
assura check [options] [files...]

Runs layers 0 and 1 (structural checks + decidable SMT).
Fast feedback loop for AI iteration.

OPTIONS:
  --layer <N>         Run only up to layer N (0, 1, or 2)
  --contract <file>   Check only this contract
  --watch             Watch for file changes and re-check
  --timeout <ms>      SMT solver timeout per query (default: 1000)
```

#### `assura verify`

```
assura verify [options] [files...]

Runs all layers including layer 2 (heavy SMT).
Used for pre-commit verification.

OPTIONS:
  --deep              Enable maximum verification depth
  --timeout <ms>      SMT solver timeout per query (default: 10000)
  --fuel <N>          Recursion unfolding depth (default: 5)
  --solver <name>     SMT solver: z3 (default), cvc5
  --stats             Show verification statistics
  --dump-smt <dir>    Write SMT queries to directory for debugging
```

#### `assura build`

```
assura build [options]

Verify + generate Rust code + compile with rustc.

OPTIONS:
  --release           Optimize generated Rust (release mode)
  --target <triple>   Rust target triple (e.g., wasm32-wasi)
  --out <dir>         Output directory (default: target/)
  --skip-verify       Skip layer 2 (only layers 0-1), for dev speed
  --keep-generated    Don't delete generated .rs files after build
```

#### `assura init`

```
assura init [name]

Create a new Assura project.

Generated structure:
  <name>/
    assura.toml          # project configuration
    contracts/
      main.assura        # initial contract file
    src/
      main.rs            # hand-written Rust entry point
    Cargo.toml           # workspace manifest
```

#### `assura explain`

```
assura explain <error_code>

Show detailed explanation of an error code with examples.

$ assura explain A05001
Error A05001: Linear variable used twice
Category: Linearity (Layer 0)

A variable with linear grade (exactly-once usage) was used more
than once. Linear types ensure resources like database connections,
file handles, and tokens are consumed exactly once.

Example:
  fn bad(conn :_1 DbConnection) -> (Result, Result) {
    (query(conn, "SELECT 1"), query(conn, "SELECT 2"))
  }           ^^^^^                     ^^^^^
  First use here                Second use here (ERROR)

Fix: Split into separate functions, each receiving its own connection.
```

### 10.3 Configuration File

`assura.toml` at the project root:

```toml
[project]
name = "my-service"
version = "0.1.0"
assura-version = "0.1"

[contracts]
path = "contracts/"

[build]
target = "native"           # or "wasm32-wasi"
generated-dir = "generated/"
keep-generated = false

[verify]
default-layer = 1           # default for `assura check`
smt-solver = "z3"
smt-timeout-ms = 5000
fuel = 5

[effects]
[effects.custom]
"audit.write" = { label = "Internal" }
"email.send"  = { label = "Public" }

[security]
labels = ["Public", "Internal", "Confidential", "Restricted"]
default-label = "Internal"
```

---

## 11. AI Agent API

The AI agent API is how AI systems interact with the Assura compiler
programmatically. It supports both gRPC and JSON-over-HTTP.

### 11.1 API Operations

#### Check

```
POST /v1/check

Request:
{
  "contracts": [
    {
      "path": "contracts/order.assura",
      "content": "service OrderService { ... }"
    }
  ],
  "implementations": [
    {
      "path": "src/order.assura-ir",
      "content": "module order { fn #0 ... }"
    }
  ],
  "options": {
    "max_layer": 2,
    "smt_timeout_ms": 5000,
    "fuel": 5
  }
}

Response:
{
  "status": "error",
  "diagnostics": [ ... ],
  "statistics": {
    "layer_0_time_ms": 12,
    "layer_1_time_ms": 87,
    "layer_2_time_ms": 2340,
    "smt_queries": 15,
    "smt_sat": 1,
    "smt_unsat": 14,
    "smt_timeout": 0
  }
}
```

#### Build

```
POST /v1/build

Request:
{
  "contracts": [ ... ],
  "implementations": [ ... ],
  "target": "wasm32-wasi",
  "release": true
}

Response:
{
  "status": "success",
  "artifacts": [
    {
      "path": "target/order.wasm",
      "size_bytes": 45230,
      "content_base64": "AGFzbQEA..."
    }
  ]
}
```

#### Explain

```
GET /v1/explain/A05001

Response:
{
  "error_code": "A05001",
  "category": "linearity",
  "layer": 0,
  "title": "Linear variable used twice",
  "description": "...",
  "examples": [ ... ],
  "fix_patterns": [ ... ]
}
```

#### Health

```
GET /v1/health

Response:
{
  "status": "healthy",
  "version": "0.1.0",
  "smt_solver": "z3",
  "smt_solver_version": "4.13.0"
}
```

### 11.2 gRPC Service Definition

```protobuf
syntax = "proto3";
package assura.v1;

service AssuraCompiler {
  rpc Check(CheckRequest) returns (CheckResponse);
  rpc Build(BuildRequest) returns (BuildResponse);
  rpc Explain(ExplainRequest) returns (ExplainResponse);
  rpc Health(HealthRequest) returns (HealthResponse);
  rpc CheckStream(stream CheckRequest) returns (stream Diagnostic);
}

message CheckRequest {
  repeated SourceFile contracts = 1;
  repeated SourceFile implementations = 2;
  CheckOptions options = 3;
}

message SourceFile {
  string path = 1;
  string content = 2;
  string hash = 3;
}

message CheckOptions {
  int32 max_layer = 1;
  int32 smt_timeout_ms = 2;
  int32 fuel = 3;
  string solver = 4;
}

message CheckResponse {
  Status status = 1;
  repeated Diagnostic diagnostics = 2;
  VerificationStats statistics = 3;
}

enum Status {
  STATUS_UNKNOWN = 0;
  STATUS_OK = 1;
  STATUS_ERROR = 2;
  STATUS_WARNING = 3;
}

message Diagnostic {
  string error_code = 1;
  Severity severity = 2;
  string category = 3;
  string message = 4;
  Location primary_location = 5;
  repeated Location secondary_locations = 6;
  ContractReference contract_reference = 7;
  Counterexample counterexample = 8;
  repeated SuggestedFix suggested_fixes = 9;
}

enum Severity {
  SEVERITY_UNKNOWN = 0;
  SEVERITY_ERROR = 1;
  SEVERITY_WARNING = 2;
  SEVERITY_INFO = 3;
  SEVERITY_HINT = 4;
}

message Location {
  string file = 1;
  int32 line = 2;
  int32 col = 3;
  int32 end_line = 4;
  int32 end_col = 5;
  string label = 6;
  string source_text = 7;
}

message ContractReference {
  string file = 1;
  int32 line = 2;
  string clause = 3;
}

message Counterexample {
  string constraint = 1;
  map<string, string> inputs = 2;
  string expected = 3;
  string actual = 4;
}

message SuggestedFix {
  string description = 1;
  float confidence = 2;
  string replacement = 3;
}

message VerificationStats {
  int32 layer_0_ms = 1;
  int32 layer_1_ms = 2;
  int32 layer_2_ms = 3;
  int32 smt_queries = 4;
  int32 smt_sat = 5;
  int32 smt_unsat = 6;
  int32 smt_timeout = 7;
}

message BuildRequest {
  repeated SourceFile contracts = 1;
  repeated SourceFile implementations = 2;
  string target = 3;
  bool release = 4;
}

message BuildResponse {
  Status status = 1;
  repeated Artifact artifacts = 2;
  CheckResponse verification = 3;
}

message Artifact {
  string path = 1;
  int64 size_bytes = 2;
  bytes content = 3;
}

message ExplainRequest {
  string error_code = 1;
}

message ExplainResponse {
  string error_code = 1;
  string category = 2;
  int32 layer = 3;
  string title = 4;
  string description = 5;
  repeated string examples = 6;
  repeated string fix_patterns = 7;
}

message HealthRequest {}

message HealthResponse {
  string status = 1;
  string version = 2;
  string smt_solver = 3;
  string smt_solver_version = 4;
}
```

### 11.3 Streaming Mode

For AI iteration loops, the `CheckStream` RPC accepts a stream of
incremental submissions. The AI sends a revised implementation, and
the compiler streams back diagnostics as they are discovered (layer 0
results first, then layer 1, then layer 2). This enables the AI to
start fixing layer 0 errors while layer 2 verification is still
running.

---

## 12. Decidability Boundaries

This section defines exactly which checks are decidable (always
terminate), semidecidable (may not terminate but have practical
mitigations), and undecidable (require timeouts).

### 12.1 Decidability Map

| Check | SMT Logic | Decidable | Layer | Budget |
|---|---|---|---|---|
| Linearity / ownership | None (algorithmic) | Yes | 0 | N/A |
| Basic typestate (finite) | None (DFA) | Yes | 0 | N/A |
| Pattern exhaustiveness | None (coverage) | Yes | 0 | N/A |
| Effect set inclusion | None (set ops) | Yes | 0 | N/A |
| Scope / lifetime | None (regions) | Yes | 0 | N/A |
| Refinement (quantifier-free) | QF_UFLIA | Yes | 1 | 1s |
| Info flow (finite lattice) | QF_DT | Yes | 1 | 1s |
| Grade arithmetic | QF_LIA | Yes | 1 | 1s |
| Unit/dimension checking | QF_DT | Yes | 1 | 1s |
| API variance | QF_DT | Yes | 1 | 1s |
| Typestate with data guards | QF_DT + QF_LIA | Yes | 1 | 1s |
| Quantified invariants | AUFLIA | **No** | 2 | 5s |
| Functional correctness | AUFLIA + UF | **No** | 2 | 10s |
| Complexity bounds | QF_LIA + LP | **Yes** | 2 | 5s |
| Termination (complex) | LIA + fuel | Semidecidable | 2 | 5s |
| Serialization roundtrip | AUFLIA + DT | **No** | 2 | 5s |
| Multi-service protocol | HORN | Semidecidable | 2 | 5s |
| Noninterference proof | FOL + quantifiers | **No** | 2 | 10s |

### 12.2 Dangerous Combinations

These combinations are known to cause solver instability:

1. **Quantified refinements + recursive measures**: Can trigger
   unbounded MBQI. Mitigation: limit fuel, use E-matching triggers.

2. **Nonlinear integer arithmetic (NIA)**: Undecidable. Avoid
   refinements with multiplication of two variables (`x * y > z`).
   Allow constant multiplication (`x * 3 > z`, which is LIA).

3. **Array theory + quantifiers**: Can cause exponential blowup.
   Mitigation: use bounded quantification (`forall i in 0..n`).

4. **Bitvector arithmetic + quantifiers**: Decidable but exponential
   in bit-width. Avoid mixing BV with quantified formulas.

### 12.3 Timeout Strategy

When the SMT solver times out:

1. **Layer 2 timeout**: Emit warning, not error. Code compiles.
   Unverified property is flagged.

2. **Layer 1 timeout**: Emit error suggesting simplified predicate.

3. **Persistent timeout**: Suggest lemma hints, property splitting,
   `trust` annotation, or property-based test fallback.

### 12.4 Trust Escape Hatch

```assura
trust "manually reviewed: hash collision resistance"
invariant { forall a, b: if hash(a) == hash(b) then a == b }
```

Trust annotations:
- Require a justification string
- Are logged in the verification report
- Are flagged in security audits
- Cannot bypass layer 0 checks (syntax, types, linearity)

### 12.5 Verification Budget Configuration

```toml
[verify.budgets]
layer_0_timeout_ms = 100
layer_1_timeout_ms = 1000
layer_2_timeout_ms = 10000
total_timeout_ms = 300000
max_fuel = 10
max_smt_queries = 1000
```

---

## 13. Type Interaction Test Cases

The six type features (refinement, dependent, linear, typestate,
effect, information flow) are each well-understood in isolation.
The hard problem is their composition. This section defines concrete
use cases that stress-test each pairwise and higher-order interaction,
identifies what can go wrong, and specifies the expected compiler
behavior.

There are C(6,2) = 15 pairwise interactions. We cover all 15, plus
4 three-way and 2 full-stack interactions, for 21 total test cases.

### Test Case 1: Refinement + Linear (Ghost Use Problem)

**Scenario**: A refinement predicate references a linear variable.

```assura
fn transfer(
    from: Account :_1,
    to: Account :_1,
    amount: {v: Money<USD> | v > 0 and v <= from.balance}
) -> (Account, Account)
  effects: database.write
```

**The problem**: The refinement `v <= from.balance` "reads" `from`.
Does this count as a use for linearity purposes? If yes, `from` is
used twice (once in the refinement, once in the body). If no, the
type checker must distinguish ghost/logical uses from computational
uses.

**Required behavior**: Refinement predicates are **ghost** (logical,
not computational). They do NOT count as a linear use. The grade of
a variable in a refinement context is always 0 (erased). The type
checker must track two usage contexts:
- `Delta_computational`: tracks runtime uses (must satisfy grades)
- `Delta_logical`: tracks refinement uses (always grade 0, unlimited)

**Expected result**: This code compiles. `from` is used once
computationally (in the body) and once logically (in the refinement).
The logical use is free.

**Test**:
```assura
-- MUST COMPILE: refinement use is ghost
fn test1(x: Int :_1, y: {v: Int | v < x}) -> Int
  effects: pure
{ x + y }

-- MUST REJECT: x used twice computationally
fn test1_bad(x: Int :_1, y: {v: Int | v < x}) -> (Int, Int)
  effects: pure
{ (x, x) }
```

---

### Test Case 2: Refinement + Typestate (Guarded Transitions)

**Scenario**: A state transition whose validity depends on a
refinement predicate over the object's data.

```assura
service LoanService {
  type Loan<State> {
    amount: Money<USD>,
    credit_score: Nat,
    approved_amount: Money<USD>?
  }
  where State in {Applied, UnderReview, Approved, Denied, Disbursed}

  fn review(loan: Loan<Applied> :_1) -> Loan<UnderReview> :_1
    effects: database.write

  fn approve(
      loan: Loan<UnderReview> :_1,
      approved: {v: Money<USD> | v > 0 and v <= loan.amount}
  ) -> Loan<Approved> :_1
    requires { loan.credit_score >= 650 }
    effects: database.write

  fn deny(loan: Loan<UnderReview> :_1) -> Loan<Denied> :_1
    effects: database.write

  fn disburse(loan: Loan<Approved> :_1) -> Loan<Disbursed> :_1
    requires { loan.approved_amount is Some }
    effects: payment.charge, database.write
}
```

**The problem**: The `approve` transition has BOTH a typestate
requirement (must be `UnderReview`) AND a refinement requirement
(`credit_score >= 650`). The type checker must verify both
independently: typestate via DFA, refinement via SMT. But what
happens when the refinement references state-dependent data?

**Interaction rule**: Typestate checking happens BEFORE refinement
checking in the same layer 0 pass. The typestate check confirms the
transition is valid. Then the refinement check confirms the data
guard. If typestate fails, refinement is not checked (no point).

**Test**:
```assura
-- MUST REJECT A06002: wrong state (Applied, need UnderReview)
fn bad1(loan: Loan<Applied> :_1) -> Loan<Approved> :_1
{ approve(loan, Money.new(1000)) }

-- MUST REJECT A04001: credit_score may be < 650
fn bad2(loan: Loan<UnderReview> :_1) -> Loan<Approved> :_1
{ approve(loan, Money.new(1000)) }

-- MUST COMPILE: both state and refinement satisfied
fn good(
    loan: Loan<UnderReview> :_1
) -> Loan<Approved> :_1
  requires { loan.credit_score >= 700 }
{ approve(loan, Money.new(loan.amount.value())) }
```

---

### Test Case 3: Refinement + Dependent (Index Arithmetic)

**Scenario**: A function that splits a vector at a refined index,
producing two vectors whose lengths must add up.

```assura
fn split_at<T>(
    v: Vec<T, n>,
    i: {x: Nat | x <= n}
) -> (Vec<T, i>, Vec<T, n - i>)
  effects: pure
  ensures { len(result.0) + len(result.1) == n }
```

**The problem**: The return type `(Vec<T, i>, Vec<T, n - i>)` uses
`i` as both a refinement-checked value AND a dependent index. The
type checker must:
1. Verify `i <= n` (refinement, SMT)
2. Compute `n - i` as a type-level index (dependent types)
3. Verify `i + (n - i) == n` (ensures clause, SMT)

**Interaction rule**: Refined values that appear in dependent
positions are first checked for their refinement predicate, then
their value is lifted into the index domain. The SMT solver sees
both the refinement constraint and the index arithmetic in the same
query.

**Test**:
```assura
-- MUST COMPILE: i is within bounds, arithmetic checks out
fn test3() -> (Vec<Int, 3>, Vec<Int, 2>)
  effects: pure
{
  let v: Vec<Int, 5> = [1, 2, 3, 4, 5]
  split_at(v, 3)
}

-- MUST REJECT A04005: index may exceed length
fn bad3<T>(v: Vec<T, n>, i: Nat) -> (Vec<T, i>, Vec<T, n - i>)
  effects: pure
{ split_at(v, i) }
  -- i has no upper bound refinement
```

---

### Test Case 4: Linear + Effect (Resource-Scoped Effects)

**Scenario**: A database transaction where the connection is linear
and effects are scoped to the connection's lifetime.

```assura
fn with_transaction<T>(
    conn: DbConnection :_1,
    body: (TxHandle :_1) -> <database.write, database.read> T
) -> T
  effects: database.write, database.read
  ensures { conn is consumed }
  ensures { transaction is committed or rolled back }
```

**The problem**: The `body` closure captures a `TxHandle` that is
linear (must be committed or rolled back exactly once). The effect
row of the closure must be a SUBSET of the effects declared by
`with_transaction`. And the linear handle must be consumed inside
the closure, not leaked out.

**Interaction rule**: When type-checking a closure:
1. The closure's effect row must be a subset of the enclosing
   function's effect row
2. Linear variables captured by the closure must be consumed within
   it (they cannot escape)
3. The closure itself may be linear (called exactly once)

**Test**:
```assura
-- MUST COMPILE: handle consumed, effects match
fn good4(conn: DbConnection :_1) -> Order
  effects: database.write, database.read
{
  with_transaction(conn, fn(tx: TxHandle :_1) -> Order {
    let order = tx.insert(new_order)    -- database.write
    tx.commit()                          -- consumes tx
    order
  })
}

-- MUST REJECT A05002: tx not consumed (never committed)
fn bad4a(conn: DbConnection :_1) -> Order
  effects: database.write, database.read
{
  with_transaction(conn, fn(tx: TxHandle :_1) -> Order {
    let order = tx.insert(new_order)
    order
    -- tx dropped without commit or rollback!
  })
}

-- MUST REJECT A07001: network effect not in closure's allowed set
fn bad4b(conn: DbConnection :_1) -> Order
  effects: database.write, database.read
{
  with_transaction(conn, fn(tx: TxHandle :_1) -> Order {
    let data = http.get("http://example.com")  -- network effect!
    tx.commit()
    data
  })
}
```

---

### Test Case 5: Typestate + Information Flow (Label Transitions)

**Scenario**: A medical record system where reviewing a record
changes its security label.

```assura
service MedicalRecords {
  type Record<State> {
    patient_name: String @Confidential,
    diagnosis: String @Restricted,
    summary: String @Internal
  }
  where State in {Draft, InReview, Approved, Published}

  -- Draft records are fully restricted
  fn submit_for_review(r: Record<Draft> :_1)
      -> Record<InReview> :_1
    effects: database.write
    -- all fields remain at their original labels

  -- Approving declassifies the summary to Public
  fn approve(r: Record<InReview> :_1)
      -> Record<Approved> :_1
    effects: database.write, audit.write
    ensures { result.summary @Public }
    declassify { r.summary to @Public }

  -- Publishing requires the summary to be public
  fn publish(r: Record<Approved> :_1)
      -> Record<Published> :_1
    requires { r.summary @Public }
    effects: database.write, network.send
}
```

**The problem**: The `approve` transition does TWO things: changes
the typestate (InReview -> Approved) AND changes the information flow
label of a field (summary from @Internal to @Public via explicit
declassification). The type checker must:
1. Track typestate transitions (DFA)
2. Track information flow labels per field
3. Verify that declassification is explicit
4. Verify that `publish` can only be called when summary is @Public

**Interaction rule**: Typestate and information flow labels are
tracked in separate contexts (Sigma for state, Lambda for labels).
A state transition may include `declassify` clauses that update the
label context. The label change is only valid inside a `declassify`
block.

**Test**:
```assura
-- MUST COMPILE: full valid pipeline
fn good5(r: Record<Draft> :_1) -> Record<Published> :_1
  effects: database.write, audit.write, network.send
{
  let r1 = submit_for_review(r)
  let r2 = approve(r1)     -- declassifies summary
  publish(r2)               -- summary is now Public, OK
}

-- MUST REJECT A08001: diagnosis is Restricted, can't go to Public
fn bad5(r: Record<Approved> :_1) -> String @Public
  effects: pure
{ r.diagnosis }

-- MUST REJECT A06001: can't publish from InReview (skip approve)
fn bad5b(r: Record<InReview> :_1) -> Record<Published> :_1
  effects: database.write, network.send
{ publish(r) }
```

---

### Test Case 6: Dependent + Effect (Sized IO)

**Scenario**: A function that reads exactly `n` bytes from a stream,
where `n` is a dependent index.

```assura
fn read_exact(
    stream: InputStream :_omega,
    n: Nat
) -> Vec<Byte, n>
  effects: io.read
  ensures { len(result) == n }

fn read_header(stream: InputStream :_omega) -> Header
  effects: io.read
{
  let magic: Vec<Byte, 4> = read_exact(stream, 4)
  requires { magic == [0x89, 0x50, 0x4E, 0x47] }  -- PNG magic

  let length_bytes: Vec<Byte, 4> = read_exact(stream, 4)
  let length: Nat = parse_u32(length_bytes)

  let data: Vec<Byte, length> = read_exact(stream, length)
  -- `length` is a runtime value used as a dependent index!

  Header { magic, length, data }
}
```

**The problem**: `length` is a runtime value obtained from IO, but
it's used as a dependent type index in `Vec<Byte, length>`. The
type checker must:
1. Accept `length` as a valid Nat index (it came from `parse_u32`,
   which returns Nat)
2. Track that `read_exact(stream, length)` returns `Vec<Byte, length>`
3. NOT try to statically verify the exact value of `length` (it's
   runtime-determined)

**Interaction rule**: Dependent indices that come from effectful
computations are treated as **abstract** at the type level. The type
checker knows `length: Nat` but not its value. Index arithmetic
(`n + m`) works abstractly. Refinement predicates on the index can
be verified only at runtime (they generate debug_assert).

**Test**:
```assura
-- MUST COMPILE: abstract index from IO
fn test6(stream: InputStream :_omega) -> Vec<Byte, n>
  effects: io.read
{
  let n: Nat = read_u32(stream)
  read_exact(stream, n)
}

-- MUST REJECT A03006: index mismatch (4 != 8)
fn bad6() -> Vec<Byte, 8>
  effects: pure
{
  let v: Vec<Byte, 4> = [1, 2, 3, 4]
  v  -- returns Vec<Byte, 4> but declared Vec<Byte, 8>
}
```

---

### Test Case 7: Linear + Information Flow (Secret Key Protocol)

**Scenario**: A cryptographic signing protocol where the private key
is both linear (use once then zeroize) AND restricted (must not leak).

```assura
fn sign_once(
    key: PrivateKey @Restricted :_1,
    message: Bytes @Public
) -> Signature @Public
  effects: crypto.sign
  ensures { verify(result, message, key.public_key()) }
{
  let sig = crypto_sign(key, message)
  -- key is consumed (linear) AND restricted (info flow)
  -- sig is Public (declassified output of restricted operation)
  zeroize(key)  -- key memory zeroed after use
  sig
}
```

**The problem**: The key is simultaneously:
- Linear (grade 1): must be consumed exactly once
- Restricted: must not flow to public sinks

The function produces a `Signature @Public` from a `@Restricted`
key. This is a declassification: the SIGNATURE is public, but the
KEY is not. The type checker must verify:
1. `key` is used exactly once (linear check)
2. `key` never flows to a public sink (info flow check)
3. The output `sig` is correctly labeled @Public (it's derived from
   the key, but the signing operation is a valid declassification)

**Interaction rule**: Linearity and information flow are orthogonal
axes. A value has both a grade AND a label. The grade tracks
how many times it's used; the label tracks where it can flow. A
`declassify` does not change the grade. Consuming a linear value
does not change its label.

**Test**:
```assura
-- MUST COMPILE: key used once, output correctly declassified
fn test7(key: PrivateKey @Restricted :_1, msg: Bytes @Public)
    -> Signature @Public
  effects: crypto.sign
{ sign_once(key, msg) }

-- MUST REJECT A05001 + A08001: key used twice AND leaked
fn bad7(key: PrivateKey @Restricted :_1) -> PrivateKey @Public
  effects: pure
{ key }  -- A08001: Restricted -> Public flow
         -- AND key is consumed by return, but if we tried to use
         -- it again, A05001 would fire

-- MUST REJECT A08002: key bytes logged
fn bad7b(key: PrivateKey @Restricted :_1) -> Unit
  effects: log.info
{
  log("Key bytes: " ++ key.to_string())  -- A08001: Restricted in Public log
  zeroize(key)
}
```

---

### Test Case 8: Typestate + Effect + Refinement (Three-Way)

**Scenario**: A payment processor where state transitions require
specific effects and are guarded by refinement predicates.

```assura
service PaymentProcessor {
  type Payment<State> {
    amount: Money<USD>,
    retries: Nat,
    last_error: String?
  }
  where State in {Pending, Charging, Charged, Failed, Refunded}

  fn charge(
      p: Payment<Pending> :_1
  ) -> Payment<Charged> :_1 | Payment<Failed> :_1
    requires { p.amount > Money.zero() }
    effects: payment.charge, log.info, database.write

  fn retry(
      p: Payment<Failed> :_1
  ) -> Payment<Pending> :_1
    requires { p.retries < 3 }
    ensures  { result.retries == p.retries + 1 }
    effects: database.write

  fn refund(
      p: Payment<Charged> :_1
  ) -> Payment<Refunded> :_1
    requires { p.amount > Money.zero() }
    effects: payment.refund, database.write, audit.write
}
```

**The problem**: Three features interact simultaneously:
1. **Typestate**: `charge` only from `Pending`, `retry` only from
   `Failed`, `refund` only from `Charged`
2. **Refinement**: `retry` only if `retries < 3` (prevents infinite
   retry loops)
3. **Effect**: `refund` requires `audit.write` (audit trail), which
   `charge` does not

The type checker must verify all three independently and then compose
the results. A retry loop must be provably bounded:

```assura
-- MUST COMPILE: bounded retry loop with all three checks
fn charge_with_retry(p: Payment<Pending> :_1)
    -> Payment<Charged> :_1 | Payment<Failed> :_1
  requires { p.retries == 0 }
  effects: payment.charge, log.info, database.write
{
  match charge(p) {
    Charged(result) => Charged(result),
    Failed(failed) => {
      if failed.retries < 3 {
        let retried = retry(failed)  -- Failed -> Pending
        charge_with_retry(retried)   -- recursive, but bounded
      } else {
        Failed(failed)
      }
    }
  }
}
```

**Interaction rule**: The type checker handles this as:
- Layer 0: typestate (DFA transitions valid), linearity (each payment
  consumed once per branch), effect (set containment)
- Layer 1: refinement (`retries < 3` guard ensures bounded recursion)
- The `decreases 3 - p.retries` annotation proves termination

---

### Test Case 9: All Six Features (Full Stack)

**Scenario**: A secure data pipeline that processes sensitive records
with exact resource management.

```assura
service SecurePipeline {
  type Record<State> {
    id: Uuid,
    data: Bytes @Restricted,
    processed_chunks: Nat,
    total_chunks: Nat
  }
  where State in {Received, Validating, Processing, Complete, Failed}

  fn process_chunk(
      record: Record<Processing> :_1,
      chunk_index: {i: Nat | i == record.processed_chunks
                          and i < record.total_chunks},
      encryption_key: AESKey @Restricted :_1
  ) -> Record<Processing> :_1
    requires { chunk_index < record.total_chunks }
    ensures  { result.processed_chunks == record.processed_chunks + 1 }
    effects: database.write, crypto.encrypt
    privacy: record.data purpose {processing, storage}
  {
    -- 1. TYPESTATE: record must be in Processing state
    -- 2. REFINEMENT: chunk_index must equal processed_chunks
    --    and be less than total_chunks
    -- 3. DEPENDENT: processed_chunks is a Nat index that
    --    tracks progress toward total_chunks
    -- 4. LINEAR: encryption_key used exactly once per chunk
    -- 5. EFFECT: only database.write and crypto.encrypt allowed
    -- 6. INFO FLOW: record.data is @Restricted, must not leak
    --    to logs or public outputs

    let encrypted = encrypt(encryption_key, record.data.chunk(chunk_index))
    -- key consumed (linear), data stays Restricted (info flow)

    let updated = record.with_processed_chunks(record.processed_chunks + 1)
    -- dependent index incremented

    updated
  }

  fn process_all(
      record: Record<Validating> :_1,
      keys: Vec<AESKey @Restricted :_1, record.total_chunks>
  ) -> Record<Complete> :_1
    requires { record.total_chunks > 0 }
    requires { len(keys) == record.total_chunks }
    ensures  { result.processed_chunks == record.total_chunks }
    effects: database.write, crypto.encrypt
    decreases record.total_chunks - record.processed_chunks
  {
    let r = transition(record, Processing)
    process_loop(r, keys, 0)
  }

  fn process_loop(
      record: Record<Processing> :_1,
      keys: Vec<AESKey @Restricted :_1, n>,
      i: {x: Nat | x <= n}
  ) -> Record<Complete> :_1
    requires { i == record.processed_chunks }
    requires { n == record.total_chunks }
    effects: database.write, crypto.encrypt
    decreases n - i
  {
    if i == record.total_chunks {
      transition(record, Complete)
    } else {
      let (key, remaining_keys) = keys.pop_first()
      -- key is linear: popped from vector, used once
      let updated = process_chunk(record, i, key)
      process_loop(updated, remaining_keys, i + 1)
    }
  }
}
```

**What this tests simultaneously**:

| Feature | What's Tested |
|---|---|
| **Refinement** | `chunk_index == record.processed_chunks` and `i < total_chunks` |
| **Dependent** | `Vec<AESKey, record.total_chunks>` (vector length = total chunks) |
| **Linear** | Each `AESKey` consumed exactly once, `record` consumed per iteration |
| **Typestate** | `Validating -> Processing -> Complete` transitions |
| **Effect** | Only `database.write` + `crypto.encrypt`, no logging of data |
| **Info flow** | `record.data @Restricted` never reaches public sinks |

**What can go wrong**:

1. The SMT query for the loop invariant (`i == processed_chunks`
   AND `i < total_chunks` AND `remaining_keys.len == total_chunks - i`)
   involves quantified arithmetic over dependent indices. This could
   time out.

2. The linear key vector `Vec<AESKey :_1, n>` means each element is
   linear. Popping an element consumes it from the vector. The type
   checker must track that the vector's length decreases by 1 each
   iteration AND that the popped key is consumed.

3. The `decreases n - i` termination measure involves both a
   refinement value (`i`) and a dependent index (`n`). The type
   checker must unify these two systems to verify termination.

---

### Test Case 10: Conditional Typestate (Branch Divergence)

**Scenario**: An operation that may transition to different states
depending on runtime data.

```assura
fn process_order(order: Order<Paid> :_1, inventory: Inventory)
    -> Order<Shipped> :_1 | Order<Cancelled> :_1
  effects: database.write, logistics.dispatch
{
  if inventory.has_stock(order.items) {
    let tracking = logistics.create_shipment(order)
    ship(order, tracking)  -- Paid -> Shipped
  } else {
    cancel(order, "Out of stock")  -- Paid -> Cancelled
  }
}
```

**The problem**: After the `if/else`, the order is in DIFFERENT
states in each branch. The return type must be a union
`Order<Shipped> | Order<Cancelled>`. The type checker must:
1. Track each branch independently
2. Join the typestate contexts
3. Verify the caller handles both possible states

**Test**:
```assura
-- MUST COMPILE: caller handles both branches
fn handle(order: Order<Paid> :_1, inv: Inventory) -> String
  effects: database.write, logistics.dispatch, email.send
{
  match process_order(order, inv) {
    Shipped(o) => {
      send_tracking_email(o)
      "Shipped"
    }
    Cancelled(o) => {
      send_cancellation_email(o)
      "Cancelled"
    }
  }
}

-- MUST REJECT A10001: non-exhaustive match (missing Cancelled)
fn bad10(order: Order<Paid> :_1, inv: Inventory) -> String
  effects: database.write, logistics.dispatch
{
  match process_order(order, inv) {
    Shipped(o) => "Shipped"
    -- Missing Cancelled case!
  }
}
```

---

### Test Case 11: Effect + Information Flow (Labeled Effects)

**Scenario**: A logging system where the log effect has a security
label, preventing sensitive data from being logged.

```assura
fn process_user(user: User) -> ProcessingResult
  effects: database.write, log.info @Public
{
  log.info("Processing user: " ++ user.id)          -- OK: id is Public
  log.info("User email: " ++ user.email)             -- A08002: email is @Internal
  log.info("User SSN: " ++ user.ssn)                 -- A08002: ssn is @Restricted

  let result = compute(user)
  log.info("Result: " ++ result.summary)              -- OK if summary is @Public
  result
}
```

**The problem**: The `log.info` effect is annotated `@Public`,
meaning it can only receive data labeled `@Public` or lower. When a
`@Restricted` value (`user.ssn`) is passed to a `@Public` effect,
the compiler must reject it.

**Interaction rule**: Each effect in the effect row carries a maximum
security label. Data flowing into an effect must have a label at or
below the effect's label. This is checked by combining the effect
row check (layer 0) with the information flow check (layer 1).

---

### Summary: Interaction Matrix

| # | Features Combined | What Breaks If Wrong |
|---|---|---|
| 1 | Refinement + Linear | Ghost use counted as computational use |
| 2 | Refinement + Typestate | State guard not verified before transition |
| 3 | Refinement + Dependent | Index arithmetic with refined bounds |
| 4 | Linear + Effect | Resource leaked in effectful closure |
| 5 | Typestate + Info Flow | Declassification tied to state transition |
| 6 | Dependent + Effect | Runtime-determined index from IO |
| 7 | Linear + Info Flow | Secret key used and leaked |
| 8 | Typestate + Effect + Refinement | Bounded retry with state, effects, and guards |
| 9 | **All six** | Secure pipeline loop with everything |
| 10 | Typestate + Pattern | Branch divergence in state |
| 11 | Effect + Info Flow | Labeled effects block sensitive data |

### Implementation Priority

For the compiler prototype, implement interactions in this order:

1. **Linear + Typestate** (easiest: typestate requires linearity)
2. **Linear + Effect** (common: resource-scoped effects)
3. **Refinement + Dependent** (core value: index safety)
4. **Effect + Info Flow** (core value: preventing PII leaks)
5. **Refinement + Linear** (ghost use rule)
6. **Typestate + Refinement** (guarded transitions)
7. **Dependent + Effect** (abstract indices from IO)
8. **Typestate + Info Flow** (label transitions)
9. **Three-way combinations** (after pairwise is solid)
10. **Full stack** (integration test, last)

Each test case above serves as both a specification and a regression
test. When the type checker handles all 11 cases correctly, the
interaction problem is solved.

---

## 14. Verification Categories

The base language (Sections 1-13) handles application-level
services. These categories add domain-specific verification
capabilities organized by concern. A project selects which
categories to activate via the profile system (Section 1.2).

CORE is always active. All others are opt-in.

### 14.CORE: Verification Infrastructure

Always active. Cannot be excluded from any profile.

#### CORE.1 Ghost Code

Variables, functions, and blocks that exist only for verification.
They are type-checked, verified by SMT, but completely erased from
the generated Rust code. Zero runtime cost.

##### Motivation

Every mature verification tool has ghost code: SPARK Ada (`Ghost`
aspect), Dafny (`ghost var`, `ghost method`), Verus (`proof fn`,
`tracked`), Creusot (`#[logic]`). The reason is fundamental:
verification often requires tracking state that the runtime code
does not need.

Examples from other Assura features:
- Monotonic state (STOR.5): needs to remember the previous value
  to verify `new >= old`, but the runtime only needs the current
  value
- Bit-level format (FMT.2): the bit cursor position is a
  verification concept; the runtime tracks it implicitly via
  byte_pos + bit_offset
- Multi-pass refinement (TEST.3): quality measurement exists only
  to prove convergence, not for runtime behavior
- Page cache (STOR.2): pin count invariants need logical tracking
  of which pages are pinned by whom

Without ghost code, these verification-only values must be
embedded in the runtime code behind `#[cfg(debug_assertions)]`,
which conflates debugging and verification.

##### Grammar

```ebnf
GhostDecl   = 'ghost' GhostItem ;

GhostItem   = 'var' Ident ':' TypeExpr ['=' Expr]
            | 'fn' FnDecl
            | 'type' TypeDecl
            | '{' StmtList '}' ;

GhostAttr   = '#[ghost]' ;
```

##### Full Example

```assura
type SortedList<T: Ord> {
  data: Vec<T>,

  // Ghost: the abstract sequence for verification
  ghost var elements: Sequence<T>,

  invariant {
    // Runtime data and ghost sequence are in sync
    data.len() == elements.len(),
    for_all(i in 0..data.len(), data[i] == elements[i]),
    // Sortedness expressed on the ghost sequence
    for_all(i in 0..elements.len()-1,
      elements[i] <= elements[i+1]
    )
  }
}

fn insert(list: &mut SortedList<T>, value: T)
  ensures {
    // Ghost sequence has the new element
    list.elements == old(list.elements).insert_sorted(value)
  }
{
  let pos = list.data.binary_search(&value)
    .unwrap_or_else(|p| p)
  list.data.insert(pos, value)

  // Ghost: update the logical sequence
  ghost { list.elements = list.elements.insert_sorted(value) }
}

// Ghost function: pure mathematical definition
ghost fn insert_sorted(seq: Sequence<T>, val: T) -> Sequence<T>
  ensures {
    result.len() == seq.len() + 1,
    result.contains(val),
    is_sorted(result)
  }
{
  // This body is verified but never compiled
  let pos = seq.find_insertion_point(val)
  seq.insert_at(pos, val)
}

// Using ghost blocks for verification state
fn binary_search(arr: &[I32], target: I32) -> Option<U32>
  requires { is_sorted(arr) }
  ensures {
    match result {
      Some(idx) => arr[idx] == target,
      None => for_all(i in 0..arr.len(), arr[i] != target)
    }
  }
{
  let mut lo: U32 = 0
  let mut hi: U32 = arr.len() as U32

  while lo < hi
    invariant {
      lo <= hi && hi <= arr.len() as U32,
      for_all(i in 0..lo, arr[i] < target),
      for_all(i in hi..arr.len() as U32, arr[i] > target)
    }
  {
    let mid = lo + (hi - lo) / 2
    if arr[mid] < target {
      lo = mid + 1
    } else if arr[mid] > target {
      hi = mid
    } else {
      return Some(mid)
    }

    // Ghost: track iteration for termination
    ghost {
      assert hi - lo < old(hi) - old(lo)
        // proves the loop terminates
    }
  }
  None
}
```

##### Verification Rule

1. **Erasure guarantee**: Ghost code cannot affect runtime behavior.
   Ghost variables cannot appear in non-ghost expressions. Ghost
   functions cannot be called from non-ghost code. Violations
   produce A54001
2. **Type checking**: Ghost code is fully type-checked using the
   same rules as runtime code
3. **SMT inclusion**: Ghost assertions and invariants are included
   in the SMT query. Ghost function bodies are available for
   inlining by the solver
4. **Ghost purity**: Ghost functions must be pure (no effects).
   Ghost variables can only be modified inside ghost blocks

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A54001 | Ghost variable `V` used in non-ghost context | Runtime code depends on verification-only state |
| A54002 | Ghost function `F` called from non-ghost code | Runtime code calls verification-only function |
| A54003 | Ghost block has side effects | Ghost code must be pure |
| A54004 | Ghost variable not updated to match runtime state | Invariant links ghost and runtime but ghost lags |
| A54005 | Ghost type used in runtime signature | Function parameter or return uses ghost type |

##### Rust Codegen

Ghost code is completely erased:

```rust
// Assura:
//   ghost var elements: Sequence<T>
//   ghost { elements = elements.insert_sorted(value) }
//
// Rust: (nothing generated)

// Ghost assertions become debug_assert in debug mode only
#[cfg(debug_assertions)]
{
    debug_assert!(is_sorted(&list.data));
}
```


#### CORE.2 Lemmas and Proof Functions

Functions that prove properties but generate no runtime code.
They exist to help the SMT solver establish facts that are
needed by other contracts.

##### Motivation

SMT solvers are powerful but not omniscient. Sometimes the solver
cannot prove a property directly but can verify it if given an
intermediate step. Lemmas provide those steps.

Examples:
- Proving a sort is correct requires transitivity of comparison
- Proving a hash table has no collisions requires properties of
  the hash function
- Proving a balanced tree stays balanced after rotation requires
  height relationships

Dafny has `lemma`, Verus has `proof fn`, Lean has `theorem`.
Without lemmas, Assura must either hope the SMT solver can prove
everything automatically (it often cannot) or encode hints in
awkward ways.

##### Grammar

```ebnf
LemmaDecl   = 'lemma' Ident ['<' TypeParams '>']
              '(' ParamList ')'
              RequiresClause
              EnsuresClause
              [LemmaBody] ;

LemmaBody   = '{' { LemmaStep } '}' ;

LemmaStep   = 'assert' Predicate
            | 'apply' LemmaIdent '(' ArgList ')'
            | 'induction' Ident
            | 'cases' Ident '{' { CaseArm } '}' ;
```

##### Full Example

```assura
// Lemma: if a list is sorted and we insert in the right
// position, the result is still sorted
lemma insert_preserves_sorted<T: Ord>(
    seq: Sequence<T>,
    val: T,
    pos: U32
)
  requires {
    is_sorted(seq),
    pos <= seq.len(),
    pos == 0 || seq[pos - 1] <= val,
    pos == seq.len() || val <= seq[pos]
  }
  ensures {
    is_sorted(seq.insert_at(pos, val))
  }
{
  // Proof by cases on the elements around the insertion point
  assert for_all(i in 0..pos,
    seq[i] <= val  // all before pos are <= val
  )
  assert for_all(i in pos..seq.len(),
    val <= seq[i]  // all after pos are >= val
  )
  // SMT can now derive sortedness of the result
}

// Lemma: transitivity of comparison (helps sort proofs)
lemma comparison_transitive<T: Ord>(a: T, b: T, c: T)
  requires { a <= b && b <= c }
  ensures { a <= c }
  // No body needed: SMT handles this directly

// Lemma: CRC32 distributes over concatenation
// (needed for chunked integrity verification)
lemma crc32_concat(data_a: &[U8], data_b: &[U8])
  requires { true }
  ensures {
    crc32(concat(data_a, data_b)) ==
      crc32_combine(crc32(data_a), crc32(data_b), data_b.len())
  }
{
  induction data_b
  // Base case: data_b is empty
  //   crc32(concat(a, [])) == crc32(a) == crc32_combine(crc32(a), 0, 0)
  // Inductive step: data_b = [head] ++ tail
  //   apply crc32_concat(data_a, tail)
  //   assert crc32(concat(a, [head]++tail)) == ...
}

// Using a lemma in a function contract
fn merge_sorted(a: &[I32], b: &[I32]) -> Vec<I32>
  requires { is_sorted(a) && is_sorted(b) }
  ensures { is_sorted(result) && result.len() == a.len() + b.len() }
{
  // ... merge logic ...

  // Invoke lemma to help the solver
  apply insert_preserves_sorted(partial_result, next_val, insert_pos)
}
```

##### Verification Rule

1. **No runtime effect**: Lemmas generate no code. They exist
   only as proof obligations for the SMT solver
2. **Must be valid**: The solver must be able to verify the lemma
   itself (the ensures follows from the requires + body)
3. **Can be applied**: `apply lemma_name(args)` adds the lemma's
   ensures clause as an assumption at that point in the proof
4. **Induction support**: `induction var` generates the base case
   and inductive step for structural induction on `var`

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A55001 | Lemma `L` could not be verified | SMT cannot prove ensures from requires |
| A55002 | Lemma applied with unsatisfied preconditions | `apply` used where requires not met |
| A55003 | Induction variable not inductively defined | `induction` on non-recursive type |
| A55004 | Lemma has side effects | Lemmas must be pure |
| A55005 | Circular lemma dependency | Lemma A depends on B which depends on A |

##### Rust Codegen

Lemmas are completely erased. No runtime code is generated.

```rust
// Assura:
//   lemma insert_preserves_sorted(...) { ... }
//   apply insert_preserves_sorted(partial, val, pos)
//
// Rust: (nothing generated for either declaration or application)
```


#### CORE.3 Frame Conditions

Explicit declarations of which state a function modifies,
enabling the verifier to prove that everything else is unchanged.

##### Motivation

Assura's effects system (Section 3) tracks what kind of side
effects occur (filesystem, database, network). But it does not
track WHICH specific variables or fields change. Frame conditions
fill this gap.

Without frame conditions, the verifier must assume a function
call could modify anything. This makes modular verification
impossible: proving a property about variable X is useless if
the next function call might silently change X.

SPARK Ada has `Global` and `Depends` aspects. Frama-C has
`assigns` clauses. Dafny has `modifies` clauses. All of them
learned that frame conditions are essential for scalable
verification.

##### Grammar

```ebnf
FrameClause   = 'modifies' '{' ModifiesTarget
                  { ',' ModifiesTarget } '}'
              | 'reads' '{' ReadsTarget
                  { ',' ReadsTarget } '}' ;

ModifiesTarget = Ident
               | Ident '.' FieldIdent
               | Ident '[' Expr ']'
               | '*' Ident  // all fields of the object ;

ReadsTarget    = Ident
               | Ident '.' FieldIdent
               | '*' Ident ;
```

##### Full Example

```assura
type BTreeNode {
  keys: Vec<I64>,
  children: Vec<BTreeNode>,
  count: U32,
  parent: Option<&BTreeNode>,
}

// Frame condition: only modifies the node's keys and count
fn insert_key(
    node: &mut BTreeNode,
    key: I64,
    pos: U32
)
  modifies { node.keys, node.count }
  // Implicit: node.children and node.parent are UNCHANGED
  requires { pos <= node.count }
  ensures {
    node.count == old(node.count) + 1,
    node.keys[pos] == key,
    // children unchanged (guaranteed by frame condition)
    node.children == old(node.children)
  }
{
  node.keys.insert(pos as usize, key)
  node.count += 1
}

// Frame condition on a struct field
type PageCache {
  pages: HashMap<U32, Page>,
  dirty_set: HashSet<U32>,
  stats: CacheStats,
}

fn mark_dirty(cache: &mut PageCache, page_id: U32)
  modifies { cache.dirty_set, cache.stats }
  // pages themselves are NOT modified
  requires { cache.pages.contains_key(page_id) }
  ensures {
    cache.dirty_set.contains(page_id),
    cache.pages == old(cache.pages)  // frame: pages unchanged
  }
{
  cache.dirty_set.insert(page_id)
  cache.stats.dirty_count += 1
}

// reads clause: function only reads these fields
fn lookup(cache: &PageCache, page_id: U32) -> Option<&Page>
  reads { cache.pages }
  // Does not read dirty_set or stats
  ensures {
    result.is_some() == cache.pages.contains_key(page_id)
  }
{
  cache.pages.get(&page_id)
}
```

##### Verification Rule

1. **Write restriction**: If `modifies { a, b }` is declared,
   the function body can only assign to `a` and `b`. Writing to
   any other state is a compile error
2. **Frame inference**: For any variable NOT in the `modifies`
   set, the verifier adds an implicit `ensures { x == old(x) }`
3. **Callee frames**: When a function with `modifies { x }` calls
   another function with `modifies { y }`, the verifier checks
   that `y` is a subset of `x` or that `y` is local state
4. **Read restriction**: If `reads { a }` is declared, the
   function body can only read from `a`. This enables the verifier
   to skip tracking changes to other fields

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A56001 | Function modifies `X` not in modifies clause | Write to undeclared target |
| A56002 | Called function modifies `X` outside caller's frame | Callee escapes caller's modifies set |
| A56003 | Function reads `X` not in reads clause | Read from undeclared source |
| A56004 | Modifies clause on pure function | Pure functions cannot have modifies |
| A56005 | Frame condition conflict with effects | modifies contradicts effects declaration |

##### Rust Codegen

Frame conditions are erased (compile-time only). In debug mode,
they generate field-level change tracking:

```rust
#[cfg(debug_assertions)]
{
    let old_children = node.children.clone();
    // ... function body ...
    debug_assert_eq!(node.children, old_children,
        "frame violation: children modified");
}
```


#### CORE.4 Axiomatic Definitions

Abstract mathematical definitions that the verifier uses for
reasoning without requiring an implementation. They define
WHAT something means, not HOW to compute it.

##### Motivation

Some concepts are easier to define mathematically than to
implement:
- "A sequence is sorted" is a one-line mathematical definition
  but a multi-line runtime check
- "Two sets are disjoint" is trivial in math but requires
  iteration at runtime
- "A tree is balanced" has a clean recursive definition but a
  complex imperative check

Axioms let the verifier reason about these concepts directly
without needing executable code. Frama-C has `axiomatic` blocks.
Dafny has `predicate` and `function` (which can be ghost).
Why3 has `axiom`.

##### Grammar

```ebnf
AxiomDecl     = 'axiom' Ident ['<' TypeParams '>']
                '(' ParamList ')' ':' TypeExpr
                '=' AxiomBody ;

AxiomBody     = Predicate
              | '{' { AxiomClause } '}' ;

AxiomClause   = 'define' ':' Predicate
              | 'property' ':' Predicate ;
```

##### Full Example

```assura
// Axiom: what "sorted" means
axiom is_sorted<T: Ord>(seq: Sequence<T>) : Bool =
  for_all(i in 0..seq.len()-1, seq[i] <= seq[i+1])

// Axiom: what "permutation" means
axiom is_permutation<T>(a: Sequence<T>, b: Sequence<T>) : Bool = {
  define: a.len() == b.len()
       && for_all(x: T, a.count(x) == b.count(x))
}

// Axiom: what a valid B-tree is
axiom is_valid_btree(node: BTreeNode, order: U32) : Bool = {
  define:
    // Keys are sorted within each node
    is_sorted(node.keys)
    // Number of children = number of keys + 1 (for internal nodes)
    && (node.is_leaf() || node.children.len() == node.keys.len() + 1)
    // All keys in left subtree < key < all keys in right subtree
    && for_all(i in 0..node.keys.len(),
         (i == 0 || all_less(node.children[i], node.keys[i]))
         && (i == node.keys.len() - 1
             || all_greater(node.children[i+1], node.keys[i]))
       )
    // Recursively valid
    && for_all(child in node.children, is_valid_btree(child, order))

  property:
    // All leaves are at the same depth
    for_all(leaf_a in leaves(node), leaf_b in leaves(node),
      depth(leaf_a) == depth(leaf_b)
    )
}

// Using axioms in contracts
fn sort(data: &mut [I32])
  ensures {
    is_sorted(result),
    is_permutation(old(data).to_seq(), result.to_seq())
  }
{
  // ... sorting implementation ...
}

fn btree_insert(tree: &mut BTree, key: I64)
  requires { is_valid_btree(tree.root, tree.order) }
  ensures { is_valid_btree(tree.root, tree.order) }
{
  // ... insertion logic ...
}
```

##### Verification Rule

1. **No runtime evaluation**: Axioms are never executed. They
   exist only in the SMT domain
2. **Consistency**: The verifier checks that axiom definitions
   are not contradictory (e.g., `axiom absurd() : Bool = true && false`)
3. **Totality**: Recursive axioms must be well-founded (they must
   terminate on all inputs). The verifier checks structural
   decrease or explicit fuel bounds
4. **Properties**: `property` clauses are additional facts the
   verifier must prove follow from the `define` clause

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A57001 | Axiom `A` is inconsistent | Definition is self-contradictory |
| A57002 | Recursive axiom not well-founded | No structural decrease in recursion |
| A57003 | Axiom property does not follow from definition | Property clause not provable |
| A57004 | Axiom used at runtime | Axiom referenced in non-ghost, non-contract context |
| A57005 | Conflicting axiom definitions | Two axioms define the same concept differently |

##### Rust Codegen

Axioms are completely erased. In debug mode, simple axioms
generate runtime checks:

```rust
// Simple axiom: can generate a runtime check
#[cfg(debug_assertions)]
fn debug_is_sorted(seq: &[i32]) -> bool {
    seq.windows(2).all(|w| w[0] <= w[1])
}

// Complex axiom (recursive, quantified): no runtime check possible
// Verified purely at compile time via SMT
```


#### CORE.5 Quantifier Triggers

Hints that tell the SMT solver how to instantiate universal
quantifiers. Without them, the solver either times out or
explores an exponential search space.

##### Motivation

Universal quantifiers (`for_all`) are the most common source of
SMT solver timeouts. When the solver sees `for_all(x: T, P(x))`,
it must decide which concrete values of `x` to try. Without
guidance, it may try none (proof fails) or too many (timeout).

Verus and Dafny both have explicit trigger syntax because they
learned this is practically essential. Triggers tell the solver:
"when you see an expression matching this pattern, instantiate
the quantifier with the matching value."

This is an implementation concern, not a mathematical one. But
without it, real verification systems hit timeout walls on
moderate-size programs.

##### Grammar

```ebnf
TriggerClause  = '#[trigger' '(' TriggerPattern
                   { ',' TriggerPattern } ')' ']' ;

TriggerPattern = Expr ;  // expression pattern the solver matches

AutoTrigger    = '#[auto_trigger]' ;  // let the solver choose
```

##### Full Example

```assura
// Without trigger: solver may time out on large arrays
fn lookup_correct(table: &HashMap<K, V>, key: K) -> Option<V>
  ensures {
    // Trigger: when the solver sees table.get(k) for any k,
    // it should instantiate this quantifier with that k
    #[trigger(table.get(x))]
    for_all(x: K,
      result == table.get(key)
    )
  }

// Sorted array: trigger on array access
fn binary_search_spec(arr: &[I32], target: I32) -> Option<U32>
  requires {
    // Trigger: when the solver sees arr[i], instantiate with
    // that i
    #[trigger(arr[i])]
    for_all(i in 0..arr.len()-1, arr[i] <= arr[i+1])
  }

// Multiple triggers: instantiate when EITHER pattern matches
lemma map_preserves_length<T, U>(
    seq: Sequence<T>,
    f: fn(T) -> U
)
  ensures {
    #[trigger(seq[i], mapped[i])]
    for_all(i in 0..seq.len(),
      mapped[i] == f(seq[i])
    )
    && mapped.len() == seq.len()
  }

// Auto-trigger: let the solver choose (simpler but less reliable)
fn contains_all(subset: &[I32], superset: &[I32]) -> Bool
  ensures {
    #[auto_trigger]
    for_all(x in subset, superset.contains(x))
  }
```

##### Verification Rule

1. **Trigger validity**: Trigger patterns must mention the bound
   variable of the quantifier. A trigger that does not reference
   the quantified variable is useless (A58001)
2. **Trigger coverage**: The trigger must be specific enough to
   avoid matching loops (where instantiation creates new matches).
   The verifier warns on potential matching loops (A58002)
3. **Auto-trigger fallback**: If no trigger is specified and
   `#[auto_trigger]` is not present, the verifier uses Z3/CVC5's
   built-in heuristic but warns that explicit triggers are
   recommended for reliability
4. **Multi-trigger**: Multiple trigger patterns create a
   conjunction: the quantifier is instantiated only when ALL
   patterns match simultaneously

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A58001 | Trigger does not mention bound variable `V` | Useless trigger pattern |
| A58002 | Potential matching loop in trigger | Trigger may cause infinite instantiation |
| A58003 | Quantifier timeout (no trigger specified) | Solver timed out; add explicit trigger |
| A58004 | Conflicting triggers on same quantifier | Multiple trigger annotations conflict |
| A58005 | Trigger pattern not found in formula | Trigger references expression not in quantifier body |

##### Rust Codegen

Triggers are erased (they are SMT directives, not runtime code).

```rust
// Triggers affect only the verification pass.
// No Rust code is generated for trigger annotations.
```


#### CORE.6 Opaque Functions

Functions whose implementation is hidden from the verifier.
The solver reasons only about the function's contract (requires/
ensures), not its body. This prevents the solver from unfolding
complex or recursive definitions and timing out.

##### Motivation

When the verifier encounters a function call, it can either:
1. **Inline** the function body (precise but expensive)
2. **Use only the contract** (fast but requires good contracts)

By default, the solver tries to inline, which causes problems:
- Recursive functions cause infinite unfolding
- Complex functions cause exponential blowup
- Implementation details leak into callers' proofs, creating
  fragile verification that breaks when internals change

Dafny has `opaque` functions. Verus has `open`/`closed` specs.
SPARK Ada uses `Refined_Post` to separate interface from body
contracts. The pattern is universal: hide what you do not need
to expose.

##### Grammar

```ebnf
OpaqueAttr    = '#[opaque]' ;

RevealStmt    = 'reveal' FnIdent ;
              // temporarily expose the body at this proof point
```

##### Full Example

```assura
// Opaque: verifier sees only the contract, not the body
#[opaque]
fn fibonacci(n: U32) -> U64
  requires { n <= 93 }  // fits in u64
  ensures {
    n == 0 => result == 0,
    n == 1 => result == 1,
    n >= 2 => result == fibonacci(n - 1) + fibonacci(n - 2)
  }
{
  // Iterative implementation (efficient)
  // Verifier does NOT see this; it uses the ensures clause
  let mut a: U64 = 0
  let mut b: U64 = 1
  for _ in 0..n {
    let temp = a + b
    a = b
    b = temp
  }
  a
}

// Caller: verifier uses only fibonacci's contract
fn fib_is_monotonic(a: U32, b: U32) -> Bool
  requires { a <= b && b <= 93 }
  ensures { fibonacci(a) <= fibonacci(b) }
{
  // Cannot prove this from contract alone; reveal the body
  reveal fibonacci
  // Now the verifier can unfold fibonacci's definition
  // (with fuel bounds to prevent infinite recursion)

  // ... inductive proof ...
  true
}

// Opaque type: hide internal representation
#[opaque]
type BloomFilter {
  bits: Vec<U64>,
  hash_count: U8,
  // Verifier cannot see these fields from outside
}

// Public contract: does not expose internal representation
fn bloom_insert(filter: &mut BloomFilter, item: &[U8])
  ensures { bloom_may_contain(filter, item) == true }

#[opaque]
fn bloom_may_contain(filter: &BloomFilter, item: &[U8]) -> Bool
  ensures {
    // False positives possible, false negatives impossible
    // (this is the ONLY fact callers can rely on)
  }
```

##### Verification Rule

1. **Body hidden**: When verifying callers of an opaque function,
   the solver uses only the requires/ensures contract. The function
   body is not available for inlining
2. **Body verified separately**: The opaque function itself is
   verified: its body must satisfy its ensures clause. This
   happens once, not at every call site
3. **Reveal**: `reveal fn_name` temporarily exposes the function
   body at a specific proof point. The verifier adds fuel bounds
   to prevent infinite unfolding of recursive functions
4. **Modular verification**: Opaque functions enable modular
   verification. Changing the body of an opaque function does not
   require re-verifying callers (as long as the contract is
   unchanged)

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A59001 | Cannot prove property: function `F` is opaque | Caller needs body but function is hidden; use `reveal` |
| A59002 | Reveal of non-opaque function | `reveal` on a transparent function (no-op warning) |
| A59003 | Opaque function contract insufficient | Body satisfies a property the contract does not expose |
| A59004 | Recursive reveal exceeded fuel | `reveal` on recursive function hit unfolding limit |
| A59005 | Opaque type field accessed externally | Code outside the module accesses hidden field |

##### Rust Codegen

Opacity is a verification concept only. The generated Rust code
is identical whether the function is opaque or not:

```rust
// Opaque functions generate normal Rust code.
// The opacity only affects the verification pass.
pub fn fibonacci(n: u32) -> u64 {
    let mut a: u64 = 0;
    let mut b: u64 = 1;
    for _ in 0..n {
        let temp = a + b;
        a = b;
        b = temp;
    }
    a
}
```

#### CORE.7 Prophecy Variables

Ghost variables determined by future events, not past state.
Standard ghost variables (CORE.1) are history variables: their
value is computed from the current and past program state. Prophecy
variables have a value that is declared now but resolved later,
enabling specification of properties that depend on how the
execution will proceed.

##### Motivation

Lock-free data structures with helping mechanisms (Michael-Scott
queue, Harris linked list, elimination stack) have non-fixed
linearization points: the point where an operation logically takes
effect may be inside ANOTHER thread's code. Proving linearizability
of these algorithms requires specifying "operation A linearizes at
the point where thread B completes the help," which is unknown when
A starts.

Without prophecy variables, you can only verify algorithms with
fixed linearization points (e.g., Treiber stack, where the LP is
always the successful CAS). This excludes the most widely deployed
lock-free data structures.

##### Syntax

```assura
// Declare a prophecy: value unknown now
ghost prophecy lp: ProgramPoint

// Constrain the prophecy (optional, narrows valid assignments)
ghost prophecy choice: {v: Nat | v < queue.len()}

// Resolve: fix the prophecy's value at this execution point
resolve lp = here
resolve choice = selected_index

// The verifier checks: for every execution, there EXISTS
// a valid prophecy assignment satisfying all constraints
// and all postconditions that reference the prophecy
```

##### Example: Michael-Scott Queue Dequeue

```assura
fn dequeue(q: &MichaelScottQueue<T>) -> Option<T> {
  ghost prophecy lp: ProgramPoint
  ghost prophecy result_val: Option<T>

  loop {
    let head = q.head.load(acquire)
    let next = head.next.load(acquire)

    if next.is_null() {
      resolve result_val = None
      resolve lp = here
      return None
    }

    let val = next.data
    if q.head.compare_exchange(head, next, acq_rel).is_ok() {
      resolve result_val = Some(val)
      resolve lp = here  // LP is THIS thread's CAS
      return Some(val)
    }
    // CAS failed: another thread helped. LP might be in
    // the other thread's code. Prophecy remains unresolved
    // for this iteration; loop retries.
  }
}
ensures result == result_val  // result matches prophecy
ensures linearized_at(lp)     // LP is between invoke and return
```

When thread B helps thread A's dequeue, the prophecy `lp` for A's
operation resolves inside B's code path. The verifier checks that
for every interleaving, there exists a valid assignment of all
prophecy variables such that the sequential specification holds.

##### Verification Rule

1. **Existential check**: For each prophecy variable P, the
   verifier introduces an existential quantifier:
   `exists P: constraints(P) && postconditions_hold(P)`
2. **Resolution obligation**: Every prophecy must be resolved on
   every execution path. A-CORE-025 fires if an unresolved
   prophecy reaches function exit
3. **Single resolution**: Each prophecy is resolved exactly once.
   A-CORE-026 fires on double resolution
4. **Layer 2**: Prophecy verification runs at Layer 2 (bounded
   SMT, <10s). The existential quantifier is handled via
   Skolemization or counterexample-guided synthesis
5. **Interaction with CORE.1**: Prophecy variables can be read
   by ghost code and used in postconditions, lemmas, and
   axiomatic definitions, just like history ghost variables

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-CORE-025 | Unresolved prophecy at function exit | Prophecy `P` not resolved on some path |
| A-CORE-026 | Prophecy resolved twice | `resolve P` appears on two paths that can both execute |
| A-CORE-027 | Prophecy type mismatch | `resolve P = expr` where expr type differs from P |
| A-CORE-028 | Prophecy constraint violated | Resolved value does not satisfy declared constraint |

##### Rust Codegen

Prophecy variables are verification-only. No runtime code is
generated. The resolved value exists only in the proof:

```rust
// Source Assura has: ghost prophecy lp: ProgramPoint
// Generated Rust: nothing. Prophecies are erased.
fn dequeue(q: &MichaelScottQueue<T>) -> Option<T> {
    loop {
        let head = q.head.load(Ordering::Acquire);
        let next = unsafe { (*head).next.load(Ordering::Acquire) };
        if next.is_null() {
            return None;
        }
        let val = unsafe { (*next).data };
        if q.head.compare_exchange(
            head, next, Ordering::AcqRel, Ordering::Acquire
        ).is_ok() {
            return Some(val);
        }
    }
}
```

#### CORE.8 Liveness Contracts

Properties that assert something good eventually happens:
a leader is eventually elected, a request is eventually served,
a stuck process eventually recovers. These complement safety
properties (bad things never happen) with progress guarantees.

##### Motivation

Safety verification proves your system never does something wrong.
But a system that does nothing at all is trivially safe. Liveness
proves the system eventually does something right. Critical
examples:

- Raft consensus: "A leader is eventually elected after partition
  recovery" (without this, the cluster could stay leaderless
  forever)
- FreeRTOS: "The scheduler eventually dispatches every ready task"
  (without this, a task could starve indefinitely)
- PX4 autopilot: "The controller eventually stabilizes after a
  disturbance" (without this, the drone could oscillate forever)
- DNS resolver: "Every query eventually receives a response or
  timeout" (without this, a query could hang indefinitely)

##### Syntax

```assura
// Eventually: at some future point, P holds
liveness leader_election {
  assume eventually_always { network.synchronous }
  prove eventually { exists n in nodes: n.role == Leader }
}

// Leads-to: whenever P holds, Q eventually follows
liveness request_service {
  prove leads_to(
    request.state == Pending,
    request.state == Served || request.state == TimedOut
  )
}

// Bounded liveness: Q follows P within K steps
liveness bounded_election {
  assume eventually_always { network.synchronous }
  prove eventually_within(steps: 3 * num_nodes) {
    exists n in nodes: n.role == Leader
  }
}

// Fairness assumption: if action is continuously enabled,
// it is eventually taken
liveness fair_scheduler {
  assume fair { task_runnable(t) } implies eventually { task_running(t) }
  prove leads_to(
    task.state == Ready,
    task.state == Running
  )
}
```

##### Example: FreeRTOS Scheduler Progress

```assura
liveness scheduler_progress {
  // Fairness: if the tick interrupt fires, the scheduler runs
  assume fair { tick_pending }
    implies eventually { scheduler_invoked }

  // If a task is the highest-priority ready task,
  // it eventually gets the CPU
  prove leads_to(
    task.priority == max_ready_priority()
      && task.state == Ready,
    task.state == Running
  )

  // Bounded version: within 2 tick periods
  prove eventually_within(ticks: 2) {
    current_task == highest_priority_ready_task()
  }
}
```

##### Example: Raft Leader Election

```assura
liveness raft_election {
  // Partial synchrony: network eventually becomes synchronous
  assume eventually_always {
    forall m in messages:
      delivered_within(m, delta)
  }

  // A leader is eventually elected
  prove eventually {
    exists n in nodes:
      n.role == Leader
      && n.term == max(node.term for node in nodes)
  }

  // Committed entries are eventually applied
  prove leads_to(
    entry.committed,
    entry.applied
  )
}
```

##### Verification Rule

Liveness is verified via **liveness-to-safety reduction** (Biere
et al.), which transforms liveness checking into safety checking
that SMT solvers handle natively:

1. **Augmentation**: The verifier adds a "lasso detector" to the
   state space. A lasso is a finite prefix followed by a cycle.
   If a liveness property fails, there exists a lasso where the
   bad thing persists forever
2. **Bounded model checking (BMC)**: The verifier unrolls the
   transition relation up to K steps and checks for lassos.
   If no lasso is found within K steps, the property holds up to
   that bound
3. **K-induction (optional)**: For unbounded proofs, the verifier
   uses k-induction: if no lasso of length <= k exists AND the
   lasso-freedom is inductive, the property holds for all lengths.
   This runs at Layer 2-3 with configurable timeouts
4. **Fairness encoding**: `assume fair` constraints are encoded
   as compassion (strong fairness) or justice (weak fairness)
   requirements in the transition system. The lasso detector
   checks that every cycle satisfies the fairness constraints
5. **Step bound K**: Configurable per-project. Default K=1000.
   For `eventually_within(steps: N)`, the bound is exactly N

```
Layer assignment:
  eventually_within  → Layer 1 (bounded, decidable, <200ms)
  eventually (BMC)   → Layer 2 (bounded K, <10s)
  eventually (k-ind) → Layer 3 (unbounded, may timeout)
  leads_to           → Layer 2-3 (depends on state space)
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-CORE-029 | Liveness violation: lasso found | BMC found a cycle where property never holds |
| A-CORE-030 | Liveness unproven within bound K | No lasso found but k-induction did not converge |
| A-CORE-031 | Missing fairness assumption | Liveness depends on scheduling but no `assume fair` |
| A-CORE-032 | Invalid fairness target | `assume fair` references undefined action or state |
| A-CORE-033 | Bounded liveness exceeded | `eventually_within(N)` but property not reached in N steps |

##### Rust Codegen

Liveness contracts are verification-only. No runtime code is
generated. In debug builds, optional runtime monitors can be
emitted:

```rust
// Source Assura has: prove eventually_within(ticks: 2) { ... }
// Generated Rust (debug only):
#[cfg(debug_assertions)]
fn check_liveness_scheduler() {
    static TICKS_SINCE_READY: AtomicU32 = AtomicU32::new(0);
    let ticks = TICKS_SINCE_READY.fetch_add(1, Ordering::Relaxed);
    debug_assert!(
        ticks < 2,
        "liveness: highest-priority task not scheduled within 2 ticks"
    );
}

// Release build: nothing generated. The proof guarantees progress.
```


### 14.MEM: Memory Safety

#### MEM.1 Memory Regions

Memory regions are typed, bounds-tracked views into byte buffers.
They enable verified pointer arithmetic without unsafe code.

##### Motivation

6 of 9 recent SQLite CVEs involved buffer overflows caused by
computing an offset into a page buffer without bounds checking.
Example: `&aData[get2byte(&aData[cellOffset + 2*iCell])]` accesses
a page buffer using a 2-byte big-endian offset read from within
the same buffer. If the offset is out of bounds, memory corruption
occurs.

##### Grammar

```ebnf
RegionType     = 'Region' '<' TypeExpr '>' ;
SliceExpr      = Expr '.' 'slice' '(' Expr ',' Expr ')' ;
RegionReadExpr = Expr '.' 'read_u8' '(' Expr ')'
               | Expr '.' 'read_u16_be' '(' Expr ')'
               | Expr '.' 'read_u32_be' '(' Expr ')'
               | Expr '.' 'read_u64_be' '(' Expr ')'
               | Expr '.' 'read_i32_be' '(' Expr ')' ;
RegionWriteExpr = Expr '.' 'write_u8' '(' Expr ',' Expr ')'
               | Expr '.' 'write_u16_be' '(' Expr ',' Expr ')'
               | Expr '.' 'write_u32_be' '(' Expr ',' Expr ')'
               | Expr '.' 'write_u64_be' '(' Expr ',' Expr ')' ;
```

##### Type System

A `Region<Size>` is a byte buffer of exactly `Size` bytes. Every
read and write operation requires a refined offset proving the access
is in bounds:

```assura
type Region<Size: Nat> {
  invariant { len(data) == Size }
}

fn read_u16_be(
    r: Region<n>,
    offset: {i: Nat | i + 2 <= n}
) -> {v: Nat | v < 65536}
  effects: pure

fn read_u32_be(
    r: Region<n>,
    offset: {i: Nat | i + 4 <= n}
) -> U32
  effects: pure

fn write_u16_be(
    r: Region<n> :_1,
    offset: {i: Nat | i + 2 <= n},
    value: {v: Nat | v < 65536}
) -> Region<n> :_1
  effects: pure

fn slice(
    r: Region<n>,
    start: {s: Nat | s <= n},
    len: {l: Nat | s + l <= n}
) -> Region<l>
  effects: pure
```

##### SQLite Example: Cell Pointer Access

```assura
contract CellPointerAccess {
  type PageBuffer = Region<PageSize>

  fn get_cell_offset(
      page: PageBuffer,
      cell_index: {i: Nat | i < cell_count},
      cell_pointer_offset: {c: Nat | c + 2 * cell_count <= PageSize}
  ) -> {offset: Nat | offset < PageSize}
    effects: pure
  {
    let raw = page.read_u16_be(cell_pointer_offset + 2 * cell_index)
    requires { raw < PageSize }
    -- COMPILE ERROR if raw could be >= PageSize
    -- (catches SQLite-class buffer overflow at compile time)
    raw
  }
}
```

##### Rust Codegen

`Region<n>` maps to `&[u8]` (read) or `&mut [u8]` (write) with
debug_assert bounds checks:

```rust
pub struct Region<'a> {
    data: &'a [u8],
}

impl<'a> Region<'a> {
    pub fn read_u16_be(&self, offset: usize) -> u16 {
        debug_assert!(offset + 2 <= self.data.len());
        u16::from_be_bytes([self.data[offset], self.data[offset + 1]])
    }
}
```

The dependent size index `n` erases. Bounds safety is proven at
compile time by the refinement checker; the debug_assert is a second
safety net.


#### MEM.2 Fixed-Width Integers

Fixed-width integers model the exact arithmetic behavior of hardware
integers, including overflow and truncation.

##### Motivation

The dominant SQLite CVE pattern is "64-bit value truncated to 32-bit
int in a size calculation." Example: CVE-2025-52099 where
`sz * cnt` overflows a 32-bit `int` in `setupLookaside()`.

##### Types

```assura
// Fixed-width unsigned
type U8   = {v: Nat | v <= 255}
type U16  = {v: Nat | v <= 65535}
type U32  = {v: Nat | v <= 4294967295}
type U64  = {v: Nat | v <= 18446744073709551615}

// Fixed-width signed
type I8   = {v: Int | -128 <= v and v <= 127}
type I16  = {v: Int | -32768 <= v and v <= 32767}
type I32  = {v: Int | -2147483648 <= v and v <= 2147483647}
type I64  = {v: Int | -9223372036854775808 <= v and v <= 9223372036854775807}

// Platform-dependent
type USize = U64  // or U32, selected by target
type ISize = I64

// Checked narrowing: REQUIRES proof that value fits
fn narrow_u64_to_u32(v: U64) -> U32
  requires { v <= 4294967295 }

fn narrow_i64_to_i32(v: I64) -> I32
  requires { -2147483648 <= v and v <= 2147483647 }

// Checked multiplication: REQUIRES proof of no overflow
fn checked_mul_u32(a: U32, b: U32) -> U32
  requires { a * b <= 4294967295 }
  ensures  { result == a * b }
```

##### Arithmetic Overflow Rules

All fixed-width arithmetic generates refinement checks:

```assura
fn alloc_size(count: U64, element_size: U64) -> USize
  requires { count * element_size <= MAX_ALLOC }
{
  let total: U64 = count * element_size
  -- Compiler emits SMT query: count * element_size <= U64_MAX
  -- If caller can't prove it, error A13004

  narrow_u64_to_usize(total)
  -- Compiler emits: total <= USIZE_MAX
  -- If 32-bit target and total > U32_MAX, error A13004
}
```

**This single feature would have caught CVE-2020-13434, CVE-2022-35737,
CVE-2025-3277, CVE-2025-7709, and CVE-2025-52099.**

##### Rust Codegen

Fixed-width types map directly to Rust primitive types:

| Assura | Rust |
|---|---|
| U8 | u8 |
| U16 | u16 |
| U32 | u32 |
| U64 | u64 |
| I32 | i32 |
| I64 | i64 |

Narrowing generates checked casts:

```rust
pub fn narrow_u64_to_u32(v: u64) -> u32 {
    debug_assert!(v <= u32::MAX as u64);
    v as u32
}
```


#### MEM.3 Allocator Contracts

Contracts for custom memory allocators that verify structural
invariants: no overlap, no double-free, free-list acyclicity.

##### Grammar

```ebnf
AllocatorDecl  = 'allocator' TypeIdent '<' TypeParam '>' '{'
                   { AllocatorInvariant }
                   AllocateOp
                   FreeOp
                 '}' ;

AllocatorInvariant = 'invariant' ':' Predicate ;
AllocateOp     = 'allocate' '{' { OperationItem } '}' ;
FreeOp         = 'free' '{' { OperationItem } '}' ;
```

##### Full Example: Buddy Allocator

```assura
allocator BuddyAllocator<BufferSize: Nat> {
  type Block {
    offset: {v: Nat | v < BufferSize},
    size: {v: Nat | is_power_of_2(v) and v > 0},
    allocated: Bool
  }

  type FreeList = List<Block>

  state {
    buffer: Region<BufferSize>,
    blocks: Set<Block>,
    free_lists: Map<Nat, FreeList>  // size -> free blocks of that size
  }

  // No two blocks overlap
  invariant {
    forall b1, b2 in blocks:
      b1 != b2 =>
        b1.offset + b1.size <= b2.offset
        or b2.offset + b2.size <= b1.offset
  }

  // All blocks fit in the buffer
  invariant {
    forall b in blocks: b.offset + b.size <= BufferSize
  }

  // Free list is acyclic (no cycle)
  invariant {
    forall size in free_lists.keys():
      is_acyclic(free_lists[size])
  }

  // Free list only contains unallocated blocks
  invariant {
    forall size in free_lists.keys():
      forall b in free_lists[size]:
        b.allocated == false
  }

  // Total allocated + free == BufferSize
  invariant {
    sum(b.size for b in blocks) == BufferSize
  }

  allocate {
    input(requested_size: {v: Nat | v > 0 and v <= BufferSize})
    output(block: Block :_1)

    ensures { block.size >= round_up_power_of_2(requested_size) }
    ensures { block.allocated == true }
    ensures { block in blocks }
    effects: pure
  }

  free {
    input(block: Block :_1)

    requires { block.allocated == true }
    requires { block in blocks }
    ensures  { block.allocated == false }
    ensures  { block in free_lists[block.size] }
    effects: pure
  }
}
```

##### Rust Codegen

Allocator contracts generate a Rust struct implementing
`std::alloc::Allocator` (unstable) or a custom allocator trait:

```rust
pub struct BuddyAllocator {
    buffer: Box<[u8]>,
    free_lists: [Vec<Block>; MAX_ORDER],
}

impl BuddyAllocator {
    pub fn allocate(&mut self, size: usize) -> Option<&mut [u8]> {
        let order = size.next_power_of_two().trailing_zeros() as usize;
        debug_assert!(order < MAX_ORDER);
        // ... buddy allocation logic ...
    }

    pub fn free(&mut self, block: Block) {
        debug_assert!(block.allocated);
        debug_assert!(!self.has_overlap(&block));
        // ... return to free list, coalesce buddies ...
    }
}
```


#### MEM.4 Circular Buffer Contracts

Contracts for ring buffers and sliding windows that track
wrap-around semantics, logical-to-physical index mapping, and
slide operations.

##### Motivation

zlib's sliding window (`deflate_state->window`) is a circular
buffer where the write pointer wraps around, valid history may
be less than window size, and `fill_window()` physically slides
data by copying the second half to the first and adjusting all
hash chain pointers. This pattern also appears in network stacks
(TCP receive buffers), audio processing (sample ring buffers),
database WAL buffers, and kernel I/O schedulers.

MEM.1 handles linear buffer bounds but cannot express wrap-around
indexing, the relationship between physical layout and logical
sequence, or the correctness of slide operations that relocate
data and update dependent pointers.

##### Grammar

```ebnf
CircularBufferDecl = 'circular_buffer' TypeIdent '<' TypeParam '>'
                     '{' CircularBufferBody '}' ;

CircularBufferBody = 'capacity' ':' Expr ','
                     'write_pos' ':' Ident ','
                     'valid_count' ':' Ident ','
                     { CircularBufferInvariant }
                     [ SlideOp ] ;

CircularBufferInvariant = 'invariant' ':' Predicate ;
SlideOp            = 'slide' '{' { OperationItem } '}' ;
```

##### Full Example: zlib Sliding Window

```assura
circular_buffer DeflateWindow<WSize: Nat> {
  storage: Region<u8>[WSize * 2],  // physical: 2x for slide
  capacity: WSize,
  write_pos: wnext,      // next write position (wraps)
  valid_count: whave,     // bytes of valid history

  // Structural invariants
  invariant: whave <= WSize
  invariant: wnext < WSize * 2

  // Logical view: the last `whave` bytes written
  ghost fn logical_view(self) -> Seq<u8> {
    if wnext >= whave {
      self.storage[(wnext - whave)..wnext]
    } else {
      // Wrap-around case
      self.storage[(WSize * 2 + wnext - whave)..] ++
        self.storage[..wnext]
    }
  }

  // Read at logical offset (0 = oldest valid)
  fn read(self, offset: {v: Nat | v < self.whave}) -> u8
    ensures result == self.logical_view()[offset]
  {
    let phys = (wnext - whave + offset) % (WSize * 2)
    self.storage[phys]
  }

  // Slide operation: move second half to first, adjust pointers
  slide {
    requires { wnext >= WSize }
    ensures  { logical_view(post) == logical_view(pre) }
    ensures  { wnext(post) == wnext(pre) - WSize }
    ensures  { whave(post) == min(whave(pre), WSize) }
    // All dependent pointers adjusted by -WSize
    ensures  {
      forall p in dependent_pointers:
        p(post) == max(0, p(pre) - WSize)
    }
  }
}
```

##### Verification Rules

1. Every index into the buffer must be reduced modulo capacity
   or proven within physical bounds
2. The `logical_view` ghost function must be consistent across
   wrap-around boundaries
3. After a `slide` operation, the logical sequence is preserved
   (bytes in the same logical order, just physically relocated)
4. Dependent pointers (hash chain entries in zlib) must all be
   adjusted by the same offset after a slide
5. Valid count never exceeds capacity

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-MEM-015 | Circular buffer index out of bounds | Access at offset >= valid_count |
| A-MEM-016 | Slide precondition violated | Slide called when write_pos < capacity |
| A-MEM-017 | Slide breaks logical view | Post-slide logical sequence != pre-slide |
| A-MEM-018 | Dependent pointer not adjusted | Pointer into buffer not updated after slide |

##### Rust Codegen

Circular buffer contracts generate a Rust struct with modular
indexing and a slide method with debug assertions:

```rust
pub struct DeflateWindow {
    storage: Box<[u8]>,  // 2 * wsize
    wsize: usize,
    wnext: usize,
    whave: usize,
}

impl DeflateWindow {
    pub fn read(&self, offset: usize) -> u8 {
        debug_assert!(offset < self.whave);
        let phys = (self.wnext + self.storage.len()
            - self.whave + offset) % self.storage.len();
        self.storage[phys]
    }

    pub fn slide(&mut self, hash_heads: &mut [u16], hash_prev: &mut [u16]) {
        debug_assert!(self.wnext >= self.wsize);
        // Copy second half to first
        self.storage.copy_within(self.wsize..self.wsize * 2, 0);
        // Adjust all hash chain pointers
        for h in hash_heads.iter_mut() {
            *h = h.saturating_sub(self.wsize as u16);
        }
        for p in hash_prev.iter_mut() {
            *p = p.saturating_sub(self.wsize as u16);
        }
        self.wnext -= self.wsize;
        self.whave = self.whave.min(self.wsize);
    }
}
```


### 14.TYPE: Types and Contracts

#### TYPE.1 Interface Contracts

Contracts on trait objects, vtables, and dynamically dispatched
functions. Every implementation must satisfy the interface contract.

##### Grammar

```ebnf
InterfaceDecl  = 'interface' TypeIdent [TypeParams] '{'
                   { InterfaceMethod }
                   { InvariantDecl }
                 '}' ;

InterfaceMethod = 'fn' Ident '(' [ParamList] ')' '->' TypeExpr
                  { InterfaceClause } ;

InterfaceClause = RequiresClause | EnsuresClause | EffectsClause ;

ImplDecl       = 'impl' TypeIdent 'for' TypeIdent '{'
                   { FnDecl }
                 '}' ;
```

##### Full Example: Virtual File System

```assura
interface VirtualFileSystem {
  type FileHandle

  fn open(path: String, flags: OpenFlags) -> FileHandle | IOError
    ensures { result is FileHandle => result.is_valid() }
    effects: filesystem.open

  fn read(
      fh: FileHandle,
      buf: Region<n> :_1,
      amount: {a: Nat | a <= n},
      offset: Nat
  ) -> (Region<n> :_1, {bytes_read: Nat | bytes_read <= amount})
    requires { fh.is_valid() }
    effects: filesystem.read

  fn write(
      fh: FileHandle,
      data: Region<n>,
      offset: Nat
  ) -> {bytes_written: Nat | bytes_written <= n}
    requires { fh.is_valid() }
    effects: filesystem.write

  fn lock(fh: FileHandle, level: LockLevel) -> Bool
    requires { valid_lock_upgrade(fh.current_lock(), level) }
    ensures  { result == true => fh.current_lock() == level }
    effects: filesystem.lock

  fn unlock(fh: FileHandle, level: LockLevel) -> Bool
    requires { level < fh.current_lock() }
    ensures  { result == true => fh.current_lock() == level }
    effects: filesystem.lock

  fn sync(fh: FileHandle, full: Bool) -> Bool
    requires { fh.is_valid() }
    effects: filesystem.sync

  // Lock ordering invariant (typestate on FileHandle)
  invariant {
    forall fh: valid_lock_upgrade(from, to) =>
      (from == Unlocked and to == Shared) or
      (from == Shared and to == Reserved) or
      (from == Reserved and to == Exclusive)
  }
}

// Platform-specific implementations
impl UnixVfs for VirtualFileSystem {
  // Must satisfy ALL interface contracts
  // Compiler verifies each method independently
}

impl WindowsVfs for VirtualFileSystem {
  // Different implementation, same contracts
}
```

##### Verification Rule

When verifying `impl Foo for Interface`:
1. Each method's implementation is checked against the interface
   method's requires/ensures/effects
2. The implementation may have STRONGER preconditions (contravariance)
3. The implementation may have WEAKER postconditions (covariance)
4. The implementation must not use effects beyond what the interface
   declares

##### Rust Codegen

Interface contracts map to Rust traits:

```rust
pub trait VirtualFileSystem {
    type FileHandle;

    fn open(&self, path: &str, flags: OpenFlags)
        -> Result<Self::FileHandle, IOError>;

    fn read(&self, fh: &Self::FileHandle, buf: &mut [u8], offset: u64)
        -> Result<usize, IOError>;

    fn write(&self, fh: &Self::FileHandle, data: &[u8], offset: u64)
        -> Result<usize, IOError>;

    fn lock(&self, fh: &Self::FileHandle, level: LockLevel) -> bool;
    fn unlock(&self, fh: &Self::FileHandle, level: LockLevel) -> bool;
    fn sync(&self, fh: &Self::FileHandle, full: bool) -> bool;
}
```


#### TYPE.2 Recursive Structural Invariants

Invariants that hold recursively across tree, graph, and linked
data structures.

##### Grammar

```ebnf
StructuralInvariant = 'structural_invariant' TypeIdent
                      [TypeParams] '{'
                        { InvariantClause }
                      '}' ;
```

##### Full Example: B-Tree

```assura
type BTreeNode<K, V, Level: Nat> {
  keys: Vec<K, n> where n >= 1 and n <= 2 * ORDER - 1,
  values: Vec<V, n>,
  children: Vec<BTreeNode<K, V, Level - 1>, n + 1>
    where Level > 0
  -- Leaf nodes (Level == 0) have no children
}

structural_invariant BTreeValid<K, V, Level: Nat> {
  // 1. Keys are sorted within each node
  forall node: BTreeNode<K, V, Level>:
    forall i in 0..len(node.keys)-1:
      node.keys[i] < node.keys[i+1]

  // 2. Subtree ordering: left < key < right
  forall node: BTreeNode<K, V, Level> where Level > 0:
    forall i in 0..len(node.keys):
      max_key(node.children[i]) < node.keys[i]
    forall i in 0..len(node.keys):
      min_key(node.children[i+1]) > node.keys[i]

  // 3. All leaves at same depth (balanced)
  depth(leftmost_leaf) == depth(rightmost_leaf)

  // 4. Minimum occupancy (except root)
  forall node where node != root:
    len(node.keys) >= ORDER - 1

  // 5. Key count matches value count
  forall node:
    len(node.keys) == len(node.values)

  // 6. Child count is key count + 1 (internal nodes)
  forall node where Level > 0:
    len(node.children) == len(node.keys) + 1
}

// Measures for recursive properties
measure max_key<K, V, Level>(node: BTreeNode<K, V, Level>) -> K
  max_key(leaf) = last(leaf.keys) where Level == 0
  max_key(internal) = max_key(last(internal.children))

measure min_key<K, V, Level>(node: BTreeNode<K, V, Level>) -> K
  min_key(leaf) = first(leaf.keys) where Level == 0
  min_key(internal) = min_key(first(internal.children))

measure depth<K, V, Level>(node: BTreeNode<K, V, Level>) -> Nat
  depth(leaf) = 0 where Level == 0
  depth(internal) = 1 + depth(first(internal.children))
```

##### Verification Approach

Structural invariants are verified by induction over the data
structure:
1. **Base case**: Verify the invariant holds for leaf nodes
2. **Inductive step**: Assuming the invariant holds for all subtrees,
   verify it holds for the parent

This maps to Z3 queries with quantified axioms over the measure
functions. Budget: layer 2, 10s timeout.


#### TYPE.3 Error Propagation Contracts

Contracts that govern how errors flow through the call stack,
preventing silent error swallowing, error masking, and error
code translation mistakes.

##### Motivation

SQLite has ~30 error codes (`SQLITE_OK` through `SQLITE_NOTICE`)
with dozens of extended codes. Error handling is the #1 source of
subtle bugs in C systems code:
- Catching `SQLITE_CORRUPT` and returning `SQLITE_OK` (hiding
  database corruption from the caller)
- Translating `SQLITE_NOMEM` to `SQLITE_ERROR` (losing the
  actionable information that memory is full)
- Ignoring the return value of `sqlite3_reset()` (it returns the
  error from the last `sqlite3_step()`, not its own status)
- Calling `sqlite3_errmsg()` after a second API call (the error
  message is overwritten)

Effects (Section 3) track what side effects occur but not how
errors propagate. Transactional rollback (STOR.4) handles the
undo path but not the error code correctness. Error propagation
contracts bridge this gap.

##### Grammar

```ebnf
ErrorPropDecl  = 'error_policy' TypeIdent '{'
                   { ErrorRule }
                 '}' ;

ErrorRule      = 'must_propagate' ':' ErrorCodeList
               | 'must_not_mask' ':' ErrorCodePattern
               | 'may_translate' ':' ErrorTranslation
               | 'must_check' ':' IdentList
               | 'must_preserve_detail' ':' ErrorCodeList ;

ErrorCodeList  = ErrorCode { ',' ErrorCode } ;
ErrorTranslation = ErrorCode '->' ErrorCode
                   ['when' Predicate] ;

ErrorCodePattern = ErrorCode '->' ErrorCode  // forbidden translation ;

PropagateAnnotation = '#[propagate(' ErrorCode ')]'
                    | '#[swallow(' ErrorCode ',' StringLit ')]' ;
```

##### Full Example: SQLite Error Propagation

```assura
// Global error policy for the SQLite port
error_policy SqliteErrors {
  // These errors MUST propagate to the caller. They cannot be
  // caught and turned into SQLITE_OK.
  must_propagate:
    SQLITE_CORRUPT,    // database corruption
    SQLITE_NOTADB,     // not a database file
    SQLITE_NOMEM,      // out of memory
    SQLITE_IOERR,      // disk I/O error
    SQLITE_FULL        // disk full

  // Error masking rules: these translations are FORBIDDEN
  must_not_mask:
    SQLITE_CORRUPT -> SQLITE_OK,    // hiding corruption
    SQLITE_CORRUPT -> SQLITE_ERROR, // downgrading corruption
    SQLITE_NOMEM -> SQLITE_ERROR,   // losing OOM detail
    SQLITE_IOERR -> SQLITE_OK      // hiding I/O failure

  // Allowed translations (explicit and documented)
  may_translate:
    SQLITE_BUSY -> SQLITE_LOCKED
      when holding_table_lock
    SQLITE_CONSTRAINT -> SQLITE_CONSTRAINT_UNIQUE
      when constraint_type == Unique
    SQLITE_CONSTRAINT -> SQLITE_CONSTRAINT_FOREIGNKEY
      when constraint_type == ForeignKey
    SQLITE_IOERR -> SQLITE_IOERR_READ
      when operation == Read
    SQLITE_IOERR -> SQLITE_IOERR_WRITE
      when operation == Write

  // These function return values MUST be checked by the caller
  must_check:
    sqlite3_reset,       // returns error from last step
    sqlite3_finalize,    // returns error from last step
    sqlite3_close,       // returns BUSY if statements open
    sqlite3_exec         // returns error from callback or SQL

  // These errors must preserve their detail across translations
  // (extended error code must survive)
  must_preserve_detail:
    SQLITE_IOERR,        // keep IOERR_READ vs IOERR_WRITE
    SQLITE_CONSTRAINT    // keep CONSTRAINT_UNIQUE vs FK
}

// Using error propagation contracts
fn read_page_from_disk(
    fd: FileDescriptor,
    page_num: U32
) -> Region<PageSize> | SqlError
  #[propagate(SQLITE_IOERR)]
  #[propagate(SQLITE_CORRUPT)]
  effects: filesystem.read
{
  let bytes = read_bytes(fd, page_num * PageSize, PageSize)?
  // ? propagates SQLITE_IOERR from read_bytes

  if not validate_page(bytes) {
    return SqlError(SQLITE_CORRUPT, "page checksum mismatch")
  }

  bytes
}

// COMPILE ERROR: masking corruption
fn bad_error_handling(
    fd: FileDescriptor,
    page_num: U32
) -> Region<PageSize>
  effects: filesystem.read
{
  match read_page_from_disk(fd, page_num) {
    Ok(bytes) => bytes,
    Err(e) => {
      // A48002: SQLITE_CORRUPT cannot be masked as SQLITE_OK
      // by returning a default page
      Region.zeroed()  // DANGEROUS: pretending corrupt page is OK
    }
  }
}

// COMPILE ERROR: ignoring must-check return value
fn bad_cleanup(stmt: Statement :_1)
  effects: database.read
{
  stmt.reset()
  // A48004: return value of sqlite3_reset not checked
  // (it carries the error from the last sqlite3_step)
}

// CORRECT: check and propagate
fn good_cleanup(
    stmt: Statement :_1
) -> Statement :_1 | SqlError
  effects: database.read
{
  let result = stmt.reset()
  match result {
    Ok(s) => s,
    Err(e) => {
      // Log the error from last step, return it
      log_error(e)
      return Err(e)
    }
  }
}

// Error detail preservation
fn handle_constraint_error(
    err: SqlError
) -> SqlError
  requires { err.code == SQLITE_CONSTRAINT }
  ensures {
    // Extended code must survive
    result.extended_code == err.extended_code
    // A48005: must_preserve_detail violated if extended code is lost
  }
{
  SqlError {
    code: err.code,
    extended_code: err.extended_code,  // preserve!
    message: format_constraint_message(err),
  }
}

// Swallowing an error explicitly (documented escape hatch)
fn optional_optimization(db: Database :_1) -> Database :_1
  effects: database.read
{
  // Explicitly documented: we try an optimization, but it's OK
  // if it fails (we fall back to the slow path)
  #[swallow(SQLITE_BUSY, "optimization hint, slow path is fine")]
  let _ = db.try_wal_checkpoint(WAL_CHECKPOINT_PASSIVE)

  db  // Continue regardless
}
```

##### Verification Rule

1. **Must-propagate**: The compiler traces every error path. If a
   `must_propagate` error is caught and not re-raised, it is a
   compile error
2. **Must-not-mask**: The compiler checks every error translation
   (match arm, map_err, ? with conversion). Forbidden translations
   are compile errors
3. **Must-check**: Every call to a `must_check` function must have
   its return value inspected. Discarding the result (including
   `let _ = ...` without `#[swallow]`) is a compile error
4. **Detail preservation**: Error translations that lose the
   extended error code for `must_preserve_detail` errors are
   compile errors
5. **Explicit swallow**: The `#[swallow]` annotation documents
   intentional error suppression with a justification string.
   Reviewers and auditors can search for all swallowed errors

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A48001 | Must-propagate error `E` swallowed | Catching and hiding a critical error |
| A48002 | Error `E` masked as `F` | Forbidden error translation |
| A48003 | Error detail lost: extended code dropped | must_preserve_detail violation |
| A48004 | Return value of `F` not checked | Ignoring must-check function result |
| A48005 | Undocumented error swallow | Error suppressed without #[swallow] annotation |

##### Rust Codegen

Error propagation contracts generate Rust error types with
compile-time checking via `must_use` and custom lints:

```rust
#[derive(Debug, Clone)]
#[must_use = "SqlError must be handled, not silently dropped"]
pub struct SqlError {
    pub code: ErrorCode,
    pub extended_code: ExtendedErrorCode,
    pub message: String,
}

// Critical errors use a wrapper that panics on drop if not handled
#[derive(Debug)]
#[must_use = "Critical errors cannot be silently dropped"]
pub struct CriticalError(SqlError);

impl Drop for CriticalError {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            panic!(
                "Critical error {} dropped without handling: {}",
                self.0.code, self.0.message,
            );
        }
    }
}

impl CriticalError {
    pub fn handle(self) -> SqlError {
        let err = self.0.clone();
        std::mem::forget(self); // disarm the drop bomb
        err
    }
}

// Must-check functions return MustUse wrapper
#[must_use = "sqlite3_reset return value must be checked"]
pub fn sqlite3_reset(stmt: &mut Statement) -> Result<(), SqlError> {
    // ...
}

// Error conversion with masking prevention
impl From<IoError> for SqlError {
    fn from(e: IoError) -> Self {
        // Preserves IO error detail
        SqlError {
            code: ErrorCode::SQLITE_IOERR,
            extended_code: match e.kind() {
                ErrorKind::NotFound => ExtendedErrorCode::IOERR_READ,
                ErrorKind::PermissionDenied => ExtendedErrorCode::IOERR_WRITE,
                _ => ExtendedErrorCode::IOERR,
            },
            message: e.to_string(),
        }
    }
}
```


### 14.SEC: Trust and Security

#### SEC.1 Untrusted Data Taint

A taint tracking system for data that crosses trust boundaries.
Data from disk, network, or user input is `untrusted` until
explicitly validated.

##### Motivation

4 of 9 SQLite CVEs were in modules (FTS, R-Tree, JSON, Session)
that parse binary data stored in shadow tables. The root cause:
these modules trust on-disk metadata (BLOBs) without sufficient
validation. The Black Hat 2017 "Many Birds, One Stone" attack
exploited this single trust assumption across multiple modules.

##### Taint Labels

```assura
// Taint is a separate axis from security labels
// Security labels: who can SEE the data (confidentiality)
// Taint labels: whether the data is TRUSTED (integrity)

taint_label untrusted   // data from outside the trust boundary
taint_label validated    // data that passed validation
taint_label trusted      // data from within the trust boundary
```

##### Grammar

```ebnf
TaintAnnotation = '@taint:' TaintLabel ;
TaintLabel      = 'untrusted' | 'validated' | 'trusted' ;
ValidateExpr    = 'validate' '{' Predicate '}' Expr ;
```

##### Taint Propagation Rules

```
// All data from disk/network/user input is untrusted
fn read_from_disk(path: String) -> Bytes @taint:untrusted
  effects: filesystem.read

// Untrusted data cannot be used where trusted data is expected
fn process(data: Bytes @taint:trusted) -> Result
  // COMPILE ERROR if called with @taint:untrusted data

// Validation converts untrusted to validated
fn validate_blob(
    data: Bytes @taint:untrusted
) -> ValidBlob @taint:validated | ParseError
  effects: pure
{
  validate {
    len(data) >= HEADER_SIZE
    and read_u32(data, 0) == MAGIC_NUMBER
    and read_u32(data, 4) <= MAX_SEGMENT_SIZE
  } data
}
```

##### Taint + Information Flow Interaction

Taint (integrity) and security labels (confidentiality) are
orthogonal. Data can be:
- `@Restricted @taint:trusted` (sensitive, from our own database)
- `@Public @taint:untrusted` (non-sensitive, from network)
- `@Restricted @taint:untrusted` (sensitive user input, not yet validated)

Both axes must be satisfied: data must be at or below the security
label AND at or above the taint level.

##### SQLite Example: FTS Shadow Table

```assura
fn read_fts_segment(
    db: Database,
    segment_id: U64
) -> FtsSegment | CorruptionError
  effects: database.read
{
  let raw: Bytes @taint:untrusted = db.read_blob("fts_segments", segment_id)

  // Every field must be validated before use
  let header_size = validate { raw.len() >= 16 } raw.read_u32(0)
    or return CorruptionError("header too small")

  let segment_size = validate { header_size <= MAX_SEGMENT } header_size
    or return CorruptionError("segment too large")

  let content = validate { raw.len() >= segment_size } raw.slice(16, segment_size)
    or return CorruptionError("truncated segment")

  FtsSegment { header_size, content }
  // FtsSegment is @taint:validated -- safe to use
}
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A28001 | Untrusted data used as trusted | Missing validation |
| A28002 | Validation predicate insufficient | Validation doesn't cover all fields |
| A28003 | Taint escapes through aliasing | Trusted alias of untrusted data |

##### Rust Codegen

Taint labels erase at runtime (same as security labels). The
compiler has already verified all validation paths. Generated Rust
includes debug_assert checks on validation predicates:

```rust
pub fn read_fts_segment(db: &Database, segment_id: u64)
    -> Result<FtsSegment, CorruptionError>
{
    let raw = db.read_blob("fts_segments", segment_id)?;

    if raw.len() < 16 {
        return Err(CorruptionError::new("header too small"));
    }
    let header_size = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;

    if header_size > MAX_SEGMENT {
        return Err(CorruptionError::new("segment too large"));
    }

    if raw.len() < header_size {
        return Err(CorruptionError::new("truncated segment"));
    }
    let content = &raw[16..header_size];

    Ok(FtsSegment { header_size, content: content.to_vec() })
}
```


#### SEC.2 FFI Boundary Contracts

Contracts for functions exposed to or called from foreign languages
(C, Python, Java via JNI). These define the safety contract at the
boundary between verified Assura code and unverified foreign callers.

##### Motivation

SQLite's entire value comes from its C API. A Rust port must expose
`sqlite3_open()`, `sqlite3_exec()`, etc. as `extern "C"` functions.
The Rust side is verified; the C caller is not. The FFI contract
defines what the caller must guarantee and what the callee promises,
making the boundary explicit rather than implicit.

##### Grammar

```ebnf
FfiDecl        = 'ffi' StringLit TypeIdent '{'
                   { FfiFunction }
                 '}' ;

FfiFunction    = 'export' Ident '(' [FfiParamList] ')'
                 '->' FfiReturnType
                 '{' { FfiClause } '}' ;

FfiParam       = Ident ':' FfiType [FfiAnnotation] ;
FfiType        = 'ptr' '<' TypeExpr '>'        // raw pointer
               | 'nullable_ptr' '<' TypeExpr '>'
               | 'cstring'                     // null-terminated
               | 'buffer' '<' Ident '>'        // ptr + length pair
               | 'opaque'                      // void*
               | 'c_int' | 'c_uint' | 'c_long'
               | 'size_t' ;

FfiAnnotation  = '#[not_null]'
               | '#[null_terminated]'
               | '#[valid_for(' Ident ')]'
               | '#[caller_frees]'
               | '#[callee_frees]'
               | '#[borrowed(' Lifetime ')]' ;

FfiClause      = 'caller_guarantees' ':' Predicate
               | 'callee_guarantees' ':' Predicate
               | 'error_convention' ':' FfiErrorConvention
               | 'thread_safety' ':' ThreadSafetyLevel ;

FfiErrorConvention = 'return_code' '(' IntLit '=' Ident
                     { ',' IntLit '=' Ident } ')'
                   | 'null_on_error'
                   | 'errno' ;

ThreadSafetyLevel = 'single_threaded'
                  | 'serialized'     // safe to call from any thread
                  | 'multi_threaded' // safe if different connections
                  ;
```

##### Full Example: SQLite C API

```assura
ffi "C" SqliteApi {
  // sqlite3_open(filename, **ppDb) -> int
  export sqlite3_open(
    filename: cstring #[not_null] #[null_terminated],
    ppDb: ptr<ptr<Connection>> #[not_null]
  ) -> c_int {
    caller_guarantees:
      filename points to valid null-terminated UTF-8

    callee_guarantees:
      result == SQLITE_OK =>
        *ppDb is a valid Connection pointer
      result != SQLITE_OK =>
        *ppDb may or may not be set
        // Caller MUST call sqlite3_close(*ppDb) even on error
        // if *ppDb is non-null (this is a real SQLite requirement)

    error_convention: return_code(
      0 = SQLITE_OK,
      7 = SQLITE_NOMEM,
      14 = SQLITE_CANTOPEN
    )

    thread_safety: serialized
  }

  // sqlite3_exec(db, sql, callback, arg, errmsg) -> int
  export sqlite3_exec(
    db: ptr<Connection> #[not_null]
        #[valid_for(connection_lifetime)],
    sql: cstring #[not_null] #[null_terminated],
    callback: nullable_ptr<ExecCallback>,
    callback_arg: opaque,
    errmsg: nullable_ptr<ptr<c_char>> #[callee_frees]
  ) -> c_int {
    caller_guarantees:
      db was returned by sqlite3_open and not yet closed
      and sql is valid UTF-8 SQL

    callee_guarantees:
      result == SQLITE_OK =>
        all SQL statements in sql were executed
      result == SQLITE_ABORT =>
        callback returned non-zero (user cancelled)
      result != SQLITE_OK and errmsg != null =>
        *errmsg points to a malloc'd error string
        that the caller must free with sqlite3_free

    error_convention: return_code(
      0 = SQLITE_OK,
      4 = SQLITE_ABORT,
      1 = SQLITE_ERROR
    )

    thread_safety: multi_threaded
    // Safe if no other thread uses the same db connection
  }

  // sqlite3_close(db) -> int
  export sqlite3_close(
    db: nullable_ptr<Connection>
  ) -> c_int {
    caller_guarantees:
      db is null or was returned by sqlite3_open
      and no other thread is using db
      and all prepared statements are finalized

    callee_guarantees:
      result == SQLITE_OK => db is freed, pointer is invalid
      result == SQLITE_BUSY => db is NOT freed, still valid
        // Caller must finalize statements and retry

    error_convention: return_code(
      0 = SQLITE_OK,
      5 = SQLITE_BUSY
    )

    thread_safety: single_threaded
  }

  // sqlite3_prepare_v2(db, sql, nByte, **ppStmt, *pzTail) -> int
  export sqlite3_prepare_v2(
    db: ptr<Connection> #[not_null],
    sql: buffer<nByte> #[not_null],
    nByte: c_int,
    ppStmt: ptr<ptr<Statement>> #[not_null],
    pzTail: nullable_ptr<ptr<c_char>>
  ) -> c_int {
    caller_guarantees:
      db is a valid open connection
      and sql points to nByte bytes of valid UTF-8
      and (nByte >= 0 or nByte == -1)
      // nByte == -1 means read until null terminator

    callee_guarantees:
      result == SQLITE_OK =>
        *ppStmt is a valid prepared statement
        and (pzTail != null => *pzTail points to first byte
             past the end of the first SQL statement in sql)
      result != SQLITE_OK =>
        *ppStmt is null

    error_convention: return_code(0 = SQLITE_OK, 1 = SQLITE_ERROR)
    thread_safety: multi_threaded
  }
}
```

##### Verification Rule

1. **Callee side**: The Assura compiler verifies that the
   implementation satisfies all `callee_guarantees` given the
   `caller_guarantees` as axioms
2. **Caller side**: When Assura code calls an FFI function, the
   compiler verifies that all `caller_guarantees` are met
3. **Lifetime tracking**: `#[valid_for]` annotations create phantom
   lifetimes in the generated Rust code
4. **Null safety**: `ptr<T>` with `#[not_null]` generates
   `NonNull<T>` in Rust; `nullable_ptr<T>` generates `*mut T`
5. **Memory ownership**: `#[caller_frees]` and `#[callee_frees]`
   annotations prevent double-free and leak

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A37001 | FFI caller guarantee not provable at call site | Caller can't prove pointer validity |
| A37002 | FFI callee guarantee not satisfied by implementation | Implementation violates promised postcondition |
| A37003 | FFI memory ownership conflict | Double-free or leak at FFI boundary |
| A37004 | FFI null pointer not checked | Nullable pointer used without null check |
| A37005 | FFI thread safety violation | Function called from wrong threading context |

##### Rust Codegen

FFI contracts generate `extern "C"` functions with safety wrappers:

```rust
/// # Safety
/// - `filename` must be non-null, valid null-terminated UTF-8
/// - `ppDb` must be non-null and point to valid memory for *mut Db
#[no_mangle]
pub unsafe extern "C" fn sqlite3_open(
    filename: *const c_char,
    ppDb: *mut *mut Connection,
) -> c_int {
    // Validate caller guarantees (debug only)
    debug_assert!(!filename.is_null());
    debug_assert!(!ppDb.is_null());

    let filename = match CStr::from_ptr(filename).to_str() {
        Ok(s) => s,
        Err(_) => {
            *ppDb = std::ptr::null_mut();
            return SQLITE_CANTOPEN;
        }
    };

    match Connection::open(filename) {
        Ok(conn) => {
            *ppDb = Box::into_raw(Box::new(conn));
            SQLITE_OK
        }
        Err(e) => {
            // SQLite contract: ppDb may still be set on error
            *ppDb = std::ptr::null_mut();
            e.to_sqlite_code()
        }
    }
}
```


#### SEC.3 Constant-Time Execution

Contracts that verify a function's execution time is independent of
secret inputs, preventing timing side-channel attacks.

##### Motivation

WireGuard's `crypto_memneq()` compares MACs using XOR accumulation
with no early exit. Curve25519 uses a constant-time Montgomery ladder.
If any branch depends on a secret value, an attacker can measure
execution time to extract the key. No other Assura feature addresses
this: taint tracking verifies where data flows, not how computation
behaves on it.

##### Grammar

```ebnf
ConstTimeAnnotation = '#[constant_time]' ;
SecretAnnotation    = '#[secret]' ;
ConstTimeBlock      = 'constant_time' '{' Block '}' ;
```

##### Full Example

```assura
#[constant_time]
fn mac_verify(
    computed: Bytes #[secret],
    received: Bytes
) -> Bool
  requires computed.len() == received.len()
  ensures result == (computed == received)
{
    // Compiler rejects: early-exit comparison
    // Compiler rejects: secret-dependent array index
    // Compiler rejects: secret-dependent branch

    let mut acc: U8 = 0
    for i in 0..computed.len() {
        acc = acc | (computed[i] ^ received[i])
    }
    acc == 0
}

fn curve25519_scalarmult(
    scalar: U256 #[secret],
    point: Point
) -> Point
{
    // Montgomery ladder: both branches execute identical operations
    constant_time {
        let mut r0 = IDENTITY
        let mut r1 = point
        for bit in (0..256).rev() {
            let b = scalar.bit(bit)  // secret-dependent value
            // cswap executes same instructions regardless of b
            cswap(b, &mut r0, &mut r1)
            r1 = point_add(r0, r1)
            r0 = point_double(r0)
            cswap(b, &mut r0, &mut r1)
        }
        r0
    }
}
```

##### Verification Rule

The verifier performs information-flow analysis on `#[secret]` data:
1. No branch condition may depend on secret data
2. No array index may depend on secret data
3. No loop bound may depend on secret data
4. No variable-time instruction (division, modulo on some architectures) on secret data
5. `constant_time { }` blocks are verified as a unit

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-SEC-010 | Secret-dependent branch | if/match condition depends on #[secret] value |
| A-SEC-011 | Secret-dependent index | Array index computed from #[secret] value |
| A-SEC-012 | Variable-time operation on secret | Division or modulo on #[secret] data |
| A-SEC-013 | Secret leaks through timing | Function not marked #[constant_time] uses #[secret] |
| A-SEC-014 | Non-constant-time call in constant_time block | Calling unmarked function from constant_time block |

##### Rust Codegen

`#[constant_time]` generates normal Rust code; the timing guarantee is
verified at compile time. The compiler may additionally emit:
- `core::hint::black_box()` around secret comparisons
- Platform-specific constant-time intrinsics (e.g., `subtle::ConstantTimeEq`)

```rust
fn mac_verify(computed: &[u8], received: &[u8]) -> bool {
    debug_assert_eq!(computed.len(), received.len());
    let mut acc = 0u8;
    for i in 0..computed.len() {
        acc |= computed[i] ^ received[i];
    }
    core::hint::black_box(acc) == 0
}
```

#### SEC.4 Secure Erasure

Contracts that guarantee secret data is zeroed in memory when no
longer needed, preventing key material from lingering.

##### Motivation

WireGuard calls `memzero_explicit()` on all handshake keys after
`wg_noise_handshake_begin_session()`. Linear types ensure single-use
but not zeroing: a key consumed by linearity could be freed without
overwriting. The compiler's dead-store elimination may optimize away
a `memset(key, 0, 32)` if it sees no subsequent read.

##### Grammar

```ebnf
SecureEraseAnnotation = '#[secure_erase]' ;
EraseExpr             = 'erase' '(' Expr ')' ;
```

##### Full Example

```assura
type SessionKey #[secure_erase] {
    key: Bytes(32),
    nonce_counter: U64
}

fn handshake_complete(
    hs: Handshake #[linear]
) -> SessionKey
  ensures memory_zeroed(hs)
{
    let session = derive_session(hs.chaining_key, hs.hash)
    erase(hs)  // compiler emits volatile zero + barrier
    session
}

fn session_expired(key: SessionKey #[linear]) -> ()
  ensures memory_zeroed(key)
{
    erase(key)
}
```

##### Verification Rule

Types marked `#[secure_erase]` must be erased before deallocation:
1. Every code path that drops the value must call `erase()` first
2. `erase()` emits a volatile write of zeros + compiler barrier
3. Copying a `#[secure_erase]` value is forbidden (linear by default)
4. Passing to a function transfers the erasure obligation

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-SEC-015 | Secret dropped without erasure | #[secure_erase] value dropped without erase() |
| A-SEC-016 | Secret copied | #[secure_erase] value cannot be copied |
| A-SEC-017 | Erasure may be optimized out | erase() not using volatile write |

##### Rust Codegen

`erase()` emits `zeroize::Zeroize` trait call or inline volatile
write with a compiler fence:

```rust
impl Drop for SessionKey {
    fn drop(&mut self) {
        // Volatile write prevents dead-store elimination
        unsafe {
            core::ptr::write_volatile(&mut self.key as *mut [u8; 32], [0u8; 32]);
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}
```

#### SEC.5 Cryptographic Specification Conformance

Contracts that verify a cryptographic implementation correctly
implements its mathematical specification, connecting C/Rust code
to formal algorithm definitions from standards documents.

##### Motivation

mbedTLS implements AES (FIPS 197), ECDSA (FIPS 186-5), RSA
(PKCS#1 v2.2), and dozens of other algorithms. A wrong constant
in the NIST P-256 curve parameters, a subtle error in modular
reduction, or an off-by-one in the key schedule would silently
produce incorrect cryptographic output. Existing features provide
building blocks (CORE.4 axioms define the math, CORE.2 lemmas
prove properties, NUM.2 verifies precomputed tables), but no
feature expresses the top-level claim: "this function correctly
implements AES-128 as defined by FIPS 197."

This is distinct from FMT.6 (Protocol Grammar Conformance), which
verifies parsers against ABNF/BNF grammars. Cryptographic
conformance verifies algorithms against algebraic/mathematical
specifications. The verification techniques differ: grammar
conformance uses language-theoretic checking; crypto conformance
uses algebraic reasoning and equational proofs.

Projects like HACL\*, Fiat-Crypto, and Jasmin provide this level
of assurance. Assura makes it a first-class language feature.

##### Grammar

```ebnf
CryptoConformanceAnnotation = '#[conforms' '(' SpecRef ')' ']' ;
SpecRef        = StringLit ;      // e.g., "FIPS_197_AES_128"
AlgorithmDecl  = '#[conforms' '(' SpecRef ')' ']'
                 'fn' Ident '(' ParamList ')' '->' Type
                 '{' AlgorithmBody '}' ;
AlgorithmBody  = { Statement } ;
```

##### Full Example: AES-128 Block Cipher

```assura
// Mathematical specification (axiomatic)
spec FIPS_197_AES_128 {
  // The AES round function
  axiom SubBytes(state: Matrix<4,4,GF256>) -> Matrix<4,4,GF256> {
    forall i, j: state_out[i][j] == sbox(state_in[i][j])
  }

  axiom ShiftRows(state: Matrix<4,4,GF256>) -> Matrix<4,4,GF256> {
    forall i, j: state_out[i][j] == state_in[i][(j + i) % 4]
  }

  axiom MixColumns(state: Matrix<4,4,GF256>) -> Matrix<4,4,GF256> {
    forall j: column_out[j] == gf_matrix_mul(MIX_MATRIX, column_in[j])
  }

  axiom AddRoundKey(state: Matrix<4,4,GF256>,
                    round_key: Matrix<4,4,GF256>) -> Matrix<4,4,GF256> {
    forall i, j: state_out[i][j] == state_in[i][j] xor round_key[i][j]
  }

  // Full AES-128: 10 rounds with specified structure
  axiom encrypt(plaintext: Bytes(16), key: Bytes(16)) -> Bytes(16) {
    let state = to_matrix(plaintext)
    let round_keys = key_expansion(key)  // 11 round keys
    let s0 = AddRoundKey(state, round_keys[0])
    let s_mid = fold(1..10, s0, fn(s, r) {
      AddRoundKey(MixColumns(ShiftRows(SubBytes(s))), round_keys[r])
    })
    let s_final = AddRoundKey(ShiftRows(SubBytes(s_mid)), round_keys[10])
    from_matrix(s_final)
  }
}

// Implementation verified against the spec
#[conforms("FIPS_197_AES_128")]
fn aes_128_encrypt(
  plaintext: &[u8; 16],
  key: &[u8; 16]
) -> [u8; 16]
  ensures result == FIPS_197_AES_128.encrypt(plaintext, key)
{
  let mut state = load_state(plaintext)
  let round_keys = expand_key(key)

  state = xor_state(state, round_keys[0])
  for r in 1..10 {
    state = sub_bytes(state)
    state = shift_rows(state)
    state = mix_columns(state)
    state = xor_state(state, round_keys[r])
  }
  state = sub_bytes(state)
  state = shift_rows(state)
  state = xor_state(state, round_keys[10])
  store_state(state)
}
```

##### Verification Rules

1. The `#[conforms(spec)]` annotation binds the function to a
   named specification; the verifier must prove the function's
   output matches the spec's definition for all valid inputs
2. Helper functions (e.g., `sub_bytes`, `mix_columns`) may have
   their own `#[conforms]` annotations for compositional proof
3. Precomputed tables used by the implementation (S-boxes, round
   constants, curve parameters) are verified against the spec's
   axiomatic definitions via NUM.2
4. Optimized implementations (AESNI intrinsics, fast NIST
   reduction) must also conform; PERF.1 escape requires the same
   conformance proof as the reference implementation
5. The spec itself is trusted (axiomatic); the implementation is
   verified against it

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-SEC-018 | Algorithm does not conform to spec | Implementation output differs from spec for some input |
| A-SEC-019 | Missing conformance spec | #[conforms] references undefined spec |
| A-SEC-020 | Helper does not conform | Sub-function used by conforming function fails its own spec |
| A-SEC-021 | Optimized path diverges | PERF.1 escape produces different output than spec |

##### Rust Codegen

Conformance contracts generate debug-mode comparison against a
reference implementation derived from the spec:

```rust
#[cfg(debug_assertions)]
fn aes_128_encrypt_checked(plaintext: &[u8; 16], key: &[u8; 16]) -> [u8; 16] {
    let result = aes_128_encrypt(plaintext, key);
    let reference = aes_128_reference(plaintext, key);
    debug_assert_eq!(result, reference,
        "AES-128 conformance failure: implementation diverges from FIPS 197");
    result
}

// Release mode: direct implementation, no runtime check
// (correctness proven at verification time)
#[cfg(not(debug_assertions))]
fn aes_128_encrypt_checked(plaintext: &[u8; 16], key: &[u8; 16]) -> [u8; 16] {
    aes_128_encrypt(plaintext, key)
}
```


### 14.CONC: Concurrency

#### CONC.1 Shared Memory Protocols

Verification of concurrent access patterns across OS processes that
share memory-mapped regions.

##### Motivation

SQLite's WAL-Reset Bug (present for 16 years, 2010-2026) was a data
race between two processes accessing the `-shm` (shared memory)
file. It required precise timing to reproduce and was never caught
by testing, fuzzing, or 100% MC/DC coverage. A formal model of the
shared memory locking protocol would have caught it.

##### Grammar

```ebnf
SharedMemoryDecl = 'shared_memory' TypeIdent '{'
                     LayoutDecl
                     { ProtocolDecl }
                     { SharedInvariant }
                   '}' ;

LayoutDecl       = 'layout' '{' { LayoutField } '}' ;
LayoutField      = Ident ':' SharedFieldType ';' ;
SharedFieldType  = 'Lock'
                 | 'Atomic' '<' TypeExpr '>'
                 | 'Array' '<' TypeExpr ',' IntLit '>'
                 | TypeExpr ;

SharedInvariant  = 'shared_invariant' ':' Predicate ;

ProtocolDecl     = 'protocol' TypeIdent '{'
                     { ProtocolStep }
                   '}' ;

ProtocolStep     = 'acquire' '(' Ident ')'
                 | 'release' '(' Ident ')'
                 | 'atomic_load' '(' Ident ')'
                 | 'atomic_store' '(' Ident ',' Expr ')'
                 | 'read' '(' Ident ')'
                 | 'write' '(' Ident ',' Expr ')'
                 | ProtocolStep '->' ProtocolStep ;
```

##### Type System

Shared memory protocols are verified using multiparty session types
extended to shared memory. The compiler models each participant
(process) as a party and verifies:

1. **No data race**: Reads and writes to non-atomic fields are
   protected by locks
2. **Lock ordering**: Locks are acquired in a consistent order
   across all protocols (no deadlock)
3. **Atomic correctness**: Atomic operations use appropriate memory
   ordering
4. **Protocol compliance**: Each process follows its declared protocol

##### Full Example: WAL Protocol

```assura
shared_memory WalIndex {
  layout {
    write_lock: Lock;
    checkpoint_lock: Lock;
    recover_lock: Lock;
    read_locks: Array<Lock, 5>;
    max_frame: Atomic<U32>;
    max_page: Atomic<U32>;
    frame_checksums: Array<U64, MAX_WAL_FRAMES>;
    checkpoint_seq: Atomic<U32>;
  }

  // Writer protocol
  protocol Writer {
    acquire(write_lock)
    -> read(frame_checksums)         // read under write lock
    -> write(frame_checksums, new)   // append new frames
    -> atomic_store(max_frame, new_max)  // publish atomically
    -> release(write_lock)
  }

  // Reader protocol
  protocol Reader {
    atomic_load(max_frame)           // snapshot max_frame
    -> acquire(read_locks[slot])     // mark our snapshot slot
    -> read(frame_checksums)         // read frames up to snapshot
    -> release(read_locks[slot])
  }

  // Checkpoint protocol
  protocol Checkpointer {
    acquire(checkpoint_lock)
    -> atomic_load(max_frame)
    -> for_each read_locks[i]:       // wait for readers past checkpoint
         wait_until(read_locks[i].snapshot >= checkpoint_frame)
    -> write(database_file, pages)   // copy WAL pages to DB
    -> atomic_store(checkpoint_seq, seq + 1)
    -> release(checkpoint_lock)
  }

  // COMPILER VERIFIES:
  // 1. Writer and Reader never write the same non-atomic field
  //    without lock protection
  // 2. Checkpointer waits for all readers before overwriting
  // 3. Lock ordering: write_lock > checkpoint_lock > read_locks
  //    (no deadlock possible)
  // 4. max_frame is always written by Writer before read by Reader
  //    (no stale snapshot)

  shared_invariant:
    forall frame_index in 0..max_frame:
      frame_checksums[frame_index] == compute_checksum(wal_file, frame_index)
}
```

##### Verification Approach

Multi-process protocols are verified using bounded model checking
(Kani/CBMC-style) at layer 2. The compiler explores all interleavings
of protocol steps up to a configurable bound:

```toml
[verify.shared_memory]
interleaving_bound = 100       # max interleaving steps to explore
process_count = 3              # max concurrent processes
timeout_ms = 30000             # 30s budget for model checking
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A29001 | Data race on shared field `F` | Unprotected concurrent access |
| A29002 | Deadlock possible | Lock ordering violation |
| A29003 | Stale read: `F` may be modified between load and use | Missing lock or atomic |
| A29004 | Protocol violation: step `S` out of order | Process doesn't follow protocol |
| A29005 | Reader may see partial write | Non-atomic multi-field update |

##### Rust Codegen

Shared memory protocols generate Rust code using platform-specific
primitives:

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use memmap2::MmapMut;

pub struct WalIndex {
    mmap: MmapMut,
}

impl WalIndex {
    pub fn max_frame(&self) -> u32 {
        let ptr = &self.mmap[MAX_FRAME_OFFSET] as *const u8 as *const AtomicU32;
        unsafe { (*ptr).load(Ordering::Acquire) }
    }

    pub fn set_max_frame(&self, val: u32) {
        let ptr = &self.mmap[MAX_FRAME_OFFSET] as *const u8 as *const AtomicU32;
        unsafe { (*ptr).store(val, Ordering::Release) }
    }
}
```

The compiler generates correct `Ordering` annotations (Acquire for
loads, Release for stores) based on the protocol analysis.


#### CONC.2 Callback and Re-entrancy Safety

Contracts on user-supplied callbacks that restrict what operations
the callback may perform, preventing re-entrancy bugs and deadlocks.

##### Motivation

SQLite has 12+ callback hooks: authorizer, busy handler, progress
handler, commit hook, rollback hook, update hook, WAL hook, collation
callback, function callback, etc. If a callback calls back into the
database (e.g., executing a query inside the authorizer), the result
is corruption or deadlock. SQLite documents these restrictions in
prose; Assura enforces them at compile time.

##### Grammar

```ebnf
CallbackDecl   = 'callback' TypeIdent '{'
                   'signature' ':' FnType
                   { CallbackConstraint }
                 '}' ;

CallbackConstraint = 'must_not_call' ':' IdentList
                   | 'must_not_reenter' ':' IdentList
                   | 'max_duration' ':' DurationExpr
                   | 'may_call' ':' IdentList
                   | 'must_be' ':' CallbackProperty ;

CallbackProperty = 'pure' | 'deterministic' | 'infallible'
                 | 'idempotent' | 'thread_safe' ;

CallbackInstall = 'on_install' ':' Predicate ;
```

##### Full Example: SQLite Callbacks

```assura
// Authorizer: called during query compilation
callback Authorizer {
  signature: (action: AuthAction, arg1: String?,
              arg2: String?, db_name: String?,
              trigger: String?) -> AuthResult

  // MUST NOT call any database operation (re-entrancy)
  must_not_reenter: Connection

  // MUST NOT allocate from the database's allocator
  must_not_call: sqlite3_malloc, sqlite3_free

  // Must complete quickly (called per-statement-node)
  must_be: infallible

  // May only read connection configuration
  may_call: sqlite3_db_config_read
}

// Busy handler: called when a lock cannot be acquired
callback BusyHandler {
  signature: (context: Opaque, count: Nat) -> Bool

  // MUST NOT call operations that acquire locks
  must_not_call: sqlite3_exec, sqlite3_step, sqlite3_prepare

  // MUST NOT re-enter the connection that triggered it
  must_not_reenter: Connection

  // Should eventually return false (termination)
  // (not enforced at compile time, but generates a warning)
}

// Progress handler: called periodically during long queries
callback ProgressHandler {
  signature: (context: Opaque) -> Bool

  // May call read-only operations on OTHER connections
  may_call: sqlite3_exec where connection != self.connection

  // MUST NOT modify the connection that installed it
  must_not_reenter: Connection

  must_be: thread_safe
}

// Collation callback: called for string comparison
callback CollationCallback {
  signature: (a: Region<n>, b: Region<m>) -> Ordering

  must_be: deterministic  // Same inputs must give same ordering
  must_be: pure           // No side effects
  must_be: infallible     // Cannot fail

  // Transitivity: if a < b and b < c then a < c
  invariant {
    forall a, b, c:
      compare(a, b) == Less and compare(b, c) == Less
      => compare(a, c) == Less
  }

  // Antisymmetry: if a < b then b > a
  invariant {
    forall a, b:
      compare(a, b) == Less <=> compare(b, a) == Greater
  }

  // Reflexivity: a == a
  invariant { forall a: compare(a, a) == Equal }
}

// Installing a callback
service Database {
  operation set_authorizer {
    input(auth: Authorizer)

    // Only one authorizer at a time
    ensures { self.authorizer == Some(auth) }

    // Previous authorizer is replaced (linear: old one dropped)
    ensures { old(self.authorizer) is dropped }

    on_install: self.state @ Open
  }
}
```

##### Verification Rule

1. When a callback type is used, the compiler collects all
   `must_not_call` and `must_not_reenter` constraints
2. Any implementation of the callback is checked: its call graph
   must not include any function in the `must_not_call` list
3. Re-entrancy is checked by verifying the callback's transitive
   call graph does not include any method on the `must_not_reenter`
   target
4. `must_be: deterministic` triggers determinism analysis (see CONC.3)
5. `must_be: pure` verifies no effects beyond `pure`
6. Callback invariants (e.g., transitivity for collation) are checked
   via SMT at Layer 2

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A34001 | Callback `C` re-enters `T` via call chain `F1->F2->...` | Re-entrancy detected in transitive call graph |
| A34002 | Callback `C` calls prohibited function `F` | Direct or indirect call to must_not_call target |
| A34003 | Callback `C` is not deterministic | Non-deterministic operation in callback body |
| A34004 | Callback `C` may fail but is marked infallible | Error path exists in callback body |
| A34005 | Callback invariant not satisfiable | Transitivity/antisymmetry not provable |

##### Rust Codegen

Callback contracts generate trait bounds with marker types:

```rust
/// Marker: callback must not re-enter Connection
pub trait NoReenter<T> {}

/// Authorizer callback trait
pub trait Authorizer: NoReenter<Connection> + Send {
    fn authorize(
        &self,
        action: AuthAction,
        arg1: Option<&str>,
        arg2: Option<&str>,
        db_name: Option<&str>,
        trigger: Option<&str>,
    ) -> AuthResult;
    // No Result<> return: infallible
}

/// Collation callback trait
pub trait Collation: Send + Sync {
    fn compare(&self, a: &[u8], b: &[u8]) -> std::cmp::Ordering;
    // Pure + deterministic: no &mut self, no interior mutability
}

impl Connection {
    pub fn set_authorizer<A: Authorizer>(&mut self, auth: A) {
        debug_assert!(self.state == ConnectionState::Open);
        self.authorizer = Some(Box::new(auth));
    }
}
```


#### CONC.3 Determinism Contracts

Contracts that guarantee identical output for identical input,
which is stronger than purity. Pure functions have no side effects;
deterministic functions additionally produce bit-identical results
across invocations, platforms, and compiler versions.

##### Motivation

SQLite guarantees that the same query on the same database produces
the same result bytes. This is critical for:
- Replication (replicas must agree)
- Testing (golden file tests)
- Digital signatures over query results
- WAL checksums (same data must produce same checksum)

Rust's `HashMap` iteration order is non-deterministic (randomized
seed). Floating-point operations can differ across platforms.
`Instant::now()` introduces time-dependence. A `pure` function
could use any of these.

##### Grammar

```ebnf
DeterministicAnnotation = '#[deterministic]' ;

DeterministicConstraints = 'deterministic_requires' '{'
                             { DeterministicRule }
                           '}' ;

DeterministicRule = 'no_hash_iteration'
                  | 'no_float_transcendentals'
                  | 'no_pointer_comparison'
                  | 'no_allocation_address'
                  | 'no_time_dependence'
                  | 'no_random'
                  | 'ordered_collections_only'
                  | 'fixed_float_rounding' ;
```

##### Full Example: Deterministic Query Execution

```assura
#[deterministic]
fn execute_select(
    db: Database,
    stmt: PreparedStatement,
    params: List<Value>
) -> List<Row>
  requires { stmt.is_valid() }
  requires { db.state @ Open }
  ensures {
    // Same db + same stmt + same params = same result, always
    forall db1, db2, params1, params2:
      db1.content == db2.content
      and params1 == params2
      => execute_select(db1, stmt, params1)
         == execute_select(db2, stmt, params2)
  }
  effects: database.read
{
  // Inside a #[deterministic] function, the compiler rejects:
  //   - HashMap (use BTreeMap)
  //   - HashSet (use BTreeSet)
  //   - Instant::now() or any time source
  //   - thread::current().id()
  //   - pointer-to-integer casts
  //   - address-dependent comparisons
  //   - f64::sin/cos/exp (platform-dependent rounding)
  //     unless #[fixed_float_rounding] is active
}

#[deterministic]
fn wal_checksum(
    data: Region<n>,
    seed: (U32, U32)
) -> (U32, U32)
  effects: pure
{
  // Checksum must be deterministic so crash recovery can
  // verify frames written by a different process
  let (mut s1, mut s2) = seed
  for i in 0..n/4 {
    let word = read_u32_native(data, i * 4)
    s1 = s1 +% word   // wrapping add
    s2 = s2 +% s1
  }
  (s1, s2)
}

// COMPILE ERROR: non-deterministic function marked deterministic
#[deterministic]
fn bad_example(items: Map<String, Int>) -> List<String>
  effects: pure
{
  items.keys()  // A35001: Map iteration order is not deterministic
                // Use BTreeMap or sort the keys explicitly
}
```

##### Banned Patterns

The compiler maintains a list of non-deterministic operations. Any
call to these from a `#[deterministic]` function is a compile error:

| Pattern | Why Non-Deterministic | Alternative |
|---|---|---|
| `HashMap` / `HashSet` | Randomized hash seed | `BTreeMap` / `BTreeSet` |
| `f64::sin`, `f64::exp` | Platform-dependent rounding | Fixed-point or `#[fixed_float_rounding]` |
| `Instant::now()` | Time-dependent | Pass timestamp as parameter |
| `thread_rng()` | Random | Pass seed as parameter |
| `ptr as usize` | Address-dependent | Use indices, not pointers |
| `Arc::as_ptr` comparison | Allocation-dependent | Compare by value |
| `TypeId::of` | Compiler-dependent | Use explicit discriminant |

##### Verification Rule

1. The compiler performs taint analysis: any non-deterministic
   source taints all downstream values
2. A `#[deterministic]` function's return value must not be tainted
3. Calling a non-deterministic function from a deterministic one
   is an error, even if the result is unused (may affect control flow)
4. `#[deterministic]` implies `pure` (no side effects)
5. `#[deterministic]` is transitive: all callees must also be
   deterministic

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A35001 | Non-deterministic collection in deterministic context | HashMap/HashSet used |
| A35002 | Platform-dependent float in deterministic context | Transcendental function used |
| A35003 | Time/random source in deterministic context | Instant, SystemTime, or RNG used |
| A35004 | Pointer-derived value in deterministic context | Address used in computation |
| A35005 | Callee `F` is not deterministic | Calling non-deterministic function |

##### Rust Codegen

Determinism contracts are primarily compile-time. The generated
Rust code uses lint attributes to catch accidental non-determinism:

```rust
// Deterministic function: generated with ordered collections
#[cfg_attr(debug_assertions, track_caller)]
pub fn execute_select(
    db: &Database,
    stmt: &PreparedStatement,
    params: &[Value],
) -> Vec<Row> {
    // Compiler ensures BTreeMap is used internally, not HashMap
    let mut results: BTreeMap<RowId, Row> = BTreeMap::new();
    // ... query execution ...
    results.into_values().collect()
}

// Deterministic checksum
#[inline]
pub fn wal_checksum(data: &[u8], seed: (u32, u32)) -> (u32, u32) {
    let (mut s1, mut s2) = seed;
    for chunk in data.chunks_exact(4) {
        let word = u32::from_ne_bytes(chunk.try_into().unwrap());
        s1 = s1.wrapping_add(word);
        s2 = s2.wrapping_add(s1);
    }
    (s1, s2)
}
```


#### CONC.4 Lock Ordering

Contracts that specify a ranked hierarchy of locks and verify no code
path acquires them out of order, preventing deadlocks.

##### Motivation

jemalloc defines ~30 ranked lock levels (witness system in
`witness.c`). `witness_lock_error_impl()` aborts on rank reversal at
runtime. Shared memory (CONC.1) verifies data-race freedom on
individual variables; it does not express "mutex A (rank 45) must be
acquired before mutex B (rank 0x1000)" across a call graph.

##### Grammar

```ebnf
LockRankDecl    = 'lock_rank' Ident '=' IntLit ';' ;
LockAnnotation  = '#[lock_rank(' Ident ')]' ;
LockOrderRule   = 'lock_order' '{' LockRankDecl { LockRankDecl } '}' ;
```

##### Full Example

```assura
lock_order {
    lock_rank CORE      = 0
    lock_rank BIN       = 10
    lock_rank ARENA     = 20
    lock_rank EXTENT    = 30
    lock_rank TCACHE    = 40
    lock_rank PROF      = 50
}

type BinMutex #[lock_rank(BIN)] { inner: Mutex }
type ArenaMutex #[lock_rank(ARENA)] { inner: Mutex }

fn arena_bin_alloc(
    arena: &Arena,
    bin: &Bin
) -> Ptr
  requires held_locks_below(BIN)
{
    arena.lock.acquire()   // rank ARENA = 20
    bin.lock.acquire()     // rank BIN = 10, ERROR: 10 < 20
}
```

##### Verification Rule

1. Each lock acquisition records the rank in a ghost set
2. Acquiring a lock with rank <= max(held_ranks) is an error
3. Releasing a lock removes it from the ghost set
4. Equal-rank locks may use address ordering (annotated explicitly)
5. Call graph analysis propagates lock requirements across functions

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-CONC-010 | Lock order violation | Acquiring lock with rank <= held max |
| A-CONC-011 | Missing lock rank annotation | Mutex without #[lock_rank] |
| A-CONC-012 | Equal-rank without address order | Same-rank locks without ordering tiebreaker |

##### Rust Codegen

Lock ordering is verified at compile time and erased. In debug mode,
a runtime witness system mirrors jemalloc's approach:

```rust
#[cfg(debug_assertions)]
thread_local! {
    static HELD_RANKS: RefCell<Vec<u32>> = RefCell::new(Vec::new());
}

fn acquire_lock(lock: &Mutex, rank: u32) {
    #[cfg(debug_assertions)]
    HELD_RANKS.with(|ranks| {
        let ranks = ranks.borrow();
        if let Some(&max) = ranks.iter().max() {
            debug_assert!(rank > max, "lock order violation: {rank} <= {max}");
        }
    });
    lock.lock();
    #[cfg(debug_assertions)]
    HELD_RANKS.with(|ranks| ranks.borrow_mut().push(rank));
}
```

#### CONC.5 Temporal State Deadlines

Contracts that bind state transitions to time bounds, verifying that
time-triggered actions occur within their deadlines.

##### Motivation

WireGuard rekeying is timer-driven: `REKEY_AFTER_TIME = 120s`,
`REJECT_AFTER_TIME = 180s`. After 180 seconds, a keypair MUST be
zeroed. Typestate tracks which transitions are valid; monotonic state
tracks values that increase. Neither expresses "after T seconds in
state S, must transition to state S'." WireGuard's `timers.c` has 5
concurrent per-peer timers with interacting deadlines.

##### Grammar

```ebnf
DeadlineAnnotation = '#[deadline(' Duration ')]' ;
TimeoutTransition  = 'timeout' '(' State ',' Duration ')' '->' State ;
Duration           = IntLit ('ms' | 's' | 'min') ;
```

##### Full Example

```assura
states KeypairState {
    Created -> Active -> Expired -> Zeroed
}

transition keypair_lifecycle {
    timeout(Active, 120s) -> Expired   // REKEY_AFTER_TIME
    timeout(Expired, 60s) -> Zeroed    // REJECT_AFTER_TIME - REKEY
    timeout(Created, 5s) -> Zeroed     // REKEY_TIMEOUT
}

contract WireGuardPeer {
    invariant keypair.state != Active
              || elapsed(keypair.activated_at) < 180s
    invariant keypair.state == Zeroed
              ==> memory_zeroed(keypair.key)
}
```

##### Verification Rule

1. Timer-triggered transitions are verified against the state machine
2. The verifier checks that all timer handlers exist and transition
   to the declared target state
3. Overlapping deadlines are detected (two timers on same state)
4. Missing timeout handlers generate compile errors

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-CONC-013 | Missing timeout handler | State has deadline but no timer handler |
| A-CONC-014 | Deadline exceeded | State held beyond declared timeout |
| A-CONC-015 | Conflicting deadlines | Two timeouts on same state |

##### Rust Codegen

Deadlines generate timer registration and handler dispatch:

```rust
impl Peer {
    fn activate_keypair(&mut self, keypair: Keypair) {
        self.keypair = keypair;
        self.timers.set(Timer::Rekey, Duration::from_secs(120));
        self.timers.set(Timer::Reject, Duration::from_secs(180));
    }

    fn handle_timer(&mut self, timer: Timer) {
        match timer {
            Timer::Rekey => self.initiate_rekey(),
            Timer::Reject => {
                self.keypair.zeroize();
                self.keypair_state = KeypairState::Zeroed;
            }
        }
    }
}
```

#### CONC.6 Weak Memory Ordering

Contracts for programs that use non-sequential-consistency atomic
orderings. Instead of a single global shared state, each thread
maintains a ghost **view** of memory. Atomic operations with
different orderings (Relaxed, Acquire, Release, AcqRel, SeqCst)
generate different ghost view constraints.

##### Motivation

Every high-performance Rust concurrent data structure uses weak
memory orderings. crossbeam-epoch uses Relaxed loads for fast
pinning checks. parking_lot uses Relaxed loads in spin loops.
arc-swap uses Acquire-Release pairs. seqlock uses Relaxed loads
with explicit fences.

Without weak memory support, Assura can only verify under
sequential consistency, which is sound (a program correct under
SeqCst is correct under weaker orderings) but incomplete (programs
that are correct under Acquire-Release may not be expressible
under SeqCst, because the SeqCst model adds unnecessary ordering
constraints).

##### Syntax

```assura
// Atomic operations carry their ordering annotation
// (matching Rust's std::sync::atomic)
shared x: AtomicU64 {
  // Per-thread views: each thread has its own view of x
  ghost view: Map<ThreadId, ViewTimestamp>
}

fn writer(x: &AtomicU64, val: u64) {
  // Release store: publishes all prior writes to x's view
  x.store(val, ordering: release)
  // Ghost effect: x.view[self_tid] = merge(x.view[self_tid], self.view)
  // All writes before this point become visible to any
  // thread that does an acquire load of x
}

fn reader(x: &AtomicU64) -> u64 {
  // Acquire load: merges x's published view into this thread's view
  let v = x.load(ordering: acquire)
  // Ghost effect: self.view = merge(self.view, x.view[writer_tid])
  // All writes the writer did before its release store
  // are now visible to this thread
  ensures v == x.value  // value is consistent with the view
}

fn fast_check(x: &AtomicU64) -> u64 {
  // Relaxed load: no view synchronization
  let v = x.load(ordering: relaxed)
  // Ghost effect: none. This thread's view is NOT updated.
  // The value may be stale (from this thread's current view)
  ensures v == x.view[self_tid].value_of(x)
}
```

##### The View Model

Each thread carries a ghost view map: a partial function from
memory locations to the last-seen write timestamp. Atomic
operations update views according to their ordering:

| Ordering | Ghost View Effect |
|---|---|
| `relaxed` | No view change. Read from this thread's current view |
| `acquire` (load) | Merge source location's published view into reader's view |
| `release` (store) | Publish writer's current view to the location |
| `acq_rel` (RMW) | Both: merge then publish |
| `seq_cst` | Total order maintained. All SeqCst ops see a single global view |
| `fence(acquire)` | Merge all prior relaxed loads' source views |
| `fence(release)` | Publish current view to all subsequent relaxed stores |

The view model is based on the GPS/RSL (Relaxed Separation Logic)
approach, adapted for SMT verification.

##### Example: seqlock Reader

```assura
struct SeqLock<T> {
  seq: AtomicU64,  // even = unlocked, odd = write in progress
  data: T,
}

fn read(lock: &SeqLock<T>) -> T {
  loop {
    let s1 = lock.seq.load(ordering: acquire)
    requires s1 % 2 == 0  // must read even (unlocked)

    fence(ordering: acquire)
    let result = lock.data.clone()

    let s2 = lock.seq.load(ordering: acquire)

    if s1 == s2 {
      // No writer intervened: result is consistent
      ensures result == lock.data.at_view(self.view)
      return result
    }
    // Writer intervened: retry. The relaxed observation of
    // an odd sequence number means the data may be torn.
  }
}
```

##### Example: crossbeam Epoch Pin Check

```assura
fn is_pinned(guard: &Guard) -> bool {
  // Fast path: relaxed load is safe because we only need
  // a hint. If stale, the worst case is an extra epoch advance.
  let epoch = GLOBAL_EPOCH.load(ordering: relaxed)
  // Under weak memory, this might read a stale epoch.
  // The contract says: the result is valid for this thread's
  // current view, but may not reflect other threads' advances.
  ensures result == (guard.local_epoch == epoch)
    || stale_view(self.view, GLOBAL_EPOCH)
}
```

##### Verification Rule

1. **View tracking**: The verifier maintains a ghost view per
   thread. Each atomic operation generates view constraints
   based on its ordering annotation
2. **Consistency check**: The verifier checks that all read
   values are consistent with the reading thread's current
   view. A Relaxed load may read any value that was written
   and not yet overwritten in the thread's view
3. **Happens-before**: An acquire-load from location X after a
   release-store to X creates a happens-before edge. The
   verifier checks that all assertions that depend on
   happens-before are reachable only through valid edges
4. **Layer 1**: View tracking for Acquire-Release is decidable
   (QF_UFLIA). Relaxed with fences may require Layer 2
5. **Interaction with CONC.1**: The shared memory protocol
   (CONC.1) specifies the protocol-level state machine. CONC.6
   specifies the memory-ordering semantics WITHIN that protocol.
   They compose: the protocol says "state A transitions to
   state B via this CAS"; CONC.6 says "the CAS uses AcqRel
   ordering, so views merge accordingly"

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-CONC-016 | Relaxed read without view check | Read depends on value ordering but uses Relaxed |
| A-CONC-017 | Missing release before acquire | Data written without Release read with Acquire |
| A-CONC-018 | View inconsistency | Thread asserts value visible but view has not merged |
| A-CONC-019 | Fence ordering mismatch | Fence type does not match surrounding operations |
| A-CONC-020 | SeqCst total order violation | SeqCst operations form a cycle in total order |

##### Rust Codegen

CONC.6 contracts compile to the same Rust atomic operations the
user specified. The ordering annotations are preserved exactly.
In debug builds, view assertions generate runtime checks:

```rust
// Source: x.store(val, ordering: release)
// Generated Rust:
x.store(val, Ordering::Release);

// Source: x.load(ordering: acquire)
// Generated Rust:
let v = x.load(Ordering::Acquire);

// Debug build: optional view consistency assertion
#[cfg(debug_assertions)]
{
    // View merge simulation for testing
    debug_assert!(
        THREAD_VIEW.lock().unwrap().is_consistent_with(v),
        "CONC.6: acquire load returned value inconsistent with view"
    );
}
```

### 14.STOR: Storage and Durability

#### STOR.1 Crash Recovery Contracts

Contracts that specify what happens when a system crashes at any
point during a multi-step operation. Unlike typestate (which tracks
normal state transitions), crash recovery reasons about
non-deterministic failure points and the procedure that restores
invariants on restart.

##### Motivation

SQLite's WAL commit has ~12 distinct crash points. If power fails
between writing a WAL frame and updating the WAL index, the recovery
algorithm must detect the inconsistency and replay valid frames.
SQLite's rollback journal has a similar protocol. These protocols
were designed by hand and verified by crash-testing. Assura makes
them verifiable at compile time.

##### Grammar

```ebnf
RecoveryDecl   = 'recovery' TypeIdent '{'
                   RecoveryStateDecl
                   { CrashPointDecl }
                   RecoveryProc
                   RecoveryInvariant
                 '}' ;

RecoveryStateDecl  = 'durable_state' '{' { FieldDecl } '}' ;
CrashPointDecl     = 'crash_point' Ident '{'
                       'at' ':' StringLit
                       'observable' ':' IdentList
                       'recovers_to' ':' Ident
                     '}' ;
RecoveryProc       = 'recover' '{' { OperationItem } '}' ;
RecoveryInvariant  = 'post_recovery' ':' Predicate ;
```

##### Full Example: WAL Commit Recovery

```assura
recovery WalCommit {
  // What is persisted on disk (survives crash)
  durable_state {
    wal_file: File,
    db_file: File,
    wal_index: SharedMemory<WalIndexHeader>,
    frames: List<WalFrame>
  }

  // Crash point 1: frame written, index not updated
  crash_point frame_written_index_stale {
    at: "after fsync(wal_file), before updating wal_index"
    observable: wal_file contains frame F
                but wal_index.max_frame < F.frame_number
    recovers_to: consistent
  }

  // Crash point 2: index updated, not fsynced
  crash_point index_updated_not_synced {
    at: "after wal_index update, before fsync(wal_index)"
    observable: wal_index.max_frame >= F.frame_number
                but index may contain garbage beyond valid frames
    recovers_to: consistent
  }

  // Crash point 3: checkpoint in progress
  crash_point checkpoint_partial {
    at: "during checkpoint, some pages copied to db_file"
    observable: db_file has mix of old and new pages
    recovers_to: consistent
  }

  // Crash point 4: WAL reset after checkpoint
  crash_point wal_reset_partial {
    at: "after checkpoint complete, during WAL truncate"
    observable: WAL file may be truncated but salt not updated
    recovers_to: consistent
  }

  // Recovery procedure: runs on database open
  recover {
    input(
      wal: File,
      db: File,
      index: SharedMemory<WalIndexHeader>
    )

    // Step 1: Reconstruct valid frame set from WAL file
    // Walk WAL from beginning, verify each frame's checksum
    // Stop at first invalid checksum (crash boundary)
    requires { wal.exists() }

    // Step 2: Rebuild WAL index from valid frames
    ensures {
      forall f in valid_frames(wal):
        index.contains(f.page_number, f.frame_number)
    }

    // Step 3: Any page in db_file is either:
    //   - the version from before the transaction, OR
    //   - the version from the last fully committed transaction
    ensures {
      forall page_num in 1..db.page_count():
        page_content(db, page_num) == original_content(page_num)
        or page_content(db, page_num) == committed_content(page_num)
    }

    effects: filesystem.read, filesystem.write
  }

  // After recovery, the database is always consistent
  post_recovery:
    database_invariant(db) and
    wal_index_matches_wal(index, wal) and
    no_partial_transactions_visible(db, wal)
}
```

##### Verification Approach

Crash recovery is verified using **bounded model checking (BMC)**:
1. Enumerate all crash points in the protocol
2. For each crash point, symbolically execute the recovery procedure
3. Verify that `post_recovery` holds after recovery from each point
4. Verify that no crash point can produce a state where recovery
   is impossible (liveness)

Budget: Layer 2, 10s per crash point. Total budget scales linearly
with the number of declared crash points.

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A32001 | Crash point `P` has no recovery path | Recovery procedure doesn't handle this state |
| A32002 | Recovery may not restore invariant `I` | Post-recovery predicate not provable |
| A32003 | Durable state modified without crash point | Write to disk without declaring what happens on crash |
| A32004 | Recovery procedure has side effects beyond repair | Recovery does more than restore consistency |

##### Rust Codegen

Crash recovery contracts generate recovery functions with
exhaustive state detection:

```rust
pub struct WalRecovery {
    wal_path: PathBuf,
    db_path: PathBuf,
}

impl WalRecovery {
    pub fn recover(&self) -> Result<(), RecoveryError> {
        let wal = File::open(&self.wal_path)?;
        let valid_frames = self.scan_valid_frames(&wal)?;

        // Rebuild index from valid frames only
        let index = WalIndex::rebuild_from(&valid_frames)?;

        // Verify post-recovery invariant in debug mode
        debug_assert!(self.verify_consistency(&index, &valid_frames));

        Ok(())
    }

    fn scan_valid_frames(&self, wal: &File)
        -> Result<Vec<WalFrame>, RecoveryError>
    {
        let mut frames = Vec::new();
        let mut offset = WAL_HEADER_SIZE;
        while offset < wal.metadata()?.len() as usize {
            match WalFrame::read_and_verify(wal, offset) {
                Ok(frame) => {
                    frames.push(frame);
                    offset += WAL_FRAME_SIZE;
                }
                Err(_) => break, // Crash boundary: stop here
            }
        }
        Ok(frames)
    }
}
```


#### STOR.2 Page Cache Contracts

Contracts for reference-counted page caches with pin/unpin
semantics, dirty tracking, and eviction policies.

##### Motivation

SQLite's pager layer is the most complex subsystem after the B-tree.
Every database page passes through a cache with these semantics:
- **Fetch**: get a page from cache or read from disk
- **Pin**: mark a page as in-use (cannot be evicted)
- **Unpin**: release a page (eligible for eviction)
- **MakeDirty**: mark a page as modified (must be written back)
- **MakeClean**: mark a page as written (can be discarded)
- **Evict**: remove unpinned clean pages under memory pressure

Bugs in this layer are catastrophic: evicting a dirty page loses
data, using an evicted page is use-after-free, double-unpin corrupts
the reference count. Allocator contracts (MEM.3) handle memory pools
but cannot express pin/unpin reference counting, dirty tracking, or
the interaction between eviction policy and transaction safety.

##### Grammar

```ebnf
CacheDecl      = 'cache' TypeIdent '<' TypeParam '>' '{'
                   CacheCapacity
                   { CacheInvariant }
                   { CacheOperation }
                 '}' ;

CacheCapacity  = 'capacity' ':' Expr ;

CacheOperation = 'fetch' | 'pin' | 'unpin'
               | 'make_dirty' | 'make_clean' | 'evict' ;

PinState       = 'pinned' '(' Ident ')' | 'unpinned' '(' Ident ')' ;
DirtyState     = 'dirty' '(' Ident ')' | 'clean' '(' Ident ')' ;
```

##### Full Example: SQLite Page Cache

```assura
cache PageCache<Page> {
  capacity: config.cache_size  // runtime-configurable

  type CacheEntry {
    page_number: U32,
    data: Region<PageSize>,
    pin_count: {v: Nat | v >= 0},
    is_dirty: Bool,
    in_journal: Bool  // written to rollback journal
  }

  // Core invariants
  invariant {
    // Pinned pages cannot be evicted
    forall entry in self.entries:
      pinned(entry) => not evictable(entry)
  }

  invariant {
    // Dirty pages cannot be evicted unless journaled
    forall entry in self.entries:
      dirty(entry) and not entry.in_journal
      => not evictable(entry)
  }

  invariant {
    // Pin count matches number of active references
    forall entry in self.entries:
      entry.pin_count == count(active_refs(entry))
  }

  invariant {
    // Cache size does not exceed capacity
    // (only unpinned clean pages are evicted to maintain this)
    count(self.entries where pinned(e) or dirty(e))
      <= self.capacity
  }

  fetch {
    input(page_num: U32)
    output(entry: CacheEntry :_1)

    ensures {
      // Returned entry is pinned (pin_count >= 1)
      pinned(entry)
    }

    ensures {
      // If page was in cache, return cached version
      // If not, read from disk and add to cache
      entry.page_number == page_num
      and entry.data == disk_page(page_num)
    }

    effects: filesystem.read
  }

  pin {
    input(entry: CacheEntry)

    requires { entry in self.entries }
    ensures  { entry.pin_count == old(entry.pin_count) + 1 }
    ensures  { pinned(entry) }

    effects: pure
  }

  unpin {
    input(entry: CacheEntry)

    requires { pinned(entry) }
    requires { entry.pin_count >= 1 }
    ensures  { entry.pin_count == old(entry.pin_count) - 1 }

    // After unpin, page may or may not still be pinned
    // (depends on whether other references exist)
    ensures {
      entry.pin_count == 0 => unpinned(entry)
    }

    effects: pure
  }

  make_dirty {
    input(entry: CacheEntry)

    requires { pinned(entry) }
    // Cannot dirty an unpinned page (someone must be using it)
    ensures  { dirty(entry) }
    ensures  { entry.is_dirty == true }

    effects: pure
  }

  make_clean {
    input(entry: CacheEntry)

    requires { dirty(entry) }
    requires {
      // Page must be written to disk or journal first
      entry.data == disk_page(entry.page_number)
      or entry.in_journal
    }
    ensures  { clean(entry) }
    ensures  { entry.is_dirty == false }

    effects: pure
  }

  evict {
    input(entry: CacheEntry)

    requires { unpinned(entry) }
    requires { clean(entry) }
    // CANNOT evict pinned or dirty pages

    ensures { entry not in self.entries }

    effects: pure
  }
}

// COMPILE ERROR: using page after unpin without re-fetch
fn bad_page_use(cache: PageCache<Page>) -> Region<PageSize>
{
  let entry = cache.fetch(42)       // pinned
  let data = entry.data
  cache.unpin(entry)                // unpinned
  data  // A44001: accessing data from unpinned cache entry
        // (entry may be evicted, data is dangling)
}

// CORRECT: pin while using, unpin when done
fn good_page_use(
    cache: PageCache<Page> :_1
) -> (PageCache<Page> :_1, Region<PageSize>)
{
  let entry = cache.fetch(42)       // pinned, pin_count = 1
  let data = copy(entry.data)       // copy data while pinned
  cache.unpin(entry)                // safe to evict now
  (cache, data)                     // return copied data
}
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A44001 | Accessing data from unpinned cache entry | Use after unpin (may be evicted) |
| A44002 | Evicting pinned page | Pin count > 0 when evict called |
| A44003 | Evicting dirty unjournaled page | Data loss: dirty page not written |
| A44004 | Double unpin: pin count already zero | Unpin called more times than pin |
| A44005 | Dirtying unpinned page | make_dirty on entry with pin_count 0 |

##### Rust Codegen

Page cache contracts generate a cache struct with runtime pin
tracking and debug assertions:

```rust
pub struct PageCache {
    entries: HashMap<u32, CacheEntry>,
    capacity: usize,
}

pub struct CacheEntry {
    page_number: u32,
    data: Box<[u8; PAGE_SIZE]>,
    pin_count: AtomicU32,
    is_dirty: bool,
    in_journal: bool,
}

pub struct PinnedPage<'cache> {
    entry: &'cache CacheEntry,
    cache: &'cache PageCache,
}

impl PageCache {
    pub fn fetch(&self, page_num: u32) -> PinnedPage<'_> {
        let entry = self.entries.get(&page_num)
            .unwrap_or_else(|| self.read_from_disk(page_num));
        entry.pin_count.fetch_add(1, Ordering::Relaxed);
        PinnedPage { entry, cache: self }
    }
}

// PinnedPage auto-unpins on drop (RAII)
impl<'cache> Drop for PinnedPage<'cache> {
    fn drop(&mut self) {
        let prev = self.entry.pin_count.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev >= 1, "Double unpin on page {}",
            self.entry.page_number);
    }
}

impl<'cache> PinnedPage<'cache> {
    pub fn data(&self) -> &[u8; PAGE_SIZE] {
        &self.entry.data
    }

    pub fn make_dirty(&mut self) {
        debug_assert!(self.entry.pin_count.load(Ordering::Relaxed) >= 1);
        // ... mark dirty ...
    }
}
```


#### STOR.3 MVCC and Snapshot Isolation

Contracts for multi-version concurrency control that guarantee
readers see a consistent snapshot while writers modify data
concurrently.

##### Motivation

SQLite's WAL mode allows concurrent readers and a single writer.
Each reader sees the database as it existed at the moment the read
transaction started, even if the writer commits new data afterward.
This is snapshot isolation: reader R at version V sees page P at
version V, not version V+1 that the writer just committed.

Shared memory protocols (CONC.1) handle the atomic read/write of
shared state. Snapshot isolation is a higher-level property: it
defines which *version* of data each transaction can see, based on
when the transaction started relative to commits.

##### Grammar

```ebnf
SnapshotDecl   = 'snapshot' TypeIdent '{'
                   VersionType
                   SnapshotInvariant
                   { SnapshotRule }
                 '}' ;

VersionType    = 'version' ':' TypeExpr ;

SnapshotInvariant = 'isolation' ':' IsolationLevel ;

IsolationLevel = 'snapshot'
               | 'serializable'
               | 'read_committed'
               | 'read_uncommitted' ;

SnapshotRule   = 'visible' '(' Ident ',' Ident ')' ':'
                 Predicate ;

VersionRef     = 'at_version' '(' Ident ',' Expr ')' ;
```

##### Full Example: WAL Snapshot Isolation

```assura
snapshot WalSnapshot {
  version: U32  // monotonically increasing commit counter

  isolation: snapshot

  // A transaction sees the state at the version when it started
  type Transaction {
    start_version: U32,
    is_writer: Bool,
    // Writer sees its own uncommitted changes
    pending_pages: Map<U32, Region<PageSize>>
  }

  // Core visibility rule: which page version does a reader see?
  visible(txn: Transaction, page: PageNumber):
    if page in txn.pending_pages and txn.is_writer {
      // Writer sees its own pending changes
      page_data == txn.pending_pages[page]
    } else {
      // Read the latest version <= txn.start_version
      page_data == page_at_version(page, txn.start_version)
    }

  // Snapshot consistency: all pages within a transaction
  // come from the same version
  invariant {
    forall txn: Transaction, p1: PageNumber, p2: PageNumber:
      version_of(visible(txn, p1)) == version_of(visible(txn, p2))
      // A reader never sees page 5 at version 10 and
      // page 8 at version 11
  }

  // Writer exclusion: only one writer at a time
  invariant {
    count(active_transactions where is_writer) <= 1
  }

  // Version monotonicity: commits increase the version
  invariant {
    forall commit c1 before c2:
      c1.version < c2.version
  }

  // No lost updates: a committed version is always readable
  invariant {
    forall committed_version v:
      forall txn where txn.start_version >= v:
        can_read(txn, v)
  }

  // Read-your-writes: writer sees its own pending changes
  invariant {
    forall txn where txn.is_writer:
      forall page in txn.pending_pages:
        visible(txn, page) == txn.pending_pages[page]
  }
}

// Using snapshots in queries
fn read_consistent_pair(
    txn: Transaction,
    page_a: U32,
    page_b: U32
) -> (Region<PageSize>, Region<PageSize>)
  requires { not txn.is_writer }
  ensures {
    // Both pages are from the same snapshot
    version_of(fst(result)) == version_of(snd(result))
    and version_of(fst(result)) == txn.start_version
  }
  effects: database.read
{
  let a = read_page(txn, page_a)
  // Even if a writer commits between these two reads,
  // txn still sees the old version of page_b
  let b = read_page(txn, page_b)
  (a, b)
}

// COMPILE ERROR: reading without a transaction
fn bad_read(db: Database, page: U32) -> Region<PageSize>
{
  db.read_page(page)
  // A45001: page read outside transaction context
  //         (no snapshot version, result may be inconsistent)
}

// COMPILE ERROR: write-write conflict detection
fn concurrent_writers(
    db: Database :_1
) -> Database :_1
{
  let txn1 = db.begin_write()  // OK: first writer
  let txn2 = db.begin_write()  // A45003: writer already active
  // ...
}
```

##### Verification Rule

1. **Snapshot consistency**: The compiler verifies that all page
   reads within a transaction use the same version (the transaction's
   start_version)
2. **Writer exclusion**: At most one write transaction can be active.
   Starting a second writer is a compile error if the first hasn't
   committed or rolled back
3. **No dirty reads**: A reader never sees uncommitted writer data
   (unless the reader IS the writer)
4. **No phantom reads**: The set of visible rows doesn't change
   during a read transaction
5. **Version tracking**: The compiler tracks which version each
   page reference belongs to and rejects mixing versions

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A45001 | Page read outside transaction context | No snapshot version for consistency |
| A45002 | Mixed versions in single transaction | Pages from different snapshots |
| A45003 | Concurrent writer conflict | Second writer while first is active |
| A45004 | Stale snapshot: version `V` no longer available | WAL checkpoint removed old version |
| A45005 | Write to read-only transaction | Modifying page in non-writer txn |

##### Rust Codegen

Snapshot isolation generates transaction wrappers with version
tracking:

```rust
pub struct ReadTransaction<'db> {
    db: &'db Database,
    snapshot_version: u32,
}

pub struct WriteTransaction<'db> {
    db: &'db mut Database,  // exclusive borrow: one writer
    snapshot_version: u32,
    pending: HashMap<u32, Box<[u8; PAGE_SIZE]>>,
}

impl<'db> ReadTransaction<'db> {
    pub fn read_page(&self, page_num: u32) -> &[u8; PAGE_SIZE] {
        // Always reads from snapshot_version, never newer
        self.db.wal.page_at_version(page_num, self.snapshot_version)
    }
}

impl<'db> WriteTransaction<'db> {
    pub fn read_page(&self, page_num: u32) -> &[u8; PAGE_SIZE] {
        // Writer sees its own pending changes first
        if let Some(page) = self.pending.get(&page_num) {
            return page;
        }
        self.db.wal.page_at_version(page_num, self.snapshot_version)
    }

    pub fn write_page(&mut self, page_num: u32, data: [u8; PAGE_SIZE]) {
        self.pending.insert(page_num, Box::new(data));
    }

    pub fn commit(self) -> Result<(), CommitError> {
        // Atomically advance version and write pending pages to WAL
        self.db.wal.commit(self.snapshot_version, &self.pending)
    }
}

impl Database {
    pub fn begin_read(&self) -> ReadTransaction<'_> {
        ReadTransaction {
            db: self,
            snapshot_version: self.wal.current_version(),
        }
    }

    // &mut self guarantees at most one writer (Rust borrow checker)
    pub fn begin_write(&mut self) -> WriteTransaction<'_> {
        WriteTransaction {
            db: self,
            snapshot_version: self.wal.current_version(),
            pending: HashMap::new(),
        }
    }
}
```


#### STOR.4 Transactional Rollback

Contracts that guarantee atomic success-or-rollback semantics
for multi-step operations. If any step fails (OOM, I/O error,
constraint violation), the entire operation is undone and the
system returns to its pre-operation state.

##### Motivation

SQLite handles out-of-memory gracefully: every `sqlite3_malloc()`
call can fail, and the system rolls back to a consistent state.
This is extraordinarily hard to get right in C (and Rust). There
are ~1,200 malloc calls in SQLite, and every single one has a
failure path. The `SQLITE_NOMEM` error propagates up the call
stack while unwinding all partial state changes.

##### Grammar

```ebnf
AtomicDecl     = '#[atomic]' FnDecl ;
RollbackClause = 'on_failure' ':' 'rollback_to' '(' Ident ')' ;
SavepointDecl  = 'savepoint' Ident '=' Expr ;
```

##### Full Example: Atomic Insert with OOM Recovery

```assura
#[atomic]
fn btree_insert(
    tree: BtCursor :_1,
    key: Region<k>,
    data: Region<d>
) -> (BtCursor :_1) | OutOfMemoryError | DiskFullError
  requires { tree.state @ ReadyToWrite }
  ensures {
    result is BtCursor =>
      contains(result, key) and
      BTreeValid(result.tree)
  }
  on_failure: rollback_to(tree)
  // If ANY allocation or I/O fails, tree is unchanged
  effects: database.write
{
  // Save pre-operation state
  savepoint pre = snapshot(tree)

  // Step 1: Find insertion point (may allocate)
  let pos = find_position(tree, key)?

  // Step 2: Insert cell (may allocate overflow pages)
  let tree = insert_cell(tree, pos, key, data)?

  // Step 3: Rebalance if needed (may allocate new pages)
  let tree = if needs_balance(tree, pos) {
    balance(tree, pos)?
  } else {
    tree
  }

  // If we get here, all steps succeeded
  tree

  // If any ? propagates an error:
  //   1. All allocated pages are freed
  //   2. All modified pages are reverted to savepoint
  //   3. tree is returned in its original state
}

// Compound atomic operation
#[atomic]
fn execute_insert_statement(
    conn: Connection :_1,
    table: String,
    values: List<Value>
) -> (Connection :_1) | SqlError
  on_failure: rollback_to(conn)
  effects: database.write
{
  let cursor = conn.open_cursor(table)?
  let encoded = encode_record(values)?
  let cursor = btree_insert(cursor, encoded.key, encoded.data)?
  // Update indices
  for idx in conn.indices_for(table) {
    let idx_cursor = conn.open_cursor(idx.name)?
    let idx_key = idx.extract_key(values)?
    let idx_cursor = btree_insert(idx_cursor, idx_key, encoded.rowid)?
  }
  conn
}
```

##### Verification Rule

1. **Savepoint capture**: At entry to an `#[atomic]` function, the
   compiler inserts a logical savepoint capturing all mutable state
2. **Error propagation**: Every `?` operator is a potential rollback
   point
3. **Rollback proof**: The compiler verifies that the `on_failure`
   state is reachable from every error point (no partial cleanup
   that leaves state inconsistent)
4. **Nested atomicity**: `#[atomic]` functions may call other
   `#[atomic]` functions; the inner one's failure triggers the
   outer one's rollback
5. **Linear state**: The rollback target must be the original
   linear value (can't roll back to an intermediate state)

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A36001 | Atomic function `F` has unrecoverable error path | Error path exists that cannot restore pre-state |
| A36002 | Savepoint `S` escapes atomic scope | Savepoint used outside its #[atomic] function |
| A36003 | Partial state modified without rollback path | Mutable state changed before error check |
| A36004 | Nested atomic function `F` swallows error | Inner failure caught without propagating to outer |

##### Rust Codegen

Atomic operations generate Rust code with explicit savepoint
and rollback logic:

```rust
pub fn btree_insert(
    tree: &mut BTree,
    key: &[u8],
    data: &[u8],
) -> Result<(), InsertError> {
    // Save rollback state
    let savepoint = tree.savepoint();

    let result = (|| -> Result<(), InsertError> {
        let pos = tree.find_position(key)?;
        tree.insert_cell(pos, key, data)?;
        if tree.needs_balance(pos) {
            tree.balance(pos)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            savepoint.commit();
            Ok(())
        }
        Err(e) => {
            savepoint.rollback(tree);
            debug_assert!(tree.verify_invariants());
            Err(e)
        }
    }
}
```


#### STOR.5 Monotonic State Contracts

Contracts for values that must never decrease (or never increase)
over time: counters, version numbers, timestamps, sequence IDs.

##### Motivation

SQLite has many monotonic values:
- **File change counter** (header offset 24): incremented on every
  commit, never decremented
- **Schema cookie** (offset 40): incremented when schema changes
- **WAL frame numbers**: always increase within a WAL
- **Transaction IDs**: monotonically increasing
- **Auto-increment rowids**: always advance, never reuse

If any of these decreases, it indicates corruption, a bug in the
port, or a recovery error. Refinement types express constraints on
current values (`v > 0`) but not temporal constraints (`v(t+1) >= v(t)`).
Typestate tracks state transitions but not numeric progression.
Monotonicity needs its own concept.

##### Grammar

```ebnf
MonotonicDecl  = 'monotonic' MonotonicKind Ident ':' TypeExpr
                 [MonotonicBound] ;

MonotonicKind  = 'increasing' | 'decreasing' | 'non_decreasing'
               | 'non_increasing' ;

MonotonicBound = 'wraps_at' Expr    // for bounded counters
               | 'saturates_at' Expr ;

MonotonicCheck = 'assert_monotonic' '(' Ident ')' ;
```

##### Full Example: SQLite Monotonic Values

```assura
module database_header {
  // File change counter: incremented on each commit
  monotonic non_decreasing file_change_counter: U32
    wraps_at U32.MAX
    // When it wraps, version_valid_for is set to a
    // value not matching the counter (triggers full
    // schema reload)

  // Schema cookie: changes when schema changes
  monotonic non_decreasing schema_cookie: U32

  // Auto-increment counter per table
  monotonic increasing auto_increment: I64
    // Strict: never reuses values, even after DELETE
}

module wal {
  // WAL frame numbers always increase
  monotonic increasing frame_number: U32

  // Salt values change on WAL reset (not monotonic)
  // but within a single WAL epoch, they are constant

  // Max frame in WAL index (updated atomically)
  monotonic non_decreasing max_valid_frame: U32
}

// Using monotonic values
fn commit_transaction(
    header: DatabaseHeader :_1,
    changes: List<PageChange>
) -> DatabaseHeader :_1
  ensures {
    // Counter must advance (not stay the same)
    header.file_change_counter > old(header.file_change_counter)
    or (old(header.file_change_counter) == U32.MAX
        and header.file_change_counter == 0)
    // Wrapping is the ONLY case where new < old
  }
  effects: database.write
{
  let counter = header.file_change_counter
  let new_counter = if counter == U32.MAX {
    header.version_valid_for = 0  // invalidate
    0  // wrap
  } else {
    counter + 1
  }
  header.file_change_counter = new_counter
  header
}

// COMPILE ERROR: decrementing a monotonic value
fn bad_rollback(header: DatabaseHeader :_1) -> DatabaseHeader :_1
{
  header.file_change_counter = header.file_change_counter - 1
  // A47001: monotonic non_decreasing value decreased
  header
}

// COMPILE ERROR: reusing auto-increment
fn bad_insert(table: Table :_1, deleted_rowid: I64) -> Table :_1
{
  let row = Row { rowid: deleted_rowid, ... }
  // A47002: monotonic increasing value reused
  // deleted_rowid was previously assigned, cannot be reused
  table.insert(row)
}

// WAL frame number contract
fn append_wal_frame(
    wal: WalFile :_1,
    frame: WalFrame
) -> WalFile :_1
  requires {
    frame.frame_number > wal.last_frame_number
    // Strict increasing: no duplicate frame numbers
  }
  ensures {
    wal.last_frame_number == frame.frame_number
  }
  effects: filesystem.write
{
  assert_monotonic(wal.max_valid_frame)
  wal.write_frame(frame)
  wal.max_valid_frame = frame.frame_number
  wal
}
```

##### Verification Rule

1. **Write analysis**: Every assignment to a monotonic variable is
   checked. The new value must satisfy the monotonic constraint
   relative to the old value
2. **Wrapping**: `wraps_at X` allows exactly one transition from
   X to a smaller value (wrap-around). All other decreases are errors
3. **Increasing vs non_decreasing**: `increasing` forbids `new == old`;
   `non_decreasing` allows it
4. **Cross-function**: Monotonicity is tracked across function
   boundaries via the linear type system (the value is linear, so
   only one function modifies it at a time)
5. **Recovery exception**: During crash recovery (STOR.1), the
   compiler suspends monotonicity checks on values being restored,
   since recovery may roll back to a previous valid state

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A47001 | Monotonic value `V` decreased | Assignment violates non_decreasing |
| A47002 | Monotonic value `V` reused | Assignment violates increasing (strict) |
| A47003 | Monotonic value `V` decreased outside wrap | Decrease without wraps_at or not at boundary |
| A47004 | Monotonic value `V` overflows without wrap policy | Saturates_at or wraps_at not declared |

##### Rust Codegen

Monotonic contracts generate wrapper types that enforce the
constraint at runtime in debug mode:

```rust
#[derive(Debug)]
pub struct MonotonicU32<const STRICT: bool> {
    value: u32,
    #[cfg(debug_assertions)]
    previous: u32,
}

impl<const STRICT: bool> MonotonicU32<STRICT> {
    pub fn new(initial: u32) -> Self {
        MonotonicU32 {
            value: initial,
            #[cfg(debug_assertions)]
            previous: initial,
        }
    }

    pub fn get(&self) -> u32 { self.value }

    pub fn advance(&mut self, new_value: u32) {
        #[cfg(debug_assertions)]
        {
            if STRICT {
                debug_assert!(new_value > self.value,
                    "Monotonic increasing: {} not > {}",
                    new_value, self.value);
            } else {
                debug_assert!(new_value >= self.value,
                    "Monotonic non-decreasing: {} not >= {}",
                    new_value, self.value);
            }
            self.previous = self.value;
        }
        self.value = new_value;
    }

    pub fn advance_wrapping(&mut self, new_value: u32) {
        // Allow exactly one wrap from MAX to smaller value
        #[cfg(debug_assertions)]
        {
            if new_value < self.value && self.value != u32::MAX {
                panic!("Non-wrapping decrease: {} -> {}",
                    self.value, new_value);
            }
        }
        self.value = new_value;
    }
}

// Type aliases for SQLite usage
pub type ChangeCounter = MonotonicU32<false>;     // non-decreasing
pub type SchemaCookie = MonotonicU32<false>;       // non-decreasing
pub type FrameNumber = MonotonicU32<true>;         // strict increasing
pub type AutoIncrement = MonotonicU32<true>;        // strict increasing
```


#### STOR.6 Storage Failure Model

Contracts that define the physical failure semantics of the
underlying storage medium, enabling verification of crash recovery
logic against a concrete failure model.

##### Motivation

littlefs's correctness depends on specific flash physics: programming
is idempotent for already-programmed bits, erased state is all-1s,
and a torn program leaves only the affected `prog_size` region
indeterminate. The FCRC mechanism CRCs the next prog-sized region
to detect torn writes. Crash recovery (STOR.1) specifies what must
be recovered; this feature specifies what can go wrong.

##### Grammar

```ebnf
StorageModelDecl  = 'storage_model' Ident '{' StorageRule { StorageRule } '}' ;
StorageRule       = 'on_crash_during' Ident ':' FailureSpec ';' ;
FailureSpec       = 'affected_region' '(' Expr ')' 'becomes' ('indeterminate' | 'unchanged') ;
EraseSemantics    = 'erase_value' ':' IntLit ';' ;
ProgSemantics     = 'prog_idempotent' ':' BoolLit ';' ;
```

##### Full Example

```assura
storage_model FlashDevice {
    erase_value: 0xFF
    prog_idempotent: true
    block_size: config.block_size
    prog_size: config.prog_size

    on_crash_during prog:
        affected_region(offset..offset + prog_size) becomes indeterminate
        // all other regions unchanged

    on_crash_during erase:
        affected_region(0..block_size) becomes indeterminate
        // block may be partially erased

    on_crash_during sync:
        // no effect; sync is a barrier, not a write
}

contract MetadataPairWrite {
    // Write to inactive block only
    requires target == pair[1]
    requires pair[0].crc_valid()

    // After crash during prog: pair[0] still valid (untouched)
    crash_safe {
        pair[0].crc_valid()
        pair[0].revision >= last_known_revision
    }

    // After successful commit: pair[1] now has higher revision
    ensures pair[1].revision > pair[0].revision
    ensures pair[1].crc_valid()
}
```

##### Verification Rule

1. The storage model declares what physical operations can fail and how
2. Crash recovery contracts (STOR.1) reference the storage model
3. The verifier checks that recovery logic handles every `indeterminate`
   region correctly (by detecting via CRC or FCRC)
4. Operations on erased blocks must verify `erase_value` assumption

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-STOR-010 | Unhandled torn write | Recovery logic does not check indeterminate region |
| A-STOR-011 | Erase assumption violated | Code assumes zeros after erase on 0xFF flash |
| A-STOR-012 | Missing failure model | Storage operation without storage_model declaration |

##### Rust Codegen

Storage model declarations generate test infrastructure:

```rust
/// Fault-injection test harness for flash failure model
struct FlashFaultInjector {
    fail_at: Option<(usize, usize)>, // (block, offset) to corrupt
}

impl FlashFaultInjector {
    fn prog(&mut self, block: usize, off: usize, data: &[u8]) -> Result<()> {
        if self.fail_at == Some((block, off)) {
            // Write partial data (torn write simulation)
            let partial = &data[..data.len() / 2];
            self.device.write(block * self.block_size + off, partial)?;
            return Err(Error::PowerLoss);
        }
        self.device.write(block * self.block_size + off, data)
    }
}
```

### 14.FMT: Data Formats and Parsing

#### FMT.1 Binary Format Contracts

Contracts for on-disk binary formats that enforce backward
compatibility across versions.

##### Grammar

```ebnf
FormatDecl     = 'format' TypeIdent '{'
                   { FormatField }
                   [CompatibilityDecl]
                 '}' ;

FormatField    = 'offset' IntLit ',' 'size' IntLit ':'
                 Ident ['=' Expr] [WhereClause] ;

CompatibilityDecl = 'compatibility' '{'
                      { FrozenDecl }
                      { ExtensibleDecl }
                    '}' ;

FrozenDecl     = 'frozen' ':' IdentList ;
ExtensibleDecl = 'extensible' ':' IdentList ;
```

##### Full Example: SQLite Database Header

```assura
format DatabaseHeader {
  offset 0,  size 16: magic = "SQLite format 3\0"
  offset 16, size 2:  page_size
    where page_size in {512, 1024, 2048, 4096, 8192, 16384, 32768, 65536}
    -- Note: value 1 means 65536 (special encoding)

  offset 18, size 1:  write_version
    where write_version in {1, 2}
    -- 1 = rollback journal, 2 = WAL

  offset 19, size 1:  read_version
    where read_version in {1, 2}

  offset 20, size 1:  reserved_space
    where reserved_space >= 0

  offset 21, size 1:  max_embedded_payload_fraction = 64

  offset 22, size 1:  min_embedded_payload_fraction = 32

  offset 23, size 1:  leaf_payload_fraction = 32

  offset 24, size 4:  file_change_counter

  offset 28, size 4:  database_size_pages
    where database_size_pages >= 0

  offset 32, size 4:  first_freelist_trunk_page

  offset 36, size 4:  freelist_page_count

  offset 40, size 4:  schema_cookie

  offset 44, size 4:  schema_format_number
    where schema_format_number in {1, 2, 3, 4}

  offset 48, size 4:  default_cache_size

  offset 52, size 4:  largest_root_btree_page

  offset 56, size 4:  text_encoding
    where text_encoding in {1, 2, 3}
    -- 1 = UTF-8, 2 = UTF-16le, 3 = UTF-16be

  offset 60, size 4:  user_version

  offset 64, size 4:  incremental_vacuum_mode

  offset 68, size 4:  application_id

  offset 72, size 20: reserved_expansion = 0

  offset 92, size 4:  version_valid_for

  offset 96, size 4:  sqlite_version_number

  compatibility {
    // These fields can NEVER change their offset, size, or meaning
    frozen: magic, page_size, write_version, read_version,
            text_encoding, schema_format_number

    // These fields may be given new valid values in future versions
    extensible: application_id, user_version
  }
}
```

##### Verification Rules

1. **Frozen fields**: Any change to a frozen field's offset, size,
   encoding, or valid values is a compile error (A31001)
2. **Extensible fields**: May add new valid values but cannot change
   offset or size
3. **New fields**: May only use `reserved_expansion` space
4. **Read compatibility**: A reader must accept all valid values
   from all versions
5. **Write compatibility**: A writer must only produce values valid
   for the target version

##### Cross-Version Testing

```assura
// The compiler can verify format compatibility across versions
format_test DatabaseHeader {
  // v1 header must be readable by v2 reader
  compatible: v1 readable_by v2

  // v2 header with new features must be rejected by v1 reader
  // (if write_version > 1)
  incompatible: v2_wal rejected_by v1_reader
    when write_version == 2
}
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A31001 | Frozen format field modified | Changed offset/size/meaning |
| A31002 | Format field overlaps | Two fields share byte range |
| A31003 | Gap in format layout | Unaccounted bytes between fields |
| A31004 | Format exceeds expected size | Header larger than spec |
| A31005 | Reserved space violated | Non-zero value in reserved field |

##### Rust Codegen

Binary format contracts generate zero-copy parser/serializer code:

```rust
pub struct DatabaseHeader<'a> {
    data: &'a [u8; 100],
}

impl<'a> DatabaseHeader<'a> {
    pub fn magic(&self) -> &[u8; 16] {
        self.data[0..16].try_into().unwrap()
    }

    pub fn page_size(&self) -> u32 {
        let raw = u16::from_be_bytes([self.data[16], self.data[17]]);
        if raw == 1 { 65536 } else { raw as u32 }
    }

    pub fn write_version(&self) -> u8 {
        self.data[18]
    }

    pub fn validate(&self) -> Result<(), FormatError> {
        if self.magic() != b"SQLite format 3\0" {
            return Err(FormatError::BadMagic);
        }
        let ps = self.page_size();
        if !ps.is_power_of_two() || ps < 512 || ps > 65536 {
            return Err(FormatError::BadPageSize);
        }
        // ... validate all fields ...
        Ok(())
    }
}
```


#### FMT.2 Bit-Level Format Contracts

Contracts for sub-byte parsing: reading individual bits, variable-length
bit fields, and bit-packed structures common in compressed data formats.

##### Motivation

FMT.1 (Binary Format) handles byte-aligned structures like
PNG chunk headers and BMP file headers. But many real-world formats
require bit-level parsing:

- **JPEG Huffman decoding**: reads variable-length bit sequences
  (1-16 bits) that are NOT byte-aligned
- **DEFLATE (zlib)**: 3-bit block headers, 5-bit length codes,
  variable-width distance codes
- **GIF LZW**: variable-width codes (starting at `min_code_size + 1`
  bits, growing as the dictionary fills)
- **PNG interlacing**: bit-packed scanline filters

Without bit-level contracts, the verifier cannot track the bit position
cursor, cannot prove that a read of N bits does not exceed the
remaining bits, and cannot verify that Huffman table lookups are safe.

##### Grammar

```ebnf
BitFormatDecl  = 'bit_format' Ident '{'
                   { BitFieldDecl }
                 '}' ;

BitFieldDecl   = BitField | BitChoice | BitAlign ;

BitField       = Ident ':' 'bits' '(' Expr ')'
                 [BitEndian] [BitConstraint] ;

BitChoice      = 'match_bits' '(' Expr ')' '{'
                   { BitPattern '=>' BitFieldDecl }
                 '}' ;

BitAlign       = 'align_to' '(' Expr ')' ;

BitEndian      = 'msb_first' | 'lsb_first' ;

BitConstraint  = 'where' Predicate ;

BitCursor      = 'bit_position' '(' Ident ')'
               | 'bits_remaining' '(' Ident ')' ;
```

##### Full Example: JPEG Huffman Decoding

```assura
// Bit-level format for JPEG entropy-coded data
bit_format JpegBitstream {
  // The bit cursor tracks position across byte boundaries
  invariant { bits_remaining(self) >= 0 }
}

// Huffman decode: read variable-length code
fn huffman_decode(
    stream: &mut JpegBitstream :read_bits,
    table: &HuffmanTable :verified
) -> U8 | DecodeError
  requires { bits_remaining(stream) >= 1 }
  ensures {
    // Consumed between 1 and 16 bits
    old(bit_position(stream)) < bit_position(stream),
    bit_position(stream) - old(bit_position(stream)) <= 16
  }
{
  let mut code: U16 = 0
  for len in 1..=16 {
    requires { bits_remaining(stream) >= 1 }
    let bit = stream.read_bits(1) as U16  // read exactly 1 bit
    code = (code << 1) | bit

    if table.has_code(code, len) {
      return table.lookup(code, len)
    }
  }
  DecodeError::InvalidHuffmanCode
}

// DEFLATE block header: 3 bits, not byte-aligned
bit_format DeflateBlockHeader {
  is_final: bits(1) msb_first,
  block_type: bits(2) msb_first
    where block_type <= 2  // 0=stored, 1=fixed, 2=dynamic; 3=reserved
}

// GIF LZW: variable-width codes
fn read_lzw_code(
    stream: &mut BitReader :read_bits,
    code_size: U8
) -> U16 | DecodeError
  requires { code_size >= 2 && code_size <= 12 }
  requires { bits_remaining(stream) >= code_size as U32 }
  ensures { result <= (1 << code_size) - 1 }
{
  stream.read_bits(code_size as U32) as U16
}
```

##### Verification Rule

1. **Bit cursor tracking**: The verifier maintains a symbolic bit
   position for each `BitReader`/`BitWriter`. Every `read_bits(N)`
   advances the cursor by N and requires `bits_remaining >= N`
2. **Cross-byte safety**: When a bit read spans a byte boundary,
   the verifier confirms the underlying byte buffer has sufficient
   data. `bits_remaining(stream) >= N` implies
   `bytes_remaining(stream.buffer) >= ceil(N + bit_offset, 8)`
3. **Variable-width bounds**: For variable-width reads (Huffman,
   LZW), the verifier checks that the maximum possible read does
   not exceed remaining bits. Loop invariants must prove the upper
   bound
4. **Alignment**: `align_to(8)` skips bits to reach a byte boundary.
   The verifier proves the skip does not exceed 7 bits

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A49001 | Bit read exceeds remaining bits | `read_bits(N)` where `bits_remaining < N` |
| A49002 | Bit field width must be constant or bounded | Variable-width field without upper bound |
| A49003 | Invalid bit alignment target | `align_to(0)` or non-power-of-2 alignment |
| A49004 | Bit cursor used after byte-level read | Mixing bit and byte reads without re-alignment |
| A49005 | Bit field constraint not satisfiable | `where` predicate on bit field is always false |

##### Rust Codegen

Bit-level format contracts generate a `BitReader` wrapper with
bounds-checked accessors:

```rust
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_offset: u8, // 0-7, bits consumed in current byte
}

impl<'a> BitReader<'a> {
    #[inline]
    pub fn read_bits(&mut self, n: u32) -> Result<u32, DecodeError> {
        debug_assert!(n <= 25, "read_bits limited to 25 for u32");
        if self.bits_remaining() < n as usize {
            return Err(DecodeError::UnexpectedEnd);
        }
        let mut value: u32 = 0;
        let mut remaining = n;
        while remaining > 0 {
            let available = 8 - self.bit_offset as u32;
            let take = remaining.min(available);
            let mask = (1u32 << take) - 1;
            let shift = available - take;
            value = (value << take)
                | ((self.data[self.byte_pos] as u32 >> shift) & mask);
            remaining -= take;
            self.bit_offset += take as u8;
            if self.bit_offset >= 8 {
                self.bit_offset = 0;
                self.byte_pos += 1;
            }
        }
        Ok(value)
    }

    #[inline]
    pub fn bits_remaining(&self) -> usize {
        (self.data.len() - self.byte_pos) * 8
            - self.bit_offset as usize
    }
}
```


#### FMT.3 String Encoding Contracts

Contracts that track text encoding (UTF-8, UTF-16LE, UTF-16BE)
through the type system, ensuring encoding-aware operations and
preventing encoding mismatch bugs.

##### Motivation

SQLite supports three text encodings: UTF-8, UTF-16LE, and
UTF-16BE. The encoding is set per-database and affects how strings
are stored, compared, and returned through the API. Encoding bugs
are subtle:
- Comparing a UTF-8 string with a UTF-16LE string without conversion
- Returning UTF-16 bytes through a UTF-8 API
- Computing string length in bytes vs characters vs code points
- Truncating a multi-byte character at a buffer boundary
- Collation functions that assume a specific encoding

Rust's `String` is always UTF-8, but a SQLite port must handle all
three encodings internally and at the FFI boundary.

##### Grammar

```ebnf
EncodingType   = 'Utf8' | 'Utf16Le' | 'Utf16Be' ;

EncodedString  = 'Text' '<' EncodingType '>' ;

EncodingDecl   = 'encoding' Ident ':' EncodingType ;

TranscodeExpr  = 'transcode' '(' Expr ',' EncodingType ')' ;

EncodingConstraint = 'encoding_matches' '(' Ident ',' Ident ')'
                   | 'valid_encoding' '(' Ident ')' ;
```

##### Full Example: Multi-Encoding Text Handling

```assura
// Text type parameterized by encoding
type Text<E: Encoding> {
  bytes: Region<n>,
  encoding: E,
  char_count: Nat,
  byte_length: Nat
}

// Encoding is a type-level parameter
enum Encoding { Utf8, Utf16Le, Utf16Be }

// Database-level encoding setting
type DatabaseEncoding = {e: Encoding |
  e in {Utf8, Utf16Le, Utf16Be}
}

// String comparison requires same encoding
fn compare_text<E: Encoding>(
    a: Text<E>,
    b: Text<E>,
    collation: Collation<E>
) -> Ordering
  requires { valid_encoding(a) }
  requires { valid_encoding(b) }
  ensures  { result is deterministic }
  effects: pure

// COMPILE ERROR: comparing different encodings
fn bad_compare(
    a: Text<Utf8>,
    b: Text<Utf16Le>
) -> Ordering
{
  compare_text(a, b)  // A43001: encoding mismatch
                       // a is Utf8, b is Utf16Le
}

// Transcoding with validation
fn transcode<From: Encoding, To: Encoding>(
    input: Text<From>
) -> Text<To> | EncodingError
  requires { valid_encoding(input) }
  ensures  {
    result is Text<To> =>
      char_count(result) == char_count(input)
      and valid_encoding(result)
  }
  effects: pure

// Length operations are encoding-aware
fn text_length<E: Encoding>(t: Text<E>) -> LengthResult
  requires { valid_encoding(t) }
  effects: pure
{
  LengthResult {
    bytes: t.byte_length,
    chars: t.char_count,
    // For UTF-16: code_units may differ from chars (surrogate pairs)
    code_units: match E {
      Utf8 => t.byte_length,
      Utf16Le | Utf16Be => t.byte_length / 2,
    }
  }
}

// Truncation must respect character boundaries
fn truncate_text<E: Encoding>(
    t: Text<E>,
    max_bytes: Nat
) -> Text<E>
  requires { valid_encoding(t) }
  ensures  {
    result.byte_length <= max_bytes
    and valid_encoding(result)
    // Never splits a multi-byte character
    and result.char_count == count_complete_chars(t, max_bytes)
  }
  effects: pure

// FFI: returning text in the caller's expected encoding
#[feature(ffi)]
fn sqlite3_column_text(
    stmt: ptr<Statement>,
    col: c_int
) -> cstring
  callee_guarantees:
    // Return value encoding matches the database encoding
    encoding_matches(result, stmt.db.encoding)
    and valid_encoding(result)
    // Null-terminated
    and result[len(result)] == 0

fn sqlite3_column_text16(
    stmt: ptr<Statement>,
    col: c_int
) -> ptr<U16>
  callee_guarantees:
    // Always returns UTF-16 in native byte order
    valid_encoding(result)
    and encoding_of(result) == native_utf16()

// Collation registration must specify encoding
fn register_collation<E: Encoding>(
    conn: Connection,
    name: String,
    collation: Collation<E>
) -> Connection
  requires { conn.state @ Open }
  ensures  {
    // Collation is only used for comparisons in encoding E
    conn.collations[name].encoding == E
  }

// Index storage encoding must match table encoding
invariant {
  forall index in database.indices:
    forall column in index.columns:
      encoding_of(index.stored_key(column))
      == database.encoding
}

// COMPILE ERROR: building index with wrong encoding
fn build_index_wrong_encoding(
    db: Database,  // encoding = Utf8
    text: Text<Utf16Le>
) -> IndexKey
{
  IndexKey.from_text(text)
  // A43003: index key encoding (Utf16Le) doesn't match
  //         database encoding (Utf8)
}
```

##### Verification Rule

1. **Encoding matching**: Operations that combine two text values
   require the same encoding type parameter. Mismatch is a
   compile error, not a runtime transcode
2. **Valid encoding**: The `valid_encoding(t)` predicate asserts
   the bytes are well-formed for the declared encoding. This is
   checked at input boundaries (file read, FFI receive) and
   preserved through operations (the compiler proves concat,
   truncate, etc. preserve validity)
3. **Transcode tracking**: Every `transcode()` call is explicit.
   The compiler rejects implicit encoding conversion
4. **Boundary safety**: Truncation and substring operations must
   not split multi-byte sequences. The compiler checks this via
   the encoding's character boundary rules
5. **FFI encoding**: `sqlite3_column_text` returns the database
   encoding; `sqlite3_column_text16` returns native UTF-16. The
   contract makes this explicit

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A43001 | Encoding mismatch: `E1` vs `E2` | Operating on texts with different encodings |
| A43002 | Text truncated at multi-byte boundary | Truncation splits a character |
| A43003 | Storage encoding doesn't match database encoding | Index/column uses wrong encoding |
| A43004 | Invalid encoding: byte sequence not valid `E` | Bytes don't form valid UTF-8/16 |
| A43005 | Implicit transcode detected | Encoding conversion without explicit transcode() |

##### Rust Codegen

Encoding contracts generate parameterized string types:

```rust
/// Marker types for encoding
pub struct Utf8;
pub struct Utf16Le;
pub struct Utf16Be;

pub trait Encoding: sealed::Sealed {
    fn validate(bytes: &[u8]) -> bool;
    fn char_count(bytes: &[u8]) -> usize;
    fn truncate_to_char_boundary(bytes: &[u8], max: usize) -> usize;
}

impl Encoding for Utf8 {
    fn validate(bytes: &[u8]) -> bool {
        std::str::from_utf8(bytes).is_ok()
    }

    fn char_count(bytes: &[u8]) -> usize {
        std::str::from_utf8(bytes)
            .map(|s| s.chars().count())
            .unwrap_or(0)
    }

    fn truncate_to_char_boundary(bytes: &[u8], max: usize) -> usize {
        if max >= bytes.len() { return bytes.len(); }
        let mut i = max;
        while i > 0 && bytes[i] & 0xC0 == 0x80 { i -= 1; }
        i
    }
}

impl Encoding for Utf16Le {
    fn validate(bytes: &[u8]) -> bool {
        if bytes.len() % 2 != 0 { return false; }
        let units: Vec<u16> = bytes.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).is_ok()
    }

    fn char_count(bytes: &[u8]) -> usize {
        let units: Vec<u16> = bytes.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units)
            .map(|s| s.chars().count())
            .unwrap_or(0)
    }

    fn truncate_to_char_boundary(bytes: &[u8], max: usize) -> usize {
        let max = max & !1; // round down to even
        if max >= bytes.len() { return bytes.len(); }
        // Check for surrogate pair split
        if max >= 2 {
            let unit = u16::from_le_bytes([bytes[max - 2], bytes[max - 1]]);
            if (0xD800..=0xDBFF).contains(&unit) {
                return max - 2; // Don't split surrogate pair
            }
        }
        max
    }
}

/// Text with compile-time encoding guarantee
pub struct Text<E: Encoding> {
    bytes: Vec<u8>,
    _encoding: std::marker::PhantomData<E>,
}

impl<E: Encoding> Text<E> {
    pub fn new(bytes: Vec<u8>) -> Result<Self, EncodingError> {
        if !E::validate(&bytes) {
            return Err(EncodingError::InvalidBytes);
        }
        Ok(Text { bytes, _encoding: PhantomData })
    }

    pub fn byte_length(&self) -> usize { self.bytes.len() }
    pub fn char_count(&self) -> usize { E::char_count(&self.bytes) }

    pub fn truncate(&self, max_bytes: usize) -> Self {
        let safe = E::truncate_to_char_boundary(&self.bytes, max_bytes);
        Text {
            bytes: self.bytes[..safe].to_vec(),
            _encoding: PhantomData,
        }
    }
}

/// Transcoding between encodings
pub fn transcode<From: Encoding, To: Encoding>(
    input: &Text<From>,
) -> Result<Text<To>, EncodingError> {
    // Decode to Unicode code points, then re-encode
    let s = decode_to_string::<From>(&input.bytes)?;
    let bytes = encode_from_string::<To>(&s)?;
    Ok(Text { bytes, _encoding: PhantomData })
}

// COMPILE ERROR: this doesn't compile because E1 != E2
// fn compare<E1: Encoding, E2: Encoding>(a: &Text<E1>, b: &Text<E2>)
//   -> Ordering { ... }

// Only same-encoding comparison compiles
pub fn compare<E: Encoding>(a: &Text<E>, b: &Text<E>)
    -> std::cmp::Ordering
{
    a.bytes.cmp(&b.bytes) // Safe: both validated as encoding E
}
```


#### FMT.4 Codec Registry / Format Dispatch

Contracts for multi-format systems where input is dispatched to
format-specific decoders based on magic bytes, headers, or file
codecs, with per-format contract sets.

##### Motivation

stb_image supports 9 image formats: PNG, JPEG, GIF, BMP, PSD,
TGA, HDR, PIC, PNM. The dispatch logic is:

1. Read first N bytes of input
2. Match magic bytes to determine format
3. Call the format-specific decoder
4. Return pixels in a uniform output format

Each format has its own contract surface (PNG needs chunk validation,
JPEG needs Huffman tables, GIF needs LZW bounds). Interface contracts
(TYPE.1) handle polymorphism but do not express:
- "The dispatch correctly identifies the format from magic bytes"
- "Each codec satisfies format-specific contracts AND the common
  output contract"
- "No format is silently dropped or misidentified"

This is a common pattern beyond image decoding: file format libraries,
network protocol handlers, serialization frameworks.

##### Grammar

```ebnf
CodecRegistryDecl = 'codec_registry' Ident '{'
                      'output' ':' TypeExpr ','
                      { CodecEntry }
                    '}' ;

CodecEntry        = 'codec' Ident '{'
                      'magic' ':' MagicPattern ','
                      'decoder' ':' FnIdent ','
                      [CodecContracts]
                    '}' ;

MagicPattern      = BytePattern
                  | 'extension' '(' StringLit { ',' StringLit } ')'
                  | 'probe' '(' FnIdent ')' ;

BytePattern       = '[' ByteExpr { ',' ByteExpr } ']'
                  | '[' ByteExpr { ',' ByteExpr } ',' '..' ']' ;

CodecContracts    = 'contracts' ':' '{' { ContractDecl } '}' ;
```

##### Full Example: stb_image Format Registry

```assura
// Common output type for all image decoders
type ImageOutput {
  pixels: Region<output_size>,
  width: U32,
  height: U32,
  channels: U8,

  invariant {
    width >= 1 && height >= 1,
    channels >= 1 && channels <= 4,
    output_size == width as U64 * height as U64 * channels as U64,
    output_size <= MAX_IMAGE_ALLOC
  }
}

codec_registry ImageCodecs {
  output: ImageOutput,

  codec Png {
    magic: [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    decoder: decode_png,
    contracts: {
      // PNG-specific: chunk CRC validation
      ensures chunk_crcs_valid(input),
      // PNG-specific: IHDR dimensions match output
      ensures result.width == ihdr.width,
      ensures result.height == ihdr.height
    }
  }

  codec Jpeg {
    magic: [0xFF, 0xD8, 0xFF, ..],
    decoder: decode_jpeg,
    contracts: {
      // JPEG-specific: Huffman tables valid
      requires valid_huffman_tables(input),
      // JPEG-specific: IDCT accuracy
      ensures idct_meets_ieee1180(result)
    }
  }

  codec Gif {
    magic: [0x47, 0x49, 0x46, 0x38, ..],  // "GIF8"
    decoder: decode_gif,
    contracts: {
      // GIF-specific: LZW code size bounds
      ensures lzw_codes_in_range(input)
    }
  }

  codec Bmp {
    magic: [0x42, 0x4D, ..],  // "BM"
    decoder: decode_bmp
  }

  codec Hdr {
    probe: is_hdr_format,  // complex header detection
    decoder: decode_hdr,
    contracts: {
      ensures result.channels == 3  // HDR is always RGB
    }
  }
}

// The registry generates the dispatch function
fn decode_image(
    data: &[U8] :tainted
) -> ImageOutput | DecodeError
  ensures {
    // Common contract: output satisfies ImageOutput invariant
    result.pixels.len() == result.width as U64
      * result.height as U64 * result.channels as U64
  }
{
  // Auto-generated: try each codec's magic pattern in order
  // First match wins
  ImageCodecs.dispatch(data)
}
```

##### Verification Rule

1. **Magic uniqueness**: The verifier checks that no two codecs
   have overlapping magic patterns. If PNG starts with
   `[0x89, 0x50, ...]` and another codec also matches those
   bytes, it is a compile error
2. **Codec completeness**: If a codec has no `contracts` block,
   it inherits only the common output contract. The verifier
   warns if format-specific invariants are likely missing
3. **Contract inheritance**: Every codec's decoder must satisfy
   both its format-specific contracts AND the registry's common
   output type invariant
4. **Probe functions**: `probe` functions must be pure (no side
   effects) and total. They receive a read-only slice and return
   bool

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A52001 | Overlapping magic patterns for codecs `A` and `B` | Ambiguous dispatch |
| A52002 | Codec decoder does not return registry output type | Return type mismatch |
| A52003 | Codec-specific contract violated by decoder | Format-specific ensures failed |
| A52004 | Probe function has side effects | Probe must be pure |
| A52005 | No codec matches input | All magic patterns failed, no fallback |

##### Rust Codegen

Codec registries generate dispatch functions with pattern matching:

```rust
pub fn decode_image(data: &[u8]) -> Result<ImageOutput, DecodeError> {
    if data.len() >= 8 && data[..8] == [0x89, 0x50, 0x4E, 0x47,
                                         0x0D, 0x0A, 0x1A, 0x0A] {
        decode_png(data)
    } else if data.len() >= 3 && data[0] == 0xFF
           && data[1] == 0xD8 && data[2] == 0xFF {
        decode_jpeg(data)
    } else if data.len() >= 4 && &data[..4] == b"GIF8" {
        decode_gif(data)
    } else if data.len() >= 2 && &data[..2] == b"BM" {
        decode_bmp(data)
    } else if is_hdr_format(data) {
        decode_hdr(data)
    } else {
        Err(DecodeError::UnknownFormat)
    }
}
```


#### FMT.5 Checksum and Integrity Contracts

Contracts that enforce data integrity verification before use.

##### Grammar

```ebnf
IntegrityDecl  = 'integrity' TypeIdent '{'
                   VerifyClause
                   OnReadClause
                   OnWriteClause
                 '}' ;

VerifyClause   = 'verify' ':' Predicate ;
OnReadClause   = 'on_read' ':' IntegrityAction ;
OnWriteClause  = 'on_write' ':' IntegrityAction ;
IntegrityAction = 'MUST' 'verify' 'before' 'using' Ident
               | 'MUST' 'compute' Ident 'from' IdentList ;
```

##### Full Example: WAL Frame

```assura
type WalFrame {
  page_number: U32,
  commit_size: U32,
  salt: (U32, U32),
  checksum: (U32, U32),
  data: Region<PageSize>
}

integrity WalFrame {
  verify: checksum == wal_checksum(page_number, commit_size, salt, data)
  on_read: MUST verify before using data
  on_write: MUST compute checksum from page_number, commit_size, salt, data
}

// COMPILE ERROR: using frame.data without verification
fn bad_read(frame: WalFrame) -> Page
  effects: pure
{ Page.from_region(frame.data) }
  // A30001: integrity not verified

// CORRECT: verify then use
fn good_read(frame: WalFrame) -> Page | CorruptionError
  effects: pure
{
  if not frame.verify_integrity() {
    return CorruptionError("checksum mismatch")
  }
  Page.from_region(frame.data)
}

// COMPILE ERROR: writing frame without computing checksum
fn bad_write(page: Page, page_num: U32) -> WalFrame
  effects: pure
{
  WalFrame {
    page_number: page_num,
    data: page.to_region(),
    checksum: (0, 0)  // A30002: checksum not computed from fields
  }
}
```

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A30001 | Data used without integrity verification | Missing checksum check |
| A30002 | Integrity field not computed from sources | Checksum hardcoded or stale |
| A30003 | Integrity check result ignored | Verified but result not used in branch |

##### Rust Codegen

Integrity contracts generate wrapper types that enforce the
verify-before-use pattern:

```rust
pub struct Unverified<T>(T);
pub struct Verified<T>(T);

impl Unverified<WalFrame> {
    pub fn verify(self) -> Result<Verified<WalFrame>, CorruptionError> {
        let frame = &self.0;
        let expected = wal_checksum(
            frame.page_number, frame.commit_size,
            frame.salt, &frame.data,
        );
        if frame.checksum != expected {
            return Err(CorruptionError::ChecksumMismatch);
        }
        Ok(Verified(self.0))
    }
}

impl Verified<WalFrame> {
    pub fn data(&self) -> &[u8] { &self.0.data }
}
// Unverified<WalFrame> has NO .data() method -- can't access without verify
```


#### FMT.6 Protocol Grammar Conformance

Contracts that verify a parser accepts exactly the language defined
by a protocol specification (RFC ABNF), with explicit annotations for
intentional deviations.

##### Motivation

picohttpparser's `get_token_to_eol` accepts bare LF as a line
terminator (deviating from RFC 9112 which requires CRLF). Its
chunked decoder rejects bare LF in chunk headers. This inconsistency
within the same library is the exact class of differential that
CVE-2023-30589 (Node.js llhttp) exploited. No existing Assura feature
verifies a parser against its protocol grammar or documents deliberate
deviations.

##### Grammar

```ebnf
ProtocolDecl     = 'protocol' Ident '{' RfcRef { ',' RfcRef } '}' ;
RfcRef           = 'rfc' '(' IntLit ')' ;
ConformsAnnotation = '#[conforms(' Ident ')]' ;
DeviationAnnotation = '#[deviation(' StringLit ')]' ;
AcceptsRule      = 'accepts' ':' ProductionRef ';' ;
RejectsRule      = 'rejects' ':' ProductionRef ';' ;
ProductionRef    = Ident '::' Ident ;
```

##### Full Example

```assura
protocol HTTP1 {
    rfc(9110),  // HTTP Semantics
    rfc(9112)   // HTTP/1.1
}

#[conforms(HTTP1)]
fn parse_request_line(
    buf: Bytes @taint:untrusted
) -> RequestLine | ParseError
{
    let method = parse_token(buf)  // RFC 9110 token production
    expect_sp(buf)                 // exactly one SP

    #[deviation("Accept bare LF per RFC 9112 Section 2.2 robustness")]
    let line_end = find_line_ending(buf)  // CRLF or LF

    // Verifier checks:
    // 1. parse_token matches RFC 9110 ABNF 'token' production
    // 2. expect_sp accepts exactly SP (0x20), not HT
    // 3. deviation is documented for bare LF acceptance
}

#[conforms(HTTP1)]
fn parse_chunk_header(
    buf: Bytes @taint:untrusted
) -> ChunkHeader | ParseError
{
    let size = parse_hex(buf)
    expect_crlf(buf)  // NO deviation: strict CRLF required
    // If this also accepted bare LF, verifier would flag
    // inconsistency with parse_request_line's deviation
}
```

##### Verification Rule

1. Functions marked `#[conforms(Protocol)]` are checked against the
   referenced RFC productions
2. Any behavior that deviates from the RFC MUST have a `#[deviation]`
   annotation explaining why
3. Inconsistent deviations across functions in the same protocol are
   flagged (one function accepts bare LF, another rejects it)
4. Strict mode: deviations are compile errors (for security-critical parsers)

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A-FMT-010 | RFC production mismatch | Parser accepts/rejects input the RFC does not |
| A-FMT-011 | Undocumented deviation | Parser deviates from RFC without #[deviation] |
| A-FMT-012 | Inconsistent deviation | Same protocol, different functions, contradictory leniency |
| A-FMT-013 | Deviation in strict mode | #[deviation] used but strict conformance required |

##### Rust Codegen

Protocol conformance is verified at compile time. Deviations generate
documentation comments and optional runtime logging:

```rust
/// Parses HTTP request line per RFC 9112.
/// DEVIATION: Accepts bare LF (RFC 9112 Section 2.2 robustness).
fn parse_request_line(buf: &[u8]) -> Result<RequestLine, ParseError> {
    let method = parse_token(buf)?;
    let sp = expect_byte(buf, b' ')?;

    // Deviation: accept LF or CRLF
    let line_end = buf.iter().position(|&b| b == b'\n')
        .ok_or(ParseError::Incomplete)?;

    #[cfg(feature = "log_deviations")]
    if buf[line_end.saturating_sub(1)] != b'\r' {
        log::debug!("bare LF accepted in request line");
    }

    Ok(RequestLine { method, ..})
}
```

### 14.NUM: Numerical and Precision

#### NUM.1 Numerical Precision Contracts

Contracts that specify acceptable floating-point precision,
rounding behavior, and numerical accuracy bounds for mathematical
computations.

##### Motivation

CONC.3 (Determinism) bans floating-point transcendentals
entirely, which works for SQLite (integer-heavy). But image and
signal processing require floating-point math with controlled
precision:

- **JPEG IDCT**: IEEE 1180-1990 specifies maximum error of +/-1
  for any pixel, and RMS error < 0.02 across an 8x8 block. The
  Rust implementation must meet this standard or images decode
  differently.
- **HDR tone mapping**: Converts linear float values to display
  gamma. Precision matters for banding artifacts.
- **Color space conversion**: sRGB to linear involves `pow(x, 2.4)`.
  Different implementations of `pow` give different results at
  different ULP (Unit of Least Precision) levels.
- **PNG gamma correction**: `pow(sample / max, gamma)` must be
  accurate enough that round-trip encode/decode preserves the
  original within 1 bit.

The problem is not "floats are bad" but "floats need contracts."
The absence of precision contracts means silent image corruption
that no safety check catches.

##### Grammar

```ebnf
PrecisionDecl  = 'precision' Ident '{'
                   { PrecisionRule }
                 '}' ;

PrecisionRule  = 'max_ulp_error' ':' Expr
               | 'max_abs_error' ':' Expr
               | 'rms_error' ':' Expr
               | 'rounding' ':' RoundingMode
               | 'reference' ':' FnIdent ;

RoundingMode   = 'nearest_even' | 'toward_zero'
               | 'toward_inf' | 'toward_neg_inf' ;

PrecisionAttr  = '#[precision(' Ident ')]' ;
```

##### Full Example: JPEG IDCT

```assura
// Precision contract for JPEG IDCT
// Based on IEEE 1180-1990 accuracy requirements
precision IdctAccuracy {
  max_abs_error: 1       // any output pixel: |actual - ref| <= 1
  rms_error: 0.02        // across 8x8 block
  reference: reference_idct_f64  // double-precision reference
}

#[precision(IdctAccuracy)]
fn idct_8x8(coefficients: &[I16; 64]) -> [U8; 64]
  requires {
    for_all(i in 0..64, coefficients[i] >= -2048
                     && coefficients[i] <= 2047)
  }
  ensures {
    for_all(i in 0..64, result[i] <= 255)
  }
{
  // Fixed-point or float implementation
  // Verifier checks against reference_idct_f64
  // using the IdctAccuracy bounds
  let mut workspace = [0f32; 64]

  // Column pass
  for col in 0..8 {
    idct_1d_column(coefficients, &mut workspace, col)
  }

  // Row pass + clamp to [0, 255]
  let mut output = [0u8; 64]
  for row in 0..8 {
    idct_1d_row(&workspace, &mut output, row)
  }

  output
}

// Reference implementation (double precision, not perf-optimized)
fn reference_idct_f64(coefficients: &[I16; 64]) -> [F64; 64]
  must_be deterministic
{
  let mut output = [0.0f64; 64]
  for y in 0..8 {
    for x in 0..8 {
      let mut sum = 0.0f64
      for v in 0..8 {
        for u in 0..8 {
          let cu = if u == 0 { 1.0 / 2.0.sqrt() } else { 1.0 }
          let cv = if v == 0 { 1.0 / 2.0.sqrt() } else { 1.0 }
          sum += cu * cv
            * coefficients[v * 8 + u] as F64
            * cos((2.0 * x as F64 + 1.0) * u as F64 * PI / 16.0)
            * cos((2.0 * y as F64 + 1.0) * v as F64 * PI / 16.0)
        }
      }
      output[y * 8 + x] = sum / 4.0
    }
  }
  output
}

// Color space conversion with precision
precision SrgbPrecision {
  max_ulp_error: 4       // within 4 ULPs of reference
  rounding: nearest_even
}

#[precision(SrgbPrecision)]
fn srgb_to_linear(srgb: F32) -> F32
  requires { srgb >= 0.0 && srgb <= 1.0 }
  ensures { result >= 0.0 && result <= 1.0 }
{
  if srgb <= 0.04045 {
    srgb / 12.92
  } else {
    ((srgb + 0.055) / 1.055).powf(2.4)
  }
}
```

##### Verification Rule

1. **Reference comparison**: The verifier evaluates both the
   implementation and the reference function on a representative
   set of inputs. For IDCT, this is the IEEE 1180 test vectors.
   Violations produce A51001
2. **ULP tracking**: For `max_ulp_error`, the verifier tracks
   floating-point operations and their cumulative error propagation
   using interval arithmetic
3. **RMS computation**: For `rms_error`, the verifier evaluates
   the reference and implementation on the test set, computes the
   RMS difference, and checks it against the bound
4. **Rounding mode**: `rounding` constraints generate
   `f32::round_ties_even()` or equivalent in Rust codegen, and
   the verifier confirms no implicit rounding mode change

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A51001 | Absolute error exceeds bound: actual `E`, max `M` | Output deviates from reference beyond max_abs_error |
| A51002 | ULP error exceeds bound: actual `E`, max `M` | Floating-point result too far from exact value |
| A51003 | RMS error exceeds bound across test set | Aggregate precision below standard |
| A51004 | No reference function for precision contract | `precision` block has no `reference` |
| A51005 | Reference function uses restricted operations | Reference must be `deterministic` and total |

##### Rust Codegen

Precision contracts generate test harnesses that verify accuracy
against the reference implementation:

```rust
#[cfg(test)]
mod precision_tests {
    use super::*;

    #[test]
    fn idct_ieee1180_accuracy() {
        // IEEE 1180 test vectors
        for block in ieee1180_test_vectors() {
            let actual = idct_8x8(&block);
            let reference = reference_idct_f64(&block);
            let mut sum_sq_err = 0.0f64;
            for i in 0..64 {
                let err = (actual[i] as f64 - reference[i]).abs();
                assert!(err <= 1.0,
                    "pixel {i}: error {err} exceeds max 1.0");
                sum_sq_err += err * err;
            }
            let rms = (sum_sq_err / 64.0).sqrt();
            assert!(rms < 0.02,
                "RMS error {rms} exceeds max 0.02");
        }
    }
}
```


#### NUM.2 Precomputed Table Verification

Contracts that prove lookup tables are correct precomputations of
their generating functions. Ensures tables match their mathematical
definitions at compile time.

##### Motivation

Systems code is full of precomputed tables used for performance:

- **Huffman tables**: prebuilt from code length arrays, used
  millions of times per decode. If a table entry is wrong,
  the output is silently corrupt.
- **Zigzag reorder table**: maps linear indices to zigzag scan
  order for JPEG's 8x8 DCT blocks. One wrong entry produces
  subtly wrong image output.
- **CRC32 tables**: 256 entries precomputed from the polynomial.
  A wrong entry produces wrong checksums that silently accept
  corrupt data.
- **Quantization tables**: JPEG quality scaling applied to base
  tables. Off-by-one in scaling produces slightly wrong images.

These tables are typically defined as `static const` arrays with
no connection to their mathematical definition. If someone edits
the table or a code generator has a bug, there is no compile-time
check. Assura's integrity contracts (FMT.5) verify checksums on
data, but not that a table matches its generating formula.

##### Grammar

```ebnf
TableDecl      = 'table' Ident ':' '[' TypeExpr ';' Expr ']'
                 '=' 'precompute' '(' GeneratingExpr ')'
                 [TableVerify] ;

GeneratingExpr = 'for' Ident 'in' RangeExpr '=>' Expr ;

TableVerify    = 'verify_against' ':' FnIdent
               | 'verify_property' ':' Predicate ;
```

##### Full Example: JPEG and CRC Tables

```assura
// Zigzag reorder table: maps linear 0..63 to zigzag scan order
table ZIGZAG_ORDER: [U8; 64] = precompute(
  for i in 0..64 => zigzag_index(i)
)
  verify_against: zigzag_index

// The verifier confirms: for all i in 0..64,
// ZIGZAG_ORDER[i] == zigzag_index(i)

// The generating function (pure, total)
fn zigzag_index(linear: U8) -> U8
  requires { linear < 64 }
  ensures { result < 64 }
  must_be deterministic
{
  // Zigzag traversal of 8x8 matrix
  let row = linear / 8
  let col = linear % 8
  // ... zigzag logic ...
}

// CRC32 table: 256 entries from polynomial 0xEDB88320
table CRC32_TABLE: [U32; 256] = precompute(
  for byte in 0..256 => crc32_for_byte(byte as U8)
)
  verify_property: for_all(i in 0..256,
    CRC32_TABLE[i] == crc32_for_byte(i as U8)
  )

fn crc32_for_byte(byte: U8) -> U32
  must_be deterministic
{
  let mut crc: U32 = byte as U32
  for _ in 0..8 {
    if crc & 1 == 1 {
      crc = (crc >> 1) ^ 0xEDB88320
    } else {
      crc = crc >> 1
    }
  }
  crc
}

// JPEG default quantization table with quality scaling
fn scaled_quant_table(
    base: &[U8; 64],
    quality: U8
) -> [U8; 64]
  requires { quality >= 1 && quality <= 100 }
  ensures {
    for_all(i in 0..64,
      result[i] >= 1 && result[i] <= 255
    )
  }
  must_be deterministic
{
  let scale = if quality < 50 {
    5000 / (quality as U16)
  } else {
    200 - 2 * (quality as U16)
  }
  let mut out = [0u8; 64]
  for i in 0..64 {
    let val = ((base[i] as U16 * scale + 50) / 100)
      .clamp(1, 255) as U8
    out[i] = val
  }
  out
}
```

##### Verification Rule

1. **Exhaustive check**: For tables with bounded index ranges
   (0..N), the verifier evaluates the generating function for
   every index and confirms equality. This is Layer 0 (structural)
   when N is small (<= 65536), Layer 1 otherwise
2. **Property check**: `verify_property` predicates are checked
   via SMT for all indices in the domain
3. **Determinism required**: Generating functions must be marked
   `deterministic` (CONC.3). A table generated from a
   non-deterministic function is a compile error
4. **Referential transparency**: If the table is used in verified
   code, the verifier may substitute `TABLE[i]` with
   `generating_fn(i)` for proof purposes

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A50001 | Table entry `T[i]` does not match generating function | `T[i] != gen(i)` for some i |
| A50002 | Generating function is not deterministic | `precompute` requires `must_be deterministic` |
| A50003 | Table index range exceeds type bounds | Index type cannot hold the range |
| A50004 | Generating function is not total over range | Function may fail for some index in range |
| A50005 | Table size mismatch | Declared size does not match range |

##### Rust Codegen

Precomputed tables generate `const` arrays with compile-time
evaluation or build-script generation:

```rust
// Small tables: const evaluation
const ZIGZAG_ORDER: [u8; 64] = {
    let mut table = [0u8; 64];
    let mut i = 0;
    while i < 64 {
        table[i] = zigzag_index(i as u8);
        i += 1;
    }
    table
};

// Large tables: build.rs generation + test verification
#[cfg(test)]
mod table_tests {
    #[test]
    fn verify_crc32_table() {
        for i in 0..256u32 {
            assert_eq!(
                CRC32_TABLE[i as usize],
                crc32_for_byte(i as u8),
                "CRC32 table mismatch at index {i}"
            );
        }
    }
}
```


### 14.PLAT: Platform and Configuration

#### PLAT.1 Platform Abstraction Contracts

Contracts that hold across all platform-specific implementations.
When code is conditionally compiled, the compiler verifies that
every platform variant satisfies the shared contract.

##### Motivation

SQLite runs on every platform: Linux, Windows, macOS, iOS, Android,
embedded RTOS, bare metal. It uses a VFS abstraction layer with
~15 platform-specific implementations. Each must provide the same
guarantees (atomicity of writes, lock semantics, sync durability)
despite wildly different OS primitives.

##### Grammar

```ebnf
PlatformDecl   = 'platform' TypeIdent '{'
                   { PlatformVariant }
                   SharedContract
                 '}' ;

PlatformVariant = '#[cfg(' CfgExpr ')]'
                  'variant' TypeIdent '{'
                    { FnDecl }
                  '}' ;

SharedContract  = 'contract' '{'
                    { RequiresClause | EnsuresClause
                      | InvariantDecl | EffectsClause }
                  '}' ;

CfgExpr        = Ident '=' StringLit
               | 'not' '(' CfgExpr ')'
               | 'any' '(' CfgExpr { ',' CfgExpr } ')'
               | 'all' '(' CfgExpr { ',' CfgExpr } ')' ;
```

##### Full Example: Platform File Sync

```assura
platform FileSync {
  // Unix: fdatasync or fsync
  #[cfg(os = "unix")]
  variant UnixSync {
    fn sync(fd: FileDescriptor, full: Bool) -> Bool
      effects: filesystem.sync
    {
      if full { fsync(fd) } else { fdatasync(fd) }
    }

    fn atomic_write(fd: FileDescriptor, data: Region<n>,
                    offset: Nat) -> Bool
      requires { n <= SECTOR_SIZE }
      effects: filesystem.write
    {
      pwrite(fd, data, offset)
    }
  }

  // Windows: FlushFileBuffers
  #[cfg(os = "windows")]
  variant WindowsSync {
    fn sync(fd: FileDescriptor, full: Bool) -> Bool
      effects: filesystem.sync
    {
      FlushFileBuffers(fd)
      // Windows has no fdatasync equivalent; always full sync
    }

    fn atomic_write(fd: FileDescriptor, data: Region<n>,
                    offset: Nat) -> Bool
      requires { n <= SECTOR_SIZE }
      effects: filesystem.write
    {
      WriteFile(fd, data, offset)
    }
  }

  // Embedded: direct flash write
  #[cfg(os = "none")]
  variant BareMetalSync {
    fn sync(fd: FileDescriptor, full: Bool) -> Bool
      effects: filesystem.sync
    {
      flash_barrier()
    }

    fn atomic_write(fd: FileDescriptor, data: Region<n>,
                    offset: Nat) -> Bool
      requires { n <= SECTOR_SIZE }
      effects: filesystem.write
    {
      flash_write(fd, data, offset)
    }
  }

  // ALL variants must satisfy these contracts
  contract {
    // After sync returns true, data is durable
    ensures {
      forall write W before sync():
        sync() == true => W is durable on storage
    }

    // Atomic writes are all-or-nothing
    ensures {
      forall atomic_write(fd, data, offset):
        result == true =>
          read(fd, offset, len(data)) == data
        // On crash during write: either old data or new data,
        // never partial
    }

    // Sync must be idempotent
    invariant { sync(fd, full) ; sync(fd, full) == sync(fd, full) }

    // Platform must not introduce effects beyond what's declared
    effects: filesystem.sync, filesystem.write
  }
}
```

##### Verification Rule

1. Each variant is verified independently against the shared contract
2. The shared contract becomes a precondition for all callers,
   regardless of which variant is active at compile time
3. If a variant cannot satisfy the shared contract (e.g., a platform
   lacks atomic write guarantees), it must be explicitly excluded
   with a `#[cfg(not(...))]` guard on the caller

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A33001 | Platform variant `V` violates shared contract | Variant doesn't satisfy ensures/invariant |
| A33002 | Missing variant for target platform `P` | Compilation target has no matching #[cfg] |
| A33003 | Variant has effects beyond shared contract | Platform-specific side effects not declared |

##### Rust Codegen

Platform contracts generate cfg-gated modules with a unified trait:

```rust
pub trait FileSync {
    fn sync(&self, fd: RawFd, full: bool) -> bool;
    fn atomic_write(&self, fd: RawFd, data: &[u8], offset: u64) -> bool;
}

#[cfg(unix)]
mod unix {
    pub struct UnixSync;
    impl FileSync for UnixSync {
        fn sync(&self, fd: RawFd, full: bool) -> bool {
            if full { libc::fsync(fd) == 0 }
            else { libc::fdatasync(fd) == 0 }
        }
        fn atomic_write(&self, fd: RawFd, data: &[u8], offset: u64) -> bool {
            debug_assert!(data.len() <= SECTOR_SIZE);
            libc::pwrite(fd, data.as_ptr() as _, data.len(), offset as _) >= 0
        }
    }
}

#[cfg(windows)]
mod windows {
    pub struct WindowsSync;
    impl FileSync for WindowsSync {
        fn sync(&self, fd: RawFd, _full: bool) -> bool {
            unsafe { FlushFileBuffers(fd as _) != 0 }
        }
        fn atomic_write(&self, fd: RawFd, data: &[u8], offset: u64) -> bool {
            debug_assert!(data.len() <= SECTOR_SIZE);
            // ... WriteFile with OVERLAPPED ...
            true
        }
    }
}
```


#### PLAT.2 Compile-Time Feature Flags

Contracts that adapt to compile-time feature configuration. When a
feature is omitted, the contracts that depend on it are eliminated
and the compiler verifies the remaining system is still consistent.

##### Motivation

SQLite has over 200 compile-time options (`SQLITE_OMIT_*` to remove
features, `SQLITE_MAX_*` to set limits, `SQLITE_ENABLE_*` to add
optional features). Examples: `SQLITE_OMIT_WAL` removes WAL support,
`SQLITE_OMIT_FOREIGN_KEY` removes foreign key enforcement,
`SQLITE_MAX_PAGE_SIZE` caps page size at compile time. When a feature
is omitted, all code paths that depend on it must be removed, and
any contract that references the removed feature must adapt. In C
this is done with `#ifdef` blocks scattered across 150,000 lines.
Assura makes this structured and verifiable.

##### Grammar

```ebnf
FeatureFlagDecl = 'feature' Ident [FeatureDefault]
                  [FeatureDeps] [FeatureExcludes] ;

FeatureDefault  = '=' 'enabled' | '=' 'disabled' ;
FeatureDeps     = 'requires' ':' IdentList ;
FeatureExcludes = 'excludes' ':' IdentList ;

FeatureGate     = '#[feature(' Ident ')]' ;
FeatureMaxDecl  = 'feature_max' Ident ':' TypeExpr '=' Expr ;

ConditionalContract = '#[feature(' Ident ')]' ContractClause ;
```

##### Full Example: SQLite Feature Configuration

```assura
// Feature declarations (in config module)
module config {
  feature wal = enabled
  feature fts5 = enabled
    requires: wal  // FTS5 needs WAL for atomic indexing
  feature rtree = disabled
  feature foreign_keys = enabled
  feature json = enabled
  feature icu = disabled
    excludes: builtin_collation  // ICU replaces built-in collation

  // Compile-time maximums that narrow refinement types
  feature_max max_page_size: Nat = 65536
  feature_max max_sql_length: Nat = 1_000_000_000
  feature_max max_column: Nat = 2000
  feature_max max_attached: Nat = 10
  feature_max max_variable_number: Nat = 32766
  feature_max max_page_count: Nat = 4294967294  // 2^32 - 2
}

// Contracts that adapt to features
service Database {
  // WAL operations only exist when WAL is enabled
  #[feature(wal)]
  operation wal_checkpoint {
    input(mode: WalCheckpointMode)
    output(pages_checkpointed: Nat)
    requires { self.state @ Open }
    effects: database.write, filesystem.write
  }

  // Page size is bounded by the compile-time maximum
  type PageSize = {v: Nat |
    v in {512, 1024, 2048, 4096, 8192, 16384, 32768, 65536}
    and v <= config.max_page_size
  }

  // Column count is bounded
  type ColumnIndex = {v: Nat | v < config.max_column}

  // Foreign key enforcement only when feature is on
  #[feature(foreign_keys)]
  invariant {
    forall fk in self.foreign_keys:
      exists row in fk.parent_table:
        row[fk.parent_column] == fk.child_value
  }
}

// FTS5 module only exists when enabled
#[feature(fts5)]
module fts5 {
  service FullTextSearch {
    operation search {
      input(query: FtsQuery, table: String)
      output(results: List<FtsResult>)
      effects: database.read
    }
  }
}

// COMPILE ERROR: using FTS5 without feature gate
fn bad_search(db: Database, query: String) -> List<Row>
  effects: database.read
{
  db.fts5_search(query)  // A38001: fts5 feature not enabled
}

// CORRECT: gate the caller too
#[feature(fts5)]
fn search_if_available(db: Database, query: String) -> List<Row>
  effects: database.read
{
  db.fts5_search(query)  // OK: caller is also gated
}
```

##### Verification Rule

1. **Feature consistency**: If feature A requires feature B,
   enabling A without B is a compile error
2. **Feature exclusion**: If A excludes B, enabling both is a
   compile error
3. **Contract narrowing**: When `feature_max max_page_size = 4096`,
   all refinement types involving page_size are automatically
   narrowed (the SMT context includes `page_size <= 4096`)
4. **Dead code elimination**: Gated modules/operations/contracts
   are fully removed when the feature is disabled; no codegen,
   no verification cost
5. **Feature propagation**: If a function calls a feature-gated
   function, it must itself be gated or guard the call with a
   feature check

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A38001 | Feature `F` not enabled | Using feature-gated item without gate |
| A38002 | Feature `F` requires `G` which is disabled | Dependency not met |
| A38003 | Features `F` and `G` are mutually exclusive | Exclusion violated |
| A38004 | Feature max `M` too small for invariant | Max value makes contract unsatisfiable |

##### Rust Codegen

Feature flags map directly to Rust's `cfg` and `feature` system:

```rust
// Cargo.toml
[features]
default = ["wal", "fts5", "foreign_keys", "json"]
wal = []
fts5 = ["wal"]
rtree = []
foreign_keys = []
json = []
icu = []

// Generated code
#[cfg(feature = "wal")]
pub mod wal {
    pub fn checkpoint(db: &mut Database, mode: CheckpointMode)
        -> Result<usize, SqlError>
    {
        debug_assert!(db.state == ConnectionState::Open);
        // ...
    }
}

// Compile-time constants from feature_max
pub const MAX_PAGE_SIZE: usize = 65536;
pub const MAX_SQL_LENGTH: usize = 1_000_000_000;
pub const MAX_COLUMN: usize = 2000;

// Refined types use the constant
pub struct PageSize(u32);
impl PageSize {
    pub fn new(v: u32) -> Option<Self> {
        if v.is_power_of_two() && v >= 512 && v <= MAX_PAGE_SIZE as u32 {
            Some(PageSize(v))
        } else {
            None
        }
    }
}
```


#### PLAT.3 Resource Limit Contracts

Contracts for runtime-configurable resource limits that change
what inputs are valid and how the system behaves when limits are
reached.

##### Motivation

SQLite's `sqlite3_limit()` lets users change 12 resource limits at
runtime: max SQL length, max column count, max expression depth, max
compound SELECT count, max VDBE operations, max function arguments,
max attached databases, max LIKE pattern length, max trigger depth,
max worker threads, max page count. These limits change the domain
of valid inputs. A query with 3000 columns is valid when
`SQLITE_LIMIT_COLUMN` is set to 4000 but rejected when set to 2000.
Static refinement types cannot express this because the bound is
not known at compile time.

##### Grammar

```ebnf
LimitDecl      = 'limit' Ident ':' TypeExpr '{'
                   'default' ':' Expr
                   'min' ':' Expr
                   'max' ':' Expr
                   'on_exceed' ':' LimitAction
                 '}' ;

LimitAction    = 'error' '(' Ident ')'
               | 'truncate'
               | 'deny' ;

LimitRef       = 'limit(' Ident ')' ;
```

##### Full Example: SQLite Runtime Limits

```assura
module limits {
  limit sql_length: Nat {
    default: 1_000_000
    min: 1
    max: config.max_sql_length  // compile-time upper bound
    on_exceed: error(SQLITE_TOOBIG)
  }

  limit column_count: Nat {
    default: 2000
    min: 1
    max: config.max_column
    on_exceed: error(SQLITE_LIMIT)
  }

  limit expr_depth: Nat {
    default: 1000
    min: 1
    max: 10000
    on_exceed: error(SQLITE_LIMIT)
  }

  limit attached_db: Nat {
    default: 10
    min: 0
    max: config.max_attached
    on_exceed: error(SQLITE_LIMIT)
  }

  limit page_count: Nat {
    default: 1073741823  // 2^30 - 1
    min: 1
    max: config.max_page_count
    on_exceed: error(SQLITE_FULL)
  }

  limit vdbe_ops: Nat {
    default: 0  // 0 = unlimited
    min: 0
    max: Nat.MAX
    on_exceed: error(SQLITE_INTERRUPT)
  }
}

// Using limits in contracts
fn parse_sql(
    conn: Connection,
    sql: String
) -> PreparedStatement | SqlError
  requires { len(sql) <= limit(sql_length) }
  requires { len(sql) > 0 }
  effects: pure
{
  let ast = parse(sql)?

  // Column count checked against runtime limit
  if count_columns(ast) > conn.limit(column_count) {
    return SqlError(SQLITE_LIMIT, "too many columns")
  }

  // Expression depth checked against runtime limit
  if expr_depth(ast) > conn.limit(expr_depth) {
    return SqlError(SQLITE_LIMIT, "expression tree too deep")
  }

  prepare(conn, ast)
}

// VDBE execution with operation limit
fn vdbe_execute(
    vm: VdbeEngine :_1,
    limit: Nat  // from conn.limit(vdbe_ops)
) -> (VdbeEngine :_1, VdbeResult)
  ensures {
    limit > 0 => vm.ops_executed <= limit
  }
  ensures {
    result is VdbeResult.Interrupted =>
      vm.ops_executed == limit
  }
  effects: database.read, database.write
```

##### Verification Rule

1. **Limit bounds**: The compiler verifies that `min <= default <= max`
   and that `max <= feature_max` (if a compile-time upper bound exists)
2. **Dynamic refinement**: Inside a function that accesses a limit,
   the SMT context includes `1 <= limit(X) <= max(X)` but the
   exact value is symbolic
3. **Exceed handling**: Every code path where a limit could be
   exceeded must either check the limit or propagate the error
4. **Limit monotonicity**: If a limit is lowered during execution,
   the compiler warns about existing objects that may violate the
   new limit

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A39001 | Limit `L` may be exceeded without check | No bounds check before limit-bounded operation |
| A39002 | Limit default outside [min, max] | Invalid limit configuration |
| A39003 | Limit max exceeds compile-time feature_max | Runtime max above static maximum |
| A39004 | Limit change may invalidate existing state | Lowering limit after creating objects at old limit |

##### Rust Codegen

Limits generate a configuration struct with runtime getter/setter
and const bounds:

```rust
pub struct Limits {
    sql_length: usize,
    column_count: usize,
    expr_depth: usize,
    attached_db: usize,
    page_count: usize,
    vdbe_ops: usize,
}

impl Limits {
    pub const fn defaults() -> Self {
        Limits {
            sql_length: 1_000_000,
            column_count: 2000,
            expr_depth: 1000,
            attached_db: 10,
            page_count: 1073741823,
            vdbe_ops: 0,
        }
    }

    pub fn set(&mut self, which: LimitId, value: usize) -> usize {
        let old = self.get(which);
        let clamped = value.clamp(
            LimitId::min(which),
            LimitId::max(which),
        );
        match which {
            LimitId::SqlLength => self.sql_length = clamped,
            LimitId::ColumnCount => self.column_count = clamped,
            // ...
        }
        old
    }
}
```


### 14.PERF: Performance

#### PERF.1 Unsafe Escape with Proof Obligation

A controlled escape hatch for performance-critical inner loops
that need raw pointer arithmetic, unchecked indexing, or SIMD
intrinsics, with a mandatory proof that safety invariants hold
at the boundary.

##### Motivation

SQLite's performance-critical code includes: cell parsing
(deserializing B-tree cells from raw bytes), record format
decoding (varint reading), memcpy for page operations, comparison
functions for sorting, and checksum computation. A naive safe Rust
port of these would add bounds checks on every byte access,
degrading performance by 10-30%. The `unsafe` escape lets the
programmer use unchecked operations inside a function while proving
at the boundary that all accesses are in-bounds.

This differs from invariant suspension (MISC.2): suspension pauses
a data structure invariant temporarily. Unsafe escape pauses
memory safety checks for performance, with a different proof
obligation (bounds, alignment, aliasing rather than structural
invariants).

##### Grammar

```ebnf
UnsafeEscape   = '#[unsafe_escape(' ProofList ')]' FnDecl ;

ProofList      = Proof { ',' Proof } ;
Proof          = 'bounds' '(' Ident ',' Expr ',' Expr ')'
               | 'aligned' '(' Ident ',' Expr ')'
               | 'no_alias' '(' Ident ',' Ident ')'
               | 'initialized' '(' Ident ',' Expr ',' Expr ')'
               | 'valid_utf8' '(' Ident ')'
               | 'non_null' '(' Ident ')' ;
```

##### Full Example: Fast Cell Parsing

```assura
// Safe wrapper with proof obligations
#[unsafe_escape(
  bounds(page, offset, offset + cell_size),
  bounds(page, header_offset, header_offset + header_size),
  aligned(page, 1)  // byte-aligned (no alignment requirement)
)]
fn parse_cell_fast(
    page: Region<PageSize>,
    offset: {v: Nat | v < PageSize},
    cell_size: {v: Nat | v > 0 and offset + v <= PageSize}
) -> CellHeader
  requires { offset + cell_size <= len(page) }
  ensures  {
    result.payload_size == read_varint(page, offset)
    and result.row_id == read_varint(page, offset + result.header_bytes)
  }
  effects: pure
{
  // Inside this function, the compiler allows:
  //   - Unchecked indexing: page[i] without bounds check
  //   - Raw pointer arithmetic: ptr.add(n) without overflow check
  //   - Transmute for varint decoding
  //
  // The compiler verifies AT THE BOUNDARY that:
  //   - All indices are within [offset, offset + cell_size)
  //   - No read extends past offset + cell_size
  //   - The bounds proof (requires clause) guarantees safety

  // Fast varint read (no bounds check per byte)
  let (payload_size, bytes1) = read_varint_unchecked(page, offset)
  let (row_id, bytes2) = read_varint_unchecked(page, offset + bytes1)

  CellHeader {
    payload_size,
    row_id,
    header_bytes: bytes1 + bytes2,
  }
}

// Fast memory comparison for B-tree key ordering
#[unsafe_escape(
  bounds(a, 0, a_len),
  bounds(b, 0, b_len),
  no_alias(a, b)
)]
fn memcmp_fast(
    a: Region<a_len>,
    b: Region<b_len>,
    compare_len: {v: Nat | v <= a_len and v <= b_len}
) -> Ordering
  requires { compare_len <= a_len and compare_len <= b_len }
  ensures  {
    result == compare_bytes(a[0..compare_len], b[0..compare_len])
  }
  effects: pure
{
  // SIMD-accelerated comparison when available
  // Falls back to word-at-a-time comparison
  // No per-byte bounds checks inside the loop
}

// COMPILE ERROR: unsafe escape without sufficient proof
#[unsafe_escape(
  bounds(page, offset, offset + size)
  // Missing: no proof that offset + size <= len(page)
)]
fn bad_parse(
    page: Region<PageSize>,
    offset: Nat,  // Not refined! Could be >= PageSize
    size: Nat
) -> CellHeader
  // No requires clause proving bounds
  // A42001: bounds proof obligation not satisfiable
{
  // ...
}
```

##### Verification Rule

1. **Proof obligations**: Each proof in the `#[unsafe_escape]` list
   becomes an SMT query. If any proof fails, the function is rejected
2. **Boundary checking**: The compiler verifies proofs at the
   function boundary (entry + all exit points), not inside the body
3. **Internal freedom**: Inside the function body, bounds checks,
   overflow checks, and alignment checks are elided
4. **Transitive safety**: An unsafe escape function may only be
   called from a context that satisfies its `requires` clauses
5. **Audit trail**: The compiler emits a report listing all unsafe
   escape functions and their proof status, for security review

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A42001 | Bounds proof not satisfiable for `R[a..b]` | Cannot prove index range is valid |
| A42002 | Alignment proof not satisfiable for `P` | Cannot prove pointer is aligned |
| A42003 | No-alias proof not satisfiable for `P, Q` | Cannot prove pointers don't alias |
| A42004 | Unsafe escape without proof obligation | Using escape hatch without any proof |
| A42005 | Proof obligation references out-of-scope variable | Proof variable not accessible |

##### Rust Codegen

Unsafe escape generates Rust `unsafe` blocks with the proof
obligations as comments and debug assertions:

```rust
/// # Safety
/// Caller guarantees:
/// - offset + cell_size <= page.len()
/// - All reads are within [offset, offset + cell_size)
#[inline(always)]
pub fn parse_cell_fast(
    page: &[u8],
    offset: usize,
    cell_size: usize,
) -> CellHeader {
    debug_assert!(offset + cell_size <= page.len());

    unsafe {
        let ptr = page.as_ptr().add(offset);

        // Varint decode without bounds checks
        let (payload_size, bytes1) = read_varint_unchecked(ptr);
        let (row_id, bytes2) = read_varint_unchecked(ptr.add(bytes1));

        CellHeader {
            payload_size,
            row_id,
            header_bytes: bytes1 + bytes2,
        }
    }
}

/// # Safety
/// Caller guarantees:
/// - a[0..compare_len] and b[0..compare_len] are valid
/// - a and b do not alias
#[inline(always)]
pub fn memcmp_fast(a: &[u8], b: &[u8], compare_len: usize)
    -> std::cmp::Ordering
{
    debug_assert!(compare_len <= a.len());
    debug_assert!(compare_len <= b.len());

    unsafe {
        let result = libc::memcmp(
            a.as_ptr() as *const _,
            b.as_ptr() as *const _,
            compare_len,
        );
        result.cmp(&0)
    }
}
```


#### PERF.2 Complexity Bounds

Contracts that specify the asymptotic time and space complexity of
operations, enabling the compiler to reject implementations that
violate performance guarantees.

##### Motivation

SQLite guarantees B-tree lookup in O(log N) time. Hash table
lookup is O(1) amortized. Full table scan is O(N). These are
documented, tested, and users depend on them. Totality checking
(Section 2) proves a function terminates, but it says nothing about
*how fast* it terminates. A function that scans the entire database
for every lookup is total but violates the O(log N) contract.

Complexity bounds are critical for a database: users choose
indices, query plans, and schema designs based on complexity
guarantees. A port that changes O(log N) lookup to O(N) is
functionally correct but practically broken.

##### Grammar

```ebnf
ComplexityAnnotation = '#[complexity(' ComplexitySpec ')]' ;

ComplexitySpec = 'time' '=' BigO
               | 'space' '=' BigO
               | 'amortized_time' '=' BigO
               | 'io_reads' '=' BigO
               | 'io_writes' '=' BigO ;

BigO           = 'O(1)' | 'O(log ' Ident ')'
               | 'O(' Ident ')' | 'O(' Ident ' log ' Ident ')'
               | 'O(' Ident '^2)' | 'O(' Ident '^' IntLit ')' ;
```

##### Full Example: SQLite Operation Complexity

```assura
// B-tree point lookup: O(log N) time, O(log N) IO reads
#[complexity(
  time = O(log N),
  space = O(1),
  io_reads = O(log N)
)]
fn btree_lookup(
    tree: BTree<K, V>,
    key: K
) -> Option<V>
  where N = tree.entry_count
  requires { BTreeValid(tree) }
  effects: database.read

// B-tree insertion: O(log N) amortized (rebalancing is rare)
#[complexity(
  amortized_time = O(log N),
  space = O(log N),    // stack frames for rebalance
  io_reads = O(log N),
  io_writes = O(log N)
)]
fn btree_insert(
    tree: BtCursor :_1,
    key: K,
    value: V
) -> BtCursor :_1
  where N = tree.entry_count
  effects: database.write

// Full table scan: O(N)
#[complexity(time = O(N), io_reads = O(N))]
fn table_scan(
    cursor: BtCursor :_1
) -> (BtCursor :_1, List<Row>)
  where N = cursor.tree.entry_count
  effects: database.read

// Hash table lookup: O(1) amortized
#[complexity(amortized_time = O(1), space = O(1))]
fn hash_lookup(
    table: HashTable<K, V>,
    key: K
) -> Option<V>
  effects: pure

// Sort: O(N log N) using merge sort
#[complexity(
  time = O(N log N),
  space = O(N),
  io_reads = O(N),
  io_writes = O(N)
)]
fn sort_result_set(
    rows: List<Row>,
    order_by: List<SortKey>
) -> List<Row>
  where N = len(rows)
  effects: database.read, database.write  // spill to temp file

// COMPILE WARNING: implementation may violate complexity bound
#[complexity(time = O(log N))]
fn suspicious_lookup(
    tree: BTree<K, V>,
    key: K
) -> Option<V>
{
  // This scans linearly! Violates O(log N) contract
  for entry in tree.entries() {   // A46001 warning
    if entry.key == key { return Some(entry.value) }
  }
  None
}

// Query planner uses complexity to choose strategy
fn choose_join_strategy(
    left: Table,
    right: Table,
    join_key: Column
) -> JoinStrategy
{
  let left_n = left.row_count
  let right_n = right.row_count

  if right.has_index(join_key) {
    // Nested loop with index: O(left_n * log(right_n))
    JoinStrategy.NestedLoopIndex
  } else if left_n * right_n < HASH_JOIN_THRESHOLD {
    // Hash join: O(left_n + right_n) but O(right_n) space
    JoinStrategy.HashJoin
  } else {
    // Sort-merge: O(left_n*log(left_n) + right_n*log(right_n))
    JoinStrategy.SortMerge
  }
}
```

##### Verification Approach

Complexity bounds are verified by a combination of:

1. **Loop analysis**: Count the number of iterations in terms of
   input size. A `for` loop over `tree.entries()` is O(N); a
   binary search loop with `mid = (lo + hi) / 2` is O(log N)
2. **Recursion depth**: A function that recurses with halving
   input is O(log N); with linear reduction is O(N)
3. **Callee composition**: If f is O(log N) and g calls f inside
   an O(N) loop, g is O(N log N)
4. **IO counting**: Reads and writes to pages count toward
   `io_reads` and `io_writes` bounds

Verification is best-effort (Layer 2, 10s). The compiler warns
when it cannot verify the bound rather than rejecting the code.
The `#[generate_tests]` annotation can generate benchmarks that
empirically validate complexity claims.

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A46001 | Implementation may exceed `O(X)` bound | Loop structure suggests higher complexity |
| A46002 | Recursive call does not reduce input | Recursion without decreasing measure |
| A46003 | Callee `F` with `O(X)` inside `O(Y)` loop | Composition exceeds declared bound |
| A46004 | IO bound exceeded: `O(X)` declared but `O(Y)` observed | More reads/writes than promised |

##### Rust Codegen

Complexity annotations generate benchmark tests that verify
the bound empirically:

```rust
#[cfg(test)]
mod complexity_tests {
    use criterion::*;

    // AUTO-GENERATED from #[complexity(time = O(log N))]
    fn btree_lookup_scaling(c: &mut Criterion) {
        let mut group = c.benchmark_group("btree_lookup");
        for size in [100, 1000, 10_000, 100_000, 1_000_000] {
            let tree = build_btree(size);
            let key = random_key(&tree);
            group.throughput(Throughput::Elements(1));
            group.bench_with_input(
                BenchmarkId::from_parameter(size),
                &size,
                |b, _| b.iter(|| tree.lookup(&key)),
            );
        }
        group.finish();
        // Post-analysis: fit curve, verify O(log N) not O(N)
    }
}
```


### 14.TEST: Testing and Verification Workflow

#### TEST.1 Test Generation from Contracts

Automatic generation of property-based tests and fuzz targets
from contract specifications. When formal verification times out
or reaches undecidable territory, the compiler generates executable
tests that check contracts empirically.

##### Motivation

SQLite has 90 million lines of test code for 150,000 lines of
source. These tests were written by hand over 25 years. With
Assura, contracts already express what the tests should check. When
SMT verification succeeds, no test is needed. When it times out
(Layer 2 budget exceeded), the compiler can automatically generate
tests that check the contract on concrete inputs, providing
practical assurance where formal proof is infeasible.

##### Grammar

```ebnf
TestGenAnnotation = '#[generate_tests' [TestGenConfig] ']' ;

TestGenConfig  = '(' { TestGenOption } ')' ;
TestGenOption  = 'strategy' '=' StrategyExpr
               | 'cases' '=' IntLit
               | 'fuzz_target' '=' BoolLit
               | 'shrink' '=' BoolLit
               | 'corpus' '=' StringLit
               | 'timeout_per_case' '=' DurationExpr ;

StrategyExpr   = 'random'
               | 'boundary'     // focus on boundary values
               | 'coverage'     // coverage-guided
               | 'mutation'     // mutation-based fuzzing
               | 'grammar'      // grammar-aware (for parsers)
               | CustomStrategy ;

CustomStrategy = Ident '(' [ExprList] ')' ;
```

##### Full Example: Generated Tests for B-Tree

```assura
// Contract on B-tree insert -- SMT may time out on the
// structural invariant (Layer 2, undecidable)
#[generate_tests(
  strategy = boundary,
  cases = 10000,
  fuzz_target = true,
  shrink = true
)]
structural_invariant BTreeValid<K, V, Level: Nat> {
  // All 6 invariants from Section TYPE.2
  // ...
}

// The compiler generates:
// 1. A proptest harness checking all 6 invariants
// 2. A libfuzzer/cargo-fuzz target for continuous fuzzing
// 3. Boundary value generators (empty tree, single element,
//    ORDER-1 elements, ORDER elements, ORDER+1 elements)

// Custom strategy for SQLite-specific patterns
#[generate_tests(
  strategy = grammar(sql_grammar),
  cases = 100000,
  fuzz_target = true,
  corpus = "tests/fuzz_corpus/"
)]
fn execute_select(
    db: Database,
    stmt: PreparedStatement,
    params: List<Value>
) -> List<Row>
  requires { stmt.is_valid() }
  ensures { ... }
```

##### What Gets Generated

For each contract with `#[generate_tests]`, the compiler produces:

**1. Property-Based Test (proptest/quickcheck)**

```rust
// AUTO-GENERATED from BTreeValid structural_invariant
#[cfg(test)]
mod generated_tests {
    use proptest::prelude::*;

    // Strategy: generate valid B-trees, then apply operations
    fn arb_btree(max_size: usize) -> impl Strategy<Value = BTree<i64, Vec<u8>>> {
        prop::collection::vec(
            (any::<i64>(), prop::collection::vec(any::<u8>(), 0..100)),
            0..max_size,
        )
        .prop_map(|entries| {
            let mut tree = BTree::new();
            for (k, v) in entries {
                let _ = tree.insert(k, v);
            }
            tree
        })
    }

    proptest! {
        #[test]
        fn btree_insert_preserves_invariant(
            tree in arb_btree(1000),
            key: i64,
            value in prop::collection::vec(any::<u8>(), 0..100),
        ) {
            let mut tree = tree;
            let _ = tree.insert(key, value);

            // Invariant 1: Keys sorted within each node
            assert!(tree.verify_sorted_keys());

            // Invariant 2: Subtree ordering
            assert!(tree.verify_subtree_ordering());

            // Invariant 3: All leaves at same depth
            assert!(tree.verify_balanced());

            // Invariant 4: Minimum occupancy
            assert!(tree.verify_min_occupancy());

            // Invariant 5: Key count == value count
            assert!(tree.verify_key_value_count());

            // Invariant 6: Child count == key count + 1
            assert!(tree.verify_child_count());
        }

        #[test]
        fn btree_delete_preserves_invariant(
            tree in arb_btree(1000),
            key: i64,
        ) {
            let mut tree = tree;
            let _ = tree.delete(&key);
            assert!(tree.verify_all_invariants());
        }

        // Boundary cases
        #[test]
        fn btree_boundary_order_minus_1(
            entries in prop::collection::vec(any::<i64>(), ORDER - 1),
        ) {
            let mut tree = BTree::new();
            for k in entries { let _ = tree.insert(k, vec![]); }
            assert!(tree.verify_all_invariants());
        }

        #[test]
        fn btree_boundary_exact_order(
            entries in prop::collection::vec(any::<i64>(), ORDER),
        ) {
            let mut tree = BTree::new();
            for k in entries { let _ = tree.insert(k, vec![]); }
            // This forces the first split
            assert!(tree.verify_all_invariants());
        }
    }
}
```

**2. Fuzz Target (cargo-fuzz / libfuzzer)**

```rust
// AUTO-GENERATED fuzz target from BTreeValid contracts
// File: fuzz/fuzz_targets/btree_invariant.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::{Arbitrary, Unstructured};

#[derive(Arbitrary, Debug)]
enum BTreeOp {
    Insert { key: i64, value: Vec<u8> },
    Delete { key: i64 },
    Search { key: i64 },
}

fuzz_target!(|ops: Vec<BTreeOp>| {
    let mut tree = BTree::new();
    for op in ops {
        match op {
            BTreeOp::Insert { key, value } => {
                let _ = tree.insert(key, value);
            }
            BTreeOp::Delete { key } => {
                let _ = tree.delete(&key);
            }
            BTreeOp::Search { key } => {
                let _ = tree.search(&key);
            }
        }
        // Check ALL structural invariants after every operation
        assert!(tree.verify_all_invariants(),
            "Invariant violated after {:?}", op);
    }
});
```

**3. Boundary Value Tests**

```rust
// AUTO-GENERATED boundary tests from refinement types
#[cfg(test)]
mod boundary_tests {
    #[test]
    fn page_size_boundaries() {
        // From: where page_size in {512, 1024, ..., 65536}
        for &size in &[512, 1024, 2048, 4096, 8192, 16384, 32768, 65536] {
            let header = DatabaseHeader::with_page_size(size);
            assert!(header.validate().is_ok());
        }
        // Just outside valid values
        assert!(DatabaseHeader::with_page_size(511).validate().is_err());
        assert!(DatabaseHeader::with_page_size(513).validate().is_err());
        assert!(DatabaseHeader::with_page_size(0).validate().is_err());
        assert!(DatabaseHeader::with_page_size(65537).validate().is_err());
    }

    #[test]
    fn fixed_width_narrowing_boundaries() {
        // From: U64 -> U32 narrowing contract
        assert!(narrow_u64_to_u32(0).is_ok());
        assert!(narrow_u64_to_u32(u32::MAX as u64).is_ok());
        assert!(narrow_u64_to_u32(u32::MAX as u64 + 1).is_err());
        assert!(narrow_u64_to_u32(u64::MAX).is_err());
    }
}
```

##### Verification Interaction

Test generation interacts with the verification layers:

| Verification Result | Test Generation Action |
|---|---|
| Layer 0 passes | No tests generated (structural check sufficient) |
| Layer 1 passes | No tests generated (SMT proved it) |
| Layer 2 passes | No tests generated (heavy SMT proved it) |
| Layer 2 times out | Generate proptest + fuzz target (practical assurance) |
| Layer 2 unknown | Generate proptest + fuzz target + warning |
| `#[generate_tests]` explicit | Always generate regardless of verification result |

##### CLI Integration

```
$ assura test --generate
Generated 47 property-based tests from 12 contracts
Generated 8 fuzz targets from 4 structural invariants
Generated 156 boundary tests from 23 refinement types

$ assura fuzz btree_invariant --time 3600
Running fuzz target: btree_invariant
Corpus: 234 entries, 1,247 executions/sec
No crashes found in 3600s

$ assura test --run-generated
Running 47 property tests (10000 cases each)...
All passed.
Running 156 boundary tests...
All passed.
```

##### Error Codes

Test generation does not add new error codes. It uses existing
verification warnings:

- **A22001** (verification timeout) triggers test generation
- **A22002** (unknown result) triggers test generation + warning
- Test failures at runtime produce standard Rust panics with
  the contract that was violated in the message


#### TEST.2 Behavioral Equivalence

Contracts that assert two implementations produce the same
observable output for all valid inputs. Used when porting from one
language/implementation to another.

##### Motivation

A Rust port of SQLite is worthless if it doesn't produce the exact
same results as the C version. "Same results" is precise: for any
database file and any SQL statement, the Rust version must produce
the same rows in the same order with the same types and the same
error codes. Behavioral equivalence contracts let you express this
at the module level and verify it via differential testing.

##### Grammar

```ebnf
EquivalenceDecl = 'equivalent' TypeIdent '~' TypeIdent '{'
                    { EquivalenceMapping }
                    { EquivalenceExclusion }
                  '}' ;

EquivalenceMapping = Ident '<->' Ident
                     [EquivalenceCondition] ;

EquivalenceCondition = 'when' Predicate ;

EquivalenceExclusion = 'except' Ident ':'  StringLit ;
```

##### Full Example: C SQLite vs Rust SQLite

```assura
// Declare behavioral equivalence between C and Rust implementations
equivalent CSqlite ~ RustSqlite {
  // Core API equivalence
  sqlite3_open <-> Connection::open
  sqlite3_close <-> Connection::close
  sqlite3_exec <-> Connection::exec
  sqlite3_prepare_v2 <-> Connection::prepare
  sqlite3_step <-> VdbeExecution::step
  sqlite3_finalize <-> VdbeExecution::finalize
  sqlite3_column_int <-> Row::get_int
  sqlite3_column_text <-> Row::get_text
  sqlite3_column_blob <-> Row::get_blob
  sqlite3_column_double <-> Row::get_double
  sqlite3_column_type <-> Row::column_type

  // For each mapping, the equivalence contract is:
  // forall valid_inputs:
  //   C_function(inputs) == Rust_function(inputs)
  // where == is defined on observable output (not internal state)

  // Error code equivalence
  sqlite3_errcode <-> Connection::error_code
    when error_code in {
      SQLITE_OK, SQLITE_ERROR, SQLITE_BUSY, SQLITE_LOCKED,
      SQLITE_NOMEM, SQLITE_READONLY, SQLITE_INTERRUPT,
      SQLITE_IOERR, SQLITE_CORRUPT, SQLITE_FULL,
      SQLITE_CANTOPEN, SQLITE_CONSTRAINT, SQLITE_MISMATCH,
      SQLITE_MISUSE, SQLITE_AUTH, SQLITE_RANGE, SQLITE_NOTADB
    }

  // Explicit exclusions (documented deviations)
  except sqlite3_randomness:
    "Rust uses a different CSPRNG; output differs but
     security properties are equivalent"

  except sqlite3_compileoption_get:
    "Compile options differ between C and Rust builds"

  except sqlite3_sourceid:
    "Source identification differs by definition"
}

// Per-function equivalence with specific invariants
fn verify_select_equivalence(
    db_bytes: Region<n>,  // raw database file
    sql: String
) -> Bool
  #[equivalence_test]
{
  let c_result = c_sqlite.open(db_bytes).exec(sql)
  let r_result = rust_sqlite.open(db_bytes).exec(sql)

  // Same number of rows
  len(c_result.rows) == len(r_result.rows)
  // Same values in same order
  and forall i in 0..len(c_result.rows):
    forall j in 0..len(c_result.rows[i].columns):
      c_result.rows[i].columns[j] == r_result.rows[i].columns[j]
  // Same error code if error
  and c_result.error_code == r_result.error_code
}

// Equivalence for file format (round-trip)
fn verify_format_equivalence(
    ops: List<SqlOperation>
) -> Bool
  #[equivalence_test]
{
  // Apply same operations to both implementations
  let c_db = apply_operations(c_sqlite, ops)
  let r_db = apply_operations(rust_sqlite, ops)

  // Database files must be byte-identical
  // (not just semantically equivalent)
  c_db.to_bytes() == r_db.to_bytes()
}
```

##### Verification Approach

Behavioral equivalence cannot be proved by SMT (it requires running
both implementations). Instead, the compiler generates:

1. **Differential test harness**: Runs both implementations on the
   same inputs and compares outputs
2. **Differential fuzz target**: Fuzzes both implementations
   simultaneously, flagging any divergence
3. **Golden file tests**: Extracts expected outputs from the
   reference implementation's test suite
4. **Migration tests**: Verifies database files created by C SQLite
   can be read by Rust SQLite and vice versa

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A41001 | Output divergence detected | Implementations produce different results |
| A41002 | Error code mismatch | Different error for same invalid input |
| A41003 | Row ordering difference | Same rows but different order |
| A41004 | Type coercion difference | Same value, different SQLite type affinity |
| A41005 | Undocumented exclusion | Divergence found that isn't in `except` list |

##### Rust Codegen

Equivalence declarations generate differential test infrastructure:

```rust
#[cfg(test)]
mod equivalence_tests {
    use libsqlite3_sys as csqlite;  // C SQLite bindings
    use crate as rsqlite;           // Rust implementation

    /// Differential fuzzing target
    #[cfg(feature = "fuzz")]
    pub fn differential_fuzz(data: &[u8]) {
        let (db_bytes, sql) = match split_fuzz_input(data) {
            Some(v) => v,
            None => return,
        };

        let c_result = run_c_sqlite(db_bytes, sql);
        let r_result = run_rust_sqlite(db_bytes, sql);

        assert_eq!(
            c_result.error_code, r_result.error_code,
            "Error code mismatch: C={}, Rust={}",
            c_result.error_code, r_result.error_code,
        );

        if c_result.error_code == SQLITE_OK {
            assert_eq!(
                c_result.rows, r_result.rows,
                "Row divergence on SQL: {}",
                std::str::from_utf8(sql).unwrap_or("<binary>"),
            );
        }
    }

    /// Run C SQLite's test suite against Rust implementation
    #[test]
    fn c_test_suite_compatibility() {
        for test_case in load_sqlite_test_cases("tests/c_compat/") {
            let r_result = run_rust_sqlite(
                &test_case.db_bytes,
                &test_case.sql,
            );
            assert_eq!(r_result, test_case.expected_output,
                "Failed on test: {}", test_case.name);
        }
    }
}
```


#### TEST.3 Multi-Pass Refinement

Contracts for computations where each pass over the data refines
the output, converging toward a final result. Each pass must
improve (or at least not worsen) the output quality.

##### Motivation

MISC.1 (Incremental/Coroutine) handles producer/consumer
patterns where each `step()` produces independent output. But
some algorithms require multiple passes that REFINE the same
output:

- **JPEG progressive mode**: The first pass produces a coarse
  image (DC coefficients only). Each subsequent pass adds AC
  coefficient bands, refining detail. After all passes, the
  image matches the baseline decode.
- **Interlaced PNG**: Adam7 interlacing makes 7 passes. Each
  pass fills in more pixels. The partial image is viewable at
  every stage but incomplete until pass 7.
- **Iterative solvers**: Newton-Raphson, gradient descent, or
  any convergent algorithm where each iteration reduces error.

Without refinement contracts, there is no way to prove:
- Each pass produces a valid (if incomplete) result
- The quality monotonically improves (or at least does not degrade)
- After all passes, the output matches the non-progressive version
- A partial result (after N < total passes) is still usable

##### Grammar

```ebnf
RefinementDecl  = 'refinement' Ident '{'
                    'passes' ':' Expr ','
                    'state' ':' TypeExpr ','
                    'quality' ':' QualityMetric ','
                    { RefinementRule }
                  '}' ;

QualityMetric   = 'metric' FnIdent
                | 'monotonic_field' Ident ;

RefinementRule  = 'after_each' ':' Predicate
                | 'after_all' ':' Predicate
                | 'convergence' ':' Predicate ;
```

##### Full Example: JPEG Progressive Decode

```assura
// Refinement contract for JPEG progressive decoding
refinement JpegProgressive {
  passes: scan_count,  // determined by SOS markers in file
  state: PartialImage,
  quality: metric image_psnr,  // quality measured by PSNR

  // After each pass, the image is valid (no garbage pixels)
  after_each:
    for_all(x in 0..state.width, y in 0..state.height,
      state.pixel(x, y).is_valid()
    )

  // After all passes, output matches baseline decode
  after_all:
    for_all(x in 0..state.width, y in 0..state.height,
      state.pixel(x, y) == baseline_decode(input).pixel(x, y)
    )

  // Each pass does not decrease quality (PSNR is non-decreasing)
  convergence:
    image_psnr(state, reference) >= image_psnr(old(state), reference)
}

type PartialImage {
  pixels: Region<image_size>,
  width: U32,
  height: U32,
  passes_completed: U32,
  coefficients: [[I16; 64]],  // accumulated DCT coefficients

  invariant {
    passes_completed <= max_passes,
    pixels.len() == width as U64 * height as U64 * 3
  }
}

fn progressive_decode_pass(
    image: &mut PartialImage :refine,
    scan: &ScanHeader,
    data: &[U8] :tainted
) -> () | DecodeError
  #[refinement(JpegProgressive)]
  requires { image.passes_completed < max_passes }
  ensures {
    image.passes_completed == old(image.passes_completed) + 1
  }
{
  // Decode this scan's spectral selection (Ss..Se)
  // and successive approximation (Ah, Al)
  for mcu in 0..image.mcu_count() {
    decode_mcu_progressive(image, scan, data, mcu)?
  }

  // Update pixels from refined coefficients
  for block in 0..image.block_count() {
    idct_and_update(image, block)
  }

  image.passes_completed += 1
}

// Adam7 interlaced PNG: 7 fixed passes
refinement PngAdam7 {
  passes: 7,
  state: InterlacedImage,
  quality: monotonic_field pixels_filled,

  after_each:
    state.pixels_filled >= adam7_cumulative_pixels(
      state.current_pass
    )

  after_all:
    state.pixels_filled == state.width * state.height
}
```

##### Verification Rule

1. **Quality monotonicity**: The verifier checks that the quality
   metric is non-decreasing after each pass. For `metric`
   functions, this is checked by SMT. For `monotonic_field`,
   the monotonic state machinery (STOR.5) is reused
2. **Convergence**: The `after_all` predicate is checked as a
   postcondition of the final pass. The verifier confirms that
   when `passes_completed == max_passes`, the predicate holds
3. **Partial validity**: The `after_each` predicate must hold
   after every pass, not just the final one. The verifier treats
   it as a loop invariant across the pass sequence
4. **Pass count**: For fixed-pass refinements (PNG Adam7: 7),
   the verifier can unroll and check each pass. For dynamic
   pass counts (JPEG progressive: determined by file), the
   verifier uses induction on pass number

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A53001 | Quality decreased after pass `N` | Refinement pass worsened output |
| A53002 | After-each invariant violated at pass `N` | Partial result is invalid |
| A53003 | After-all predicate not satisfied | Final output does not match expected |
| A53004 | Pass count exceeds declared maximum | More passes than `passes` specifies |
| A53005 | Refinement state not initialized before first pass | Missing initialization |

##### Rust Codegen

Refinement contracts generate a pass-tracking wrapper with
quality assertions:

```rust
pub struct RefinementState<T> {
    inner: T,
    passes_completed: u32,
    max_passes: u32,
    #[cfg(debug_assertions)]
    last_quality: Option<f64>,
}

impl<T> RefinementState<T> {
    pub fn apply_pass<F>(&mut self, pass_fn: F) -> Result<(), DecodeError>
    where
        F: FnOnce(&mut T) -> Result<(), DecodeError>,
    {
        assert!(self.passes_completed < self.max_passes,
            "exceeded max passes");

        pass_fn(&mut self.inner)?;
        self.passes_completed += 1;

        #[cfg(debug_assertions)]
        {
            let quality = self.measure_quality();
            if let Some(prev) = self.last_quality {
                debug_assert!(quality >= prev,
                    "quality decreased: {prev} -> {quality}");
            }
            self.last_quality = Some(quality);
        }

        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.passes_completed == self.max_passes
    }
}
```


### 14.MISC: Specialized

#### MISC.1 Incremental and Coroutine Contracts

Contracts for functions that produce results incrementally (one
at a time), suspending between yields and resuming on the next
call.

##### Motivation

SQLite's `sqlite3_step()` is a coroutine: each call advances the
VDBE virtual machine until it produces a row (`SQLITE_ROW`) or
finishes (`SQLITE_DONE`). Between calls, the VM is suspended with
its full state preserved. The caller must follow the protocol:
call `step()` until `DONE`, then `finalize()`. Calling `step()`
after `DONE` is undefined. Calling `finalize()` before `DONE` is
allowed (abort). This is a producer/consumer protocol that typestate
alone cannot fully express because the number of yields is
data-dependent.

##### Grammar

```ebnf
IncrementalDecl = 'incremental' TypeIdent [TypeParams] '{'
                    YieldType
                    FinalType
                    { IncrementalInvariant }
                    { IncrementalTransition }
                  '}' ;

YieldType       = 'yields' ':' TypeExpr ;
FinalType       = 'completes' ':' TypeExpr ;

IncrementalTransition = 'on' IncrementalEvent ':'
                        IncrementalAction ;

IncrementalEvent  = 'step' | 'abort' | 'reset' | 'error' ;
IncrementalAction = '{' { OperationItem } '}' ;
```

##### Full Example: VDBE Step Protocol

```assura
incremental VdbeExecution<Row> {
  // What each step() produces
  yields: Row

  // What finalize() returns after all rows
  completes: StepResult  // Done | Error

  // States: Ready -> Stepping -> Done | Aborted
  states { Ready, Stepping, Done, Aborted, Error }

  // Valid transitions
  transition Ready -> Stepping via step
  transition Stepping -> Stepping via step  // more rows
  transition Stepping -> Done via step      // no more rows
  transition Stepping -> Aborted via abort
  transition Ready -> Aborted via abort
  transition Done -> Ready via reset
  transition Aborted -> Ready via reset
  transition Error -> Aborted via abort
  transition Aborted -> Aborted via abort  // idempotent

  on step {
    requires { self.state @ Ready or self.state @ Stepping }

    ensures {
      result is Yield(row) =>
        self.state @ Stepping
        and row satisfies stmt.column_contracts
    }

    ensures {
      result is Complete(StepResult.Done) =>
        self.state @ Done
    }

    ensures {
      result is Complete(StepResult.Error(e)) =>
        self.state @ Error
    }

    effects: database.read
  }

  on abort {
    // Can abort at any time (releases resources)
    requires {
      self.state @ Ready
      or self.state @ Stepping
      or self.state @ Error
      or self.state @ Aborted
    }

    ensures { self.state @ Aborted }

    // All held locks released
    ensures {
      forall lock in old(self.held_locks): lock.released
    }

    effects: database.read
  }

  on reset {
    // Re-execute from the beginning
    requires {
      self.state @ Done or self.state @ Aborted
    }

    ensures { self.state @ Ready }
    ensures { self.cursor_position == 0 }

    effects: pure
  }

  // Invariant: stepping must make progress or terminate
  invariant {
    self.state @ Stepping =>
      self.instruction_pointer > old(self.instruction_pointer)
      or self.state != old(self.state)
  }

  // Invariant: resource cleanup on terminal states
  invariant {
    (self.state @ Done or self.state @ Aborted) =>
      self.held_locks == empty
      and self.temp_tables == empty
  }
}

// Usage
fn query_all_rows(
    stmt: VdbeExecution<Row> :_1
) -> (VdbeExecution<Row> :_1, List<Row>)
  requires { stmt.state @ Ready }
  ensures  { fst(result).state @ Done or fst(result).state @ Error }
  effects: database.read
{
  let mut rows = List.empty()
  let mut s = stmt

  loop {
    match s.step() {
      Yield(row) => {
        rows = rows.append(row)
        // s is now @ Stepping, loop continues
      }
      Complete(StepResult.Done) => {
        // s is now @ Done
        return (s, rows)
      }
      Complete(StepResult.Error(e)) => {
        s = s.abort()
        return (s, rows)  // partial results
      }
    }
  }
}
```

##### Verification Rule

1. **Protocol compliance**: The compiler verifies that `step()` is
   only called in `Ready` or `Stepping` states (typestate check)
2. **Termination**: If the `invariant` clause includes a progress
   measure, the compiler checks that the coroutine cannot step
   forever without producing a result or transitioning to `Done`
3. **Resource cleanup**: The `on abort` contract ensures all
   resources are released regardless of when the abort happens
4. **Linear ownership**: The incremental value is linear, preventing
   two threads from stepping the same coroutine

##### Error Codes

| Code | Message | Cause |
|---|---|---|
| A40001 | Step called in invalid state `S` | Stepping after Done or Aborted |
| A40002 | Incremental value not finalized | Value dropped without reaching Done or Aborted |
| A40003 | Incremental progress not guaranteed | Step may loop without yielding or completing |
| A40004 | Resources not released on terminal state | Held locks or temp tables survive abort |

##### Rust Codegen

Incremental contracts generate Rust iterators or async streams:

```rust
pub enum StepResult<T> {
    Row(T),
    Done,
    Error(SqlError),
}

pub struct VdbeExecution<'conn> {
    vm: VdbeVm,
    conn: &'conn Connection,
    state: VdbeState,
}

impl<'conn> VdbeExecution<'conn> {
    pub fn step(&mut self) -> StepResult<Row> {
        debug_assert!(
            self.state == VdbeState::Ready
            || self.state == VdbeState::Stepping
        );
        match self.vm.execute_next() {
            VmResult::Row(row) => {
                self.state = VdbeState::Stepping;
                StepResult::Row(row)
            }
            VmResult::Done => {
                self.state = VdbeState::Done;
                self.release_locks();
                StepResult::Done
            }
            VmResult::Error(e) => {
                self.state = VdbeState::Error;
                StepResult::Error(e)
            }
        }
    }

    pub fn abort(&mut self) {
        self.release_locks();
        self.drop_temp_tables();
        self.state = VdbeState::Aborted;
    }

    pub fn reset(&mut self) {
        debug_assert!(
            self.state == VdbeState::Done
            || self.state == VdbeState::Aborted
        );
        self.vm.reset();
        self.state = VdbeState::Ready;
    }
}

// Also implements Iterator for convenience
impl<'conn> Iterator for VdbeExecution<'conn> {
    type Item = Result<Row, SqlError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.step() {
            StepResult::Row(row) => Some(Ok(row)),
            StepResult::Done => None,
            StepResult::Error(e) => Some(Err(e)),
        }
    }
}

// Drop ensures cleanup even if user forgets to abort
impl<'conn> Drop for VdbeExecution<'conn> {
    fn drop(&mut self) {
        if self.state != VdbeState::Done
            && self.state != VdbeState::Aborted
        {
            self.abort();
        }
    }
}
```


#### MISC.2 Scoped Invariant Suspension

Temporary suspension of an invariant within a function, with a
proof obligation that the invariant is restored before the function
returns.

##### Motivation

SQLite's B-tree `balance()` is the most complex function in the
codebase. It redistributes cells across sibling pages, temporarily
violating the B-tree ordering invariant. The invariant must be
restored before `balance()` returns.

##### Grammar

```ebnf
SuspendAnnotation = '#[suspend_invariant(' TypeIdent ')]' ;
RestoredClause    = 'ensures' '{' TypeIdent 'restored' '}' ;
```

##### Usage

```assura
fn balance(
    cursor: BtCursor :_1,
    parent_index: Nat
) -> BtCursor :_1
  #[suspend_invariant(BTreeValid)]
  ensures { BTreeValid restored }
  ensures { all_keys(old(cursor.tree)) == all_keys(cursor.tree) }
  effects: database.write
{
  // During this function:
  // - BTreeValid is NOT checked on intermediate states
  // - The cursor is linear (no other function can observe the tree)
  // - The compiler verifies BTreeValid holds at every return point

  let (left, middle, right) = split_siblings(cursor, parent_index)
  // BTreeValid is violated here: keys may be unbalanced

  let redistributed = redistribute_cells(left, middle, right)
  // BTreeValid is being restored: cells are moved between siblings

  merge_siblings(cursor, redistributed)
  // BTreeValid must hold here (compiler checks at return)
}
```

##### Verification Rule

When `#[suspend_invariant(I)]` is present:
1. The invariant `I` is NOT checked on intermediate states within
   the function body
2. The invariant `I` IS checked at every return point (including
   early returns and error paths)
3. The suspended variable must be linear (to prevent observation
   during suspension)
4. Calling any function that expects `I` to hold within the
   suspension scope is a compile error


## Appendix A: Grammar Statistics

| Category | Production Rules | Keywords |
|---|---|---|
| Lexical | 8 | 53 |
| Top-level | 5 | - |
| Services | 3 | - |
| Types | 12 | - |
| Contracts | 10 | - |
| Predicates | 8 | - |
| Extended layers (8-27) | 14 | - |
| Extern/Bind | 4 | - |
| Effects | 3 | - |
| CORE (verification infrastructure) | 16 | 22 |
| MEM (memory safety) | 14 | 10 |
| TYPE (types and contracts) | 8 | 6 |
| SEC (trust and security) | 16 | 18 |
| CONC (concurrency) | 20 | 28 |
| STOR (storage and durability) | 15 | 20 |
| FMT (data formats and parsing) | 18 | 22 |
| NUM (numerical and precision) | 6 | 8 |
| PLAT (platform and configuration) | 8 | 10 |
| PERF (performance) | 6 | 6 |
| TEST (testing and verification) | 8 | 8 |
| MISC (specialized) | 4 | 4 |
| **Total** | **195** | **199** |

Keywords by category:

- **CORE**: `ghost`, `lemma`, `apply`, `induction`, `cases`,
  `modifies`, `reads`, `axiom`, `define`, `property`,
  `trigger`, `auto_trigger`, `opaque`, `reveal`,
  `prophecy`, `resolve`, `liveness`, `eventually`,
  `leads_to`, `eventually_within`, `eventually_always`,
  `fair`
- **MEM**: `region`, `allocator`, `shared_memory`, `layout`,
  `atomic`, `atomic_load`, `circular_buffer`, `write_pos`,
  `valid_count`, `slide`
- **TYPE**: `interface`, `impl`, `structural_invariant`,
  `error_policy`, `must_propagate`, `must_not_mask`
- **SEC**: `ffi`, `export`, `caller_guarantees`,
  `callee_guarantees`, `error_convention`, `constant_time`,
  `secret`, `secure_erase`, `erase`, `spec`, `conforms`,
  `axiom_spec`, `algorithm`
- **CONC**: `callback`, `must_not_call`, `must_not_reenter`,
  `may_call`, `must_be`, `deterministic`, `lock_rank`,
  `lock_order`, `deadline`, `timeout`, `ordering`,
  `relaxed`, `acquire`, `release`, `acq_rel`, `seq_cst`,
  `fence`, `view`, `stale_view`, `merge`
- **STOR**: `recovery`, `durable_state`, `crash_point`,
  `recovers_to`, `cache`, `pinned`, `snapshot`, `monotonic`,
  `storage_model`, `on_crash_during`, `erase_value`, `prog_idempotent`
- **FMT**: `format`, `bit_format`, `bits`, `codec_registry`,
  `codec`, `magic`, `integrity`, `encoding_matches`,
  `protocol`, `rfc`, `conforms`, `deviation`, `accepts`, `rejects`
- **NUM**: `precision`, `max_ulp_error`, `max_abs_error`,
  `table`, `precompute`, `verify_against`
- **PLAT**: `platform`, `variant`, `cfg`, `feature`,
  `limit`, `on_exceed`
- **PERF**: `unsafe_escape`, `bounds`, `complexity`,
  `amortized_time`
- **TEST**: `generate_tests`, `equivalent`, `refinement`,
  `passes`, `quality`, `convergence`
- **MISC**: `incremental`, `yields`, `frozen`, `extensible`

## Appendix B: Type System Summary

| Feature | Source | SMT Required | Decidable |
|---|---|---|---|
| Refinement types | Liquid Haskell, Flux | Yes (QF_UFLIA) | Yes (QF) |
| Dependent types (restricted) | Idris 2 | Yes (LIA) | Yes |
| Linear/graded types | Granule, QTT | No (algorithmic) | Yes |
| Typestate | Plaid | No (DFA) | Yes |
| Effect rows | Koka | No (set ops) | Yes |
| Information flow | Jif, FlowCaml | Yes (QF_DT) | Yes |
| Totality | Idris 2, Agda | Partial (fuel) | Semidecidable |
| Memory regions | ATS, Flux | Yes (QF_UFLIA) | Yes |
| Fixed-width integers | Ada, SPARK | Yes (QF_BV/LIA) | Yes |
| Untrusted data taint | Jif, Ur/Web | Yes (QF_DT) | Yes |
| Shared memory protocols | Session types, TLA+ | BMC | Bounded |
| Allocator contracts | Separation logic | Yes (HORN) | Semidecidable |
| Interface contracts | Eiffel, SPARK | Inherited | Inherited |
| Structural invariants | Dafny, Lean | Yes (AUFLIA) | No |
| Integrity contracts | Vest | Yes (AUFLIA) | No |
| Invariant suspension | Iris, RustBelt | Yes (AUFLIA) | No |
| Binary format contracts | Kaitai Struct, Daedalus | Yes (QF_BV) | Yes |
| Crash recovery | TLA+, Coyote | BMC | Bounded |
| Platform abstraction | Rust cfg, C #ifdef | Inherited | Inherited |
| Callback re-entrancy | Cyclone, RustBelt | No (call graph) | Yes |
| Determinism | Ur/Web | No (taint analysis) | Yes |
| Transactional rollback | STM, Haskell | Yes (pre/post) | Yes |
| FFI boundary | Prusti, Creusot | Yes (QF_UFLIA) | Yes |
| Test generation | QuickCheck, Hypothesis | N/A (meta) | N/A |
| Feature flags | Rust cfg, C #ifdef | No (structural) | Yes |
| Resource limits | Ada constraints | Yes (QF_LIA) | Yes |
| Incremental/coroutine | Session types | No (DFA) | Yes |
| Behavioral equivalence | QuickChick, Csmith | N/A (testing) | N/A |
| Unsafe escape | Prusti, Verus | Yes (QF_UFLIA) | Yes |
| String encoding | Ur/Web, ATS | Yes (QF_BV) | Yes |
| Page cache | Linear Haskell, ATS | No (ref counting) | Yes |
| MVCC/snapshot isolation | TLA+, Alloy | BMC | Bounded |
| Complexity bounds | RAML, AARA | Partial (heuristic) | Semidecidable |
| Monotonic state | TLA+, Coyote | No (temporal logic) | Yes |
| Error propagation | Elm, Midori | No (taint analysis) | Yes |
| Bit-level format | Kaitai Struct, Nom | Yes (QF_BV) | Yes |
| Precomputed tables | Coq, Lean | No (exhaustive eval) | Yes |
| Numerical precision | Fluctuat, Gappa | Partial (interval arith) | Bounded |
| Codec registry | Serde, nom | No (structural) | Yes |
| Multi-pass refinement | TLA+, Dafny | Yes (induction) | Semidecidable |
| Ghost code | SPARK, Dafny, Verus | No (erased) | N/A |
| Lemmas / proof functions | Dafny, Verus, Lean | Yes (SMT) | Semidecidable |
| Frame conditions | Frama-C, SPARK, Dafny | No (structural) | Yes |
| Axiomatic definitions | Frama-C, Dafny, Why3 | Yes (axiom) | Assumed |
| Quantifier triggers | Verus, Dafny, Z3 | N/A (SMT hint) | N/A |
| Opaque functions | Dafny, Verus | No (contract only) | Yes |
| Prophecy variables | Iris, TaDA, Abadi-Lamport | Yes (existential) | Semidecidable |
| Liveness contracts | TLA+, SPIN, nuXmv | BMC + k-induction | Bounded/Semidecidable |
| Constant-time execution | ct-verif, FaCT | No (info flow) | Yes |
| Secure erasure | Zeroize, SecureZeroMemory | No (erasure check) | Yes |
| Lock ordering | SPARK, ThreadSanitizer | No (rank tracking) | Yes |
| Temporal deadlines | TLA+, UPPAAL | BMC | Bounded |
| Storage failure model | TLA+, CrashMonkey | BMC | Bounded |
| Protocol grammar conformance | Everparse, Hammer | Yes (grammar check) | Yes |
| Circular buffer contracts | ATS, Dafny | Yes (QF_UFLIA) | Yes |
| Crypto spec conformance | HACL\*, Fiat-Crypto, Jasmin | Yes (algebra) | Semidecidable |
| Weak memory ordering | GPS, RSL, Iris-Relaxed | Yes (view logic) | Decidable (QF_UFLIA) |

## Appendix C: Rust Codegen Summary

| Assura Construct | Rust Representation |
|---|---|
| Refined type | Newtype + debug_assert |
| Typestate | PhantomData + state modules |
| Effects | Capability traits as parameters |
| Linear types | Move semantics (ownership) |
| Info flow labels | Erased (compile-time only) |
| Dependent indices | Erased (compile-time only) |
| Contracts | debug_assert in debug mode |
| Measures | cfg(debug_assertions) functions |
| Extern | Trait definition |
| Bind | Wrapper with assertions |
| Memory regions | &[u8] / &mut [u8] with bounds checks |
| Fixed-width integers | u8/u16/u32/u64/i32/i64 with checked casts |
| Taint labels | Erased; Unverified&lt;T&gt;/Verified&lt;T&gt; wrappers |
| Shared memory | mmap + AtomicU32 with Ordering annotations |
| Allocator contracts | std::alloc::Allocator trait impl |
| Interface contracts | Rust trait definitions |
| Structural invariants | Recursive debug_assert functions |
| Integrity contracts | Unverified&lt;T&gt;/Verified&lt;T&gt; wrappers |
| Invariant suspension | Scoped unsafe block with restore assert |
| Binary format contracts | Zero-copy parser structs over &[u8] |
| Crash recovery | Recovery function + savepoint structs |
| Platform abstraction | cfg-gated modules + unified trait |
| Callback re-entrancy | Marker traits (NoReenter&lt;T&gt;) + Send bounds |
| Determinism | Lint attrs + BTreeMap enforcement |
| Transactional rollback | Savepoint/rollback closures |
| FFI boundary | extern "C" + safety wrappers + debug_assert |
| Test generation | proptest harness + libfuzzer target + boundary tests |
| Feature flags | Cargo features + cfg-gated modules + const bounds |
| Resource limits | Config struct with clamped setter + const defaults |
| Incremental/coroutine | Iterator impl + state enum + Drop cleanup |
| Behavioral equivalence | Differential test harness + fuzz target |
| Unsafe escape | unsafe block + debug_assert proofs + #[inline(always)] |
| String encoding | PhantomData&lt;E&gt; + Encoding trait + validated constructors |
| Page cache | PinnedPage RAII guard + AtomicU32 pin count |
| MVCC/snapshot isolation | ReadTransaction/WriteTransaction + &mut exclusion |
| Complexity bounds | Criterion benchmarks (empirical verification) |
| Monotonic state | MonotonicU32 wrapper + debug_assert on advance |
| Error propagation | #[must_use] + CriticalError drop bomb + MustUse wrappers |
| Bit-level format | BitReader wrapper with bounds-checked read_bits |
| Precomputed tables | const arrays + build.rs generation + test verification |
| Numerical precision | Test harness with reference comparison + ULP checks |
| Codec registry | Dispatch function with magic-byte pattern matching |
| Multi-pass refinement | RefinementState wrapper with quality tracking |
| Ghost code | Completely erased; debug_assert in debug mode |
| Lemmas / proof functions | Completely erased (proof-only) |
| Frame conditions | Erased; debug_assert field equality in debug mode |
| Axiomatic definitions | Erased; simple axioms become debug_assert checks |
| Quantifier triggers | Erased (SMT directives only) |
| Opaque functions | Normal Rust code (opacity is verification-only) |
| Prophecy variables | Completely erased (verification-only) |
| Liveness contracts | Erased; optional debug_assert monitors in debug mode |
| Constant-time execution | Normal code + core::hint::black_box for secrets |
| Secure erasure | Drop impl with volatile zero write + compiler fence |
| Lock ordering | Erased; debug_assert witness system in debug mode |
| Temporal deadlines | Timer registration + handler dispatch |
| Storage failure model | Fault-injection test harness |
| Protocol grammar conformance | Erased; deviation comments + optional runtime logging |
| Circular buffer contracts | Struct with modular indexing + slide method + debug_assert |
| Crypto spec conformance | debug_assert(result == reference) in debug; erased in release |
| Weak memory ordering | Rust atomic operations with exact ordering preserved; debug view checks |

## Appendix D: Error Code Summary (All Categories)

### Core Language Errors

| Range | Category | Count | Layer |
|---|---|---|---|
| A-LANG-001-005 | Syntax | 5 | 0 |
| A-LANG-006-010 | Name resolution | 5 | 0 |
| A-LANG-011-016 | Type mismatch | 6 | 0 |
| A-LANG-017-023 | Refinement violation | 7 | 1 |
| A-LANG-024-028 | Linearity | 5 | 0 |
| A-LANG-029-033 | Typestate | 5 | 0 |
| A-LANG-034-038 | Effect violation | 5 | 0 |
| A-LANG-039-043 | Information flow | 5 | 1 |
| A-LANG-044-047 | Totality | 4 | 0-2 |
| A-LANG-048 | Pattern exhaustiveness | 1 | 0 |
| A-LANG-049-052 | Business invariant | 4 | 1-2 |
| A-LANG-053-055 | Concurrency | 3 | 0 |
| A-LANG-056-059 | Numerical precision | 4 | 0-1 |
| A-LANG-060 | Temporal ordering | 1 | 0 |
| A-LANG-061 | Idempotency | 1 | 0 |
| A-LANG-062-064 | Privacy | 3 | 0-1 |
| A-LANG-065-067 | Schema evolution | 3 | 0 |
| A-LANG-068 | Crash safety | 1 | 1 |
| A-LANG-069 | Audit trail | 1 | 0 |
| A-LANG-070 | Serialization | 1 | 2 |
| A-LANG-071-073 | API evolution | 3 | 0 |
| A-LANG-074-076 | Complexity bounds | 3 | 2 |
| A-LANG-077 | Protocol violation | 1 | 1-2 |
| A-LANG-078 | Observability | 1 | 0 |
| A-LANG-079 | Regulatory compliance | 1 | 1-2 |
| A-LANG-080 | i18n completeness | 1 | 0 |
| A-LANG-081 | Module / import | 1 | 0 |

### Category Errors

| Range | Category | Count | Layer |
|---|---|---|---|
| A-CORE-xxx | Ghost code, lemmas, frames, axioms, triggers, opaque, prophecy, liveness | 39 | 0-3 |
| A-MEM-xxx | Regions, fixed-width, allocators, circular buffers | 12 | 0-1 |
| A-TYPE-xxx | Interface, structural invariants, error propagation | 14 | 0-1 |
| A-SEC-xxx | Taint, FFI, constant-time, secure erasure, crypto conformance | 20 | 0-2 |
| A-CONC-xxx | Shared memory, callback, determinism, lock ordering, deadlines, weak memory | 26 | 0-2 |
| A-STOR-xxx | Crash recovery, page cache, MVCC, rollback, monotonic, storage model | 25 | 0-2 |
| A-FMT-xxx | Binary format, bit-level, string encoding, codec, checksum, protocol | 27 | 0-1 |
| A-NUM-xxx | Numerical precision, precomputed tables | 10 | 0-2 |
| A-PLAT-xxx | Platform, feature flags, resource limits | 11 | 0-1 |
| A-PERF-xxx | Unsafe escape, complexity bounds | 9 | 1-2 |
| A-TEST-xxx | Test generation, behavioral equivalence, multi-pass | 10 | 0-2 |
| A-MISC-xxx | Incremental/coroutine, invariant suspension | 4 | 0 |
| **Total** | | **~278** | |

