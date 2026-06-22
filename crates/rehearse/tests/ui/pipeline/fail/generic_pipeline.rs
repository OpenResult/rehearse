use rehearse::pipeline;

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[pipeline]
fn bad<T>(value: T) -> rehearse::Plan<Services, T, Error> {
    Ok(value)
}

fn main() {}
