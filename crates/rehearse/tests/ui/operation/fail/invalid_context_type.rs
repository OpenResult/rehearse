use rehearse::operation;

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct Error;

#[operation(impact = read)]
async fn bad(#[context] services: Services) -> Result<(), Error> {
    let _ = services;
    Ok(())
}

fn main() {}

