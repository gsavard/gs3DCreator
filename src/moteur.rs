//! Moteur de découpe par gabarits de personnage.
//!
//! - `charger()`  : import, réparation, orientation (Z vers le haut), mise à l'échelle
//! - `reperes()`  : détection heuristique cou / hanches / taille / épaules
//! - `decouper()` : applique des coupes localisées (gabarits 6, 5, 4a, 4b, 3, 2 pièces)
//! - `exporter()` : STL orientés, rotules/goujons, 3MF, ZIP et aperçu GLB

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use manifold3d::{CrossSection, Manifold};
use serde::{Deserialize, Serialize};

use crate::maillage::{self as ml, M4, Maillage, V3, aligner, appliquer};
use crate::sections::{self as sc, Poly};

pub const SEGMENTS: i32 = 48;
pub const EPS: f64 = 0.05;

pub struct Gabarit {
    pub cle: &'static str,
    #[allow(dead_code)]
    pub nom: &'static str,
    pub tete: bool,
    pub bras: bool,
    /// "separees", "ensemble" ou "" (aucune coupe de jambes)
    pub jambes: &'static str,
}

pub const GABARITS: &[Gabarit] = &[
    Gabarit {
        cle: "6",
        nom: "6 pièces",
        tete: true,
        bras: true,
        jambes: "separees",
    },
    Gabarit {
        cle: "5",
        nom: "5 pièces",
        tete: false,
        bras: true,
        jambes: "separees",
    },
    Gabarit {
        cle: "4a",
        nom: "4 pièces (bras)",
        tete: false,
        bras: true,
        jambes: "ensemble",
    },
    Gabarit {
        cle: "4b",
        nom: "4 pièces (jambes)",
        tete: true,
        bras: false,
        jambes: "separees",
    },
    Gabarit {
        cle: "3",
        nom: "3 pièces",
        tete: true,
        bras: false,
        jambes: "ensemble",
    },
    Gabarit {
        cle: "2",
        nom: "2 pièces",
        tete: true,
        bras: false,
        jambes: "",
    },
];

pub fn gabarit(cle: &str) -> Option<&'static Gabarit> {
    GABARITS.iter().find(|g| g.cle == cle)
}

