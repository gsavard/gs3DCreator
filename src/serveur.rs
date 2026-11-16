//! Interface web locale (API JSON + fichiers statiques intégrés au binaire).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as Chemin, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::couleur::{self, Peinture};
use crate::ia::{self, Fournisseur};
use crate::maillage::{self as ml, Maillage};
use crate::moteur::{self as mo, Config, Coupe, Reperes};

pub const APP: &str = "gs3dcreator";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(RustEmbed)]
#[folder = "web/"]
struct Statique;

struct Session {
    source: PathBuf,
    nom: String,
    dossier: PathBuf,
    tm: Option<Arc<Maillage>>,
    rep: Option<Reperes>,
}

/// Modèle ouvert dans l'onglet Colorier.
struct SessionCouleur {
    nom: String,
    dossier: PathBuf,
    tm: Arc<Maillage>,
}

#[derive(Clone)]
struct Etat {
    travail: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    couleurs: Arc<Mutex<HashMap<String, SessionCouleur>>>,
    taches: ia::Taches,
}

struct Erreur(StatusCode, String);

impl IntoResponse for Erreur {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "detail": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for Erreur {
    fn from(e: anyhow::Error) -> Self {
        Erreur(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
    }
}

type Rep<T> = Result<T, Erreur>;

fn introuvable() -> Erreur {
    Erreur(
        StatusCode::NOT_FOUND,
        "Modèle introuvable. Téléverse-le de nouveau.".into(),
    )
}

pub fn routeur() -> Router {
    let travail = std::env::temp_dir().join(APP);
    let _ = std::fs::create_dir_all(&travail);
    let etat = Etat {
        travail,
        sessions: Arc::new(Mutex::new(HashMap::new())),
        couleurs: Arc::new(Mutex::new(HashMap::new())),
        taches: Arc::new(Mutex::new(HashMap::new())),
    };
    Router::new()
        .route("/api/sante", get(sante))
        .route("/api/modeles", post(televerser))
        .route("/api/modeles/{sid}/preparer", post(preparer))
        .route("/api/modeles/{sid}/decouper", post(decouper))
        .route("/api/modeles/{sid}/fichier/{nom}", get(fichier))
        .route("/api/modeles/{sid}/resultat/{nom}", get(resultat))
        .route("/api/couleur", post(couleur_ouvrir))
        .route("/api/couleur/{sid}/maillage", get(couleur_maillage))
        .route("/api/couleur/{sid}/exporter", post(couleur_exporter))
        .route("/api/couleur/{sid}/fichier/{nom}", get(couleur_fichier))
        .route("/api/fournisseurs", get(fournisseurs).post(enregistrer_fournisseur))
        .route("/api/fournisseurs/{id}", delete(supprimer_fournisseur))
        .route("/api/fournisseurs/{id}/tester", post(tester_fournisseur))
        .route("/api/images/analyser", post(analyser_image))
        .route("/api/images", get(lister_generations).post(generer_image))
        .route("/api/images/{id}", get(etat_generation))
        .route("/api/images/{id}/fichier/{nom}", get(fichier_generation))
        .fallback(statique)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .with_state(etat)
}

async fn sante() -> Json<Value> {
    Json(json!({ "app": APP, "version": VERSION }))
}

async fn televerser(State(e): State<Etat>, mut mp: Multipart) -> Rep<Json<Value>> {
    while let Some(champ) = mp
        .next_field()
        .await
        .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi invalide : {x}")))?
    {
        if champ.name() != Some("fichier") {
            continue;
        }
        let nom_fichier = champ.file_name().unwrap_or("").to_string();
        let ext = ml::extension(Path::new(&nom_fichier));
        if !ml::FORMATS.contains(&ext.as_str()) {
            let e = if ext.is_empty() {
                "inconnu".to_string()
            } else {
                format!(".{ext}")
            };
            return Err(Erreur(
                StatusCode::BAD_REQUEST,
                format!("Format {e} non pris en charge. Utilise STL, OBJ ou GLB."),
            ));
        }
        let donnees = champ
            .bytes()
            .await
            .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi interrompu : {x}")))?;
        let sid = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let dossier = e.travail.join(&sid);
        std::fs::create_dir_all(&dossier).map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
        let source = dossier.join(format!("source.{ext}"));
        std::fs::write(&source, &donnees).map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
        let nom = Path::new(&nom_fichier)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "modele".into());
        e.sessions.lock().unwrap().insert(
            sid.clone(),
            Session {
                source,
                nom: nom.clone(),
                dossier,
                tm: None,
                rep: None,
            },
        );
        return Ok(Json(json!({ "id": sid, "nom": nom })));
    }
    Err(Erreur(StatusCode::BAD_REQUEST, "Aucun fichier reçu.".into()))
}

#[derive(Deserialize)]
struct Preparation {
    #[serde(default = "auto")]
    axe: String,
    #[serde(default = "cent_cinquante")]
    hauteur: f64,
}
fn auto() -> String {
    "auto".into()
}
fn cent_cinquante() -> f64 {
    150.0
}

async fn preparer(State(e): State<Etat>, Chemin(sid): Chemin<String>, Json(p): Json<Preparation>) -> Rep<Json<Value>> {
    let (source, dossier) = {
        let s = e.sessions.lock().unwrap();
        let s = s.get(&sid).ok_or_else(introuvable)?;
        (s.source.clone(), s.dossier.clone())
    };
    let calc = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let (tm, axe) = mo::charger(&source, &p.axe, Some(p.hauteur))?;
        let e = tm.etendue();
        let rep = mo::reperes(&ml::vers_manifold(&tm)?, e[2])?;
        ml::ecrire_glb(&[("modele".to_string(), tm.clone())], &dossier.join("modele.glb"))?;
        Ok((tm, axe, rep))
    })
    .await
    .map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
    let (tm, axe, rep) = calc?;
    let e3 = tm.etendue();
    let coupes: serde_json::Map<String, Value> = mo::GABARITS
        .iter()
        .map(|g| {
            (
                g.cle.to_string(),
                serde_json::to_value(mo::coupes_defaut(&rep, g)).unwrap(),
            )
        })
        .collect();
    let reponse = json!({
        "axe": axe,
        "triangles": tm.faces.len(),
        "dims": e3.map(|v| (v * 10.0).round() / 10.0),
        "reperes": rep,
        "coupes": coupes,
        "apercu": format!("/api/modeles/{sid}/fichier/modele.glb"),
    });
    let mut s = e.sessions.lock().unwrap();
    let s = s.get_mut(&sid).ok_or_else(introuvable)?;
    s.tm = Some(Arc::new(tm));
    s.rep = Some(rep);
    Ok(Json(reponse))
}

