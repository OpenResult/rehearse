# rehearse

Build typed operation plans in Rust, then inspect, rehearse, or execute them.

`rehearse` is an effect-aware operation planning library. Users explicitly
declare operation impact, compose operations into an ordered plan, and then pick
an interpretation:

- describe the static plan without invoking operation bodies;
- dry-run safe work while skipping or denying mutations;
- execute the full plan with fail-fast semantics.

The library records declared impact. It does not infer or prove whether
arbitrary Rust code mutates state.

## Install

```toml
[dependencies]
rehearse = "0.2.0"
```

The `macros` feature is enabled by default and re-exports the `#[operation]`,
`#[pipeline]`, and `step!` macros from `rehearse-macros`.

Enable structured serialization for descriptions, reports, and public status
types with:

```toml
[dependencies]
rehearse = { version = "0.2.0", features = ["serde"] }
```

## Quickstart

```rust
use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation failed")
    }
}

impl std::error::Error for Error {}

#[operation(impact = read)]
async fn read_version(#[context] _services: &Services) -> Result<String, Error> {
    Ok("current".to_owned())
}

#[operation(impact = write)]
async fn deploy(version: String) -> Result<String, Error> {
    Ok(format!("deployed {version}"))
}

#[pipeline]
fn release(version: String) -> Plan<Services, String, Error> {
    let _current = rehearse::step!(read_version())?;
    let result = rehearse::step!(deploy(version))?;
    Ok(result)
}
```

Calling `release(...)` builds the plan only. Operation bodies run only through
`dry_run` or `execute`, and the default safe dry-run policy skips writes and
deletes.

## Documentation

- API docs: <https://docs.rs/rehearse>
- Repository: <https://github.com/OpenResult/rehearse>
