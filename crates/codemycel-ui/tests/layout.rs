use codemycel_ui::canvas::{Link, Point, Viewport, force_layout, hit_test};

fn ring(count: usize) -> (Vec<f64>, Vec<Link>) {
    let radii = (0..count)
        .map(|index| 6.0 + (index % 5) as f64 * 4.0)
        .collect();
    let links = (0..count)
        .map(|index| Link {
            source: index,
            target: (index + 1) % count,
            weight: 1 + index % 7,
        })
        .chain((0..count / 3).map(|index| Link {
            source: index,
            target: index * 2 % count,
            weight: 3,
        }))
        .collect();
    (radii, links)
}

#[test]
fn force_layout_is_deterministic_for_fixed_seed() {
    let (radii, links) = ring(40);
    let viewport = Viewport::new(800.0, 600.0, 24.0);
    let first = force_layout(&radii, &links, viewport, 7);
    let second = force_layout(&radii, &links, viewport, 7);
    let other_seed = force_layout(&radii, &links, viewport, 8);
    assert_eq!(first, second);
    assert_ne!(first, other_seed);
}

#[test]
fn layout_keeps_nodes_within_viewport_bounds() {
    let viewport = Viewport::new(640.0, 360.0, 16.0);
    for count in [1, 2, 13, 120] {
        let (radii, links) = ring(count);
        let positions = force_layout(&radii, &links, viewport, 1);
        assert_eq!(positions.len(), count);
        for (point, radius) in positions.iter().zip(&radii) {
            assert!(point.x.is_finite() && point.y.is_finite());
            assert!(point.x - radius >= 16.0 - 1e-9, "{point:?} r={radius}");
            assert!(
                point.x + radius <= 640.0 - 16.0 + 1e-9,
                "{point:?} r={radius}"
            );
            assert!(point.y - radius >= 16.0 - 1e-9, "{point:?} r={radius}");
            assert!(
                point.y + radius <= 360.0 - 16.0 + 1e-9,
                "{point:?} r={radius}"
            );
        }
    }
}

#[test]
fn hit_test_finds_nearest_node_within_radius() {
    let positions = [
        Point::new(100.0, 100.0),
        Point::new(130.0, 100.0),
        Point::new(400.0, 300.0),
    ];
    let radii = [20.0, 20.0, 8.0];
    assert_eq!(
        hit_test(&positions, &radii, Point::new(112.0, 100.0)),
        Some(0)
    );
    assert_eq!(
        hit_test(&positions, &radii, Point::new(118.0, 100.0)),
        Some(1)
    );
    assert_eq!(hit_test(&positions, &radii, Point::new(400.0, 309.0)), None);
    assert_eq!(hit_test(&positions, &radii, Point::new(250.0, 200.0)), None);
}