#[derive(Deserialize)]
struct Decoupe {
    coupes: Vec<Coupe>,
    #[serde(default = "rotule")]
    connecteur: String,
    #[serde(default = "deux_cinq")]
    rayon: f64,
    #[serde(default = "zero_deux")]
    jeu: f64,
    #[serde(default = "six")]
    profondeur: f64,
    #[serde(default = "volume")]
    volume: [f64; 3],
}
fn rotule() -> String {
    "rotule".into()
}
fn deux_cinq() -> f64 {
    2.5
}
fn zero_deux() -> f64 {
    0.2
}
fn six() -> f64 {
    6.0
}
fn volume() -> [f64; 3] {
    [250.0; 3]
}

async fn decouper(State(e): State<Etat>, Chemin(sid): Chemin<String>, Json(d): Json<Decoupe>) -> Rep<Json<Value>> {
    let (tm, nom, dossier) = {
        let s = e.sessions.lock().unwrap();
        let s = s.get(&sid).ok_or_else(introuvable)?;
        let tm =
            s.tm.clone()
                .ok_or_else(|| Erreur(StatusCode::CONFLICT, "Prépare le modèle avant de le découper.".into()))?;
        (tm, s.nom.clone(), s.dossier.clone())
    };
    if d.coupes.is_empty() {
        return Err(Erreur(StatusCode::BAD_REQUEST, "Ajoute au moins une coupe.".into()));
    }
    if !["rotule", "goujon", "tenon", "aucun"].contains(&d.connecteur.as_str()) {
        return Err(Erreur(StatusCode::BAD_REQUEST, "Connecteur inconnu.".into()));
    }
    let cfg = Config {
        connecteur: d.connecteur.clone(),
        rayon: d.rayon.clamp(0.5, 20.0),
        jeu: d.jeu.clamp(0.0, 2.0),
        profondeur: d.profondeur.clamp(1.0, 50.0),
        volume: d.volume,
        ..Config::default()
    };
    let nom2 = nom.clone();
    let coupes = d.coupes.clone();
    let calc = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let res = mo::decouper(&tm, &coupes, &cfg)?;
        let (infos, zp) = mo::exporter(&res, &cfg, &dossier.join("resultat"), &nom2)?;
        Ok((res.goujons.len(), res.avertissements, infos, zp))
    })
    .await
    .map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
    let (ngoujons, avert, infos, zp) = calc?;
    let zip_nom = zp.file_name().unwrap().to_string_lossy().to_string();
    Ok(Json(json!({
        "pieces": infos,
        "goujons": ngoujons,
        "type_connecteur": d.connecteur,
        "avertissements": avert,
        "apercu": format!("/api/modeles/{sid}/resultat/apercu.glb"),
        "zip": format!("/api/modeles/{sid}/resultat/{}", url_composant(&zip_nom)),
        "mf": format!("/api/modeles/{sid}/resultat/{}", url_composant(&format!("{nom}.3mf"))),
    })))
}

