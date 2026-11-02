//! Pure ordered option packing. Composite descendants belong to their parent's single cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Width {
    Narrow,
    Composite,
    Wide,
}

pub(crate) struct Entry {
    pub width: Width,
    pub advanced: bool,
    /// Measured minimum row width, including translated labels and controls.
    pub minimum: f32,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Plan {
    pub rows: Vec<Vec<usize>>,
    pub more: Vec<Vec<usize>>,
    pub more_count: usize,
    pub section_labels: bool,
}

pub(crate) fn choice_width(values: usize) -> Width {
    if (4..=5).contains(&values) { Width::Wide } else { Width::Narrow }
}

/// `width` is the usable width after the caller subtracts the inter-column gutter.
pub(crate) fn layout(entries: &[Entry], width: f32) -> Plan {
    let mut main = Vec::new();
    let mut more = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        if entry.advanced || main.len() >= 10 {
            more.push(i);
        } else {
            main.push(i);
        }
    }
    let pack = |indices: &[usize]| {
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut pending = Vec::new();
        for &i in indices {
            let Some(entry) = entries.get(i) else { continue };
            let wide = entry.width == Width::Wide || !width.is_finite() || entry.minimum > width.max(0.0) / 2.0;
            if wide {
                if !pending.is_empty() {
                    rows.push(std::mem::take(&mut pending));
                }
                rows.push(vec![i]);
            } else {
                pending.push(i);
                if pending.len() == 2 {
                    rows.push(std::mem::take(&mut pending));
                }
            }
        }
        if !pending.is_empty() {
            rows.push(pending);
        }
        rows
    };
    Plan { rows: pack(&main), more: pack(&more), more_count: more.len(), section_labels: entries.len() > 6 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_options_layout_counts_and_order() {
        for count in [0_usize, 1, 2, 3, 6, 10, 12] {
            let entries: Vec<_> = (0..count).map(|_| Entry { width: Width::Narrow, advanced: false, minimum: 100.0 }).collect();
            let plan = layout(&entries, 600.0);
            assert_eq!(plan.rows.iter().flatten().copied().collect::<Vec<_>>(), (0..count.min(10)).collect::<Vec<_>>());
            assert_eq!(plan.more.iter().flatten().copied().collect::<Vec<_>>(), (10..count).collect::<Vec<_>>());
            assert_eq!(plan.rows.len(), count.min(10).div_ceil(2));
            assert_eq!(plan.more_count, count.saturating_sub(10));
            assert_eq!(plan.section_labels, count > 6);
        }
    }

    #[test]
    fn theme_options_layout_wide_composite_dropdown_advanced_and_width() {
        let mut entries: Vec<_> = (0..13).map(|_| Entry { width: Width::Narrow, advanced: false, minimum: 100.0 }).collect();
        entries[1].width = Width::Wide;
        entries[2].width = Width::Composite;
        entries[3].advanced = true;
        let plan = layout(&entries, 600.0);
        assert_eq!(&plan.rows[..3], &[vec![0], vec![1], vec![2, 4]]);
        assert_eq!(plan.rows.iter().flatten().count(), 10, "advanced does not consume the ten offered cells");
        assert_eq!(plan.more.iter().flatten().copied().collect::<Vec<_>>(), [3, 11, 12]);
        assert_eq!(choice_width(3), Width::Narrow);
        assert_eq!(choice_width(4), Width::Wide);
        assert_eq!(choice_width(5), Width::Wide);
        assert_eq!(choice_width(6), Width::Narrow);
        let advanced_only = [Entry { width: Width::Composite, advanced: true, minimum: 100.0 }];
        assert!(layout(&advanced_only, 600.0).rows.is_empty());
        assert_eq!(layout(&advanced_only, 600.0).more, [vec![0]], "advanced stays in More even below ten entries");
        entries[0].minimum = 350.0;
        assert_eq!(layout(&entries, 600.0).rows[0], [0]);
        assert!(layout(&entries, 150.0).rows.iter().all(|row| row.len() == 1));
    }
}
