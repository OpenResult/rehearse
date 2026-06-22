#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGISTRY_DIR="${LOCAL_REGISTRY_DIR:-"$ROOT/target/local-registry"}"
INDEX_DIR="$REGISTRY_DIR/index"
DOWNLOAD_DIR="$REGISTRY_DIR/dl"
STAGE_DIR="$REGISTRY_DIR/stage"
TARGET_DIR="$REGISTRY_DIR/target"
CONSUMER_DIR="$REGISTRY_DIR/consumer"
CARGO_HOME_DIR="$REGISTRY_DIR/cargo-home"
REGISTRY_NAME="rehearse-local"
REGISTRY_URL="file://$INDEX_DIR"
CRATES_IO_INDEX="https://github.com/rust-lang/crates.io-index"
VERSION="0.1.1"

log() {
    printf '[local-publish] %s\n' "$*"
}

crate_index_path() {
    local name="$1"
    local lower
    lower="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')"

    case "${#lower}" in
        1)
            printf '%s/1/%s' "$INDEX_DIR" "$lower"
            ;;
        2)
            printf '%s/2/%s' "$INDEX_DIR" "$lower"
            ;;
        3)
            printf '%s/3/%s/%s' "$INDEX_DIR" "${lower:0:1}" "$lower"
            ;;
        *)
            printf '%s/%s/%s/%s' "$INDEX_DIR" "${lower:0:2}" "${lower:2:2}" "$lower"
            ;;
    esac
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

copy_crate() {
    local name="$1"
    local source="$2"
    local dest="$DOWNLOAD_DIR/$name-$VERSION.crate"

    cp "$source" "$dest"
    sha256_file "$dest"
}

write_index_config() {
    cat > "$INDEX_DIR/config.json" <<EOF
{"dl":"file://$DOWNLOAD_DIR/{crate}-{version}.crate","api":""}
EOF
}