pub fn nom_lisible(label: &str) -> String {
    match label {
        "tete" => "Tête".into(),
        "torse" => "Torse".into(),
        "bras_1" => "Bras 1".into(),
        "bras_2" => "Bras 2".into(),
        "jambe_1" => "Jambe 1".into(),
        "jambe_2" => "Jambe 2".into(),
        "jambes" => "Jambes".into(),
        "fragment" => "Fragment".into(),
        l if l.starts_with("extra_") => format!("Pièce {}", l.rsplit('_').next().unwrap_or("")),
        l => l.into(),
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// rotule | goujon | tenon | aucun
    pub connecteur: String,
    pub rayon: f64,
    pub jeu: f64,
    pub profondeur: f64,
    pub paroi: f64,
    pub par_ilot: usize,
    pub volume: [f64; 3],
}

impl Default for Config {
    fn default() -> Self {
        Config {
            connecteur: "rotule".into(),
            rayon: 2.5,
            jeu: 0.2,
            profondeur: 6.0,
            paroi: 1.6,
            par_ilot: 2,
            volume: [250.0; 3],
        }
    }
}

#[derive(Clone, Debug)]
pub struct FaceCoupe {
    pub normale_ext: V3,
    pub aire: f64,
    pub a_tenon: bool,
}

#[derive(Clone)]
pub struct Piece {
    pub m: Manifold,
    pub label: String,
    pub faces: Vec<FaceCoupe>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coupe {
    pub label: String,
    pub point: [f64; 3],
    pub normale: [f64; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct Reperes {
    pub jambes_xy: Vec<[f64; 2]>,
    #[serde(rename = "H")]
    pub h: f64,
    pub z_cou: f64,
    pub z_hanche: f64,
    pub z_taille: f64,
    pub axe_lat: String,
    pub mid_lat: f64,
    pub demi_largeur: f64,
    #[serde(skip)]
    pub piste: Vec<[f64; 3]>,
    pub avertissements: Vec<String>,
}

// ------------------------------------------------------------------ chargement
/// Charge, répare, oriente (Z vers le haut), met à l'échelle et centre le modèle.
/// Retourne le maillage et l'axe vertical retenu.
pub fn charger(chemin: &Path, axe: &str, hauteur: Option<f64>) -> Result<(Maillage, String)> {
    let mut tm = ml::charger_fichier(chemin)?;
    ml::reparer(&mut tm);
    let mut axe = axe.to_string();
    if axe == "auto" {
        let (lo, hi) = tm.bornes();
        let max = lo.iter().chain(hi.iter()).fold(0.0f64, |m, v| m.max(v.abs()));
        let ext = ml::extension(chemin);
        axe = if ext == "glb" || ext == "gltf" || max <= 1.5 {
            "y".into()
        } else {
            "z".into()
        };
    }
    match axe.as_str() {
        "y" => tm.transformer(&ml::rotation(PI / 2.0, V3::x())),
        "x" => tm.transformer(&ml::rotation(-PI / 2.0, V3::y())),
        _ => {}
    }
    if let Some(h) = hauteur {
        let e = tm.etendue()[2];
        if h > 0.0 && e > 0.0 {
            tm.mettre_a_echelle(h / e);
        }
    }
    let (lo, hi) = tm.bornes();
    tm.translater([-(lo[0] + hi[0]) / 2.0, -(lo[1] + hi[1]) / 2.0, -lo[2]]);
    Ok((tm, axe))
}

// ------------------------------------------------------------------ repères
fn meilleur_recouvrement(polys: &[Poly], reference: &Poly) -> usize {
    let mut best = (0usize, -1.0);
    for (i, p) in polys.iter().enumerate() {
        let a = sc::aire_intersection(p, reference);
        if a > best.1 {
            best = (i, a);
        }
    }
    best.0
}

fn suivre(polys: &[Poly], reference: &Poly) -> usize {
    let i = meilleur_recouvrement(polys, reference);
    if sc::aire_intersection(&polys[i], reference) == 0.0 {
        sc::plus_proche(polys, sc::centre(reference)).unwrap_or(i)
    } else {
        i
    }
}

/// Détecte cou, hanches (entrejambe), taille et épaules par analyse des sections.
pub fn reperes(m: &Manifold, h: f64) -> Result<Reperes> {
    let n = 96;
    let niveaux: Vec<f64> = (0..n)
        .map(|i| 0.02 * h + (0.96 * h) * i as f64 / (n - 1) as f64)
        .collect();
    let s: Vec<Vec<Poly>> = niveaux.iter().map(|&z| sc::ilots(m, z)).collect();
    let mut avert = Vec::new();

    // --- entrejambe : on suit deux jambes depuis le bas jusqu'à leur fusion
    let mut z_hanche = None;
    let mut entrejambe = None;
    for (k, &z) in niveaux.iter().enumerate() {
        if z > 0.40 * h {
            break;
        }
        if z < 0.08 * h || s[k].len() < 2 {
            continue;
        }
        let mut tri: Vec<&Poly> = s[k].iter().collect();
        tri.sort_by(|a, b| sc::aire(b).total_cmp(&sc::aire(a)));
        let (mut a, mut b) = (tri[0].clone(), tri[1].clone());
        if sc::aire(&b) < 0.3 * sc::aire(&a) {
            continue;
        }
        for j in k + 1..niveaux.len() {
            if niveaux[j] > 0.75 * h || s[j].is_empty() {
                break;
            }
            let na = suivre(&s[j], &a);
            let nb = suivre(&s[j], &b);
            if na == nb {
                entrejambe = Some(niveaux[j]);
                z_hanche = Some(niveaux[j - 1]);
                break;
            }
            a = s[j][na].clone();
            b = s[j][nb].clone();
        }
        break;
    }
    let (entrejambe, z_hanche) = match (entrejambe, z_hanche) {
        (Some(e), Some(zh)) => (e, zh),
        _ => {
            avert.push("Jambes non détectées (pose fermée, robe ou socle) : niveaux par défaut.".to_string());
            (0.45 * h, 0.42 * h)
        }
    };
    let z_taille = (entrejambe + 0.08 * h).min(0.62 * h);

    // --- tronc : on suit l'îlot du bassin vers le haut
    let cible = entrejambe + 0.03 * h;
    let k0 = (0..n)
        .min_by(|&a, &b| (niveaux[a] - cible).abs().total_cmp(&(niveaux[b] - cible).abs()))
        .unwrap_or(0);
    if s[k0].is_empty() {
        bail!(
            "Section vide au niveau du bassin : le modèle n'est peut-être pas debout. \
             Essaie un autre axe vertical."
        );
    }
    let mut cur = s[k0]
        .iter()
        .max_by(|a, b| sc::aire(a).total_cmp(&sc::aire(b)))
        .unwrap()
        .clone();
    // (z, aire, centre, bornes)
    let mut piste: BTreeMap<usize, (f64, f64, [f64; 2], [f64; 4])> = BTreeMap::new();
    for j in k0..n {
        if s[j].is_empty() {
            break;
        }
        let i = suivre(&s[j], &cur);
        cur = s[j][i].clone();
        piste.insert(j, (niveaux[j], sc::aire(&cur), sc::centre(&cur), sc::bornes(&cur)));
    }

    // --- cou : un vrai creux — tête plus large au-dessus, épaules plus larges en dessous
    let pts: Vec<_> = piste.values().cloned().collect();
    let mut cands: Vec<(f64, f64)> = Vec::new();
    for &(z, a, _, _) in &pts {
        if !(0.60 * h..=0.95 * h).contains(&z) {
            continue;
        }
        let dessus = pts
            .iter()
            .filter(|v| z < v.0 && v.0 <= z + 0.20 * h)
            .map(|v| v.1)
            .fold(f64::NAN, f64::max);
        let dessous = pts
            .iter()
            .filter(|v| z - 0.15 * h <= v.0 && v.0 < z)
            .map(|v| v.1)
            .fold(f64::NAN, f64::max);
        if dessus.is_nan() || dessous.is_nan() {
            continue;
        }
        let voisin = dessus.min(dessous);
        if voisin >= 1.25 * a {
            cands.push((a / voisin, z));
        }
    }
    let z_cou = match cands
        .iter()
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
    {
        Some(c) => c.1,
        None => {
            avert.push("Cou non détecté : niveau par défaut, ajuste-le avec le curseur.".to_string());
            0.82 * h
        }
    };

    // --- épaules : largeur du tronc au niveau de l'abdomen
    let za = (z_taille + z_cou) / 2.0;
    let ja = *piste
        .keys()
        .min_by(|&&a, &&b| (piste[&a].0 - za).abs().total_cmp(&(piste[&b].0 - za).abs()))
        .unwrap();
    let [bx0, by0, bx1, by1] = piste[&ja].3;
    let (axe_lat, mid, demi) = if (bx1 - bx0) >= (by1 - by0) {
        ("x", (bx0 + bx1) / 2.0, (bx1 - bx0) / 2.0)
    } else {
        ("y", (by0 + by1) / 2.0, (by1 - by0) / 2.0)
    };

    // --- centre de chaque jambe au niveau des hanches
    let mut jambes_xy = Vec::new();
    let mut secs = sc::ilots(m, z_hanche - 0.5);
    secs.sort_by(|a, b| sc::aire(b).total_cmp(&sc::aire(a)));
    if secs.len() >= 2 && sc::aire(&secs[1]) >= 0.3 * sc::aire(&secs[0]) {
        jambes_xy = vec![sc::centre(&secs[0]), sc::centre(&secs[1])];
        let i = if axe_lat == "x" { 0 } else { 1 };
        jambes_xy.sort_by(|a, b| a[i].total_cmp(&b[i]));
    }

    Ok(Reperes {
        jambes_xy,
        h,
        z_cou,
        z_hanche,
        z_taille,
        axe_lat: axe_lat.into(),
        mid_lat: mid,
        demi_largeur: demi,
        piste: piste.values().map(|v| [v.0, v.2[0], v.2[1]]).collect(),
        avertissements: avert,
    })
}

fn centre_a(rep: &Reperes, z: f64) -> [f64; 2] {
    rep.piste
        .iter()
        .min_by(|a, b| (a[0] - z).abs().total_cmp(&(b[0] - z).abs()))
        .map(|v| [v[1], v[2]])
        .unwrap_or([0.0, 0.0])
}

// ------------------------------------------------------------------ gabarits
fn r3(v: [f64; 3]) -> [f64; 3] {
    v.map(|x| (x * 1000.0).round() / 1000.0)
}

/// Coupes proposées pour un gabarit. Chaque coupe : label, point (centre du plan,
/// aussi utilisé comme graine) et normale orientée vers la pièce à détacher.
pub fn coupes_defaut(rep: &Reperes, g: &Gabarit) -> Vec<Coupe> {
    let h = rep.h;
    let il = if rep.axe_lat == "x" { 0 } else { 1 };
    let io = 1 - il;
    let mut coupes = Vec::new();
    if g.tete {
        let c = centre_a(rep, rep.z_cou);
        coupes.push(Coupe {
            label: "tete".into(),
            point: r3([c[0], c[1], rep.z_cou]),
            normale: [0.0, 0.0, 1.0],
        });
    }
    if g.bras {
        let z_ep = rep.z_cou - 0.06 * h;
        let c = centre_a(rep, z_ep);
        for (k, s) in [(1, -1.0), (2, 1.0)] {
            let mut n = [0.0; 3];
            n[il] = s;
            let mut pt = [0.0, 0.0, z_ep];
            pt[il] = rep.mid_lat + s * rep.demi_largeur;
            pt[io] = c[io];
            coupes.push(Coupe {
                label: format!("bras_{k}"),
                point: r3(pt),
                normale: n,
            });
        }
    }
    match g.jambes {
        "separees" => {
            let mut xy = rep.jambes_xy.clone();
            if xy.len() < 2 {
                let c = centre_a(rep, rep.z_hanche);
                xy = [-1.0, 1.0]
                    .iter()
                    .map(|s| {
                        let mut v = c;
                        v[il] += s * rep.demi_largeur / 2.0;
                        v
                    })
                    .collect();
            }
            for (k, v) in xy.iter().enumerate() {
                coupes.push(Coupe {
                    label: format!("jambe_{}", k + 1),
                    point: r3([v[0], v[1], rep.z_hanche]),
                    normale: [0.0, 0.0, -1.0],
                });
            }
        }
        "ensemble" => {
            let c = centre_a(rep, rep.z_taille);
            coupes.push(Coupe {
                label: "jambes".into(),
                point: r3([c[0], c[1], rep.z_taille]),
                normale: [0.0, 0.0, -1.0],
            });
        }
        _ => {}
    }
    coupes
}

// ------------------------------------------------------------------ connecteurs
pub fn rayon_boule(cfg: &Config) -> f64 {
    (1.6 * cfg.rayon).max(3.0)
}

/// Boule à base aplatie (imprimable sans support).
pub fn boule(rb: f64) -> Manifold {
    Manifold::sphere(rb, SEGMENTS).trim_by_plane([0.0, 0.0, 1.0], -0.8 * rb)
}

pub fn cylindre(r: f64, h: f64, chanfrein: f64) -> Manifold {
    if chanfrein <= 0.0 {
        return Manifold::cylinder(h, r, r, SEGMENTS, false);
    }
    let c = chanfrein;
    let profil =
        CrossSection::from_polygons(&[vec![[0.0, 0.0], [r - c, 0.0], [r, c], [r, h - c], [r - c, h], [0.0, h]]]);
    Manifold::revolve(&profil, SEGMENTS, 360.0)
}

fn pos_monde(rinv: &M4, xy: [f64; 2], z0: f64) -> [f64; 3] {
    ml::appliquer_pt(rinv, [xy[0], xy[1], z0])
}

// ------------------------------------------------------------------ coupe localisée
/// Coupe `corps` par le plan ; du côté positif, ne garde que les composantes qui
/// touchent les îlots les plus proches des graines (une pièce par graine). Tout le
/// reste est recollé au corps. Retourne (pièces extraites, corps, avertissements).
fn coupe_locale(
    corps: Piece,
    normale: V3,
    point: V3,
    graines: &[V3],
    cfg: &Config,
    goujons: &mut Vec<f64>,
) -> (Vec<Piece>, Piece, Vec<String>) {
    let n = normale.normalize();
    let (pos, neg) = corps.m.split_by_plane([n.x, n.y, n.z], n.dot(&point));
    if pos.is_empty() || neg.is_empty() {
        return (
            vec![],
            corps,
            vec!["Un plan de coupe ne traverse pas le modèle ; coupe ignorée.".into()],
        );
    }
    let r = aligner(n, V3::z());
    let rinv = r.try_inverse().unwrap_or_else(M4::identity);
    let z0 = ml::appliquer_pt(&r, [point.x, point.y, point.z])[2];

    let comps: Vec<Manifold> = pos.decompose().into_iter().filter(|c| c.volume() > 0.5).collect();
    let mut contacts: Vec<(usize, Poly)> = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        for p in sc::ilots(&appliquer(c, &r), z0 + 0.02) {
            contacts.push((i, p));
        }
    }
    if contacts.is_empty() {
        return (
            vec![],
            corps,
            vec!["Aucune section exploitable sur un plan ; coupe ignorée.".into()],
        );
    }

    let mut choix: Vec<(usize, Vec<Poly>)> = Vec::new();
    for g in graines {
        let g2 = ml::appliquer_pt(&r, [g.x, g.y, g.z]);
        let (i, _) = contacts
            .iter()
            .min_by(|a, b| sc::distance(&a.1, [g2[0], g2[1]]).total_cmp(&sc::distance(&b.1, [g2[0], g2[1]])))
            .unwrap();
        if !choix.iter().any(|(k, _)| k == i) {
            let ps = contacts
                .iter()
                .filter(|(k, _)| k == i)
                .map(|(_, p)| p.clone())
                .collect();
            choix.push((*i, ps));
        }
    }

    let retour: Vec<Manifold> = comps
        .iter()
        .enumerate()
        .filter(|(i, _)| !choix.iter().any(|(k, _)| k == i))
        .map(|(_, c)| c.clone())
        .collect();
    let reste = if retour.is_empty() {
        neg
    } else {
        let mut tous = vec![neg];
        tous.extend(retour);
        Manifold::batch_union(&tous)
    };

    let m_ref = corps.m.clone();
    let tm_ref = ml::vers_maillage(&m_ref);
    let mut nouveau = Piece {
        m: reste,
        label: corps.label.clone(),
        faces: corps.faces.clone(),
    };
    let mut extraites = Vec::new();
    for (i, polys) in &choix {
        let mut part = comps[*i].clone();
        let aire: f64 = polys.iter().map(sc::aire).sum();
        let f_part = FaceCoupe {
            normale_ext: -n,
            aire,
            a_tenon: false,
        };
        let mut f_corps = FaceCoupe {
            normale_ext: n,
            aire,
            a_tenon: false,
        };
        if cfg.connecteur != "aucun" {
            let (rr, jeu) = (cfg.rayon, cfg.jeu);
            for ilot in polys {
                if sc::aire(ilot) < PI * (rr + jeu + cfg.paroi).powi(2) {
                    continue;
                }
                if cfg.connecteur == "rotule" {
                    // boule adaptée à la section : centrée au point le plus « épais »
                    let c = sc::pole(ilot);
                    let mut rb = rayon_boule(cfg).min(sc::distance_contour(ilot, c) - jeu - 1.2);
                    if rb < 2.0 {
                        continue;
                    }
                    let pm = pos_monde(&rinv, c, z0);
                    // distance 3D réelle jusqu'à la surface : la cavité ne doit jamais percer
                    rb = rb.min(ml::distance_surface(&tm_ref, pm) - jeu - 1.2);
                    if rb < 2.0 {
                        continue;
                    }
                    let t = rinv * ml::translation([c[0], c[1], z0]);
                    // rotation arbitraire : aucun sommet de la sphère ne tombe pile sur le plan de coupe
                    let cav = appliquer(&Manifold::sphere(rb + jeu, SEGMENTS).rotate(17.3, 23.9, 11.1), &t);
                    part = part.difference(&cav);
                    nouveau.m = nouveau.m.difference(&cav);
                    goujons.push((rb * 100.0).round() / 100.0);
                    continue;
                }
                for xy in sc::positions(ilot, rr + jeu, cfg.paroi, cfg.par_ilot) {
                    let pm = pos_monde(&rinv, xy, z0);
                    let l = cfg
                        .profondeur
                        .min(0.7 * ml::epaisseur(&m_ref, pm, n))
                        .min(0.7 * ml::epaisseur(&m_ref, pm, -n));
                    if l < 2.0 {
                        continue;
                    }
                    let w = rinv * ml::translation([xy[0], xy[1], z0]);
                    if cfg.connecteur == "goujon" {
                        let hh = l + jeu;
                        let trou = appliquer(&cylindre(rr + jeu, 2.0 * hh, 0.0).translate(0.0, 0.0, -hh), &w);
                        part = part.difference(&trou);
                        nouveau.m = nouveau.m.difference(&trou);
                        goujons.push(2.0 * l - 2.0 * jeu);
                    } else {
                        // tenon sur le corps (côté -n), trou dans la pièce (côté +n)
                        let ten = appliquer(
                            &Manifold::cylinder(l + 0.5, rr, rr, SEGMENTS, false).translate(0.0, 0.0, -0.5),
                            &w,
                        );
                        let tr = appliquer(
                            &Manifold::cylinder(l + jeu + EPS, rr + jeu, rr + jeu, SEGMENTS, false)
                                .translate(0.0, 0.0, -EPS),
                            &w,
                        );
                        nouveau.m = nouveau.m.union(&ten);
                        part = part.difference(&tr);
                        f_corps.a_tenon = true;
                    }
                }
            }
        }
        nouveau.faces.push(f_corps);
        extraites.push(Piece {
            m: part,
            label: String::new(),
            faces: vec![f_part],
        });
    }
    (extraites, nouveau, vec![])
}

/// Indice de la pièce qui contient le point (centre du plan), sinon la plus proche.
/// Permet de recouper une pièce déjà détachée (ex. couper la botte d'une jambe).
fn piece_contenant(pieces: &[Piece], pt: [f64; 3]) -> usize {
    let cube = Manifold::cube(0.6, 0.6, 0.6, true).translate(pt[0], pt[1], pt[2]);
    for (i, p) in pieces.iter().enumerate() {
        if !p.m.intersection(&cube).is_empty() {
            return i;
        }
    }
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| (i, ml::distance_surface(&ml::vers_maillage(&p.m), pt)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

pub struct Resultat {
    pub pieces: Vec<Piece>,
    pub goujons: Vec<f64>,
    pub avertissements: Vec<String>,
}

/// Applique les coupes dans l'ordre. Chaque coupe agit sur la pièce qui contient son
/// point et détache la partie, du côté de sa normale, qui touche la section la plus
/// proche de ce point ; tout le reste demeure dans la pièce d'origine.
pub fn decouper(tm: &Maillage, coupes: &[Coupe], cfg: &Config) -> Result<Resultat> {
    let mut pieces = vec![Piece {
        m: ml::vers_manifold(tm)?,
        label: "torse".into(),
        faces: vec![],
    }];
    let mut goujons = Vec::new();
    let mut avert = Vec::new();
    for c in coupes {
        let n = V3::from(c.normale);
        if n.norm() < 1e-9 {
            continue;
        }
        let pt = V3::from(c.point);
        let k = piece_contenant(&pieces, c.point);
        let corps = pieces[k].clone();
        let (ex, reste, a) = coupe_locale(corps, n, pt, &[pt], cfg, &mut goujons);
        avert.extend(a.into_iter().map(|x| format!("{} : {}", nom_lisible(&c.label), x)));
        pieces[k] = reste;
        for mut p in ex {
            p.label = c.label.clone();
            pieces.push(p);
        }
    }

    let mut finales = Vec::new();
    for p in pieces {
        let mut morceaux: Vec<Manifold> = p.m.decompose().into_iter().filter(|m| m.volume() > 0.5).collect();
        morceaux.sort_by(|a, b| b.volume().total_cmp(&a.volume()));
        let mut it = morceaux.into_iter();
        let Some(premier) = it.next() else { continue };
        finales.push(Piece {
            m: premier,
            label: p.label.clone(),
            faces: p.faces.clone(),
        });
        for m in it {
            finales.push(Piece {
                m,
                label: "fragment".into(),
                faces: vec![],
            });
        }
    }
    let nfrag = finales.iter().filter(|p| p.label == "fragment").count();
    if nfrag > 0 {
        avert.push(format!(
            "{nfrag} fragment(s) détaché(s) sans connecteur : à coller, ou déplace un plan de quelques mm."
        ));
    }
    Ok(Resultat {
        pieces: finales,
        goujons,
        avertissements: avert,
    })
}

// ------------------------------------------------------------------ export
/// Pose la pièce à plat sur sa plus grande face de coupe (sans tenon).
pub fn orienter(p: &Piece) -> Maillage {
    let mut m = p.m.clone();
    if let Some(f) = p
        .faces
        .iter()
        .filter(|f| !f.a_tenon)
        .max_by(|a, b| a.aire.total_cmp(&b.aire))
    {
        m = appliquer(&m, &aligner(f.normale_ext, -V3::z()));
    }
    let mut tm = ml::vers_maillage(&m.simplify(0.005));
    tm.poser_a_l_origine();
    tm
}

/// Dispose les maillages en rangées sur le plateau.
pub fn disposer(meshes: &mut [Maillage], largeur: f64, esp: f64) {
    let (mut x, mut y, mut rh) = (0.0, 0.0, 0.0f64);
    for tm in meshes.iter_mut() {
        let e = tm.etendue();
        if x > 0.0 && x + e[0] > largeur {
            x = 0.0;
            y += rh + esp;
            rh = 0.0;
        }
        tm.translater([x, y, 0.0]);
        x += e[0] + esp;
        rh = rh.max(e[1]);
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct InfoPiece {
    pub nom: String,
    pub label: String,
    pub titre: String,
    pub dims: [f64; 3],
    pub volume_cm3: f64,
    pub tient: bool,
}

pub fn tient(e: [f64; 3], vol: [f64; 3]) -> bool {
    let mut a = e;
    let mut b = vol;
    a.sort_by(f64::total_cmp);
    b.sort_by(f64::total_cmp);
    a.iter().zip(b.iter()).all(|(x, y)| x <= y)
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Exporte les pièces : un STL par pièce (posée à plat), les rotules/goujons,
/// un 3MF disposé sur le plateau, un ZIP et un aperçu GLB coloré.
pub fn exporter(res: &Resultat, cfg: &Config, dossier: &Path, nom_base: &str) -> Result<(Vec<InfoPiece>, PathBuf)> {
    std::fs::create_dir_all(dossier)?;
    for e in std::fs::read_dir(dossier)?.flatten() {
        if e.path().is_file() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let mut infos = Vec::new();
    let mut meshes = Vec::new();
    let mut noms = Vec::new();
    let mut apercu = Vec::new();
    let mut compte: BTreeMap<String, usize> = BTreeMap::new();
    for p in &res.pieces {
        let c = compte.entry(p.label.clone()).or_insert(0);
        *c += 1;
        let nom = if p.label == "fragment" {
            format!("fragment_{c}")
        } else {
            p.label.clone()
        };
        apercu.push((nom.clone(), ml::vers_maillage(&p.m)));
        let t = orienter(p);
        ml::ecrire_stl(&t, &dossier.join(format!("{nom}.stl")))?;
        let e = t.etendue();
        infos.push(InfoPiece {
            nom: nom.clone(),
            label: p.label.clone(),
            titre: nom_lisible(&p.label),
            dims: e.map(r1),
            volume_cm3: r1(p.m.volume() / 1000.0),
            tient: tient(e, cfg.volume),
        });
        meshes.push(t);
        noms.push(nom);
    }
    let rot = cfg.connecteur == "rotule";
    for (j, &l) in res.goujons.iter().enumerate() {
        let m = if rot {
            boule(l)
        } else {
            cylindre(cfg.rayon, l.max(3.0), (cfg.rayon / 3.0).min(0.6))
        };
        let mut t = ml::vers_maillage(&m);
        t.poser_a_l_origine();
        if rot {
            ml::ecrire_stl(&t, &dossier.join(format!("boule_r{l:.1}.stl")))?;
        } else if j == 0 {
            ml::ecrire_stl(&t, &dossier.join("goujon.stl"))?;
        }
        meshes.push(t);
        noms.push(format!("{}_{:02}", if rot { "boule" } else { "goujon" }, j + 1));
    }
    disposer(&mut meshes, cfg.volume[0], 5.0);
    let objets: Vec<(String, Maillage)> = noms.into_iter().zip(meshes).collect();
    ml::ecrire_3mf(&objets, &dossier.join(format!("{nom_base}.3mf")))?;
    ml::ecrire_glb(&apercu, &dossier.join("apercu.glb"))?;

    let zp = dossier.join(format!("{nom_base}_pieces.zip"));
    let mut fichiers: Vec<PathBuf> = std::fs::read_dir(dossier)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(ml::extension(p).as_str(), "stl" | "3mf"))
        .collect();
    fichiers.sort();
    let mut z = zip::ZipWriter::new(std::fs::File::create(&zp)?);
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for f in fichiers {
        z.start_file(f.file_name().unwrap().to_string_lossy(), o)?;
        z.write_all(&std::fs::read(&f)?)?;
    }
    z.finish()?;
    Ok((infos, zp))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Personnage simplifié : jambes, bassin, torse, cou, tête, bras.
    fn personnage() -> Maillage {
        let cyl = |h: f64, r1: f64, r2: f64| Manifold::cylinder(h, r1, r2, 32, false);
        let mut parts = Vec::new();
        for s in [-1.0, 1.0] {
            parts.push(cyl(80.0, 7.0, 9.0).translate(s * 10.0, 0.0, 2.0));
            parts.push(Manifold::cube(14.0, 22.0, 6.0, true).translate(s * 10.0, 4.0, 3.0));
            parts.push(
                cyl(62.0, 4.5, 5.5)
                    .rotate(0.0, -s * 18.0, 0.0)
                    .translate(s * 26.0, 0.0, 78.0),
            );
            parts.push(Manifold::sphere(6.5, 32).translate(s * 25.0, 0.0, 138.0));
        }
        parts.push(Manifold::cube(36.0, 18.0, 24.0, true).translate(0.0, 0.0, 90.0));
        parts.push(cyl(50.0, 16.0, 19.0).scale(1.2, 0.65, 1.0).translate(0.0, 0.0, 95.0));
        parts.push(cyl(14.0, 5.0, 5.0).translate(0.0, 0.0, 142.0));
        parts.push(Manifold::sphere(13.0, 32).translate(0.0, 0.0, 166.0));
        ml::vers_maillage(&Manifold::batch_union(&parts))
    }

    fn prepare() -> (Maillage, Reperes) {
        let mut tm = personnage();
        let e = tm.etendue()[2];
        tm.mettre_a_echelle(150.0 / e);
        let rep = reperes(&ml::vers_manifold(&tm).unwrap(), tm.etendue()[2]).unwrap();
        (tm, rep)
    }

    #[test]
    fn reperes_detectes() {
        let (_, rep) = prepare();
        assert!(rep.avertissements.is_empty(), "{:?}", rep.avertissements);
        assert_eq!(rep.jambes_xy.len(), 2);
        assert!(rep.z_hanche < rep.z_taille && rep.z_taille < rep.z_cou);
        assert!((120.0..140.0).contains(&rep.z_cou), "cou à {}", rep.z_cou);
        assert_eq!(rep.axe_lat, "x");
    }

    #[test]
    fn gabarits_et_connecteurs() {
        let (tm, rep) = prepare();
        for (cle, attendu) in [("6", 6), ("5", 5), ("4a", 4), ("4b", 4), ("3", 3), ("2", 2)] {
            let g = gabarit(cle).unwrap();
            let res = decouper(&tm, &coupes_defaut(&rep, g), &Config::default()).unwrap();
            let pieces = res.pieces.iter().filter(|p| p.label != "fragment").count();
            assert_eq!(pieces, attendu, "gabarit {cle}");
        }
        for con in ["rotule", "goujon", "tenon", "aucun"] {
            let cfg = Config {
                connecteur: con.into(),
                ..Config::default()
            };
            let res = decouper(&tm, &coupes_defaut(&rep, gabarit("3").unwrap()), &cfg).unwrap();
            let total: f64 = res.pieces.iter().map(|p| p.m.volume()).sum();
            assert!(total > 40_000.0, "{con} : volume {total}");
            if con == "aucun" || con == "tenon" {
                assert!(res.goujons.is_empty());
            } else {
                assert!(!res.goujons.is_empty(), "{con} sans connecteur");
            }
        }
    }

    #[test]
    fn export_complet() {
        let (tm, rep) = prepare();
        let cfg = Config::default();
        let res = decouper(&tm, &coupes_defaut(&rep, gabarit("6").unwrap()), &cfg).unwrap();
        let d = std::env::temp_dir().join(format!("gs3d-test-{}", std::process::id()));
        let (infos, zp) = exporter(&res, &cfg, &d, "perso").unwrap();
        assert_eq!(infos.len(), 6);
        assert!(infos.iter().all(|i| i.tient));
        for f in ["perso.3mf", "apercu.glb", "torse.stl", "tete.stl"] {
            assert!(d.join(f).is_file(), "{f} manquant");
        }
        assert!(zp.is_file());
        // relecture des fichiers produits
        let t = ml::charger_fichier(&d.join("tete.stl")).unwrap();
        assert!(
            ml::vers_manifold(&{
                let mut t = t;
                ml::reparer(&mut t);
                t
            })
            .is_ok()
        );
        assert!(!ml::charger_fichier(&d.join("apercu.glb")).unwrap().faces.is_empty());
        assert!(!ml::charger_fichier(&d.join("perso.3mf")).unwrap().faces.is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn alignement() {
        for a in [V3::x(), -V3::z(), V3::new(0.3, -0.2, 0.9), V3::z()] {
            let m = aligner(a, V3::z());
            let r = m.transform_vector(&a.normalize());
            assert!((r - V3::z()).norm() < 1e-9, "{a:?} → {r:?}");
        }
    }
}
