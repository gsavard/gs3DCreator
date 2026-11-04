//! Maillages triangulés : import (STL, OBJ, glTF/GLB, PLY, 3MF, OFF), réparation,
//! transformations, conversions vers/depuis Manifold et export (STL, 3MF, GLB).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use manifold3d::Manifold;
use nalgebra::{Matrix4, Vector3};

pub type V3 = Vector3<f64>;
pub type M4 = Matrix4<f64>;

/// Formats acceptés à l'import.
pub const FORMATS: &[&str] = &["stl", "obj", "glb", "gltf", "ply", "3mf", "off"];

#[derive(Clone, Debug, Default)]
pub struct Maillage {
    pub sommets: Vec<[f64; 3]>,
    pub faces: Vec<[u32; 3]>,
}

impl Maillage {
    pub fn bornes(&self) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &self.sommets {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        if self.sommets.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (lo, hi)
    }

    pub fn etendue(&self) -> [f64; 3] {
        let (lo, hi) = self.bornes();
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }

    pub fn translater(&mut self, t: [f64; 3]) {
        for v in &mut self.sommets {
            for i in 0..3 {
                v[i] += t[i];
            }
        }
    }

    pub fn mettre_a_echelle(&mut self, s: f64) {
        for v in &mut self.sommets {
            for c in v.iter_mut() {
                *c *= s;
            }
        }
    }

    pub fn transformer(&mut self, m: &M4) {
        for v in &mut self.sommets {
            let p = m.transform_point(&nalgebra::Point3::new(v[0], v[1], v[2]));
            *v = [p.x, p.y, p.z];
        }
        // une transformation à déterminant négatif inverse l'orientation des faces
        if m.fixed_view::<3, 3>(0, 0).determinant() < 0.0 {
            for f in &mut self.faces {
                f.swap(1, 2);
            }
        }
    }

    pub fn ajouter(&mut self, autre: &Maillage) {
        let base = self.sommets.len() as u32;
        self.sommets.extend_from_slice(&autre.sommets);
        self.faces
            .extend(autre.faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
    }

    /// Volume signé (positif si les normales pointent vers l'extérieur).
    pub fn volume_signe(&self) -> f64 {
        let mut v = 0.0;
        for f in &self.faces {
            let a = V3::from(self.sommets[f[0] as usize]);
            let b = V3::from(self.sommets[f[1] as usize]);
            let c = V3::from(self.sommets[f[2] as usize]);
            v += a.dot(&b.cross(&c));
        }
        v / 6.0
    }

    /// Place le coin minimal de la boîte englobante à l'origine.
    pub fn poser_a_l_origine(&mut self) {
        let (lo, _) = self.bornes();
        self.translater([-lo[0], -lo[1], -lo[2]]);
    }
}

// ---------------------------------------------------------------------- géométrie
/// Matrice de rotation 4×4 qui amène le vecteur `a` sur le vecteur `b`.
pub fn aligner(a: V3, b: V3) -> M4 {
    let a = a.normalize();
    let b = b.normalize();
    let c = a.dot(&b);
    let mut m = M4::identity();
    if c > 1.0 - 1e-12 {
        return m;
    }
    let r = if c < -1.0 + 1e-12 {
        // rotation de 180° autour d'un axe perpendiculaire à a
        let mut axe = a.cross(&V3::x());
        if axe.norm() < 1e-6 {
            axe = a.cross(&V3::y());
        }
        let axe = axe.normalize();
        nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(axe), std::f64::consts::PI).into_inner()
    } else {
        let v = a.cross(&b);
        let k = nalgebra::Matrix3::new(0.0, -v.z, v.y, v.z, 0.0, -v.x, -v.y, v.x, 0.0);
        nalgebra::Matrix3::identity() + k + k * k * (1.0 / (1.0 + c))
    };
    m.fixed_view_mut::<3, 3>(0, 0).copy_from(&r);
    m
}

pub fn rotation(angle: f64, axe: V3) -> M4 {
    nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(axe), angle).to_homogeneous()
}

