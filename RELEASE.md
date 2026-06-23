# Release Runbook

This runbook is for publishing the workspace crates to crates.io.

## Prerequisites

- Confirm the crates.io account has a verified email address.
- Put the crates.io token in `.env.local` without committing it:

```bash
export CARGO_REGISTRY_TOKEN=cio_your_crates_io_token_here
```

- Use a clean checkout for the final publish:

```bash
git status --short
```

## Local Gates

Run the standard gates:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Run the local artifact-resolution smoke test:

```bash
rm -rf target/local-registry
scripts/publish-local.sh
```

Run the guarded publish workflow in safe dry-run mode:

```bash
cargo run -p rehearse --example deploy
```

## Publish

The deploy example publishes in dependency order:

1. `rehearse-macros`
2. wait until `rehearse-macros` is indexed
3. `rehearse`
4. wait until `rehearse` is indexed

Run it only when the token is present and the checkout is clean:

```bash
cargo run -p rehearse --example deploy -- --execute
```

If `rehearse-macros` publishes successfully and a later step fails, do not
rerun the full workflow unchanged. Verify `rehearse-macros` is indexed, then
publish only the remaining runtime crate or revise the workflow for a recovery
release.

## Verify Published Crates

Check crates.io and docs.rs:

```bash
curl -fsS -H 'User-Agent: rehearse-release-check (https://github.com/OpenResult/rehearse)' \
    https://crates.io/api/v1/crates/rehearse | python3 -m json.tool
curl -fsS -H 'User-Agent: rehearse-release-check (https://github.com/OpenResult/rehearse)' \
    https://crates.io/api/v1/crates/rehearse-macros | python3 -m json.tool
curl -fsS -o /dev/null -w '%{http_code}\n' https://docs.rs/rehearse/0.2.0/rehearse/
curl -fsS -o /dev/null -w '%{http_code}\n' https://docs.rs/rehearse-macros/0.2.0/rehearse_macros/
```

Compile a consumer from crates.io under `target/published-consumer`:

```bash
rm -rf target/published-consumer
mkdir -p target/published-consumer/src
cat > target/published-consumer/Cargo.toml <<'EOF'
[package]
name = "published-consumer"
version = "0.2.0"
edition = "2021"
publish = false

[dependencies]
rehearse = "0.2.0"

[workspace]
EOF
cat > target/published-consumer/src/lib.rs <<'EOF'
use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Context;

#[derive(Debug)]
struct Error;

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, Error> {
    Ok(value + 1)
}

#[pipeline]
fn build(value: u32) -> Plan<Context, u32, Error> {
    let output = rehearse::step!(add_one(value))?;
    Ok(output)
}

fn compile_smoke() {
    let _ = build(41);
}
EOF
CARGO_HOME=target/published-consumer/cargo-home \
    cargo check --manifest-path target/published-consumer/Cargo.toml
CARGO_HOME=target/published-consumer/cargo-home \
    cargo tree --manifest-path target/published-consumer/Cargo.toml
```

## Tag

After the publish and verification succeed, create and push the release tag if
it does not already exist:

```bash
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

## Follow-Up

The workspace intentionally does not declare `rust-version` yet. Choose and
verify an MSRV in a later phase before adding it to package metadata.
