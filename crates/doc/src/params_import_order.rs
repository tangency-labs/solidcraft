//! Dependency order for imported rows, independent of document evaluation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Only the final definition contributes expression dependencies. Duplicate definitions
/// still apply in input order so that the last write and inherited metadata win.
pub(super) fn order(rows: &[(&str, &str, bool)], references: impl Fn(&str) -> Vec<String>) -> Option<Vec<usize>> {
    let mut last = BTreeMap::new();
    let mut previous = vec![None; rows.len()];
    for (index, (name, _, _)) in rows.iter().enumerate() {
        previous[index] = last.insert(*name, index);
    }
    let mut dependents = vec![Vec::new(); rows.len()];
    let mut counts = vec![0; rows.len()];
    for (index, (name, expression, is_text)) in rows.iter().enumerate() {
        let mut dependencies: BTreeSet<usize> = if last.get(name) == Some(&index) && !is_text {
            references(expression).iter().filter_map(|name| last.get(name.as_str()).copied()).collect()
        } else {
            BTreeSet::new()
        };
        dependencies.extend(previous[index]);
        counts[index] = dependencies.len();
        for dependency in dependencies {
            dependents[dependency].push(index);
        }
    }
    let mut ready: VecDeque<usize> = counts.iter().enumerate().filter_map(|(index, count)| (*count == 0).then_some(index)).collect();
    let mut ordered = Vec::with_capacity(rows.len());
    while let Some(index) = ready.pop_front() {
        ordered.push(index);
        for dependent in &dependents[index] {
            counts[*dependent] -= 1;
            if counts[*dependent] == 0 {
                ready.push_back(*dependent);
            }
        }
    }
    (ordered.len() == rows.len()).then_some(ordered)
}

#[cfg(test)]
mod tests {
    use super::order;

    fn refs(text: &str) -> Vec<String> {
        text.split_whitespace().filter(|word| word.chars().all(char::is_alphabetic)).map(str::to_string).collect()
    }

    #[test]
    fn forward_dependencies_apply_first() {
        assert_eq!(order(&[("a", "b", false), ("b", "1", false)], refs), Some(vec![1, 0]));
    }

    #[test]
    fn overwritten_expression_does_not_create_a_cycle() {
        let rows = [("a", "b", false), ("b", "a", false), ("a", "1", false)];
        assert_eq!(order(&rows, refs), Some(vec![0, 2, 1]));
    }

    #[test]
    fn an_earlier_duplicate_never_overwrites_the_final_value() {
        let rows = [("a", "c", false), ("a", "5", false), ("c", "7", false)];
        let sorted = order(&rows, refs).unwrap();
        let mut final_a = "";
        for index in sorted {
            if rows[index].0 == "a" {
                final_a = rows[index].1;
            }
        }
        assert_eq!(final_a, "5");
    }

    #[test]
    fn text_is_literal_even_when_it_names_itself_or_a_number() {
        assert_eq!(order(&[("label", "label", true), ("n", "label", true)], |_| panic!("text must not be scanned")), Some(vec![0, 1]));
    }

    #[test]
    fn final_numeric_cycles_are_rejected() {
        assert_eq!(order(&[("a", "b", false), ("b", "a", false)], refs), None);
        assert_eq!(order(&[("a", "a", false)], refs), None);
    }

    #[test]
    fn all_duplicate_rows_remain_in_original_order() {
        assert_eq!(order(&[("a", "1", false), ("a", "2", false), ("a", "3", false)], refs), Some(vec![0, 1, 2]));
    }
}
