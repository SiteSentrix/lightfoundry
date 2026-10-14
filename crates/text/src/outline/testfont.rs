//! A tiny TrueType font made in code for tests (the repository holds no font files).

/// Glyph 'A' is a square with a
/// square hole (advance 700), 'B' a plain square (advance 500); capitals are 700 units of 1000.
pub fn tiny_font() -> Vec<u8> {
    fn be16(v: &mut Vec<u8>, x: i32) {
        v.extend_from_slice(&(x as i16).to_be_bytes());
    }
    fn be32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    // glyf: .notdef empty, A with a hole, B plain.
    let simple = |contours: &[&[(i32, i32)]]| {
        let mut g = Vec::new();
        be16(&mut g, contours.len() as i32);
        let pts: Vec<(i32, i32)> = contours.iter().flat_map(|c| c.iter().copied()).collect();
        let (xs, ys): (Vec<i32>, Vec<i32>) = pts.iter().copied().unzip();
        for v in [
            xs.iter().copied().min().unwrap_or(0),
            ys.iter().copied().min().unwrap_or(0),
            xs.iter().copied().max().unwrap_or(0),
            ys.iter().copied().max().unwrap_or(0),
        ] {
            be16(&mut g, v);
        }
        let mut end = -1;
        for c in contours {
            end += c.len() as i32;
            be16(&mut g, end);
        }
        be16(&mut g, 0); // no instructions
        g.extend(std::iter::repeat_n(1u8, pts.len())); // on-curve, 16-bit deltas
        let (mut px, mut py) = (0, 0);
        for (x, _) in &pts {
            be16(&mut g, x - px);
            px = *x;
        }
        for (_, y) in &pts {
            be16(&mut g, y - py);
            py = *y;
        }
        while g.len() % 4 != 0 {
            g.push(0);
        }
        g
    };
    let a = simple(&[&[(0, 0), (600, 0), (600, 700), (0, 700)], &[(150, 150), (150, 550), (450, 550), (450, 150)]]);
    let b = simple(&[&[(0, 0), (400, 0), (400, 700), (0, 700)]]);
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    be32(&mut loca, 0);
    be32(&mut loca, glyf.len() as u32);
    glyf.extend(&a);
    be32(&mut loca, glyf.len() as u32);
    glyf.extend(&b);
    be32(&mut loca, glyf.len() as u32);

    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0);
    be16(&mut head, 1000); // units per em
    head.extend([0u8; 16]); // created, modified
    for v in [0, 0, 600, 700] {
        be16(&mut head, v);
    }
    be16(&mut head, 0);
    be16(&mut head, 8);
    be16(&mut head, 2);
    be16(&mut head, 1); // long loca
    be16(&mut head, 0);

    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    for v in [800, -200, 0, 700, 0, 0, 600, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut hhea, v);
    }
    hhea.truncate(36);
    // numberOfHMetrics is the last field.
    hhea[34..36].copy_from_slice(&3u16.to_be_bytes());

    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    be16(&mut maxp, 3);
    for v in [8, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut maxp, v);
    }

    let mut hmtx = Vec::new();
    for (adv, lsb) in [(500, 0), (700, 0), (500, 0)] {
        be16(&mut hmtx, adv);
        be16(&mut hmtx, lsb);
    }

    let mut cmap = Vec::new();
    be16(&mut cmap, 0);
    be16(&mut cmap, 1);
    be16(&mut cmap, 3);
    be16(&mut cmap, 1);
    be32(&mut cmap, 12);
    for v in [4, 32, 0, 4, 4, 1, 0, 66, 0xFFFFu16 as i32, 0, 65, 0xFFFFu16 as i32, -64, 1, 0, 0] {
        be16(&mut cmap, v);
    }

    let tables: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    be16(&mut out, tables.len() as i32);
    be16(&mut out, 64);
    be16(&mut out, 2);
    be16(&mut out, 48);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        be32(&mut out, 0);
        be32(&mut out, offset as u32);
        be32(&mut out, data.len() as u32);
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend(body);
    out
}
