# assura-mcp

`assura mcp` is the Model Context Protocol server for the Assura compiler. There is no separate `assura-mcp` binary.

Install the CLI, then start the server:

```bash
cargo install assura --locked
assura mcp
```

A bare terminal looks idle. MCP hosts must spawn this process. Example host config:

```json
{
  "mcpServers": {
    "assura": {
      "command": "assura",
      "args": ["mcp"]
    }
  }
}
```

Verification returns Verified, Counterexample, or Unknown. That is not a claim that the code is secure.

- [docs/AI-AGENTS.md](../../docs/AI-AGENTS.md)
- [docs/WHAT-WE-PROVE.md](../../docs/WHAT-WE-PROVE.md)

## License

MIT OR Apache-2.0.