pub fn translation(t: [f64; 3]) -> M4 {
    M4::new_translation(&V3::from(t))
}

pub fn appliquer_pt(m: &M4, p: [f64; 3]) -> [f64; 3] {
    let q = m.transform_point(&nalgebra::Point3::new(p[0], p[1], p[2]));
    [q.x, q.y, q.z]
}

/// Applique une matrice 4×4 à un Manifold.
pub fn appliquer(m: &Manifold, t: &M4) -> Manifold {
    let a = [
        t[(0, 0)],
        t[(1, 0)],
        t[(2, 0)],
        t[(0, 1)],
        t[(1, 1)],
        t[(2, 1)],
        t[(0, 2)],
        t[(1, 2)],
        t[(2, 2)],
        t[(0, 3)],
        t[(1, 3)],
        t[(2, 3)],
    ];
    m.transform(&a)
}

// ---------------------------------------------------------------------- conversions
pub fn vers_manifold(tm: &Maillage) -> Result<Manifold> {
    let verts: Vec<f64> = tm.sommets.iter().flat_map(|v| v.iter().copied()).collect();
    let tris: Vec<u64> = tm.faces.iter().flat_map(|f| f.iter().map(|&i| i as u64)).collect();
    let erreur = || {
        anyhow!(
            "Le maillage n'est pas fermé (non manifold). Répare-le d'abord, \
             par exemple avec la 3D Print Toolbox de Blender."
        )
    };
    let m = Manifold::from_mesh_f64(&verts, 3, &tris).map_err(|_| erreur())?;
    if m.status().is_err() || m.is_empty() {
        return Err(erreur());
    }
    Ok(m)
}

pub fn vers_maillage(m: &Manifold) -> Maillage {
    let (props, n, tris) = m.to_mesh_f64();
    let sommets = props.chunks(n).map(|c| [c[0], c[1], c[2]]).collect();
    let faces = tris
        .chunks(3)
        .map(|c| [c[0] as u32, c[1] as u32, c[2] as u32])
        .collect();
    Maillage { sommets, faces }
}

// ---------------------------------------------------------------------- requêtes
/// Distance jusqu'à la paroi en partant d'un point, dans une direction (rayon).
pub fn epaisseur(m: &Manifold, origine: [f64; 3], dir: V3) -> f64 {
    let d = dir.normalize();
    let o = V3::from(origine) + d * 0.01;
    let fin = o + d * 1.0e5;
    let d = m
        .ray_cast([o.x, o.y, o.z], [fin.x, fin.y, fin.z])
        .iter()
        .map(|h| (V3::from(h.position) - o).norm())
        .filter(|d| *d > 1e-9)
        .fold(f64::INFINITY, f64::min);
    if d.is_finite() { d } else { 0.0 }
}

