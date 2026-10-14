use super::*;
use crate::{Polyline, ellipse_path, flatten, rect_path};

fn same_shape(a: &BezPath, b: &BezPath) {
    let (fa, fb) = (flatten(a, 1e-4), flatten(b, 1e-4));
    assert_eq!(fa.len(), fb.len(), "same number of sub-paths");
    for (la, lb) in fa.iter().zip(&fb) {
        assert_eq!(la.closed, lb.closed);
        for p in &la.points {
            assert!(lb.distance_to(*p) < 2e-3 || (!lb.closed && nearest_point_distance(lb, *p) < 2e-3), "{p:?} is off the other path");
        }
    }
}

fn nearest_point_distance(l: &Polyline, p: Point) -> f64 {
    l.points.windows(2).map(|w| crate::distance_to_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
}

fn curve_path() -> BezPath {
    let mut p = BezPath::new();
    p.move_to((0.0, 0.0));
    p.curve_to((10.0, 0.0), (20.0, 10.0), (30.0, 10.0));
    p.line_to((30.0, 30.0));
    p
}

mod conversion_tests {
    use super::*;

    #[test]
    fn a_rectangle_is_four_corner_nodes_and_round_trips() {
        let path = rect_path(0.0, 0.0, 20.0, 10.0);
        let np = NodePath::from_bez(&path).unwrap();
        assert_eq!(np.subpaths.len(), 1);
        assert!(np.subpaths[0].closed && np.subpaths[0].nodes.len() == 4);
        assert!(np.subpaths[0].nodes.iter().all(|n| n.in_h.is_none() && n.out_h.is_none() && !n.smooth));
        same_shape(&path, &np.to_bez());
        assert!(matches!(np.to_bez().elements().last(), Some(PathEl::ClosePath)));
        assert_eq!(np.to_bez().elements().len(), 5, "move, three lines, close: the closing line is implied");
    }

    #[test]
    fn an_ellipse_is_four_smooth_nodes_with_handles() {
        let path = ellipse_path(10.0, 10.0, 8.0, 5.0);
        let np = NodePath::from_bez(&path).unwrap();
        let sp = &np.subpaths[0];
        assert!(sp.closed);
        assert_eq!(sp.nodes.len(), 4, "the closing curve folds into the first node");
        assert!(sp.nodes.iter().all(|n| n.smooth && n.in_h.is_some() && n.out_h.is_some()), "{:?}", sp.nodes);
        same_shape(&path, &np.to_bez());
    }

    #[test]
    fn quadratics_become_cubics_without_changing_the_shape() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.quad_to((10.0, 20.0), (20.0, 0.0));
        let np = NodePath::from_bez(&p).unwrap();
        assert!(np.subpaths[0].nodes[0].out_h.is_some());
        same_shape(&p, &np.to_bez());
    }

    #[test]
    fn open_paths_stay_open_and_multiple_subpaths_survive() {
        let mut p = curve_path();
        p.move_to((100.0, 100.0));
        p.line_to((110.0, 100.0));
        p.line_to((110.0, 110.0));
        p.close_path();
        let np = NodePath::from_bez(&p).unwrap();
        assert_eq!(np.subpaths.len(), 2);
        assert!(!np.subpaths[0].closed && np.subpaths[1].closed);
        same_shape(&p, &np.to_bez());
    }

    #[test]
    fn degenerate_input_is_dropped_not_fatal() {
        let mut p = BezPath::new();
        p.move_to((1.0, 1.0));
        p.move_to((5.0, 5.0));
        p.line_to((6.0, 6.0));
        let np = NodePath::from_bez(&p).unwrap();
        assert_eq!(np.subpaths.len(), 1, "a lone MoveTo is not a sub-path");
        assert_eq!(NodePath::from_bez(&BezPath::new()).unwrap(), NodePath::default());
        assert_eq!(NodePath::default().to_bez().elements().len(), 0);
    }

    #[test]
    fn non_finite_and_oversized_paths_are_errors() {
        let mut bad = BezPath::new();
        bad.move_to((f64::NAN, 0.0));
        bad.line_to((1.0, 1.0));
        assert_eq!(NodePath::from_bez(&bad), Err(NodeError::NotFinite));
        let mut huge = BezPath::new();
        huge.move_to((0.0, 0.0));
        for i in 0..(MAX_NODES + 5) {
            huge.line_to((i as f64, 0.0));
        }
        assert_eq!(NodePath::from_bez(&huge), Err(NodeError::TooMany));
    }
}

