use rehearse::{operation, PlanBuilder};

struct Context;
struct Box;

#[operation(impact = pure)]
async fn compute(#[context] context: &Context, __rehearse_context: u32) -> Result<u32, ()> {
    let _ = context;
    Ok(__rehearse_context + 1)
}

fn main() {
    let _ = Box;
    let mut builder = PlanBuilder::<Context, ()>::new("hygiene");
    let output = builder.add(compute(41));
    let _plan = builder.finish(output);
}
