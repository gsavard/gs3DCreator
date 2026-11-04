//! Découpe en ligne de commande (plans libres, découpe automatique au volume
//! d'impression, profil de sections) — équivalent de l'ancien `decoupe3d.py`.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use manifold3d::Manifold;

use crate::maillage::{self as ml, Maillage, V3, aligner, appliquer};
use crate::moteur::{self as mo, Config, EPS, FaceCoupe, Piece, SEGMENTS};
use crate::sections as sc;

pub struct Plan {
    pub normale: V3,
    pub point: V3,
}

/// `z=120` ou `nx,ny,nz,px,py,pz`
pub fn parser_plan(txt: &str) -> Result<Plan> {
    let t = txt.trim().to_lowercase();
    if let Some((axe, val)) = t.split_once('=') {
        let n = match axe.trim() {
            "x" => V3::x(),
            "y" => V3::y(),
            "z" => V3::z(),
            a => bail!("Axe inconnu : {a}"),
        };
        let v: f64 = val
            .trim()
            .parse()
            .map_err(|_| anyhow!("Valeur de plan invalide : {val}"))?;
        return Ok(Plan {
            normale: n,
            point: n * v,
        });
    }
    let v: Vec<f64> = t
        .split(',')
        .map(|x| x.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| anyhow!("Plan attendu : 'z=120' ou 'nx,ny,nz,px,py,pz'"))?;
    if v.len() != 6 {
        bail!("Plan attendu : 'z=120' ou 'nx,ny,nz,px,py,pz'");
    }
    let n = V3::new(v[0], v[1], v[2]);
    if n.norm() < 1e-9 {
        bail!("Normale nulle");
    }
    Ok(Plan {
        normale: n.normalize(),
        point: V3::new(v[3], v[4], v[5]),
    })
}

pub fn charger_et_reparer(chemin: &Path, echelle: f64) -> Result<Maillage> {
    let mut tm = ml::charger_fichier(chemin)?;
    if echelle != 1.0 {
        tm.mettre_a_echelle(echelle);
    }
    ml::reparer(&mut tm);
    Ok(tm)
}

/// Coupe toutes les pièces traversées par le plan (les deux côtés sont conservés).
fn couper(pieces: Vec<Piece>, plan: &Plan, cfg: &Config, goujons: &mut Vec<Manifold>) -> Vec<Piece> {
    let n = plan.normale;
    let offset = n.dot(&plan.point);
    let r = aligner(n, V3::z());
    let rinv = r.try_inverse().unwrap();
    let z0 = ml::appliquer_pt(&r, [plan.point.x, plan.point.y, plan.point.z])[2];
    let mut resultat = Vec::new();
    for pc in pieces {
        let (haut, bas) = pc.m.split_by_plane([n.x, n.y, n.z], offset);
        if haut.is_empty() || bas.is_empty() || haut.volume() < 1.0 || bas.volume() < 1.0 {
            resultat.push(pc);
            continue;
        }
        let ilots = sc::ilots(&appliquer(&pc.m, &r), z0);
        let aire: f64 = ilots.iter().map(sc::aire).sum();
        let mut p_haut = Piece {
            m: haut,
            label: format!("{}H", pc.label),
            faces: pc.faces.clone(),
        };
        let mut p_bas = Piece {
            m: bas,
            label: format!("{}B", pc.label),
            faces: pc.faces.clone(),
        };
        let f_haut = FaceCoupe {
            normale_ext: -n,
            aire,
            a_tenon: false,
        };
        let mut f_bas = FaceCoupe {
            normale_ext: n,
            aire,
            a_tenon: false,
        };

        if cfg.connecteur != "aucun" {
            let (rr, jeu) = (cfg.rayon, cfg.jeu);
            for ilot in &ilots {
                if sc::aire(ilot) < std::f64::consts::PI * (rr + jeu + cfg.paroi).powi(2) {
                    continue;
                }
                for xy in sc::positions(ilot, rr + jeu, cfg.paroi, cfg.par_ilot) {
                    let pm = ml::appliquer_pt(&rinv, [xy[0], xy[1], z0]);
                    let e_h = ml::epaisseur(&pc.m, pm, n);
                    let e_b = ml::epaisseur(&pc.m, pm, -n);
                    let l = cfg.profondeur.min(0.7 * e_h).min(0.7 * e_b);
                    if l < 2.0 {
                        println!("   ↳ connecteur ignoré (trop mince : {:.1} mm)", e_h.min(e_b));
                        continue;
                    }
                    let w = rinv * ml::translation([xy[0], xy[1], z0]);
                    if cfg.connecteur == "goujon" {
                        let h = l + jeu;
                        let trou = appliquer(&mo::cylindre(rr + jeu, 2.0 * h, 0.0).translate(0.0, 0.0, -h), &w);
                        p_haut.m = p_haut.m.difference(&trou);
                        p_bas.m = p_bas.m.difference(&trou);
                        goujons.push(mo::cylindre(rr, 2.0 * l - 2.0 * jeu, (rr / 3.0).min(0.6)));
                    } else {
                        // tenon intégré sur la pièce du bas, trou dans celle du haut
                        let tenon = Manifold::cylinder(l + 0.5, rr, rr, SEGMENTS, false)
                            .translate(0.0, 0.0, -0.5)
                            .union(&Manifold::cylinder(0.4, rr, rr - 0.4, SEGMENTS, false).translate(0.0, 0.0, l));
                        let trou = Manifold::cylinder(l + jeu + 0.5 + EPS, rr + jeu, rr + jeu, SEGMENTS, false)
                            .translate(0.0, 0.0, -EPS);
                        p_bas.m = p_bas.m.union(&appliquer(&tenon, &w));
                        p_haut.m = p_haut.m.difference(&appliquer(&trou, &w));
                        f_bas.a_tenon = true;
                    }
                }
            }
        }
        p_haut.faces.push(f_haut);
        p_bas.faces.push(f_bas);
        resultat.push(p_haut);
        resultat.push(p_bas);
    }
    resultat
}

