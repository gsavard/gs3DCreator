//! Coloriage des parties d'un modèle : lecture des couleurs d'un 3MF, export 3MF
//! multi-couleurs (un matériau par triangle) et STL séparés par couleur.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::maillage::{self as ml, Maillage};

/// Couleur des triangles non peints (indice 0 de la palette).
pub const COULEUR_BASE: [u8; 3] = [0xBD, 0xB8, 0xAE];

/// Couleur de chaque triangle, sous forme d'indices dans une palette.
#[derive(Clone, Debug)]
pub struct Peinture {
    pub palette: Vec<[u8; 3]>,
    pub faces: Vec<u8>,
}

impl Peinture {
    pub fn vierge(n: usize) -> Self {
        Peinture {
            palette: vec![COULEUR_BASE],
            faces: vec![0; n],
        }
    }
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// « #RRGGBB » ou « #RRGGBBAA » (l'alpha est ignoré).
pub fn depuis_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if !s.is_ascii() || (s.len() != 6 && s.len() != 8) {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([v(0)?, v(2)?, v(4)?])
}

/// Charge un modèle (STL, OBJ, GLB, PLY, 3MF, OFF). Un 3MF coloré fournit sa palette.
pub fn charger(chemin: &Path) -> Result<(Maillage, Peinture)> {
    if ml::extension(chemin) == "3mf" {
        if let Some(r) = lire_3mf_couleurs(chemin)? {
            return Ok(r);
        }
    }
    let tm = ml::charger_fichier(chemin)?;
    let n = tm.faces.len();
    Ok((tm, Peinture::vierge(n)))
}

fn attribut(corps: &str, k: &str) -> Option<String> {
    for q in ['"', '\''] {
        let motif = format!(" {k}={q}");
        if let Some(i) = corps.find(&motif) {
            let reste = &corps[i + motif.len()..];
            if let Some(j) = reste.find(q) {
                return Some(reste[..j].trim().to_string());
            }
        }
    }
    None
}

/// Lit un 3MF avec matériaux de base (`basematerials`) ou groupes de couleurs
/// (extension Materials). Renvoie `None` si le fichier ne contient aucune couleur.
fn lire_3mf_couleurs(chemin: &Path) -> Result<Option<(Maillage, Peinture)>> {
    let mut zip = zip::ZipArchive::new(File::open(chemin)?).context("3MF illisible (zip)")?;
    let noms: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            zip.by_index(i)
                .ok()
                .map(|f| f.name().map(|n| n.to_string()).unwrap_or_default())
        })
        .filter(|n| n.to_ascii_lowercase().ends_with(".model"))
        .collect();

    let mut tm = Maillage::default();
    let mut palette = vec![COULEUR_BASE];
    let mut connues: HashMap<[u8; 3], u8> = HashMap::from([(COULEUR_BASE, 0u8)]);
    let mut faces_c: Vec<u8> = Vec::new();

    for nom in noms {
        let mut xml = String::new();
        zip.by_name(&nom)?.read_to_string(&mut xml)?;
        let xml = xml.replace(['\n', '\r', '\t'], " ");

        // (identifiant du groupe, indice) → indice de palette
        let mut materiaux: HashMap<(u32, u32), u8> = HashMap::new();
        let mut groupe: Option<u32> = None;
        let mut idx_groupe = 0u32;
        let (mut obj_pid, mut obj_pindex) = (None::<u32>, 0u32);
        let mut part = Maillage::default();
        let mut part_c: Vec<u8> = Vec::new();

        let mut enregistrer = |c: [u8; 3], palette: &mut Vec<[u8; 3]>| -> u8 {
            if let Some(&i) = connues.get(&c) {
                return i;
            }
            if palette.len() >= 255 {
                return 0;
            }
            palette.push(c);
            let i = (palette.len() - 1) as u8;
            connues.insert(c, i);
            i
        };

        for balise in xml.split('<').skip(1) {
            let fin = balise.find('>').unwrap_or(balise.len());
            let corps = &balise[..fin];
            let nom_b = corps.split_whitespace().next().unwrap_or("");
            let local = nom_b.rsplit(':').next().unwrap_or(nom_b);
            match local {
                "basematerials" | "colorgroup" => {
                    groupe = attribut(corps, "id").and_then(|v| v.parse().ok());
                    idx_groupe = 0;
                }
                "/basematerials" | "/colorgroup" => groupe = None,
                "base" | "color" => {
                    if let Some(g) = groupe {
                        let c = attribut(corps, "displaycolor")
                            .or_else(|| attribut(corps, "color"))
                            .and_then(|s| depuis_hex(&s))
                            .unwrap_or(COULEUR_BASE);
                        let i = enregistrer(c, &mut palette);
                        materiaux.insert((g, idx_groupe), i);
                        idx_groupe += 1;
                    }
                }
                "object" => {
                    obj_pid = attribut(corps, "pid").and_then(|v| v.parse().ok());
                    obj_pindex = attribut(corps, "pindex").and_then(|v| v.parse().ok()).unwrap_or(0);
                    part = Maillage::default();
                    part_c.clear();
                }
                "vertex" => {
                    let f = |k: &str| attribut(corps, k).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    part.sommets.push([f("x"), f("y"), f("z")]);
                }
                "triangle" => {
                    let u = |k: &str| attribut(corps, k).and_then(|v| v.parse::<u32>().ok());
                    part.faces
                        .push([u("v1").unwrap_or(0), u("v2").unwrap_or(0), u("v3").unwrap_or(0)]);
                    let pid = u("pid").or(obj_pid);
                    let p1 = u("p1").unwrap_or(obj_pindex);
                    part_c.push(pid.and_then(|g| materiaux.get(&(g, p1)).copied()).unwrap_or(0));
                }
                "/object" => {
                    let n = part.sommets.len() as u32;
                    if part.faces.iter().any(|f| f.iter().any(|&i| i >= n)) {
                        bail!("3MF invalide : indice de sommet hors limites.");
                    }
                    tm.ajouter(&part);
                    faces_c.append(&mut part_c);
                    part = Maillage::default();
                }
                _ => {}
            }
        }
    }
    if tm.faces.is_empty() || palette.len() < 2 || faces_c.len() != tm.faces.len() {
        return Ok(None);
    }
    Ok(Some((
        tm,
        Peinture {
            palette,
            faces: faces_c,
        },
    )))
}