mod edit_tests {
    use super::*;

    #[test]
    fn moving_a_node_carries_its_handles_and_reshapes_the_adjacent_curve() {
        let mut np = NodePath::from_bez(&curve_path()).unwrap();
        let handle_before = np.subpaths[0].nodes[1].in_h;
        np.move_node(0, 1, Point::new(40.0, 20.0)).unwrap();
        assert_eq!(np.subpaths[0].nodes[1].p, Point::new(40.0, 20.0));
        assert_eq!(np.subpaths[0].nodes[1].in_h, handle_before, "handles are relative, so they travel with the node");
        let (c1, c2) = controls(&np.subpaths[0].nodes[0], &np.subpaths[0].nodes[1]).unwrap();
        assert_eq!(c1, Point::new(10.0, 0.0));
        assert_eq!(c2, Point::new(30.0, 20.0));
    }

    #[test]
    fn dragging_a_handle_of_a_smooth_node_turns_the_other_one_and_corners_stay_independent() {
        let mut np = NodePath::from_bez(&ellipse_path(0.0, 0.0, 10.0, 10.0)).unwrap();
        let node = np.subpaths[0].nodes[0].clone();
        assert!(node.smooth);
        let in_len = node.in_h.unwrap().hypot();
        np.move_handle(0, 0, Side::Out, node.p + Vec2::new(0.0, 7.0)).unwrap();
        let n = &np.subpaths[0].nodes[0];
        let (i, o) = (n.in_h.unwrap(), n.out_h.unwrap());
        assert!((o.hypot() - 7.0).abs() < 1e-9);
        assert!((i.hypot() - in_len).abs() < 1e-9, "the other handle keeps its length");
        assert!((i.x * o.y - i.y * o.x).abs() < 1e-9 && i.dot(o) < 0.0, "and points the opposite way");
        np.set_smooth(0, 0, false).unwrap();
        np.move_handle(0, 0, Side::Out, node.p + Vec2::new(5.0, 5.0)).unwrap();
        assert_eq!(np.subpaths[0].nodes[0].in_h, Some(i), "a corner node's other handle does not move");
    }

    #[test]
    fn inserting_on_a_line_splits_it_and_on_a_curve_keeps_the_shape_exactly() {
        let path = curve_path();
        for (segment, t) in [(0, 0.5), (0, 0.2), (0, 0.9), (1, 0.5)] {
            let mut np = NodePath::from_bez(&path).unwrap();
            let idx = np.insert(0, segment, t).unwrap();
            assert_eq!(idx, segment + 1);
            assert_eq!(np.subpaths[0].nodes.len(), 4);
            same_shape(&path, &np.to_bez());
        }
        let mut np = NodePath::from_bez(&path).unwrap();
        np.insert(0, 1, 0.5).unwrap();
        assert_eq!(np.subpaths[0].nodes[2].p, Point::new(30.0, 20.0));
        let on_curve =
            point_at(&NodePath::from_bez(&path).unwrap().subpaths[0].nodes[0], &NodePath::from_bez(&path).unwrap().subpaths[0].nodes[1], 0.3);
        let mut np = NodePath::from_bez(&path).unwrap();
        np.insert(0, 0, 0.3).unwrap();
        assert!(np.subpaths[0].nodes[1].p.distance(on_curve) < 1e-9, "the new node sits at t on the curve");
    }