fn boite(p: &Piece) -> ([f64; 3], [f64; 3]) {
    ml::vers_maillage(&p.m).bornes()
}

fn entre_dans_volume(p: &Piece, vol: [f64; 3]) -> bool {
    let (lo, hi) = boite(p);
    mo::tient([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], vol.map(|v| v - 2.0))
}

fn decoupe_auto(mut pieces: Vec<Piece>, cfg: &Config, goujons: &mut Vec<Manifold>) -> Vec<Piece> {
    for _ in 0..30 {
        let Some(i) = pieces.iter().position(|p| !entre_dans_volume(p, cfg.volume)) else {
            return pieces;
        };
        let pc = pieces.remove(i);
        let (lo, hi) = boite(&pc);
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let axe = (0..3).max_by(|&a, &b| ext[a].total_cmp(&ext[b])).unwrap();
        let mut n = V3::zeros();
        n[axe] = 1.0;
        let centre = V3::new((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0);
        println!(
            "   auto : coupe de {} sur {}={:.1}",
            pc.label,
            ["x", "y", "z"][axe],
            centre[axe]
        );
        pieces.extend(couper(
            vec![pc],
            &Plan {
                normale: n,
                point: centre,
            },
            cfg,
            goujons,
        ));
    }
    println!("⚠  Limite d'itérations atteinte en mode auto.");
    pieces
}

/// Coupe le modèle en tranches le long d'un axe : nombre d'îlots et aire de chaque section.
pub fn afficher_profil(tm: &Maillage, axe: &str, pas: f64) -> Result<()> {
    let i = match axe {
        "x" => 0,
        "y" => 1,
        _ => 2,
    };
    let mut n = V3::zeros();
    n[i] = 1.0;
    let m = ml::vers_manifold(tm)?;
    let r = aligner(n, V3::z());
    let mr = appliquer(&m, &r);
    let (lo, hi) = tm.bornes();
    let mut lignes = Vec::new();
    let mut v = lo[i] + pas / 2.0;
    while v < hi[i] {
        let mut p = [0.0; 3];
        p[i] = v;
        let z = ml::appliquer_pt(&r, p)[2];
        let il = sc::ilots(&mr, z);
        lignes.push((v, il.len(), il.iter().map(sc::aire).sum::<f64>()));
        v += pas;
    }
    let amax = lignes.iter().map(|l| l.2).fold(0.0, f64::max).max(1.0);
    println!(
        "\nProfil selon {} (haut en bas) — îlots = morceaux séparés dans la tranche",
        axe.to_uppercase()
    );
    println!("{axe:>8} | îlots | aire mm² | ");
    for (v, k, a) in lignes.iter().rev() {
        let barre = "█".repeat((40.0 * a / amax) as usize);
        println!("{v:8.1} | {k:5} | {a:8.0} | {barre}");
    }
    Ok(())
}

pub fn afficher_info(nom: &str, tm: &Maillage) {
    let (lo, hi) = tm.bornes();
    let e = tm.etendue();
    let etanche = ml::vers_manifold(tm).is_ok();
    println!(
        "Modèle : {nom}  |  {} triangles  |  étanche : {}",
        tm.faces.len(),
        if etanche { "oui" } else { "non" }
    );
    println!(
        "Boîte : X {:.1}→{:.1}  Y {:.1}→{:.1}  Z {:.1}→{:.1}  (taille {:.1} × {:.1} × {:.1} mm)",
        lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], e[0], e[1], e[2]
    );
}

pub struct OptionsDecoupe {
    pub modele: PathBuf,
    pub sortie: PathBuf,
    pub plans: Vec<String>,
    pub auto: bool,
    pub echelle: f64,
    pub cfg: Config,
}