fn url_composant(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn envoyer(chemin: PathBuf) -> Rep<Response> {
    let donnees = std::fs::read(&chemin).map_err(|_| Erreur(StatusCode::NOT_FOUND, "Fichier introuvable.".into()))?;
    let nom = chemin.file_name().unwrap().to_string_lossy().to_string();
    let mime = mime_guess::from_path(&chemin).first_or_octet_stream();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-store")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename*=UTF-8''{}", url_composant(&nom)),
        )
        .body(Body::from(donnees))
        .unwrap())
}

fn nom_sur(nom: &str) -> String {
    Path::new(nom)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

async fn fichier(State(e): State<Etat>, Chemin((sid, nom)): Chemin<(String, String)>) -> Rep<Response> {
    let d = e
        .sessions
        .lock()
        .unwrap()
        .get(&sid)
        .map(|s| s.dossier.clone())
        .ok_or_else(introuvable)?;
    envoyer(d.join(nom_sur(&nom)))
}

async fn resultat(State(e): State<Etat>, Chemin((sid, nom)): Chemin<(String, String)>) -> Rep<Response> {
    let d = e
        .sessions
        .lock()
        .unwrap()
        .get(&sid)
        .map(|s| s.dossier.clone())
        .ok_or_else(introuvable)?;
    envoyer(d.join("resultat").join(nom_sur(&nom)))
}

// ------------------------------------------------------------------ image → 3D

async fn fournisseurs() -> Json<Value> {
    let liste: Vec<Value> = ia::lire_config().iter().map(Fournisseur::public).collect();
    Json(json!({ "fournisseurs": liste, "types": ia::TYPES }))
}

async fn enregistrer_fournisseur(Json(f): Json<Fournisseur>) -> Rep<Json<Value>> {
    Ok(Json(ia::enregistrer(f)?.public()))
}

async fn supprimer_fournisseur(Chemin(id): Chemin<String>) -> Rep<Json<Value>> {
    ia::supprimer(&id).map_err(|e| Erreur(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

async fn tester_fournisseur(Chemin(id): Chemin<String>) -> Rep<Json<Value>> {
    let f = ia::trouver(&id).map_err(|e| Erreur(StatusCode::NOT_FOUND, e.to_string()))?;
    let (message, modeles) = ia::tester(&f)
        .await
        .map_err(|e| Erreur(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "message": message, "modeles": modeles })))
}

const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp"];
const IMAGE_MAX: usize = 20 * 1024 * 1024;

/// Lit un formulaire : l'image (champ « image ») et les autres champs texte.
async fn lire_formulaire(mut mp: Multipart) -> Rep<(Vec<u8>, String, String, HashMap<String, String>)> {
    let mut image = None;
    let mut champs = HashMap::new();
    while let Some(champ) = mp
        .next_field()
        .await
        .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi invalide : {x}")))?
    {
        let nom = champ.name().unwrap_or("").to_string();
        if nom == "image" {
            let fichier = champ.file_name().unwrap_or("image.png").to_string();
            let ext = ml::extension(Path::new(&fichier));
            if !IMAGES.contains(&ext.as_str()) {
                return Err(Erreur(
                    StatusCode::BAD_REQUEST,
                    "Image PNG, JPG ou WEBP seulement.".into(),
                ));
            }
            let mime = mime_guess::from_ext(&ext).first_or_octet_stream().to_string();
            let o = champ
                .bytes()
                .await
                .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi interrompu : {x}")))?;
            if o.len() > IMAGE_MAX {
                return Err(Erreur(
                    StatusCode::BAD_REQUEST,
                    "Image trop lourde (20 Mo maximum).".into(),
                ));
            }
            image = Some((o.to_vec(), mime, fichier));
        } else {
            champs.insert(nom, champ.text().await.unwrap_or_default());
        }
    }
    let (o, mime, fichier) = image.ok_or_else(|| Erreur(StatusCode::BAD_REQUEST, "Aucune image reçue.".into()))?;
    Ok((o, mime, fichier, champs))
}

fn fournisseur_choisi(champs: &HashMap<String, String>) -> Rep<Fournisseur> {
    let id = champs.get("fournisseur").map(String::as_str).unwrap_or("");
    ia::trouver(id).map_err(|e| Erreur(StatusCode::BAD_REQUEST, e.to_string()))
}

async fn analyser_image(mp: Multipart) -> Rep<Json<Value>> {
    let (image, mime, _, champs) = lire_formulaire(mp).await?;
    let f = fournisseur_choisi(&champs)?;
    let question = champs.get("question").cloned().unwrap_or_default();
    let texte = ia::analyser(&f, &image, &mime, &question)
        .await
        .map_err(|e| Erreur(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "texte": texte, "modele": f.modele })))
}

