use rehearse::{operation, pipeline, Plan};

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}

#[pipeline]
fn bad() -> Plan<(), u32, ()> {
    let value = step!(seed(1))?;
    let output = step!(seed({ return unreachable!(); }))?;
    Ok(output)
}

fn main() {}