    #[test]
    fn inserting_into_a_closing_segment_works() {
        let mut np = NodePath::from_bez(&rect_path(0.0, 0.0, 10.0, 10.0)).unwrap();
        let idx = np.insert(0, 3, 0.5).unwrap();
        assert_eq!(np.subpaths[0].nodes[idx].p, Point::new(0.0, 5.0));
        same_shape(&rect_path(0.0, 0.0, 10.0, 10.0), &np.to_bez());
    }

    #[test]
    fn deleting_nodes_respects_the_minimum_for_open_and_closed_paths() {
        let mut np = NodePath::from_bez(&rect_path(0.0, 0.0, 10.0, 10.0)).unwrap();
        np.delete(0, 2).unwrap();
        assert_eq!(np.subpaths[0].nodes.len(), 3);
        assert_eq!(np.delete(0, 0), Err(NodeError::TooFewNodes(4)), "a closed path keeps three nodes");
        let mut open = NodePath::from_bez(&curve_path()).unwrap();
        open.delete(0, 1).unwrap();
        assert_eq!(open.delete(0, 0), Err(NodeError::TooFewNodes(3)), "an open path keeps two");
    }

    #[test]
    fn segments_convert_between_line_and_curve() {
        let mut np = NodePath::from_bez(&rect_path(0.0, 0.0, 30.0, 30.0)).unwrap();
        np.set_segment_curve(0, 0, true).unwrap();
        let (c1, c2) = controls(&np.subpaths[0].nodes[0], &np.subpaths[0].nodes[1]).unwrap();
        assert_eq!((c1, c2), (Point::new(10.0, 0.0), Point::new(20.0, 0.0)));
        assert!(np.to_bez().elements().iter().any(|e| matches!(e, PathEl::CurveTo(..))));
        np.set_segment_curve(0, 0, false).unwrap();
        assert!(controls(&np.subpaths[0].nodes[0], &np.subpaths[0].nodes[1]).is_none());
        np.set_segment_curve(0, 0, true).unwrap();
        np.set_segment_curve(0, 0, true).unwrap();
        assert_eq!(controls(&np.subpaths[0].nodes[0], &np.subpaths[0].nodes[1]).unwrap(), (c1, c2), "converting a curve again changes nothing");
    }

    #[test]
    fn making_a_node_smooth_creates_handles_along_its_neighbours_and_closing_works() {
        let mut np = NodePath::from_bez(&rect_path(0.0, 0.0, 30.0, 30.0)).unwrap();
        np.set_smooth(0, 1, true).unwrap();
        let n = &np.subpaths[0].nodes[1];
        assert!(n.smooth && n.in_h.is_some() && n.out_h.is_some());
        assert!((n.in_h.unwrap() + n.out_h.unwrap()).hypot() < 1e-9, "equal and opposite");
        let mut open = NodePath::from_bez(&curve_path()).unwrap();
        assert!(!open.subpaths[0].closed);
        open.set_closed(0, true).unwrap();
        assert!(matches!(open.to_bez().elements().last(), Some(PathEl::ClosePath)));
        open.set_closed(0, false).unwrap();
        let mut two = NodePath::from_bez(&{
            let mut p = BezPath::new();
            p.move_to((0.0, 0.0));
            p.line_to((5.0, 5.0));
            p
        })
        .unwrap();
        assert_eq!(two.set_closed(0, true), Err(NodeError::TooFewNodes(3)));
    }

    #[test]
    fn nearest_finds_the_segment_and_parameter_under_a_point() {
        let np = NodePath::from_bez(&rect_path(0.0, 0.0, 20.0, 10.0)).unwrap();
        let hit = np.nearest(Point::new(5.0, -2.0)).unwrap();
        assert_eq!((hit.subpath, hit.segment), (0, 0));
        assert!((hit.t - 0.25).abs() < 1e-9 && (hit.distance - 2.0).abs() < 1e-9);
        let curved = NodePath::from_bez(&curve_path()).unwrap();
        let on = point_at(&curved.subpaths[0].nodes[0], &curved.subpaths[0].nodes[1], 0.4);
        let hit = curved.nearest(on).unwrap();
        assert!(hit.distance < 0.05 && hit.segment == 0 && (hit.t - 0.4).abs() < 0.05, "{hit:?}");
        assert!(NodePath::default().nearest(Point::ORIGIN).is_none());
    }

