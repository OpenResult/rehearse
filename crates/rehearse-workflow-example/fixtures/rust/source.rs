// Illustrative Rust service source. The workflow lab never compiles this file.
fn health() -> &'static str {
    "UP"
}

fn main() {
    println!("{}", health());
}

#[test]
fn reports_healthy() {
    assert_eq!(health(), "UP");
}
