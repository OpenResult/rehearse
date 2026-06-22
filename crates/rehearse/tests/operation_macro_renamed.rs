use std::process::Command;

#[test]
fn renamed_runtime_dependency_compiles() {
    let fixture_dir = std::env::temp_dir().join(format!(
        "rehearse-renamed-runtime-fixture-{}",
        std::process::id()
    ));
    if fixture_dir.exists() {
        std::fs::remove_dir_all(&fixture_dir).expect("failed to clean previous fixture dir");
    }
    std::fs::create_dir_all(fixture_dir.join("src")).expect("failed to create fixture dir");

    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml = format!(
        r#"[package]
name = "renamed-runtime-fixture"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
renamed_rehearse = {{ package = "rehearse", path = "{}" }}
"#,
        runtime_path.display()
    );
    std::fs::write(fixture_dir.join("Cargo.toml"), cargo_toml)
        .expect("failed to write fixture manifest");
    std::fs::write(
        fixture_dir.join("src").join("main.rs"),
        r#"use renamed_rehearse::{operation, PlanBuilder};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, Error> {
    Ok(value + 1)
}

fn main() {
    let mut builder = PlanBuilder::<Services, Error>::new("renamed");
    let output = builder.add(add_one(1_u32));
    let _plan = builder.finish(output);
}
"#,
    )
    .expect("failed to write fixture source");

    let status = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(fixture_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", fixture_dir.join("target"))
        .status()
        .expect("failed to run cargo check for renamed runtime fixture");

    assert!(status.success());
}
