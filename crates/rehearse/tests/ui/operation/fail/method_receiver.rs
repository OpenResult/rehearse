use rehearse::operation;

#[derive(Debug)]
struct Error;

struct Services;

impl Services {
    #[operation(impact = read)]
    async fn bad(&self) -> Result<(), Error> {
        Ok(())
    }
}

fn main() {}