async fn generer_image(State(e): State<Etat>, mp: Multipart) -> Rep<Json<Value>> {
    let (image, mime, fichier, champs) = lire_formulaire(mp).await?;
    let f = fournisseur_choisi(&champs)?;
    if f.famille() != "image3d" {
        return Err(Erreur(
            StatusCode::BAD_REQUEST,
            format!(
                "« {} » est un modèle de langage : il ne produit pas de maillage. Choisis un serveur image→3D.",
                f.nom
            ),
        ));
    }
    let options: ia::Options = match champs.get("options") {
        Some(s) if !s.is_empty() => {
            serde_json::from_str(s).map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Options invalides : {x}")))?
        }
        _ => ia::Options::default(),
    };
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let dossier = e.travail.join(format!("img-{id}"));
    std::fs::create_dir_all(&dossier).map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
    let ext = ml::extension(Path::new(&fichier));
    let _ = std::fs::write(dossier.join(format!("image.{ext}")), &image);
    let nom = Path::new(&fichier)
        .file_stem()
        .map(|s| {
            s.to_string_lossy()
                .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "modele".into());
    let tache = ia::Tache {
        id: id.clone(),
        nom,
        fournisseur: f.nom.clone(),
        etat: "attente".into(),
        message: "En file d'attente…".into(),
        debut: Some(std::time::Instant::now()),
        duree: 0.0,
        resultat: None,
        dossier,
    };
    e.taches.lock().unwrap().insert(id.clone(), tache);
    ia::lancer(e.taches.clone(), id.clone(), f, image, mime, fichier, options);
    Ok(Json(json!({ "id": id })))
}

fn vue_tache(t: &ia::Tache) -> Value {
    let mut v = serde_json::to_value(t).unwrap();
    if let Some(d) = t.debut
        && !["termine", "erreur"].contains(&t.etat.as_str())
    {
        v["duree"] = json!((d.elapsed().as_secs_f64() * 10.0).round() / 10.0);
    }
    if let Some(r) = &t.resultat {
        let base = format!("/api/images/{}/fichier", t.id);
        v["apercu"] = json!(format!("{base}/apercu.glb"));
        v["stl"] = json!(format!("{base}/{}", url_composant(r["stl"].as_str().unwrap_or(""))));
    }
    let ext = ["png", "jpg", "jpeg", "webp"]
        .iter()
        .find(|x| t.dossier.join(format!("image.{x}")).exists());
    if let Some(x) = ext {
        v["image"] = json!(format!("/api/images/{}/fichier/image.{x}", t.id));
    }
    v
}

async fn etat_generation(State(e): State<Etat>, Chemin(id): Chemin<String>) -> Rep<Json<Value>> {
    let t = e.taches.lock().unwrap();
    let t = t
        .get(&id)
        .ok_or_else(|| Erreur(StatusCode::NOT_FOUND, "Génération introuvable.".into()))?;
    Ok(Json(vue_tache(t)))
}

async fn lister_generations(State(e): State<Etat>) -> Json<Value> {
    let t = e.taches.lock().unwrap();
    let mut l: Vec<&ia::Tache> = t.values().collect();
    l.sort_by_key(|x| std::cmp::Reverse(x.debut));
    Json(json!({ "generations": l.into_iter().map(vue_tache).collect::<Vec<_>>() }))
}

async fn fichier_generation(State(e): State<Etat>, Chemin((id, nom)): Chemin<(String, String)>) -> Rep<Response> {
    let d = e
        .taches
        .lock()
        .unwrap()
        .get(&id)
        .map(|t| t.dossier.clone())
        .ok_or_else(|| Erreur(StatusCode::NOT_FOUND, "Génération introuvable.".into()))?;
    envoyer(d.join(nom_sur(&nom)))
}

async fn statique(uri: Uri) -> Response {
    let mut chemin = uri.path().trim_start_matches('/').to_string();
    if chemin.is_empty() || chemin.ends_with('/') {
        chemin.push_str("index.html");
    }
    match Statique::get(&chemin) {
        Some(f) => {
            let mime = mime_guess::from_path(&chemin).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], f.data.into_owned()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Introuvable").into_response(),
    }
}

// ---------------------------------------------------------------------- colorier
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// Nom de fichier sans caractères problématiques.
fn nom_propre(nom: &str) -> String {
    let n: String = nom
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_ ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let n = n.trim().to_string();
    if n.is_empty() { "modele".into() } else { n }
}

async fn couleur_ouvrir(State(e): State<Etat>, mut mp: Multipart) -> Rep<Json<Value>> {
    while let Some(champ) = mp
        .next_field()
        .await
        .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi invalide : {x}")))?
    {
        if champ.name() != Some("fichier") {
            continue;
        }
        let nom_fichier = champ.file_name().unwrap_or("").to_string();
        let ext = ml::extension(Path::new(&nom_fichier));
        if !ml::FORMATS.contains(&ext.as_str()) {
            return Err(Erreur(
                StatusCode::BAD_REQUEST,
                "Format non pris en charge. Utilise STL ou 3MF (OBJ, GLB, PLY et OFF fonctionnent aussi).".into(),
            ));
        }
        let donnees = champ
            .bytes()
            .await
            .map_err(|x| Erreur(StatusCode::BAD_REQUEST, format!("Envoi interrompu : {x}")))?;
        let sid = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let dossier = e.travail.join(format!("c{sid}"));
        std::fs::create_dir_all(&dossier).map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
        let source = dossier.join(format!("source.{ext}"));
        std::fs::write(&source, &donnees).map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
        let nom = nom_propre(
            &Path::new(&nom_fichier)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        let (tm, peinture) = tokio::task::spawn_blocking(move || couleur::charger(&source))
            .await
            .map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))??;
        let (lo, hi) = tm.bornes();
        let reponse = json!({
            "id": sid,
            "nom": nom,
            "triangles": tm.faces.len(),
            "sommets": tm.sommets.len(),
            "dims": tm.etendue().map(|v| (v * 100.0).round() / 100.0),
            "min": lo,
            "max": hi,
            "palette": peinture.palette.iter().map(|c| couleur::hex(*c)).collect::<Vec<_>>(),
            "faces": B64.encode(&peinture.faces),
            "colore": peinture.palette.len() > 1,
        });
        e.couleurs.lock().unwrap().insert(
            sid,
            SessionCouleur {
                nom,
                dossier,
                tm: Arc::new(tm),
            },
        );
        return Ok(Json(reponse));
    }
    Err(Erreur(StatusCode::BAD_REQUEST, "Aucun fichier reçu.".into()))
}

