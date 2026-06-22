use rehearse::operation;

#[derive(Debug)]
struct Error;

#[operation(impact = pure)]
async fn bad<T>(value: T) -> Result<T, Error> {
    Ok(value)
}

fn main() {}

