//! Pure, stable ordering. Unknown sorts last in both directions; no identity mutation.
use super::SharedObject;
use crate::filters::value::Field;
use chrono::{DateTime, Utc};
use std::cmp::Ordering;

pub fn rows(
    rows: &mut Vec<SharedObject>,
    field: &Field,
    descending: bool,
    now: DateTime<Utc>,
    metrics: Option<&dyn super::Metrics>,
) {
    // Compute values once, not inside O(n log n) comparisons. Input is in canonical
    // namespace/name order from the store; stable sorting preserves that tie order.
    let mut keyed: Vec<_> = rows
        .drain(..)
        .map(|object| (field.read(&object, now, metrics), object))
        .collect();
    keyed.sort_by(|(a, _), (b, _)| match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            if descending {
                b.order(a)
            } else {
                a.order(b)
            }
        }
    });
    rows.extend(keyed.into_iter().map(|(_, object)| object));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::{Value, json};
    use std::sync::Arc;
    #[test]
    fn typed_stable_unknown_last_in_both_directions() {
        for (field, values) in [
            (
                "field:/spec/value",
                vec![json!(2), json!(10), json!(2), Value::Null],
            ),
            (
                "memory:field:/spec/value",
                vec![
                    json!("500Mi"),
                    json!("1Gi"),
                    json!("500Mi"),
                    json!("garbage"),
                ],
            ),
            (
                "cpu:field:/spec/value",
                vec![json!("500m"), json!("1"), json!("500m"), Value::Null],
            ),
            (
                "percent:field:/spec/value",
                vec![json!("9%"), json!("100%"), json!("9%"), Value::Null],
            ),
            (
                "bool:field:/spec/value",
                vec![json!(false), json!(true), json!(false), Value::Null],
            ),
        ] {
            let original: Vec<_> = values.into_iter().enumerate().map(|(i, value)| Arc::new(Object::new(json!({
                "metadata":{"name":format!("row{i}"),"uid":format!("uid{i}")},"spec":{"value":value}
            })))).collect();
            for (desc, expected) in [
                (false, vec!["uid0", "uid2", "uid1", "uid3"]),
                (true, vec!["uid1", "uid0", "uid2", "uid3"]),
            ] {
                let mut input = original.clone();
                rows(
                    &mut input,
                    &Field::parse(field).expect("field"),
                    desc,
                    Utc::now(),
                    None,
                );
                assert_eq!(
                    input.iter().map(|o| o.uid.as_str()).collect::<Vec<_>>(),
                    expected,
                    "{field}, descending={desc}"
                );
            }
        }
    }
    #[test]
    fn count_field_from_string_data_sorts_descending() {
        let original: Vec<_> = [("a", json!("2")), ("b", json!("10")), ("c", Value::Null)]
            .into_iter()
            .map(|(name, rank)| {
                let value = if rank.is_null() {
                    json!({"metadata":{"name":name,"uid":name},"data":{}})
                } else {
                    json!({"metadata":{"name":name,"uid":name},"data":{"rank":rank}})
                };
                Arc::new(Object::new(value))
            })
            .collect();
        for (desc, expected) in [(false, vec!["a", "b", "c"]), (true, vec!["b", "a", "c"])] {
            let mut input = original.clone();
            rows(
                &mut input,
                &Field::parse("count:field:/data/rank").expect("field"),
                desc,
                Utc::now(),
                None,
            );
            assert_eq!(
                input.iter().map(|o| o.uid.as_str()).collect::<Vec<_>>(),
                expected,
                "descending={desc}"
            );
        }
    }
    #[test]
    fn mixed_numeric_order_is_exact_and_transitive() {
        use crate::filters::value::Scalar::*;
        let scalars = [
            Integer(i128::MIN),
            Number(-2.5),
            Integer(-2),
            Number(1.5),
            Integer(2),
            Number(9007199254740992.0),
            Integer(9007199254740993),
            Number(-(i128::MIN as f64)),
        ];
        for (i, a) in scalars.iter().enumerate() {
            for (j, b) in scalars.iter().enumerate() {
                assert_eq!(a.order(b), i.cmp(&j));
            }
        }
    }
}
