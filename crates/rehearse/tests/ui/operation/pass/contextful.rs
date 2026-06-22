use rehearse::{operation, PlanBuilder};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[derive(Clone)]
struct Session(String);

#[operation(impact = session)]
async fn login(
    #[context] services: &Services,
    username: String,
) -> Result<Session, Error> {
    let _ = services;
    Ok(Session(username))
}

fn main() {
    let mut builder = PlanBuilder::<Services, Error>::new("contextful");
    let session = builder.add(login("alice".to_owned()));
    let _plan = builder.finish(session);
}

