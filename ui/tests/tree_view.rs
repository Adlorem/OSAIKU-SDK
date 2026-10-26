use osaiku_ui::layout::Rect;
use osaiku_ui::tree_view::{Geometry, Navigation, Tree};
#[test]
fn hierarchy_expansion_and_selection_use_keys_not_rows() {
    let mut t = Tree::<8>::NEW;
    t.push(1, 0, true).unwrap();
    t.push(2, 1, true).unwrap();
    t.push(3, 1, true).unwrap();
    t.push(4, 2, false).unwrap();
    t.push(5, 3, false).unwrap();
    t.set_expanded(1, true).unwrap();
    t.set_expanded(2, true).unwrap();
    t.set_expanded(3, true).unwrap();
    assert_eq!(
        t.visible().map(|r| r.key).collect::<Vec<_>>(),
        [1, 2, 4, 3, 5]
    );
    t.select(4).unwrap();
    t.set_expanded(2, false).unwrap();
    assert_eq!(t.selected(), 2);
    assert!(t.select(4).is_err());
    assert_eq!(t.navigate(Navigation::Next), Some(3));
    assert_eq!(t.navigate(Navigation::Child), Some(5));
    assert_eq!(t.navigate(Navigation::Parent), Some(3));
}
#[test]
fn refresh_keeps_only_surviving_keys_and_rejects_bad_structure() {
    let mut t = Tree::<4>::NEW;
    assert!(t.push(1, 2, false).is_err());
    t.push(1, 0, true).unwrap();
    assert!(t.push(1, 0, true).is_err());
    t.push(2, 1, false).unwrap();
    assert!(t.push(3, 2, false).is_err());
    t.set_expanded(1, true).unwrap();
    t.select(2).unwrap();
    t.begin_refresh();
    t.push(1, 0, true).unwrap();
    t.push(3, 1, false).unwrap();
    t.finish_refresh();
    assert_eq!(t.selected(), 1);
    assert!(t.expanded(1));
    assert_eq!(t.visible().count(), 2);
}
#[test]
fn virtual_rows_clip_and_share_disclosure_geometry() {
    let g = Geometry {
        row_height: 36,
        indent: 18,
        toggle_width: 24,
    };
    let viewport = Rect {
        x: 10,
        y: 40,
        width: 300,
        height: 180,
    };
    let range = g.range(128, 39, viewport, 8).unwrap();
    assert!(range.count <= 8);
    let (toggle, label) = g.row(viewport, 1, 2, 39).unwrap();
    assert_eq!((toggle.x, toggle.y), (46, 37));
    assert_eq!(label.x, toggle.x + toggle.width as i32);
    assert_eq!(label.x + label.width as i32, 310);
    assert!(g.range(128, 0, viewport, 2).is_err());
}

#[test]
fn tree_bounds_and_refreshed_node_types_remain_valid() {
    let mut tree = Tree::<32>::NEW;
    for key in 1..=osaiku_ui::MAX_DEPTH as u64 {
        tree.push(key, key - 1, true).unwrap();
    }
    assert!(tree.push(25, 24, false).is_err());
    let mut tree = Tree::<2>::NEW;
    tree.push(1, 0, true).unwrap();
    tree.push(2, 1, false).unwrap();
    assert!(tree.push(3, 1, false).is_err());
    tree.set_expanded(1, true).unwrap();
    tree.select(2).unwrap();
    tree.begin_refresh();
    tree.push(1, 0, false).unwrap();
    tree.finish_refresh();
    assert!(!tree.expanded(1));
    assert_eq!(tree.selected(), 1);
    assert_eq!(tree.visible().count(), 1);
}
#[test]
fn every_row_ignores_collapse_but_reports_retained_expansion() {
    let mut t = Tree::<8>::NEW;
    t.push(1, 0, true).unwrap();
    t.push(2, 1, true).unwrap();
    t.push(3, 1, true).unwrap();
    t.push(4, 2, false).unwrap();
    t.push(5, 3, false).unwrap();
    t.set_expanded(1, true).unwrap();
    assert_eq!(t.visible().map(|r| r.key).collect::<Vec<_>>(), [1, 2, 3]);
    let every: Vec<_> = t.every().map(|r| (r.key, r.depth, r.expanded)).collect();
    assert_eq!(
        every,
        [
            (1, 0, true),
            (2, 1, false),
            (4, 2, false),
            (3, 1, false),
            (5, 2, false)
        ]
    );
    // The retained model is unchanged: a filtered view expands nothing.
    assert_eq!(t.visible().count(), 3);
    assert!(Tree::<2>::NEW.every().next().is_none());
}
