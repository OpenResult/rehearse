use rehearse::operation;

#[derive(Debug)]
struct Error;

#[operation(impact = read)]
async fn bad(value: &str) -> Result<(), Error> {
    let _ = value;
    Ok(())
}

fn main() {}

