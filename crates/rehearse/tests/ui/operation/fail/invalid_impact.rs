use rehearse::operation;

#[derive(Debug)]
struct Error;

#[operation(impact = maybe)]
async fn bad() -> Result<(), Error> {
    Ok(())
}

fn main() {}

