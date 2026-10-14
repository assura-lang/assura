# assura-llm

LLM providers used by [Assura](https://github.com/assura-lang/assura) for
contract suggestions and auto-implement.

`LlmProvider` is the plugin trait. `HttpProvider` talks to an
OpenAI-compatible or Anthropic-compatible API. `MockProvider` is for tests.
Responses can be cached by content hash.

Depend on this crate when you supply a model to an Assura tool. Checking and
proving a contract does not require it. Use
[assura-pipeline](https://crates.io/crates/assura-pipeline) for that.

## Documentation

- [For AI agents](https://github.com/assura-lang/assura/blob/main/docs/AI-AGENTS.md)
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
