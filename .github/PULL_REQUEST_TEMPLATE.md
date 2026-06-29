## Summary

<!-- What does this PR do? Why is it needed? -->

## Changes

<!-- List the key changes. -->

## Testing

<!-- How was this tested? Include commands you ran. -->

```bash
cargo test -p <crate> --locked --lib
cargo clippy --workspace --locked -- -D warnings
```

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --locked -- -D warnings` passes
- [ ] Tests added or updated
- [ ] Demo files still parse (`cargo run --bin assura -- check demos/libwebp-huffman.assura`)
