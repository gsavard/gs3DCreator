//! Interface web locale (API JSON + fichiers statiques intégrés au binaire).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as Chemin, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{Value, json};

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

#[derive(Clone)]
struct Etat {
    travail: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
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
    };
    Router::new()
        .route("/api/sante", get(sante))
        .route("/api/modeles", post(televerser))
        .route("/api/modeles/{sid}/preparer", post(preparer))
        .route("/api/modeles/{sid}/decouper", post(decouper))
        .route("/api/modeles/{sid}/fichier/{nom}", get(fichier))
        .route("/api/modeles/{sid}/resultat/{nom}", get(resultat))
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
