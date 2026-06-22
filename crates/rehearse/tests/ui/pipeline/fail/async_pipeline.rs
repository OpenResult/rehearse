use rehearse::pipeline;

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[pipeline]
async fn bad() -> rehearse::Plan<Services, u32, Error> {
    Ok(value)
}

fn main() {}
