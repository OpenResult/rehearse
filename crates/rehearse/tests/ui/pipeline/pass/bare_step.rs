use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug, Clone)]
struct Error;

#[derive(Clone)]
struct Session;

#[derive(Clone)]
struct Deployment;

#[operation(impact = session)]
async fn login() -> Result<Session, Error> {
    Ok(Session)
}

#[operation(impact = write)]
async fn apply(session: Session) -> Result<Deployment, Error> {
    let _ = session;
    Ok(Deployment)
}

#[operation(impact = read)]
async fn verify(deployment: Deployment) -> Result<(), Error> {
    let _ = deployment;
    Ok(())
}

#[pipeline]
fn deploy() -> Plan<Services, Deployment, Error> {
    let session = step!(login())?;
    let deployment = step!(apply(session))?;
    step!(verify(deployment))?;
    Ok(deployment)
}

fn main() {
    let _plan = deploy();
}