/// Maillage en binaire : nombre de sommets (u32), nombre de triangles (u32),
/// positions (f32 × 3 × sommets), indices (u32 × 3 × triangles), petit-boutiste.
async fn couleur_maillage(State(e): State<Etat>, Chemin(sid): Chemin<String>) -> Rep<Response> {
    let tm = e
        .couleurs
        .lock()
        .unwrap()
        .get(&sid)
        .map(|s| s.tm.clone())
        .ok_or_else(introuvable)?;
    let mut o = Vec::with_capacity(8 + tm.sommets.len() * 12 + tm.faces.len() * 12);
    o.extend_from_slice(&(tm.sommets.len() as u32).to_le_bytes());
    o.extend_from_slice(&(tm.faces.len() as u32).to_le_bytes());
    for v in &tm.sommets {
        for c in v {
            o.extend_from_slice(&(*c as f32).to_le_bytes());
        }
    }
    for f in &tm.faces {
        for i in f {
            o.extend_from_slice(&i.to_le_bytes());
        }
    }
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(o))
        .unwrap())
}

#[derive(Deserialize)]
struct ExportCouleur {
    palette: Vec<String>,
    /// Indice de palette de chaque triangle (octets), en base64.
    faces: String,
}

async fn couleur_exporter(
    State(e): State<Etat>,
    Chemin(sid): Chemin<String>,
    Json(d): Json<ExportCouleur>,
) -> Rep<Json<Value>> {
    let (tm, nom, dossier) = {
        let s = e.couleurs.lock().unwrap();
        let s = s.get(&sid).ok_or_else(introuvable)?;
        (s.tm.clone(), s.nom.clone(), s.dossier.join("export"))
    };
    let mauvais = |m: &str| Erreur(StatusCode::BAD_REQUEST, m.to_string());
    if d.palette.is_empty() || d.palette.len() > 255 {
        return Err(mauvais("Palette invalide."));
    }
    let palette = d
        .palette
        .iter()
        .map(|h| couleur::depuis_hex(h))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| mauvais("Couleur invalide dans la palette."))?;
    let faces = B64
        .decode(d.faces.as_bytes())
        .map_err(|_| mauvais("Couleurs des triangles illisibles."))?;
    let peinture = Peinture { palette, faces };
    let nom2 = nom.clone();
    let calc = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier)?;
        couleur::ecrire_3mf(&tm, &peinture, &nom2, &dossier.join(format!("{nom2}_couleur.3mf")))?;
        let (zp, infos) = couleur::ecrire_zip_stl(&tm, &peinture, &nom2, &dossier)?;
        Ok((zp, infos, tm.faces.len()))
    })
    .await
    .map_err(|x| Erreur(StatusCode::INTERNAL_SERVER_ERROR, x.to_string()))?;
    let (zp, infos, total) = calc?;
    let zip_nom = zp.file_name().unwrap().to_string_lossy().to_string();
    Ok(Json(json!({
        "mf": format!("/api/couleur/{sid}/fichier/{}", url_composant(&format!("{nom}_couleur.3mf"))),
        "zip": format!("/api/couleur/{sid}/fichier/{}", url_composant(&zip_nom)),
        "couleurs": infos.iter().map(|(c, n, f)| json!({
            "hex": couleur::hex(*c), "triangles": n, "pct": (*n as f64 * 1000.0 / total as f64).round() / 10.0, "fichier": f,
        })).collect::<Vec<_>>(),
    })))
}

async fn couleur_fichier(State(e): State<Etat>, Chemin((sid, nom)): Chemin<(String, String)>) -> Rep<Response> {
    let d = e
        .couleurs
        .lock()
        .unwrap()
        .get(&sid)
        .map(|s| s.dossier.clone())
        .ok_or_else(introuvable)?;
    envoyer(d.join("export").join(nom_sur(&nom)))
}
