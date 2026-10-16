use super::*;

fn series(values: &[Option<f64>]) -> Series<'_> {
    Series {
        label: "EV",
        color: egui::Color32::WHITE,
        values,
    }
}

fn rect() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0))
}

#[test]
fn y_bounds_ignore_gaps_and_add_headroom() {
    let a = [Some(10.0), None, Some(20.0)];
    let b = [Some(15.0), Some(f64::NAN)];
    let (lo, hi) = y_bounds(&[series(&a), series(&b)]).unwrap();
    assert!((lo - 9.5).abs() < 1e-9);
    assert!((hi - 20.5).abs() < 1e-9);
}

#[test]
fn y_bounds_flat_empty_and_floor() {
    let flat = [Some(5.0), Some(5.0)];
    let (lo, hi) = y_bounds(&[series(&flat)]).unwrap();
    assert!(lo < 5.0 && hi > 5.0);
    assert!(y_bounds(&[series(&[None, None])]).is_none());
    assert!(y_bounds(&[]).is_none());
    let near_zero = [Some(0.0), Some(100.0)];
    assert_eq!(y_bounds(&[series(&near_zero)]).unwrap().0, 0.0);
}

#[test]
fn to_screen_maps_corners() {
    let r = rect();
    assert_eq!(to_screen(0, 3, 0.0, (0.0, 10.0), r), egui::pos2(0.0, 50.0));
    assert_eq!(
        to_screen(2, 3, 10.0, (0.0, 10.0), r),
        egui::pos2(100.0, 0.0)
    );
    assert_eq!(to_screen(1, 3, 5.0, (0.0, 10.0), r), egui::pos2(50.0, 25.0));
    // Single point is centered horizontally.
    assert_eq!(to_screen(0, 1, 5.0, (0.0, 10.0), r).x, 50.0);
}

#[test]
fn hover_index_snaps_to_nearest_point() {
    let r = rect();
    assert_eq!(hover_index(0.0, 5, r), Some(0));
    assert_eq!(hover_index(30.0, 5, r), Some(1));
    assert_eq!(hover_index(100.0, 5, r), Some(4));
    assert_eq!(hover_index(50.0, 1, r), Some(0));
    assert_eq!(hover_index(-1.0, 5, r), None);
    assert_eq!(hover_index(101.0, 5, r), None);
    assert_eq!(hover_index(50.0, 0, r), None);
}
