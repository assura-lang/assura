# Launch post pack

Drafts for public announcements. **Do not post until** demo GIF, compare
page, and what-we-prove page are live (they are on main as of PR #1406).

**Do not claim:** uncited percentages; affiliation with assura.dev;
"proves all security bugs"; every clause always Verified.

**Canonical links:** [URLS.md](../URLS.md)

## Show HN (draft)

**Title:** Show HN: Assura – contracts for AI-written code, proved with Z3, emits Rust

**Body sketch:**

```
Assura is a contract-first language: you write what the program must do;
SMT (Z3/CVC5) checks it; the toolchain generates Rust.

Motivation: AI writes a lot of code; unit tests often mirror the same bugs.
We want structured Verified / Counterexample / Unknown results agents can loop on.

Demo GIF: https://github.com/assura-lang/assura/blob/main/assets/demo/assura-check.gif
Docs: https://assura-lang.github.io/assura/
Install: cargo install assura --locked
  (or shell installer from GitHub Releases)

What we prove (honest limits): …/WHAT-WE-PROVE.html
Compared to Dafny/Verus: …/COMPARE.html

Happy to answer questions about the SMT surface, AI IR loop, and what's still Unknown.
```

Stay in comments for 24h after posting.

## Lobsters (draft)

**Title:** Assura: contract-first language with Z3/CVC5 verification and Rust emit

**Tags:** `plt` `formalmethods` `rust` `ai` (adjust to site norms)

**Tone:** technical, link WHAT-WE-PROVE and COMPARE early; no marketing stats.

## r/rust (draft)

Lead with: ships as Rust; `cargo install assura`; optional Verus comparison
("we are not verifying arbitrary existing Rust-in-place; contracts are a
separate surface + AI implement loop").

## r/ProgrammingLanguages (draft)

Lead with design: contract language vs host, layer 0–2, AI acceptance policy
for Unknown, link SPEC / COMPARE.

## This Week in Rust (draft)

```
Assura 0.4.x – contract-first verification language that emits Rust.
cargo install assura --locked
https://github.com/assura-lang/assura
https://assura-lang.github.io/assura/
```

## Product Hunt / Dev Hunt

Only after: polished gallery (GIF), maker bio, docs URL not confused with assura.dev.
