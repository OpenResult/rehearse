use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[derive(Clone)]
struct Session(String);

#[derive(Clone)]
struct Deployment(String);

#[operation(impact = session)]
async fn login(#[context] services: &Services, credentials: String) -> Result<Session, Error> {
    let _ = services;
    Ok(Session(credentials))
}

#[operation(impact = write)]
async fn apply(#[context] services: &Services, session: Session) -> Result<Deployment, Error> {
    let _ = services;
    Ok(Deployment(session.0))
}

#[pipeline]
fn deploy(credentials: String) -> Plan<Services, Deployment, Error> {
    let session = step!(login(credentials))?;
    let deployment = step!(apply(session))?;
    Ok(deployment)
}

fn main() {
    let _plan = deploy("secret".to_owned());
}
