# rehearse-macros

Procedural macros for the `rehearse` operation planning crate.

Most users should depend on `rehearse` instead of this crate directly:

```toml
[dependencies]
rehearse = "0.2.0"
```

The runtime crate enables its default `macros` feature and re-exports:

- `#[operation]`
- `#[pipeline]`
- `step!`

Use this crate directly only if you intentionally disable default features on
`rehearse` and want to depend on the macro frontend separately.

## Documentation

- Runtime crate: <https://crates.io/crates/rehearse>
- Macro API docs: <https://docs.rs/rehearse-macros>
- Repository: <https://github.com/OpenResult/rehearse>
