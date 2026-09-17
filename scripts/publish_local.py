#!/usr/bin/env python3
"""Build and resolve local Cargo packages without a registry server (Python 3.11+)."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

REGISTRY = "rehearse-local"
CRATES_IO = "https://github.com/rust-lang/crates.io-index"
MARKER = ".rehearse-local-registry"


def run(args, cwd, env=None, capture=False):
    return subprocess.run(args, cwd=cwd, env=env, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def prepare_directory(directory, root):
    """Only clean a generated child of a marked directory owned by this checkout."""
    root = root.resolve()
    directory = directory.expanduser().absolute()
    if ".." in directory.parts:
        raise ValueError("registry path must not contain '..'")
    if any(part.is_symlink() for part in [directory, *directory.parents]):
        raise ValueError("registry path must not traverse symlinks")
    directory = directory.resolve()
    home = Path.home().resolve()
    if directory in {root, *root.parents, home, *home.parents}:
        raise ValueError("refusing repository, home, root, or ancestor registry path")
    marker = directory / MARKER
    generated = directory / "generated"
    ownership = f"rehearse-local-registry-v1\n{root}\n"
    if directory.exists():
        if not directory.is_dir():
            raise ValueError("registry path must be a directory")
        if marker.is_symlink() or generated.is_symlink():
            raise ValueError("registry marker and generated directory must not be symlinks")
        if marker.exists():
            if not marker.is_file() or marker.read_text() != ownership:
                raise ValueError("registry directory belongs to another checkout or format")
        elif any(directory.iterdir()):
            raise ValueError("refusing nonempty registry directory without an ownership marker; choose a new LOCAL_REGISTRY_DIR")
    directory.mkdir(parents=True, exist_ok=True)
    if not marker.exists():
        with marker.open("x") as output:
            output.write(ownership)
    if generated.exists():
        shutil.rmtree(generated)
    generated.mkdir()
    return generated


def toml_value(value):
    """Write generated manifests using TOML inline values; source formatting stays intact."""
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(map(toml_value, value)) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{toml_value(k)} = {toml_value(v)}" for k, v in value.items()) + " }"
    raise ValueError(f"unsupported generated TOML value: {type(value).__name__}")


def write_toml(path, data):
    path.write_text("".join(f"{toml_value(k)} = {toml_value(v)}\n" for k, v in data.items()))


def index_entry(package, checksum, local_packages):
    dependencies = []
    for dependency in package["dependencies"]:
        name = dependency["name"]
        dependencies.append({
            "name": dependency["rename"] or name,
            "package": name if dependency["rename"] else None,
            "req": dependency["req"],
            "features": dependency["features"],
            "optional": dependency["optional"],
            "default_features": dependency["uses_default_features"],
            "target": dependency["target"],
            "kind": dependency["kind"] or "normal",
            "registry": None if name in local_packages else dependency["registry"] or CRATES_IO,
        })
    return {"name": package["name"], "vers": package["version"], "deps": dependencies,
            "cksum": checksum, "features": package["features"], "yanked": False,
            "links": package["links"], "rust_version": package["rust_version"]}


def index_path(name):
    name = name.lower()
    if len(name) < 3:
        return Path(str(len(name))) / name
    if len(name) == 3:
        return Path("3") / name[0] / name
    return Path(name[:2]) / name[2:4] / name


def add_package(package, archive, index, downloads, local_packages):
    destination = downloads / archive.name
    shutil.copyfile(archive, destination)
    entry = index_entry(package, hashlib.sha256(destination.read_bytes()).hexdigest(), local_packages)
    path = index / index_path(package["name"])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(entry) + "\n")
    run(["git", "add", "."], index)
    run(["git", "-c", "user.name=rehearse local publish", "-c",
         "user.email=rehearse-local@example.invalid", "commit", "-qm",
         f"publish {package['name']} {package['version']}"], index)


def stage_workspace(root, stage):
    stage.mkdir()
    for name in ["Cargo.toml", "Cargo.lock", "README.md", "LICENSE-APACHE"]:
        shutil.copyfile(root / name, stage / name)
    for name in ["rehearse", "rehearse-macros"]:
        shutil.copytree(root / "crates" / name, stage / "crates" / name,
                        ignore=shutil.ignore_patterns("target", ".git"))
    manifest = stage / "crates/rehearse/Cargo.toml"
    data = tomllib.loads(manifest.read_text())
    dependency = data["dependencies"]["rehearse-macros"]
    dependency.pop("path")
    dependency["registry"] = REGISTRY
    write_toml(manifest, data)


def check_consumers(base, index, version, env, toolchain):
    cargo = ["cargo"] + ([f"+{toolchain}"] if toolchain else [])
    configurations = [("default", True, []), ("manual", False, []),
                      ("serde", False, ["serde"]), ("all", True, ["serde"])]
    for name, macros, features in configurations:
        consumer = base / f"consumer-{name}"
        (consumer / "src").mkdir(parents=True)
        (consumer / ".cargo").mkdir()
        write_toml(consumer / ".cargo/config.toml", {"registries": {REGISTRY: {"index": index.as_uri()}}})
        write_toml(consumer / "Cargo.toml", {
            "package": {"name": f"rehearse-consumer-{name}", "version": "0.0.0", "edition": "2021", "publish": False},
            "workspace": {},
            "dependencies": {"rehearse": {"version": f"={version}", "registry": REGISTRY,
                                             "default-features": macros, "features": features},
                             **({"serde_json": "1"} if features else {})},
        })
        source = '''use rehearse::{Impact, Operation, OperationMetadata, Plan, PlanBuilder};
fn manual() -> Plan<(), u32, ()> {
    let mut builder = PlanBuilder::new("manual");
    let value = builder.add(Operation::sync(OperationMetadata::new("seed", Impact::Pure), (), |_, ()| Ok(42)));
    builder.try_finish(value).unwrap()
}
fn main() {
    let plan = manual();
    assert_eq!(plan.len(), 1);
'''
        if macros:
            source += '''    let macro_plan = pipeline_plan();
    assert_eq!(macro_plan.len(), 1);
'''
        if features:
            source += '''    let json = serde_json::to_string(&plan.describe()).unwrap();
    let restored: rehearse::PlanDescription = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.len(), 1);
'''
        source += "}\n"
        if macros:
            source += '''#[rehearse::operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}
#[rehearse::pipeline]
fn pipeline_plan() -> Plan<(), u32, ()> {
    let output = rehearse::step!(seed(42))?;
    Ok(output)
}
'''
        (consumer / "src/main.rs").write_text(source)
        run([*cargo, "run", "--quiet"], consumer, env)
        tree = run([*cargo, "tree"], consumer, env, capture=True)
        print(tree, end="")
        assert f"rehearse v{version} (registry `{REGISTRY}`)" in tree
        if macros:
            assert f"rehearse-macros v{version} (proc-macro) (registry `{REGISTRY}`)" in tree
        else:
            assert "rehearse-macros" not in tree


def main():
    root = Path(__file__).resolve().parent.parent
    base = prepare_directory(Path(os.environ.get("LOCAL_REGISTRY_DIR", root / "target/local-registry")), root)
    print(f"[local-publish] generated files: {base}", flush=True)
    metadata = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"], root, capture=True))
    packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    version = packages["rehearse"]["version"]
    if packages["rehearse-macros"]["version"] != version:
        raise ValueError("workspace crates must share a release version")
    index, downloads, stage = base / "index", base / "dl", base / "stage"
    for directory in [index, downloads, base / "cargo-home"]:
        directory.mkdir()
    (index / "config.json").write_text(json.dumps({"dl": downloads.as_uri() + "/{crate}-{version}.crate"}))
    run(["git", "init", "-q"], index)
    for name in ["rehearse-macros", "rehearse"]:
        work = root if name == "rehearse-macros" else stage
        if name == "rehearse":
            stage_workspace(root, stage)
        target = base / f"{name}-target"
        env = os.environ.copy()
        if name == "rehearse":
            env["CARGO_HOME"] = str(base / "cargo-home")
        args = ["cargo", "package", "--manifest-path", str(work / "crates" / name / "Cargo.toml"),
                "--allow-dirty", "--target-dir", str(target)]
        if name == "rehearse":
            args += ["--registry", REGISTRY, "--config", f"registries.{REGISTRY}.index={json.dumps(index.as_uri())}"]
        run(args, work, env)
        archive = target / "package" / f"{name}-{version}.crate"
        add_package(packages[name], archive, index, downloads, packages)
    env = os.environ.copy()
    env["CARGO_HOME"] = str(base / "cargo-home")
    env["CARGO_TARGET_DIR"] = str(base / "consumer-target")
    check_consumers(base, index, version, env, os.environ.get("REHEARSE_CONSUMER_TOOLCHAIN"))
    print(f"[local-publish] all four consumers checked successfully using registry '{REGISTRY}'")
    print(f"[local-publish] archives: {downloads}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"[local-publish] {error}")
