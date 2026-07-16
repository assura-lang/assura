# Case studies

Short public write-ups of demos you can run today. Full sources live under
[`demos/`](../demos/). Showcase (must-pass) vs EXPECT FAIL taxonomy:
[demos/README.md](../demos/README.md).

## Heartbleed-class bounds (CVE-2014-0160)

**Story:** OpenSSL Heartbleed read attacker-controlled `payload_length`
without ensuring the record actually contained that many bytes.

**Demo:** [`demos/heartbleed.assura`](../demos/heartbleed.assura)

**Run:**

```bash
assura check demos/heartbleed.assura
```

**What the contract encodes:** preconditions on record layout and padding
so response size cannot exceed the buffer (bounds safety as SMT-checkable
ensures on inputs).

**What is proved:** when the contract is in the supported fragment and
check reports Verified for those clauses, the modeled over-read is
impossible under the stated requires. See [What we prove](WHAT-WE-PROVE.md).

## libwebp Huffman / heap overflow class (CVE-2023-4863)

**Story:** a CVSS 9.8 heap buffer overflow in libwebp (WebP) affected
browsers and Electron apps.

**Demo:** [`demos/libwebp-huffman.assura`](../demos/libwebp-huffman.assura)

**Run:**

```bash
assura check demos/libwebp-huffman.assura
# optional:
assura check demos/libwebp-huffman.assura --verbose --stats
```

**What the contract encodes:** memory region, taint, table, and related
feature surface aimed at the class of bug (see file header comments).

**What is proved:** structural + SMT obligations that fire for the
features actually modeled on that file. Treat Unknown limitation markers
as incomplete encoding, not green proof.

## Showcase identity (happy path)

**Story:** smallest result-bearing path for onboarding.

**Demo:** [`demos/showcase-echo.assura`](../demos/showcase-echo.assura)
(or the copy-paste file in [GETTING-STARTED.md](GETTING-STARTED.md))

**Run:**

```bash
assura check demos/showcase-echo.assura
```

`assura check` synthesizes analyzable IR for simple `result == x` shapes
so you can see **Verified** without hand-written IR.

## Next

- More patterns: [Cookbook](COOKBOOK.md)
- AI implement loop: [GETTING-STARTED.md](GETTING-STARTED.md), design note
  [DESIGN-AI-VERIFICATION-LOOP.md](DESIGN-AI-VERIFICATION-LOOP.md)