pub fn decouper_plans(o: &OptionsDecoupe) -> Result<()> {
    let tm = charger_et_reparer(&o.modele, o.echelle)?;
    afficher_info(&o.modele.file_name().unwrap_or_default().to_string_lossy(), &tm);
    let mut pieces = vec![Piece {
        m: ml::vers_manifold(&tm)?,
        label: "P".into(),
        faces: vec![],
    }];
    let mut goujons = Vec::new();
    for txt in &o.plans {
        let plan = parser_plan(txt)?;
        println!("Coupe : {txt}");
        pieces = couper(pieces, &plan, &o.cfg, &mut goujons);
    }
    if o.auto {
        pieces = decoupe_auto(pieces, &o.cfg, &mut goujons);
    }

    // Nettoyage : sépare les morceaux disjoints éventuels
    let mut finales = Vec::new();
    for pc in pieces {
        let morceaux = pc.m.decompose();
        if morceaux.len() == 1 {
            finales.push(pc);
        } else {
            for (k, m) in morceaux.into_iter().enumerate() {
                if m.volume() > 1.0 {
                    finales.push(Piece {
                        m,
                        label: format!("{}{k}", pc.label),
                        faces: pc.faces.clone(),
                    });
                }
            }
        }
    }
    finales.sort_by(|a, b| b.m.volume().total_cmp(&a.m.volume()));

    std::fs::create_dir_all(&o.sortie)?;
    let mut objets = Vec::new();
    for (i, pc) in finales.iter().enumerate() {
        let t = mo::orienter(pc);
        let nom = format!("piece_{:02}", i + 1);
        ml::ecrire_stl(&t, &o.sortie.join(format!("{nom}.stl")))?;
        let e = t.etendue();
        let ok = if mo::tient(e, o.cfg.volume) {
            "✓"
        } else {
            "✗ TROP GRAND"
        };
        println!(
            "  {nom}: {:.1} × {:.1} × {:.1} mm  vol {:.1} cm³  {ok}",
            e[0],
            e[1],
            e[2],
            pc.m.volume() / 1000.0
        );
        objets.push((nom, t));
    }
    for (j, g) in goujons.iter().enumerate() {
        let mut t = ml::vers_maillage(g);
        t.poser_a_l_origine();
        if j == 0 {
            ml::ecrire_stl(&t, &o.sortie.join("goujon.stl"))?;
        }
        objets.push((format!("goujon_{:02}", j + 1), t));
    }
    if !goujons.is_empty() {
        println!("  + {} goujons à imprimer (goujon.stl)", goujons.len());
    }
    let mut meshes: Vec<Maillage> = objets.iter().map(|o| o.1.clone()).collect();
    mo::disposer(&mut meshes, o.cfg.volume[0], 5.0);
    let objets: Vec<(String, Maillage)> = objets.into_iter().map(|o| o.0).zip(meshes).collect();
    ml::ecrire_3mf(&objets, &o.sortie.join("assemblage.3mf"))?;
    println!(
        "\n→ {} pièces exportées dans {}/ (STL + assemblage.3mf)",
        finales.len(),
        o.sortie.display()
    );
    Ok(())
}

/// Découpe d'un personnage selon un gabarit, sans interface (repères automatiques).
pub fn decouper_gabarit(
    modele: &Path,
    sortie: &Path,
    cle: &str,
    axe: &str,
    hauteur: Option<f64>,
    cfg: &Config,
) -> Result<()> {
    let g = mo::gabarit(cle).ok_or_else(|| anyhow!("Gabarit inconnu : {cle} (6, 5, 4a, 4b, 3, 2)"))?;
    let (tm, axe) = mo::charger(modele, axe, hauteur)?;
    let e = tm.etendue();
    println!(
        "Modèle : {}  |  axe vertical {axe}  |  {:.1} × {:.1} × {:.1} mm",
        modele.display(),
        e[0],
        e[1],
        e[2]
    );
    let rep = mo::reperes(&ml::vers_manifold(&tm)?, e[2])?;
    for a in &rep.avertissements {
        println!("⚠  {a}");
    }
    println!(
        "Repères : cou {:.1} mm · taille {:.1} mm · hanches {:.1} mm",
        rep.z_cou, rep.z_taille, rep.z_hanche
    );
    let coupes = mo::coupes_defaut(&rep, g);
    let res = mo::decouper(&tm, &coupes, cfg)?;
    let nom = modele.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let (infos, zp) = mo::exporter(&res, cfg, sortie, &nom)?;
    for i in &infos {
        println!(
            "  {:<10} {:.1} × {:.1} × {:.1} mm  {:.1} cm³ {}",
            i.titre,
            i.dims[0],
            i.dims[1],
            i.dims[2],
            i.volume_cm3,
            if i.tient { "✓" } else { "✗ TROP GRANDE" }
        );
    }
    for a in &res.avertissements {
        println!("⚠  {a}");
    }
    println!(
        "\n→ {} pièces, {} connecteurs · {}",
        infos.len(),
        res.goujons.len(),
        zp.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans() {
        let p = parser_plan("z=120").unwrap();
        assert_eq!(p.normale, V3::z());
        assert_eq!(p.point.z, 120.0);
        let p = parser_plan(" 0,0,2, 1,2,3 ").unwrap();
        assert!((p.normale - V3::z()).norm() < 1e-12);
        assert!(parser_plan("w=3").is_err());
        assert!(parser_plan("1,2,3").is_err());
    }
}
