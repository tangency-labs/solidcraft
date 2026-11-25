use super::{CreateRouteParams, EditRouteParams, MoveRoutePointParams, RouteListParams, RouteNameParams};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CreateRouteParams>(),
        &["colours", "elbow", "fittings", "frame", "name", "nps", "points", "schedule", "segments", "start", "valves"],
    );
    assert_keys(
        input_schema_for::<EditRouteParams>(),
        &["colours", "elbow", "fittings", "frame", "name", "nps", "points", "route", "schedule", "segments", "start", "valves"],
    );
    assert_keys(input_schema_for::<MoveRoutePointParams>(), &["at", "carry", "delta", "route", "to"]);
    assert_keys(input_schema_for::<RouteNameParams>(), &["route"]);
    assert_keys(input_schema_for::<RouteListParams>(), &[]);
}