/// Indices de palette réellement utilisés, dans l'ordre, et table de renumérotation.
fn utilises(p: &Peinture) -> (Vec<usize>, Vec<u8>) {
    let mut vu = vec![false; p.palette.len()];
    for &f in &p.faces {
        vu[f as usize] = true;
    }
    let anciens: Vec<usize> = (0..p.palette.len()).filter(|&i| vu[i]).collect();
    let mut remap = vec![0u8; p.palette.len()];
    for (n, &a) in anciens.iter().enumerate() {
        remap[a] = n as u8;
    }
    (anciens, remap)
}

fn valider(tm: &Maillage, p: &Peinture) -> Result<()> {
    if p.faces.len() != tm.faces.len() {
        bail!("Le nombre de couleurs ne correspond pas au nombre de triangles.");
    }
    if p.palette.is_empty() || p.faces.iter().any(|&f| f as usize >= p.palette.len()) {
        bail!("Indice de couleur hors de la palette.");
    }
    Ok(())
}

fn xml_echap(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 3MF d'un seul objet, avec une couleur (matériau de base) par triangle.
pub fn ecrire_3mf(tm: &Maillage, p: &Peinture, nom: &str, chemin: &Path) -> Result<()> {
    use std::fmt::Write as _;
    valider(tm, p)?;
    let (anciens, remap) = utilises(p);
    let mut m = String::with_capacity(1 << 20);
    m.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"fr-CA\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
         <metadata name=\"Application\">gs3Dcreator</metadata>\n<resources>\n<basematerials id=\"1\">\n",
    );
    for (n, &a) in anciens.iter().enumerate() {
        let _ = writeln!(
            m,
            "<base name=\"Couleur {}\" displaycolor=\"{}\"/>",
            n + 1,
            hex(p.palette[a])
        );
    }
    let _ = writeln!(
        m,
        "</basematerials>\n<object id=\"2\" name=\"{}\" type=\"model\" pid=\"1\" pindex=\"{}\"><mesh><vertices>",
        xml_echap(nom),
        remap[p.faces.first().copied().unwrap_or(0) as usize]
    );
    for v in &tm.sommets {
        let _ = writeln!(m, "<vertex x=\"{:.5}\" y=\"{:.5}\" z=\"{:.5}\"/>", v[0], v[1], v[2]);
    }
    m.push_str("</vertices><triangles>\n");
    for (t, &c) in tm.faces.iter().zip(&p.faces) {
        let _ = writeln!(
            m,
            "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\" pid=\"1\" p1=\"{}\"/>",
            t[0], t[1], t[2], remap[c as usize]
        );
    }
    m.push_str("</triangles></mesh></object>\n</resources>\n<build>\n<item objectid=\"2\"/>\n</build>\n</model>\n");

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
    z.write_all(m.as_bytes())?;
    z.finish()?;
    Ok(())
}

