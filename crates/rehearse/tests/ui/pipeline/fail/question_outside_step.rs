use rehearse::{operation, pipeline};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, Error> {
    Ok(value)
}

fn maybe() -> Result<u32, Error> {
    Ok(1)
}

#[pipeline]
fn bad() -> rehearse::Plan<Services, u32, Error> {
    let value = maybe()?;
    let output = step!(seed(value))?;
    Ok(output)
}

fn main() {}
