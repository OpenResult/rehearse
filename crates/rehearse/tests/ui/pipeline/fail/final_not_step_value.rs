use rehearse::{operation, pipeline};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn seed() -> Result<u32, Error> {
    Ok(1)
}

#[pipeline]
fn bad() -> rehearse::Plan<Services, u32, Error> {
    let value = 1_u32;
    step!(seed())?;
    Ok(value)
}

fn main() {}
