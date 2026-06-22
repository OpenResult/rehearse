use rehearse::{operation, pipeline};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn seed() -> Result<bool, Error> {
    Ok(true)
}

#[operation(impact = pure)]
async fn done() -> Result<u32, Error> {
    Ok(1)
}

#[pipeline]
fn bad() -> rehearse::Plan<Services, u32, Error> {
    let value = step!(seed())?;
    if value {
        let _ = 1;
    }
    let output = step!(done())?;
    Ok(output)
}

fn main() {}
