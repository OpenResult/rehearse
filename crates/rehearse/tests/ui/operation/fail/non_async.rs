use rehearse::operation;

#[derive(Debug)]
struct Error;

#[operation(impact = read)]
fn bad() -> Result<(), Error> {
    Ok(())
}

fn main() {}

