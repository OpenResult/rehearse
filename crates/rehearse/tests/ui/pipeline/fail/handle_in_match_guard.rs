use rehearse::{operation, pipeline, Plan};

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}

#[pipeline]
fn bad() -> Plan<(), u32, ()> {
    let value = step!(seed(1))?;
    let _matched = match Some(3) {
        Some(other) if value > 0 => other,
        _ => 0,
    };
    Ok(value)
}

fn main() {}
