//! Grouping a picture's colours into a few palette entries, for tracing each colour on its own.

/// Most palette entries asked for.
pub const MAX_COLORS: usize = 8;
/// Pixels whose every channel is at least this are treated as paper, not ink.
const PAPER: u8 = 230;
/// Most pixels looked at to choose the palette; the rest are only assigned to it.
const SAMPLE: usize = 50_000;
const ITERATIONS: usize = 10;

/// The label of a paper (background) pixel.
pub const BACKGROUND: u8 = u8::MAX;

pub struct Quantized {
    pub palette: Vec<[u8; 3]>,
    /// One label per pixel: an index into `palette`, or [`BACKGROUND`].
    pub labels: Vec<u8>,
}

/// Chooses up to `colors` palette entries for `rgb` (three bytes per pixel) by k-means on a sample
/// of the non-paper pixels, then labels every pixel with its nearest entry. `None` when the inputs
/// are inconsistent or the picture has no ink.
pub fn quantize(rgb: &[u8], colors: usize) -> Option<Quantized> {
    if !(1..=MAX_COLORS).contains(&colors) || rgb.is_empty() || !rgb.len().is_multiple_of(3) {
        return None;
    }
    let pixels: Vec<[u8; 3]> = rgb.as_chunks::<3>().0.to_vec();
    let is_paper = |p: &[u8; 3]| p.iter().all(|c| *c >= PAPER);
    let ink: Vec<[u8; 3]> = pixels.iter().filter(|p| !is_paper(p)).copied().collect();
    if ink.is_empty() {
        return None;
    }
    let step = (ink.len() / SAMPLE).max(1);
    let sample: Vec<[u8; 3]> = ink.iter().step_by(step).copied().collect();

    // Farthest-point start, so the entries begin spread across the colours that are really there.
    let mean = {
        let n = sample.len() as f64;
        let sum = sample.iter().fold([0.0; 3], |a, p| [a[0] + f64::from(p[0]), a[1] + f64::from(p[1]), a[2] + f64::from(p[2])]);
        [sum[0] / n, sum[1] / n, sum[2] / n]
    };
    let dist = |a: &[f64; 3], b: &[u8; 3]| (0..3).map(|i| (a[i] - f64::from(b[i])).powi(2)).sum::<f64>();
    let mut centres: Vec<[f64; 3]> = Vec::new();
    let first = sample.iter().min_by(|a, b| dist(&mean, a).total_cmp(&dist(&mean, b)))?;
    centres.push([f64::from(first[0]), f64::from(first[1]), f64::from(first[2])]);
    while centres.len() < colors {
        let far = sample.iter().max_by(|a, b| {
            let da = centres.iter().map(|c| dist(c, a)).fold(f64::INFINITY, f64::min);
            let db = centres.iter().map(|c| dist(c, b)).fold(f64::INFINITY, f64::min);
            da.total_cmp(&db)
        })?;
        let gap = centres.iter().map(|c| dist(c, far)).fold(f64::INFINITY, f64::min);
        if gap < 1.0 {
            break; // fewer distinct colours than asked for
        }
        centres.push([f64::from(far[0]), f64::from(far[1]), f64::from(far[2])]);
    }
    let nearest =
        |centres: &[[f64; 3]], p: &[u8; 3]| centres.iter().enumerate().min_by(|a, b| dist(a.1, p).total_cmp(&dist(b.1, p))).map_or(0, |(i, _)| i);
    for _ in 0..ITERATIONS {
        let mut sums = vec![([0.0f64; 3], 0usize); centres.len()];
        for p in &sample {
            let (sum, n) = &mut sums[nearest(&centres, p)];
            for i in 0..3 {
                sum[i] += f64::from(p[i]);
            }
            *n += 1;
        }
        for (c, (sum, n)) in centres.iter_mut().zip(&sums) {
            if *n > 0 {
                *c = [sum[0] / *n as f64, sum[1] / *n as f64, sum[2] / *n as f64];
            }
        }
    }
    let palette: Vec<[u8; 3]> = centres.iter().map(|c| [c[0].round() as u8, c[1].round() as u8, c[2].round() as u8]).collect();
    let labels = pixels.iter().map(|p| if is_paper(p) { BACKGROUND } else { nearest(&centres, p) as u8 }).collect();
    Some(Quantized { palette, labels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(px: &[[u8; 3]]) -> Vec<u8> {
        px.iter().flatten().copied().collect()
    }

    #[test]
    fn distinct_colours_get_their_own_entries_and_paper_is_background() {
        let (red, blue, white) = ([220, 20, 20], [20, 20, 220], [255, 255, 255]);
        let q = quantize(&picture(&[red, red, blue, white, blue, red]), 2).unwrap();
        assert_eq!(q.palette.len(), 2);
        assert_eq!(q.labels[3], BACKGROUND);
        assert_eq!((q.labels[0], q.labels[1], q.labels[5]), (q.labels[0], q.labels[0], q.labels[0]));
        assert_ne!(q.labels[0], q.labels[2]);
        assert_eq!(q.labels[2], q.labels[4]);
        let near = |p: [u8; 3], want: [u8; 3]| p.iter().zip(want).all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() < 3);
        assert!(near(q.palette[q.labels[0] as usize], red) && near(q.palette[q.labels[2] as usize], blue));
    }

    #[test]
    fn asking_for_more_colours_than_exist_gives_fewer() {
        let q = quantize(&picture(&[[10, 10, 10], [10, 10, 10]]), 5).unwrap();
        assert_eq!(q.palette.len(), 1);
    }

    #[test]
    fn nothing_to_quantize_or_nonsense_input_is_none() {
        assert!(quantize(&[], 2).is_none());
        assert!(quantize(&[1, 2], 2).is_none());
        assert!(quantize(&picture(&[[255, 255, 255]]), 2).is_none(), "only paper");
        assert!(quantize(&picture(&[[0, 0, 0]]), 0).is_none());
        assert!(quantize(&picture(&[[0, 0, 0]]), MAX_COLORS + 1).is_none());
    }
}
