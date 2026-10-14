# assura-mcp

MCP server for [Assura](https://github.com/assura-lang/assura).

The registered tools are `assura_check`, `assura_infer`, `assura_explain`,
`assura_type_map`, `assura_ir_prompt`, and `assura_ir_verify`. Tool results
are JSON text.

Depend on this crate to embed the server. The `assura mcp` command runs it
on stdio. Check uses the default Z3 solver.

## Documentation

- [For AI agents](https://github.com/assura-lang/assura/blob/main/docs/AI-AGENTS.md)

## License

MIT OR Apache-2.0.
