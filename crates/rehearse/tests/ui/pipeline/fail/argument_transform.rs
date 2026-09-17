use rehearse::{operation, pipeline, Plan};

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}

#[pipeline]
fn bad() -> Plan<(), u32, ()> {
    let value = step!(seed(1))?;
    let output = step!(seed(value.node().index() as u32))?;
    Ok(output)
}

fn main() {}
