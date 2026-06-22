use rehearse::{operation, Input, PlanBuilder};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[operation(impact = pure)]
async fn seed(value: String) -> Result<String, Error> {
    Ok(value)
}

#[operation(impact = pure)]
async fn join(left: String, middle: String, right: String) -> Result<String, Error> {
    Ok(format!("{left}:{middle}:{right}"))
}

fn main() {
    let mut builder = PlanBuilder::<Services, Error>::new("inputs");
    let middle = builder.add(seed("value".to_owned()));
    let output = builder.add(join(
        "literal".to_owned(),
        middle,
        Input::literal("input".to_owned()),
    ));
    let _plan = builder.finish(output);
}