/// Distance du point à la surface la plus proche (recherche exhaustive).
pub fn distance_surface(tm: &Maillage, p: [f64; 3]) -> f64 {
    let p = V3::from(p);
    tm.faces
        .iter()
        .map(|f| {
            let a = V3::from(tm.sommets[f[0] as usize]);
            let b = V3::from(tm.sommets[f[1] as usize]);
            let c = V3::from(tm.sommets[f[2] as usize]);
            (point_triangle(p, a, b, c) - p).norm()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Point le plus proche de `p` sur le triangle abc (Ericson, Real-Time Collision Detection).
fn point_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(&ap);
    let d2 = ac.dot(&ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(&bp);
    let d4 = ac.dot(&bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(&cp);
    let d6 = ac.dot(&cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

// ---------------------------------------------------------------------- réparation
/// Fusionne les sommets confondus, retire les faces dégénérées ou en double,
/// rend l'orientation cohérente (normales vers l'extérieur) et bouche les trous.
pub fn reparer(tm: &mut Maillage) {
    fusionner_sommets(tm, 1e-6);
    nettoyer_faces(tm);
    retirer_sommets_orphelins(tm);
    orienter_faces(tm);
    boucher_trous(tm);
    if tm.volume_signe() < 0.0 {
        for f in &mut tm.faces {
            f.swap(1, 2);
        }
    }
}

fn fusionner_sommets(tm: &mut Maillage, tol: f64) {
    let mut index: HashMap<[i64; 3], u32> = HashMap::new();
    let mut nouveaux = Vec::new();
    let mut remap = Vec::with_capacity(tm.sommets.len());
    for v in &tm.sommets {
        let k = [
            (v[0] / tol).round() as i64,
            (v[1] / tol).round() as i64,
            (v[2] / tol).round() as i64,
        ];
        let id = *index.entry(k).or_insert_with(|| {
            nouveaux.push(*v);
            (nouveaux.len() - 1) as u32
        });
        remap.push(id);
    }
    tm.sommets = nouveaux;
    for f in &mut tm.faces {
        for i in f.iter_mut() {
            *i = remap[*i as usize];
        }
    }
}

fn nettoyer_faces(tm: &mut Maillage) {
    let mut vues = HashSet::new();
    let s = &tm.sommets;
    tm.faces.retain(|f| {
        if f[0] == f[1] || f[1] == f[2] || f[0] == f[2] {
            return false;
        }
        let a = V3::from(s[f[0] as usize]);
        let b = V3::from(s[f[1] as usize]);
        let c = V3::from(s[f[2] as usize]);
        if (b - a).cross(&(c - a)).norm() <= 1e-12 {
            return false;
        }
        let mut k = *f;
        k.sort_unstable();
        vues.insert(k)
    });
}

fn retirer_sommets_orphelins(tm: &mut Maillage) {
    let mut remap = vec![u32::MAX; tm.sommets.len()];
    let mut nouveaux = Vec::new();
    for f in &mut tm.faces {
        for i in f.iter_mut() {
            let k = *i as usize;
            if remap[k] == u32::MAX {
                remap[k] = nouveaux.len() as u32;
                nouveaux.push(tm.sommets[k]);
            }
            *i = remap[k];
        }
    }
    tm.sommets = nouveaux;
}

/// Rend l'orientation des faces cohérente dans chaque composante connexe,
/// puis oriente chaque composante pour que son volume soit positif.
fn orienter_faces(tm: &mut Maillage) {
    let n = tm.faces.len();
    let mut aretes: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, f) in tm.faces.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            aretes.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut vu = vec![false; n];
    for depart in 0..n {
        if vu[depart] {
            continue;
        }
        let mut comp = vec![depart];
        let mut file = VecDeque::from([depart]);
        vu[depart] = true;
        while let Some(i) = file.pop_front() {
            let f = tm.faces[i];
            for k in 0..3 {
                let (a, b) = (f[k], f[(k + 1) % 3]);
                let voisins = &aretes[&(a.min(b), a.max(b))];
                if voisins.len() != 2 {
                    continue; // arête de bord ou non manifold : on ne propage pas
                }
                for &j in voisins {
                    if j == i || vu[j] {
                        continue;
                    }
                    // une orientation cohérente parcourt l'arête partagée en sens inverse
                    let g = tm.faces[j];
                    let meme_sens = (0..3).any(|m| g[m] == a && g[(m + 1) % 3] == b);
                    if meme_sens {
                        tm.faces[j].swap(1, 2);
                    }
                    vu[j] = true;
                    comp.push(j);
                    file.push_back(j);
                }
            }
        }
        let vol: f64 = comp
            .iter()
            .map(|&i| {
                let f = tm.faces[i];
                let a = V3::from(tm.sommets[f[0] as usize]);
                let b = V3::from(tm.sommets[f[1] as usize]);
                let c = V3::from(tm.sommets[f[2] as usize]);
                a.dot(&b.cross(&c))
            })
            .sum();
        if vol < 0.0 {
            for &i in &comp {
                tm.faces[i].swap(1, 2);
            }
        }
    }
}

/// Bouche les trous : chaque boucle d'arêtes de bord est triangulée en éventail
/// autour de son barycentre.
fn boucher_trous(tm: &mut Maillage) {
    let mut compte: HashMap<(u32, u32), i32> = HashMap::new();
    for f in &tm.faces {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            *compte.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    // arêtes de bord orientées dans le sens opposé à leur face (sens du trou)
    let mut suivant: HashMap<u32, Vec<u32>> = HashMap::new();
    for f in &tm.faces {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            if compte[&(a.min(b), a.max(b))] == 1 {
                suivant.entry(b).or_default().push(a);
            }
        }
    }
    if suivant.is_empty() {
        return;
    }
    let mut nouvelles = Vec::new();
    let mut nouveaux_sommets = Vec::new();
    let base = tm.sommets.len() as u32;
    let departs: Vec<u32> = suivant.keys().copied().collect();
    for d in departs {
        while let Some(vs) = suivant.get_mut(&d) {
            let Some(mut cur) = vs.pop() else { break };
            let mut boucle = vec![d];
            let mut ok = false;
            for _ in 0..100_000 {
                if cur == d {
                    ok = true;
                    break;
                }
                boucle.push(cur);
                match suivant.get_mut(&cur).and_then(|v| v.pop()) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            if !ok || boucle.len() < 3 {
                continue;
            }
            if boucle.len() == 3 {
                nouvelles.push([boucle[0], boucle[1], boucle[2]]);
                continue;
            }
            let mut c = V3::zeros();
            for &i in &boucle {
                c += V3::from(tm.sommets[i as usize]);
            }
            c /= boucle.len() as f64;
            let ic = base + nouveaux_sommets.len() as u32;
            nouveaux_sommets.push([c.x, c.y, c.z]);
            for k in 0..boucle.len() {
                nouvelles.push([boucle[k], boucle[(k + 1) % boucle.len()], ic]);
            }
        }
    }
    tm.sommets.extend(nouveaux_sommets);
    tm.faces.extend(nouvelles);
}

// ---------------------------------------------------------------------- import
pub fn charger_fichier(chemin: &Path) -> Result<Maillage> {
    let ext = extension(chemin);
    let tm = match ext.as_str() {
        "stl" => lire_stl(chemin)?,
        "obj" => lire_obj(chemin)?,
        "glb" | "gltf" => lire_gltf(chemin)?,
        "ply" => lire_ply(chemin)?,
        "3mf" => lire_3mf(chemin)?,
        "off" => lire_off(chemin)?,
        e => bail!(
            "Format {} non pris en charge. Utilise STL, OBJ ou GLB.",
            if e.is_empty() { "inconnu" } else { e }
        ),
    };
    if tm.faces.is_empty() {
        bail!("Le fichier ne contient aucun triangle.");
    }
    Ok(tm)
}

pub fn extension(chemin: &Path) -> String {
    chemin
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn lire_stl(chemin: &Path) -> Result<Maillage> {
    let mut f = BufReader::new(File::open(chemin)?);
    let m = stl_io::read_stl(&mut f).context("Lecture du STL impossible")?;
    Ok(Maillage {
        sommets: m
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        faces: m
            .faces
            .iter()
            .map(|t| [t.vertices[0] as u32, t.vertices[1] as u32, t.vertices[2] as u32])
            .collect(),
    })
}

fn lire_obj(chemin: &Path) -> Result<Maillage> {
    let opts = tobj::LoadOptions {
        triangulate: true,
        single_index: false,
        ..Default::default()
    };
    let (modeles, _) = tobj::load_obj(chemin, &opts).context("Lecture de l'OBJ impossible")?;
    let mut tm = Maillage::default();
    for m in modeles {
        let p = &m.mesh.positions;
        let part = Maillage {
            sommets: p.chunks(3).map(|c| [c[0] as f64, c[1] as f64, c[2] as f64]).collect(),
            faces: m.mesh.indices.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
        };
        tm.ajouter(&part);
    }
    Ok(tm)
}

fn lire_gltf(chemin: &Path) -> Result<Maillage> {
    let (doc, buffers, _) = gltf::import(chemin).context("Lecture du glTF/GLB impossible")?;
    let mut tm = Maillage::default();
    fn parcourir(n: gltf::Node, parent: M4, buffers: &[gltf::buffer::Data], tm: &mut Maillage) {
        let local = M4::from_column_slice(
            &n.transform()
                .matrix()
                .concat()
                .iter()
                .map(|&v| v as f64)
                .collect::<Vec<_>>(),
        );
        let monde = parent * local;
        if let Some(mesh) = n.mesh() {
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let r = prim.reader(|b| Some(&buffers[b.index()]));
                let Some(pos) = r.read_positions() else { continue };
                let sommets: Vec<[f64; 3]> = pos.map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
                let idx: Vec<u32> = match r.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..sommets.len() as u32).collect(),
                };
                let mut part = Maillage {
                    sommets,
                    faces: idx
                        .chunks(3)
                        .filter(|c| c.len() == 3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect(),
                };
                part.transformer(&monde);
                tm.ajouter(&part);
            }
        }
        for c in n.children() {
            parcourir(c, monde, buffers, tm);
        }
    }
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    match scene {
        Some(s) => {
            for n in s.nodes() {
                parcourir(n, M4::identity(), &buffers, &mut tm);
            }
        }
        None => {
            for n in doc.nodes() {
                parcourir(n, M4::identity(), &buffers, &mut tm);
            }
        }
    }
    Ok(tm)
}

fn lire_ply(chemin: &Path) -> Result<Maillage> {
    use ply_rs::{parser::Parser, ply::Property};
    let mut f = BufReader::new(File::open(chemin)?);
    let ply = Parser::<ply_rs::ply::DefaultElement>::new()
        .read_ply(&mut f)
        .map_err(|e| anyhow!("Lecture du PLY impossible : {e}"))?;
    let num = |p: Option<&Property>| -> f64 {
        match p {
            Some(Property::Float(v)) => *v as f64,
            Some(Property::Double(v)) => *v,
            Some(Property::Int(v)) => *v as f64,
            Some(Property::UInt(v)) => *v as f64,
            Some(Property::Short(v)) => *v as f64,
            Some(Property::UShort(v)) => *v as f64,
            Some(Property::Char(v)) => *v as f64,
            Some(Property::UChar(v)) => *v as f64,
            _ => 0.0,
        }
    };
    let mut tm = Maillage::default();
    if let Some(vs) = ply.payload.get("vertex") {
        tm.sommets = vs
            .iter()
            .map(|v| [num(v.get("x")), num(v.get("y")), num(v.get("z"))])
            .collect();
    }
    if let Some(fs) = ply.payload.get("face") {
        for f in fs {
            let p = f.get("vertex_indices").or_else(|| f.get("vertex_index"));
            let idx: Vec<u32> = match p {
                Some(Property::ListInt(v)) => v.iter().map(|&i| i as u32).collect(),
                Some(Property::ListUInt(v)) => v.clone(),
                Some(Property::ListShort(v)) => v.iter().map(|&i| i as u32).collect(),
                Some(Property::ListUShort(v)) => v.iter().map(|&i| i as u32).collect(),
                Some(Property::ListUChar(v)) => v.iter().map(|&i| i as u32).collect(),
                Some(Property::ListChar(v)) => v.iter().map(|&i| i as u32).collect(),
                _ => continue,
            };
            for k in 1..idx.len().saturating_sub(1) {
                tm.faces.push([idx[0], idx[k], idx[k + 1]]);
            }
        }
    }
    Ok(tm)
}

fn lire_off(chemin: &Path) -> Result<Maillage> {
    let mut txt = String::new();
    File::open(chemin)?.read_to_string(&mut txt)?;
    let mut jetons = txt
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(|l| l.split_whitespace())
        .map(str::to_owned);
    let entete = jetons.next().ok_or_else(|| anyhow!("OFF vide"))?;
    let lire = |j: &mut dyn Iterator<Item = String>| -> Result<f64> {
        j.next()
            .ok_or_else(|| anyhow!("OFF tronqué"))?
            .parse::<f64>()
            .map_err(|e| anyhow!("OFF invalide : {e}"))
    };
    let (nv, nf) = if entete == "OFF" {
        let nv = lire(&mut jetons)? as usize;
        let nf = lire(&mut jetons)? as usize;
        lire(&mut jetons)?;
        (nv, nf)
    } else if let Some(reste) = entete.strip_prefix("OFF") {
        let nv = reste.parse::<usize>()?;
        let nf = lire(&mut jetons)? as usize;
        lire(&mut jetons)?;
        (nv, nf)
    } else {
        bail!("En-tête OFF absent");
    };
    let mut tm = Maillage::default();
    for _ in 0..nv {
        tm.sommets
            .push([lire(&mut jetons)?, lire(&mut jetons)?, lire(&mut jetons)?]);
    }
    for _ in 0..nf {
        let n = lire(&mut jetons)? as usize;
        let idx: Vec<u32> = (0..n)
            .map(|_| lire(&mut jetons).map(|v| v as u32))
            .collect::<Result<_>>()?;
        for k in 1..n.saturating_sub(1) {
            tm.faces.push([idx[0], idx[k], idx[k + 1]]);
        }
    }
    Ok(tm)
}

fn lire_3mf(chemin: &Path) -> Result<Maillage> {
    let mut zip = zip::ZipArchive::new(File::open(chemin)?).context("3MF illisible (zip)")?;
    let mut tm = Maillage::default();
    let noms: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            zip.by_index(i)
                .ok()
                .map(|f| f.name().map(|n| n.to_string()).unwrap_or_default())
        })
        .filter(|n| n.to_ascii_lowercase().ends_with(".model"))
        .collect();
    for nom in noms {
        let mut xml = String::new();
        zip.by_name(&nom)?.read_to_string(&mut xml)?;
        let mut part = Maillage::default();
        // analyse minimale : on lit les balises <vertex>, <triangle> et <mesh> (avec ou sans préfixe)
        for balise in xml.split('<').skip(1) {
            let fin = balise.find('>').unwrap_or(balise.len());
            let corps = &balise[..fin];
            let nom_b = corps.split_whitespace().next().unwrap_or("");
            let local = nom_b.rsplit(':').next().unwrap_or(nom_b);
            let att = |k: &str| -> f64 {
                for motif in [format!(" {k}=\""), format!(" {k}='")] {
                    if let Some(i) = corps.find(&motif) {
                        let reste = &corps[i + motif.len()..];
                        let q = motif.chars().last().unwrap();
                        if let Some(j) = reste.find(q) {
                            return reste[..j].trim().parse().unwrap_or(0.0);
                        }
                    }
                }
                0.0
            };
            match local {
                "mesh" => {
                    tm.ajouter(&part);
                    part = Maillage::default();
                }
                "vertex" => part.sommets.push([att("x"), att("y"), att("z")]),
                "triangle" => part.faces.push([att("v1") as u32, att("v2") as u32, att("v3") as u32]),
                _ => {}
            }
        }
        tm.ajouter(&part);
    }
    Ok(tm)
}

// ---------------------------------------------------------------------- export
pub fn ecrire_stl(tm: &Maillage, chemin: &Path) -> Result<()> {
    let mut f = std::io::BufWriter::new(File::create(chemin)?);
    let mut entete = [0u8; 80];
    let txt = b"gs3Dcreator";
    entete[..txt.len()].copy_from_slice(txt);
    f.write_all(&entete)?;
    f.write_all(&(tm.faces.len() as u32).to_le_bytes())?;
    for t in &tm.faces {
        let a = V3::from(tm.sommets[t[0] as usize]);
        let b = V3::from(tm.sommets[t[1] as usize]);
        let c = V3::from(tm.sommets[t[2] as usize]);
        let n = (b - a).cross(&(c - a));
        let n = if n.norm() > 0.0 { n.normalize() } else { n };
        for v in [n, a, b, c] {
            for k in 0..3 {
                f.write_all(&(v[k] as f32).to_le_bytes())?;
            }
        }
        f.write_all(&[0, 0])?;
    }
    Ok(())
}

/// 3MF multi-objets (un objet par pièce, nommé).
pub fn ecrire_3mf(objets: &[(String, Maillage)], chemin: &Path) -> Result<()> {
    use std::fmt::Write as _;
    let mut modele = String::with_capacity(1 << 20);
    modele.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"fr-CA\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
         <metadata name=\"Application\">gs3Dcreator</metadata>\n<resources>\n",
    );
    for (i, (nom, tm)) in objets.iter().enumerate() {
        let _ = writeln!(
            modele,
            "<object id=\"{}\" name=\"{}\" type=\"model\"><mesh><vertices>",
            i + 1,
            xml_echap(nom)
        );
        for v in &tm.sommets {
            let _ = writeln!(
                modele,
                "<vertex x=\"{:.5}\" y=\"{:.5}\" z=\"{:.5}\"/>",
                v[0], v[1], v[2]
            );
        }
        modele.push_str("</vertices><triangles>\n");
        for t in &tm.faces {
            let _ = writeln!(modele, "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
        }
        modele.push_str("</triangles></mesh></object>\n");
    }
    modele.push_str("</resources>\n<build>\n");
    for i in 0..objets.len() {
        let _ = writeln!(modele, "<item objectid=\"{}\"/>", i + 1);
    }
    modele.push_str("</build>\n</model>\n");

    let mut z = zip::ZipWriter::new(File::create(chemin)?);
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("[Content_Types].xml", o)?;
    z.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n",
    )?;
    z.start_file("_rels/.rels", o)?;
    z.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n",
    )?;
    z.start_file("3D/3dmodel.model", o)?;
    z.write_all(modele.as_bytes())?;
    z.finish()?;
    Ok(())
}

fn xml_echap(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// GLB minimal : un nœud nommé par maillage (positions + indices).
pub fn ecrire_glb(objets: &[(String, Maillage)], chemin: &Path) -> Result<()> {
    use serde_json::json;
    let mut bin: Vec<u8> = Vec::new();
    let (mut vues, mut acces, mut maillages, mut noeuds) = (vec![], vec![], vec![], vec![]);
    for (i, (nom, tm)) in objets.iter().enumerate() {
        let (lo, hi) = tm.bornes();
        let off = bin.len();
        for v in &tm.sommets {
            for c in v {
                bin.extend_from_slice(&(*c as f32).to_le_bytes());
            }
        }
        vues.push(json!({"buffer": 0, "byteOffset": off, "byteLength": bin.len() - off, "target": 34962}));
        acces.push(
            json!({"bufferView": 2 * i, "componentType": 5126, "count": tm.sommets.len(),
                          "type": "VEC3", "min": lo.map(|v| v as f32), "max": hi.map(|v| v as f32)}),
        );
        let off = bin.len();
        for t in &tm.faces {
            for k in t {
                bin.extend_from_slice(&k.to_le_bytes());
            }
        }
        vues.push(json!({"buffer": 0, "byteOffset": off, "byteLength": bin.len() - off, "target": 34963}));
        acces.push(
            json!({"bufferView": 2 * i + 1, "componentType": 5125, "count": tm.faces.len() * 3, "type": "SCALAR"}),
        );
        maillages.push(json!({"name": nom, "primitives": [{"attributes": {"POSITION": 2 * i}, "indices": 2 * i + 1}]}));
        noeuds.push(json!({"name": nom, "mesh": i}));
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": "gs3Dcreator"},
        "scene": 0,
        "scenes": [{"nodes": (0..objets.len()).collect::<Vec<_>>()}],
        "nodes": noeuds, "meshes": maillages, "accessors": acces, "bufferViews": vues,
        "buffers": [{"byteLength": bin.len()}],
    });
    let mut js = serde_json::to_vec(&doc)?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut f = std::io::BufWriter::new(File::create(chemin)?);
    f.write_all(b"glTF")?;
    f.write_all(&2u32.to_le_bytes())?;
    f.write_all(&(total as u32).to_le_bytes())?;
    f.write_all(&(js.len() as u32).to_le_bytes())?;
    f.write_all(b"JSON")?;
    f.write_all(&js)?;
    f.write_all(&(bin.len() as u32).to_le_bytes())?;
    f.write_all(b"BIN\0")?;
    f.write_all(&bin)?;
    Ok(())
}
