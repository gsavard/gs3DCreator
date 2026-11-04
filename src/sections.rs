//! Sections planes (2D) : îlots, distances et placement des connecteurs.

use geo::{
    Area, BooleanOps, Buffer, Centroid, Contains, Distance, Euclidean, InteriorPoint, LineString, MultiPolygon, Point,
    Polygon,
};
use manifold3d::Manifold;

pub type Poly = Polygon<f64>;

/// Îlots (polygones avec trous) de la section horizontale à la hauteur z.
pub fn ilots(m_rot: &Manifold, z: f64) -> Vec<Poly> {
    let mut ext: Vec<Poly> = Vec::new();
    let mut trous: Vec<Poly> = Vec::new();
    for c in m_rot.slice_at_z(z) {
        if c.len() < 3 {
            continue;
        }
        let ls: LineString<f64> = c.iter().map(|p| (p[0], p[1])).collect();
        let p = Polygon::new(ls, vec![]);
        let a = p.signed_area();
        if a.abs() <= 1e-6 {
            continue;
        }
        if a > 0.0 {
            ext.push(p);
        } else {
            trous.push(p);
        }
    }
    let mut res = Vec::new();
    for o in ext {
        let interieurs: Vec<LineString<f64>> = trous
            .iter()
            .filter(|h| h.interior_point().map(|pt| o.contains(&pt)).unwrap_or(false))
            .map(|h| h.exterior().clone())
            .collect();
        res.push(Polygon::new(o.exterior().clone(), interieurs));
    }
    res.into_iter().filter(|p| p.unsigned_area() > 0.5).collect()
}

pub fn aire(p: &Poly) -> f64 {
    p.unsigned_area()
}

pub fn centre(p: &Poly) -> [f64; 2] {
    p.centroid().map(|c| [c.x(), c.y()]).unwrap_or([0.0, 0.0])
}

pub fn bornes(p: &Poly) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for c in p.exterior().coords() {
        b[0] = b[0].min(c.x);
        b[1] = b[1].min(c.y);
        b[2] = b[2].max(c.x);
        b[3] = b[3].max(c.y);
    }
    b
}

pub fn aire_intersection(a: &Poly, b: &Poly) -> f64 {
    a.intersection(b).unsigned_area()
}

/// Distance du point au polygone (0 s'il est à l'intérieur).
pub fn distance(p: &Poly, xy: [f64; 2]) -> f64 {
    Euclidean.distance(&Point::new(xy[0], xy[1]), p)
}

/// Distance du point au contour (extérieur et trous).
pub fn distance_contour(p: &Poly, xy: [f64; 2]) -> f64 {
    let pt = Point::new(xy[0], xy[1]);
    std::iter::once(p.exterior())
        .chain(p.interiors().iter())
        .map(|ls| Euclidean.distance(&pt, ls))
        .fold(f64::INFINITY, f64::min)
}

pub fn plus_proche(polys: &[Poly], xy: [f64; 2]) -> Option<usize> {
    polys
        .iter()
        .enumerate()
        .min_by(|a, b| distance(a.1, xy).total_cmp(&distance(b.1, xy)))
        .map(|(i, _)| i)
}

/// Pôle d'inaccessibilité : le point le plus éloigné du contour.
pub fn pole(p: &Poly) -> [f64; 2] {
    match polylabel::polylabel(p, &0.1) {
        Ok(pt) => [pt.x(), pt.y()],
        Err(_) => centre(p),
    }
}

/// Choisit 1 ou 2 positions de connecteurs (2 = anti-rotation) dans une section.
pub fn positions(poly: &Poly, r_trou: f64, paroi: f64, nmax: usize) -> Vec<[f64; 2]> {
    let erode: MultiPolygon<f64> = poly.buffer(-(r_trou + paroi));
    let Some(zone) = erode
        .0
        .iter()
        .max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))
    else {
        return vec![];
    };
    if zone.unsigned_area() <= 0.0 {
        return vec![];
    }
    let p1 = pole(zone);
    let mut pts = vec![p1];
    if nmax >= 2 {
        let mut meilleur = (0.0, [0.0, 0.0]);
        for c in zone.exterior().coords() {
            let d = ((c.x - p1[0]).powi(2) + (c.y - p1[1]).powi(2)).sqrt();
            if d > meilleur.0 {
                meilleur = (d, [c.x, c.y]);
            }
        }
        if meilleur.0 > 3.5 * r_trou {
            let q = meilleur.1;
            let p2 = [p1[0] + 0.85 * (q[0] - p1[0]), p1[1] + 0.85 * (q[1] - p1[1])];
            if zone.contains(&Point::new(p2[0], p2[1])) {
                pts.push(p2);
            }
        }
    }
    pts
}