/// Sous-maillage formé des triangles d'une couleur (sommets compactés).
fn extraire(tm: &Maillage, p: &Peinture, couleur: usize) -> Maillage {
    let mut nouveau: HashMap<u32, u32> = HashMap::new();
    let mut r = Maillage::default();
    for (t, &c) in tm.faces.iter().zip(&p.faces) {
        if c as usize != couleur {
            continue;
        }
        let mut f = [0u32; 3];
        for k in 0..3 {
            f[k] = *nouveau.entry(t[k]).or_insert_with(|| {
                r.sommets.push(tm.sommets[t[k] as usize]);
                (r.sommets.len() - 1) as u32
            });
        }
        r.faces.push(f);
    }
    r
}

/// Un STL par couleur (mêmes coordonnées : ils se superposent dans le trancheur), réunis dans un ZIP.
/// Renvoie le chemin du ZIP et, par couleur utilisée, (couleur, nombre de triangles, nom du STL).
pub fn ecrire_zip_stl(
    tm: &Maillage,
    p: &Peinture,
    nom: &str,
    dossier: &Path,
) -> Result<(PathBuf, Vec<([u8; 3], usize, String)>)> {
    valider(tm, p)?;
    let (anciens, _) = utilises(p);
    let zip_chemin = dossier.join(format!("{nom}_couleurs_stl.zip"));
    let mut z = zip::ZipWriter::new(File::create(&zip_chemin)?);
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut infos = Vec::new();
    for (n, &a) in anciens.iter().enumerate() {
        let sous = extraire(tm, p, a);
        let fichier = format!("{nom}_{:02}_{}.stl", n + 1, hex(p.palette[a]).trim_start_matches('#'));
        let chemin = dossier.join(&fichier);
        ml::ecrire_stl(&sous, &chemin)?;
        z.start_file(&fichier, o)?;
        z.write_all(&std::fs::read(&chemin)?)?;
        infos.push((p.palette[a], sous.faces.len(), fichier));
    }
    z.finish()?;
    Ok((zip_chemin, infos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Maillage {
        let sommets = vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [0., 0., 10.],
            [10., 0., 10.],
            [10., 10., 10.],
            [0., 10., 10.],
        ];
        let faces = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        Maillage { sommets, faces }
    }

    #[test]
    fn hex_aller_retour() {
        assert_eq!(depuis_hex("#FF8000"), Some([255, 128, 0]));
        assert_eq!(depuis_hex("ff800080"), Some([255, 128, 0]));
        assert_eq!(depuis_hex("#12"), None);
        assert_eq!(depuis_hex("#ééé"), None);
        assert_eq!(hex([1, 2, 255]), "#0102FF");
    }

    #[test]
    fn export_puis_relecture() {
        let tm = cube();
        let p = Peinture {
            palette: vec![COULEUR_BASE, [255, 0, 0], [0, 0, 255], [0, 255, 0]],
            // la couleur 2 (bleu) n'est pas utilisée : elle doit disparaître, le vert devenir l'indice 2
            faces: vec![0, 0, 1, 1, 3, 3, 0, 0, 1, 1, 3, 3],
        };
        let d = std::env::temp_dir().join(format!("gs3d-couleur-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("cube.3mf");
        ecrire_3mf(&tm, &p, "cube", &f).unwrap();
        let (tm2, p2) = charger(&f).unwrap();
        assert_eq!(tm2.faces.len(), 12);
        assert_eq!(p2.palette[1..], [[255, 0, 0], [0, 255, 0]]);
        // couleurs d'origine retrouvées triangle par triangle
        for (a, b) in p.faces.iter().zip(&p2.faces) {
            assert_eq!(p.palette[*a as usize], p2.palette[*b as usize]);
        }
        let (zp, infos) = ecrire_zip_stl(&tm, &p, "cube", &d).unwrap();
        assert!(zp.is_file());
        assert_eq!(infos.len(), 3);
        assert_eq!(infos.iter().map(|i| i.1).sum::<usize>(), 12);
        for (_, _, nom) in &infos {
            assert!(!ml::charger_fichier(&d.join(nom)).unwrap().faces.is_empty());
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn indice_invalide_refuse() {
        let tm = cube();
        let p = Peinture {
            palette: vec![COULEUR_BASE],
            faces: vec![1; 12],
        };
        assert!(ecrire_3mf(&tm, &p, "x", Path::new("/tmp/x.3mf")).is_err());
    }
}
