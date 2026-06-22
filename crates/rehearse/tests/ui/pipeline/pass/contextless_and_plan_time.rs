use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, Error> {
    Ok(value + 1)
}

#[operation(impact = pure)]
async fn double(value: u32) -> Result<u32, Error> {
    Ok(value * 2)
}

#[pipeline]
fn calculate(input: u32, bump: bool) -> Plan<Services, u32, Error> {
    let base = input + 1;
    if bump {
        let _ = base + 1;
    }

    let first = step!(add_one(base))?;
    let second = step!(double(first))?;
    Ok(second)
}

fn main() {
    let _plan = calculate(1, true);
}
