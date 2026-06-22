use rehearse::operation;

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct Error;

#[operation(impact = read)]
async fn bad(
    #[context] first: &Services,
    #[context] second: &Services,
) -> Result<(), Error> {
    let _ = (first, second);
    Ok(())
}

fn main() {}

