use rehearse::operation;

#[operation(impact = read)]
async fn bad() -> u32 {
    1
}

fn main() {}

