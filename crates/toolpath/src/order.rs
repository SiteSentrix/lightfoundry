use lightfoundry_geom::{Point, Polyline};

/// Orders polylines for cutting: shapes nested inside others first (so parts don't drop out of
/// the sheet before their holes are cut), then nearest-neighbour from `start`.
/// Closed polylines are rotated to begin at their vertex nearest the head.
pub fn order_polylines(lines: Vec<Polyline>, start: Point) -> Vec<Polyline> {
    const MAX_FOR_FULL_ORDERING: usize = 4000;
    if lines.len() > MAX_FOR_FULL_ORDERING {
        return lines;
    }
    let depths: Vec<usize> = (0..lines.len()).map(|i| depth(&lines, i)).collect();
    let mut remaining: Vec<(usize, Polyline)> = depths.into_iter().zip(lines).collect();
    let mut out = Vec::with_capacity(remaining.len());
    let mut at = start;
    while !remaining.is_empty() {
        let max_depth = remaining.iter().map(|(d, _)| *d).max().unwrap_or(0);
        let mut best: Option<(usize, bool, f64)> = None;
        for (i, (d, pl)) in remaining.iter().enumerate() {
            if *d != max_depth {
                continue;
            }
            let (Some(first), Some(last)) = (pl.points.first(), pl.points.last()) else { continue };
            let nearest_vertex =
                if pl.closed { pl.points.iter().map(|p| at.distance(*p)).fold(f64::INFINITY, f64::min) } else { at.distance(*first) };
            let rev_dist = if pl.closed { f64::INFINITY } else { at.distance(*last) };
            let (dist, rev) = if rev_dist < nearest_vertex { (rev_dist, true) } else { (nearest_vertex, false) };
            if best.is_none_or(|(_, _, bd)| dist < bd) {
                best = Some((i, rev, dist));
            }
        }
        let Some((i, rev, _)) = best else { break };
        let (_, mut pl) = remaining.swap_remove(i);
        if rev {
            pl.points.reverse();
        } else if pl.closed {
            rotate_to_nearest(&mut pl, at);
        }
        if let Some(end) = if pl.closed { pl.points.first() } else { pl.points.last() } {
            at = *end;
        }
        out.push(pl);
    }
    out
}

fn depth(lines: &[Polyline], i: usize) -> usize {
    let Some(p) = lines.get(i).and_then(|l| l.points.first()) else { return 0 };
    lines.iter().enumerate().filter(|(j, other)| *j != i && other.contains(*p)).count()
}

fn rotate_to_nearest(pl: &mut Polyline, at: Point) {
    let idx = pl.points.iter().enumerate().min_by(|a, b| at.distance(*a.1).total_cmp(&at.distance(*b.1))).map(|(i, _)| i).unwrap_or(0);
    pl.points.rotate_left(idx);
}
