use serde_json::json;

use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Thing(u64);

impl Record for Thing {
    fn id(&self) -> u64 {
        self.0
    }
}

fn things(ids: &[u64]) -> ById<Thing> {
    let mut list = ById::default();
    list.add(ids.iter().map(|id| Arc::new(Thing(*id))));
    list
}

#[test]
fn records_go_where_their_ids_put_them_and_are_found_there() {
    let mut list = things(&[30, 10]);
    list.add([Arc::new(Thing(20))]);
    assert_eq!(list.iter().map(|t| t.0).collect::<Vec<_>>(), [10, 20, 30]);
    assert_eq!((list.index_of(20), list.by_id(30).map(|t| t.0)), (Some(1), Some(30)));
    assert_eq!((list.index_of(25), list.by_id(25)), (None, None));
    assert!(ById::<Thing>::default().is_empty() && !list.is_empty());
}

#[test]
fn a_file_holds_the_plain_list() {
    let list = things(&[1, 2]);
    assert_eq!(serde_json::to_value(&list).unwrap(), json!([1, 2]));
    assert_eq!(serde_json::from_value::<ById<Thing>>(json!([1, 2])).unwrap(), list);
}

#[test]
fn the_rules_name_the_kind_and_the_record() {
    let fine = |_: &Thing| Ok(());
    things(&[1, 2]).check(2, "thing", fine).unwrap();
    assert!(things(&[1, 2, 3]).check(2, "thing", fine).is_err_and(|e| e.to_string().contains("at most 2 things")));
    let mut out_of_order = things(&[1, 2]);
    out_of_order.reverse();
    assert!(out_of_order.check(2, "thing", fine).is_err_and(|e| e.to_string().contains("lists its things by id")));
    out_of_order.put_in_order();
    out_of_order.check(2, "thing", fine).unwrap();
    let odd = |t: &Thing| require(t.0.is_multiple_of(2), || "it is odd".into());
    let e = things(&[2, 3]).check(2, "thing", odd).unwrap_err().to_string();
    assert!(e.contains(&format!("thing {}: it is odd", ids::encode(3))), "{e}");
}
