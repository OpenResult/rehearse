use rehearse::{operation, PlanBuilder};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, Error> {
    Ok(value + 1)
}

fn main() {
    let mut builder = PlanBuilder::<Services, Error>::new("contextless");
    let output = builder.add(add_one(41_u32));
    let _plan = builder.finish(output);
}