write_macros_index() {
    local checksum="$1"
    local path
    path="$(crate_index_path "rehearse-macros")"
    mkdir -p "$(dirname "$path")"
    cat > "$path" <<EOF
{"name":"rehearse-macros","vers":"$VERSION","deps":[{"name":"proc-macro-crate","req":"^3","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"$CRATES_IO_INDEX","package":"proc-macro-crate"},{"name":"proc-macro2","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"$CRATES_IO_INDEX","package":"proc-macro2"},{"name":"quote","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"$CRATES_IO_INDEX","package":"quote"},{"name":"syn","req":"^2","features":["full","visit"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"$CRATES_IO_INDEX","package":"syn"}],"cksum":"$checksum","features":{},"yanked":false,"links":null}
EOF
}

write_runtime_index() {
    local checksum="$1"
    local path
    path="$(crate_index_path "rehearse")"
    mkdir -p "$(dirname "$path")"
    cat > "$path" <<EOF
{"name":"rehearse","vers":"$VERSION","deps":[{"name":"rehearse-macros","req":"^$VERSION","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null,"package":"rehearse-macros"}],"cksum":"$checksum","features":{"default":["macros"],"macros":["dep:rehearse-macros"]},"yanked":false,"links":null}
EOF
}

commit_index() {
    (
        cd "$INDEX_DIR"
        git init -q
        git add .
        git -c user.name="rehearse local publish" \
            -c user.email="rehearse-local@example.invalid" \
            commit -qm "publish local rehearse $VERSION"
    )
}

stage_workspace() {
    mkdir -p "$STAGE_DIR"
    local excludes=(--exclude .git --exclude target)
    if [[ "$REGISTRY_DIR" == "$ROOT/"* ]]; then
        local relative_registry="${REGISTRY_DIR#"$ROOT"/}"
        excludes+=(--exclude "$relative_registry" --exclude "./$relative_registry")
    fi

    (
        cd "$ROOT"
        tar \
            "${excludes[@]}" \
            -cf - .
    ) | (
        cd "$STAGE_DIR"
        tar -xf -
    )

    sed -i.bak \
        's|rehearse-macros = { version = "0.1.1", path = "../rehearse-macros", optional = true }|rehearse-macros = { version = "0.1.1", registry = "rehearse-local", optional = true }|' \
        "$STAGE_DIR/crates/rehearse/Cargo.toml"
    rm "$STAGE_DIR/crates/rehearse/Cargo.toml.bak"
}

write_consumer() {
    mkdir -p "$CONSUMER_DIR/src" "$CONSUMER_DIR/.cargo"
    cat > "$CONSUMER_DIR/.cargo/config.toml" <<EOF
[registries.$REGISTRY_NAME]
index = "$REGISTRY_URL"
EOF

    cat > "$CONSUMER_DIR/Cargo.toml" <<EOF
[package]
name = "rehearse-local-consumer"
version = "0.1.1"
edition = "2021"
publish = false

[dependencies]
rehearse = { version = "$VERSION", registry = "$REGISTRY_NAME" }

[workspace]
EOF

    cat > "$CONSUMER_DIR/src/lib.rs" <<'EOF'
#![allow(dead_code)]

use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct Error;

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, Error> {
    Ok(value + 1)
}

#[pipeline]
fn build(value: u32) -> Plan<Services, u32, Error> {
    let output = rehearse::step!(add_one(value))?;
    Ok(output)
}

fn compile_smoke() {
    let _plan = build(41);
}
EOF
}

rm -rf "$REGISTRY_DIR"
mkdir -p "$INDEX_DIR" "$DOWNLOAD_DIR" "$TARGET_DIR" "$CARGO_HOME_DIR"

log "root: $ROOT"
log "registry: $REGISTRY_DIR"
log "git commit: $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || printf 'unknown')"
if ! git -C "$ROOT" diff --quiet --ignore-submodules -- || \
    ! git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
    log "git status: dirty"
else
    log "git status: clean"
fi

write_index_config

log "packaging rehearse-macros"
cargo package \
    --manifest-path "$ROOT/crates/rehearse-macros/Cargo.toml" \
    --allow-dirty \
    --target-dir "$TARGET_DIR/macros-target"
MACROS_CRATE="$TARGET_DIR/macros-target/package/rehearse-macros-$VERSION.crate"
MACROS_CHECKSUM="$(copy_crate "rehearse-macros" "$MACROS_CRATE")"
write_macros_index "$MACROS_CHECKSUM"
commit_index

stage_workspace

log "packaging staged rehearse against $REGISTRY_NAME"
(
    cd "$STAGE_DIR"
    CARGO_HOME="$CARGO_HOME_DIR" cargo package \
        --manifest-path "$STAGE_DIR/crates/rehearse/Cargo.toml" \
        --allow-dirty \
        --registry "$REGISTRY_NAME" \
        --config "registries.$REGISTRY_NAME.index='$REGISTRY_URL'" \
        --target-dir "$TARGET_DIR/runtime-target"
)
RUNTIME_CRATE="$TARGET_DIR/runtime-target/package/rehearse-$VERSION.crate"
RUNTIME_CHECKSUM="$(copy_crate "rehearse" "$RUNTIME_CRATE")"
write_runtime_index "$RUNTIME_CHECKSUM"
commit_index

write_consumer

log "checking consumer crate"
(
    cd "$CONSUMER_DIR"
    CARGO_HOME="$CARGO_HOME_DIR" cargo check
    TREE_OUTPUT="$(CARGO_HOME="$CARGO_HOME_DIR" cargo tree)"
    printf '%s\n' "$TREE_OUTPUT"
    grep -F 'rehearse v0.1.1 (registry `rehearse-local`)' <<< "$TREE_OUTPUT" >/dev/null
    grep -F 'rehearse-macros v0.1.1 (proc-macro) (registry `rehearse-local`)' <<< "$TREE_OUTPUT" >/dev/null
)

log "local registry contains:"
log "  $DOWNLOAD_DIR/rehearse-macros-$VERSION.crate"
log "  $DOWNLOAD_DIR/rehearse-$VERSION.crate"
log "consumer checked successfully using registry '$REGISTRY_NAME'"
