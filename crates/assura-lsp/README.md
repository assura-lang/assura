# assura-lsp

Language Server Protocol implementation for
[Assura](https://github.com/assura-lang/assura).

The server reports parse, name, and type diagnostics, and it answers
go-to-definition, hover, completion, and document-symbol requests. It calls
the shared compiler pipeline. It does not re-implement those passes.

Depend on this crate to embed the server. The `assura lsp` command starts it
on stdin and stdout.

## Documentation

- [For AI agents](https://github.com/assura-lang/assura/blob/main/docs/AI-AGENTS.md)
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
