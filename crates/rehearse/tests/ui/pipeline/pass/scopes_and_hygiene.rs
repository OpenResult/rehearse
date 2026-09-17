use rehearse::{operation, pipeline, Plan};

#[operation(impact = pure)]
async fn seed(value: u32) -> Result<u32, ()> {
    Ok(value)
}

#[pipeline]
fn build(__rehearse_builder: u32) -> Plan<(), u32, ()> {
    let value = step!(seed(__rehearse_builder))?;
    {
        let value = 2;
        let _ = value + 1;
    }
    fn ordinary(value: u32) -> u32 { value + 1 }
    let _ordinary = ordinary(3);
    let _closure = |value: u32| value + 1;
    for value in [1, 2] { let _ = value + 1; }
    let _matched = match Some(3) { Some(value) => value + 1, None => 0 };
    if let Some(value) = Some(3) { let _ = value + 1; }
    let value = step!(seed(value))?;
    step!(seed(value))?;
    let value = 8;
    let output = step!(seed(value + 1))?;
    Ok(output)
}

fn main() { let _ = build(1); }