    #[test]
    fn bad_indices_and_values_are_errors() {
        let mut np = NodePath::from_bez(&rect_path(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!(np.move_node(9, 0, Point::ORIGIN), Err(NodeError::NoSubpath(9)));
        assert_eq!(np.move_node(0, 9, Point::ORIGIN), Err(NodeError::NoNode(9)));
        assert_eq!(np.move_node(0, 0, Point::new(f64::NAN, 0.0)), Err(NodeError::NotFinite));
        assert_eq!(np.move_handle(0, 0, Side::In, Point::new(0.0, f64::INFINITY)), Err(NodeError::NotFinite));
        assert_eq!(np.insert(0, 9, 0.5), Err(NodeError::NoSegment(9)));
        assert_eq!(np.insert(0, 0, f64::NAN), Err(NodeError::NotFinite));
        assert_eq!(np.set_segment_curve(0, 9, true), Err(NodeError::NoSegment(9)));
        assert_eq!(np.delete(0, 9), Err(NodeError::NoNode(9)));
        assert_eq!(np.set_smooth(3, 0, true), Err(NodeError::NoSubpath(3)));
        assert_eq!(np, NodePath::from_bez(&rect_path(0.0, 0.0, 10.0, 10.0)).unwrap(), "failed edits change nothing");
    }
}

mod smoothing_tests {
    use kurbo::Shape;

    use super::*;

    fn circle_points(n: usize, r: f64) -> Vec<Point> {
        (0..n)
            .map(|i| Point::new(r * (i as f64 * std::f64::consts::TAU / n as f64).cos(), r * (i as f64 * std::f64::consts::TAU / n as f64).sin()))
            .collect()
    }

    #[test]
    fn gentle_bends_become_smooth_nodes_and_sharp_ones_stay_corners() {
        let sub = smooth_polyline(&circle_points(12, 50.0), true, 60.0);
        assert!(sub.closed && sub.nodes.len() == 12 && sub.nodes.iter().all(|n| n.smooth && n.in_h.is_some() && n.out_h.is_some()));
        let square = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        let sharp = smooth_polyline(&square, true, 60.0);
        assert!(sharp.nodes.iter().all(|n| !n.smooth && n.in_h.is_none()), "90 degree bends stay corners");
        let open = smooth_polyline(&circle_points(6, 5.0)[..4], false, 100.0);
        assert!(!open.nodes[0].smooth && !open.nodes[3].smooth, "the ends of an open line are corners");
        assert!(open.nodes[1].smooth && open.nodes[2].smooth);
    }

    #[test]
    fn the_curve_stays_close_to_the_polygon_and_degenerate_input_is_safe() {
        let pts = circle_points(16, 40.0);
        let path = NodePath { subpaths: vec![smooth_polyline(&pts, true, 60.0)] }.to_bez();
        let b = path.bounding_box();
        assert!((b.width() - 80.0).abs() < 2.0 && (b.height() - 80.0).abs() < 2.0, "{b:?}");
        for odd in [
            vec![],
            vec![Point::new(1.0, 1.0)],
            vec![Point::new(1.0, 1.0); 5],
            vec![Point::new(f64::NAN, 0.0), Point::new(1.0, 1.0), Point::new(2.0, 0.0)],
        ] {
            let _ = NodePath { subpaths: vec![smooth_polyline(&odd, true, 60.0)] }.to_bez();
        }
        assert_eq!(smooth_polyline(&[], false, 60.0).nodes.len(), 0);
    }
}
