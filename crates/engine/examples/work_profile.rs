//! A public-safe command/mesh work report; counts are not elapsed-time or latency measurements.
use serde_json::{Value, json};
use solidcraft_engine::{EngineError, Session};
use solidcraft_work::capture;

fn measured(session: &mut Session, label: &str, command: &str, params: Value) -> Result<Value, EngineError> {
    let (result, work) = capture(|| {
        let result = session.execute(command, &params)?;
        for body in &session.world_state().bodies {
            let _ = body.mesh();
        }
        Ok(result)
    });
    println!("{label}: complete={}", work.complete());
    for (counter, value) in work.iter().filter(|(_, value)| *value != 0) {
        println!("  {}: {value}", counter.name());
    }
    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::default();
    session.execute("component.create", &json!({"name":"Rotor","grounded":false}))?;
    measured(&mut session, "cold box and reference mesh", "solid.box", json!({"body_name":"Arm","length":12,"width":2,"height":2}))?;
    session.execute("component.activate", &json!({"component":"root"}))?;
    session
        .execute("joint.as_built", &json!({"type":"revolute","name":"Turn","a":0,"b":"Rotor","at":{"occurrence":0,"point":[0,0,0],"z":[0,0,1]}}))?;
    for body in &session.world_state().bodies {
        let _ = body.mesh();
    }
    measured(&mut session, "warm joint drive", "joint.drive", json!({"joint":"Turn","value":15}))?;
    measured(&mut session, "warm selection", "select.clear", json!({}))?;
    Ok(())
}
