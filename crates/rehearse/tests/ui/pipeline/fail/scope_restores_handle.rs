use rehearse::{operation, pipeline, Plan};

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}

#[pipeline]
fn bad() -> Plan<(), u32, ()> {
    let value = step!(seed(1))?;
    { let value = 2; let _ = value + 1; }
    let _alias = value;
    Ok(value)
}

fn main() {}
