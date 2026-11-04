//! Image → 3D : fournisseurs d'IA (serveurs image→3D locaux et LLM compatibles OpenAI),
//! tâches de génération en arrière-plan et conversion du maillage reçu en STL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::maillage as ml;
use crate::moteur as mo;
use crate::serveur::APP;

// ------------------------------------------------------------------ types de fournisseurs

/// Famille : « image3d » (produit un maillage) ou « llm » (modèle de texte/vision).
#[derive(Serialize, Clone, Copy)]
pub struct TypeFournisseur {
    pub cle: &'static str,
    pub nom: &'static str,
    pub famille: &'static str,
    pub url_defaut: &'static str,
    pub cle_requise: bool,
    pub aide: &'static str,
}

pub const TYPES: &[TypeFournisseur] = &[
    TypeFournisseur {
        cle: "hunyuan3d",
        nom: "Hunyuan3D-2 / 2.1 (serveur API)",
        famille: "image3d",
        url_defaut: "http://127.0.0.1:8081",
        cle_requise: false,
        aide: "Démarre « python api_server.py --port 8081 » dans le dépôt Hunyuan3D-2 ou 2.1 (GPU NVIDIA).",
    },
    TypeFournisseur {
        cle: "generique",
        nom: "Serveur image→3D générique (HTTP)",
        famille: "image3d",
        url_defaut: "http://127.0.0.1:8000/generate",
        cle_requise: false,
        aide: "Reçoit l'image en multipart (champ « image ») et renvoie un GLB, OBJ, STL ou PLY, \
               ou un JSON contenant « model_base64 » ou « url ». Convient à TripoSR, Stable Fast 3D, TRELLIS, etc.",
    },
    TypeFournisseur {
        cle: "lmstudio",
        nom: "LM Studio",
        famille: "llm",
        url_defaut: "http://127.0.0.1:1234/v1",
        cle_requise: false,
        aide: "Active le serveur local dans LM Studio (onglet Developer) et charge un modèle vision.",
    },
    TypeFournisseur {
        cle: "ollama",
        nom: "Ollama",
        famille: "llm",
        url_defaut: "http://127.0.0.1:11434/v1",
        cle_requise: false,
        aide: "Ex. : « ollama pull qwen2.5vl » ou « llava ». L'API compatible OpenAI est sous /v1.",
    },
    TypeFournisseur {
        cle: "openrouter",
        nom: "OpenRouter",
        famille: "llm",
        url_defaut: "https://openrouter.ai/api/v1",
        cle_requise: true,
        aide: "Clé API sur openrouter.ai/keys. Choisis un modèle qui accepte les images.",
    },
    TypeFournisseur {
        cle: "openai",
        nom: "Autre (compatible OpenAI)",
        famille: "llm",
        url_defaut: "http://127.0.0.1:8080/v1",
        cle_requise: false,
        aide: "vLLM, llama.cpp server, LocalAI, OpenAI, Mistral… tout point d'accès /v1/chat/completions.",
    },
];

pub fn type_de(cle: &str) -> Option<&'static TypeFournisseur> {
    TYPES.iter().find(|t| t.cle == cle)
}

// ------------------------------------------------------------------ configuration

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Fournisseur {
    #[serde(default)]
    pub id: String,
    pub nom: String,
    #[serde(rename = "type")]
    pub genre: String,
    pub url: String,
    #[serde(default)]
    pub cle: String,
    #[serde(default)]
    pub modele: String,
    /// Délai maximal d'une génération, en secondes.
    #[serde(default = "delai_defaut")]
    pub delai: u64,
}
fn delai_defaut() -> u64 {
    900
}

impl Fournisseur {
    pub fn famille(&self) -> &'static str {
        type_de(&self.genre).map(|t| t.famille).unwrap_or("llm")
    }
    /// Vue envoyée au navigateur : la clé n'est jamais renvoyée.
    pub fn public(&self) -> Value {
        json!({
            "id": self.id, "nom": self.nom, "type": self.genre, "famille": self.famille(),
            "url": self.url, "modele": self.modele, "delai": self.delai,
            "cle_definie": !self.cle.is_empty(),
        })
    }
}

fn fichier_config() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join(APP).join("fournisseurs.json")
}

pub fn lire_config() -> Vec<Fournisseur> {
    std::fs::read_to_string(fichier_config())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn ecrire_config(liste: &[Fournisseur]) -> Result<()> {
    let f = fichier_config();
    std::fs::create_dir_all(f.parent().unwrap())?;
    std::fs::write(&f, serde_json::to_string_pretty(liste)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Ajoute ou met à jour un fournisseur. Une clé vide conserve la clé existante.
pub fn enregistrer(mut f: Fournisseur) -> Result<Fournisseur> {
    if type_de(&f.genre).is_none() {
        bail!("Type de fournisseur inconnu : {}", f.genre);
    }
    f.nom = f.nom.trim().to_string();
    f.url = f.url.trim().trim_end_matches('/').to_string();
    if f.nom.is_empty() {
        f.nom = type_de(&f.genre).unwrap().nom.to_string();
    }
    if !(f.url.starts_with("http://") || f.url.starts_with("https://")) {
        bail!("L'adresse doit commencer par http:// ou https://");
    }
    f.delai = f.delai.clamp(30, 7200);
    let mut liste = lire_config();
    if let Some(ex) = liste.iter_mut().find(|x| !f.id.is_empty() && x.id == f.id) {
        if f.cle.is_empty() {
            f.cle = ex.cle.clone();
        }
        *ex = f.clone();
    } else {
        f.id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        liste.push(f.clone());
    }
    ecrire_config(&liste)?;
    Ok(f)
}

pub fn supprimer(id: &str) -> Result<()> {
    let mut liste = lire_config();
    let n = liste.len();
    liste.retain(|f| f.id != id);
    if liste.len() == n {
        bail!("Fournisseur introuvable.");
    }
    ecrire_config(&liste)
}

pub fn trouver(id: &str) -> Result<Fournisseur> {
    lire_config()
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| anyhow!("Fournisseur introuvable. Configure-le dans « Fournisseurs IA »."))
}

// ------------------------------------------------------------------ client HTTP

fn client(delai: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("gs3dcreator/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(delai)
        .build()?)
}

fn avec_cle(rb: reqwest::RequestBuilder, f: &Fournisseur) -> reqwest::RequestBuilder {
    let mut rb = if f.cle.is_empty() { rb } else { rb.bearer_auth(&f.cle) };
    if f.genre == "openrouter" {
        rb = rb
            .header("HTTP-Referer", "https://github.com/gsavard/gs3DCreator")
            .header("X-Title", "gs3Dcreator");
    }
    rb
}

fn message_reseau(e: reqwest::Error, url: &str) -> anyhow::Error {
    if e.is_connect() {
        anyhow!("Impossible de joindre {url}. Le serveur est-il démarré ?")
    } else if e.is_timeout() {
        anyhow!("Délai dépassé en attendant {url}.")
    } else {
        anyhow!("Erreur réseau ({url}) : {e}")
    }
}

async fn verifier(r: reqwest::Response) -> Result<reqwest::Response> {
    if r.status().is_success() {
        return Ok(r);
    }
    let code = r.status();
    let corps = r.text().await.unwrap_or_default();
    let detail: String = serde_json::from_str::<Value>(&corps)
        .ok()
        .and_then(|j| {
            j.pointer("/error/message")
                .or(j.get("detail"))
                .or(j.get("text"))
                .or(j.get("error"))
                .map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))
        })
        .unwrap_or_else(|| corps.chars().take(300).collect());
    bail!("Le fournisseur a répondu {code} : {detail}")
}

/// Teste la connexion. Pour un LLM, renvoie la liste des modèles disponibles.
pub async fn tester(f: &Fournisseur) -> Result<(String, Vec<String>)> {
    let c = client(Duration::from_secs(15))?;
    match f.genre.as_str() {
        "hunyuan3d" => {
            // 2.1 expose /health ; 2.0 n'a pas de point de santé : on vérifie que /docs répond.
            for chemin in ["/health", "/docs"] {
                let url = format!("{}{chemin}", f.url);
                if let Ok(r) = c.get(&url).send().await
                    && r.status().is_success()
                {
                    let v = if chemin == "/health" { "2.1" } else { "2.0" };
                    return Ok((format!("Serveur Hunyuan3D joignable (API {v})."), vec![]));
                }
            }
            c.get(&f.url).send().await.map_err(|e| message_reseau(e, &f.url))?;
            Ok((
                "Le serveur répond, mais ce n'est peut-être pas Hunyuan3D.".into(),
                vec![],
            ))
        }
        "generique" => {
            c.get(&f.url).send().await.map_err(|e| message_reseau(e, &f.url))?;
            Ok(("Le serveur répond.".into(), vec![]))
        }
        _ => {
            let url = format!("{}/models", f.url);
            let r = avec_cle(c.get(&url), f)
                .send()
                .await
                .map_err(|e| message_reseau(e, &url))?;
            let j: Value = verifier(r).await?.json().await.context("Réponse /models illisible")?;
            let mut modeles: Vec<String> = j
                .get("data")
                .and_then(|d| d.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.get("id")?.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            modeles.sort();
            Ok((format!("Connexion réussie : {} modèle(s).", modeles.len()), modeles))
        }
    }
}

// ------------------------------------------------------------------ analyse par LLM vision

const CONSIGNE: &str = "Tu prépares une image pour un générateur image→3D (Hunyuan3D, TRELLIS, TripoSR) \
dont le résultat sera imprimé en 3D. Réponds en français, en 6 lignes au plus : \
1) ce que montre l'image (sujet principal) ; \
2) si l'image convient (un seul sujet entier, bien éclairé, fond simple, vue de face ou de trois quarts) ; \
3) les problèmes probables (parties coupées, éléments fins ou flottants, transparence, plusieurs sujets) ; \
4) une suggestion concrète pour améliorer le résultat ; \
5) une hauteur d'impression raisonnable en mm.";

pub async fn analyser(f: &Fournisseur, image: &[u8], mime: &str, question: &str) -> Result<String> {
    if f.famille() != "llm" {
        bail!("Choisis un fournisseur LLM (LM Studio, Ollama, OpenRouter…) pour l'analyse.");
    }
    if f.modele.is_empty() {
        bail!("Choisis un modèle pour ce fournisseur (bouton Tester).");
    }
    let texte = if question.trim().is_empty() { CONSIGNE } else { question };
    let corps = json!({
        "model": f.modele,
        "temperature": 0.2,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": texte},
                {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{}", B64.encode(image))}}
            ]
        }]
    });
    let url = format!("{}/chat/completions", f.url);
    let c = client(Duration::from_secs(f.delai.min(600)))?;
    let r = avec_cle(c.post(&url), f)
        .json(&corps)
        .send()
        .await
        .map_err(|e| message_reseau(e, &url))?;
    let j: Value = verifier(r).await?.json().await.context("Réponse du LLM illisible")?;
    j.pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow!("Le LLM n'a renvoyé aucun texte. Le modèle accepte-t-il les images ?"))
}

// ------------------------------------------------------------------ génération image → 3D

#[derive(Deserialize, Clone)]
pub struct Options {
    #[serde(default = "graine")]
    pub graine: u64,
    #[serde(default = "resolution")]
    pub resolution: u32,
    #[serde(default = "etapes")]
    pub etapes: u32,
    #[serde(default = "guidage")]
    pub guidage: f64,
    #[serde(default = "vrai")]
    pub retirer_fond: bool,
    #[serde(default = "cent_cinquante")]
    pub hauteur: f64,
    #[serde(default = "auto")]
    pub axe: String,
    #[serde(default)]
    pub socle: bool,
}
fn graine() -> u64 {
    1234
}
fn resolution() -> u32 {
    256
}
fn etapes() -> u32 {
    30
}
fn guidage() -> f64 {
    5.0
}
fn vrai() -> bool {
    true
}
fn cent_cinquante() -> f64 {
    150.0
}
fn auto() -> String {
    "auto".into()
}

impl Default for Options {
    fn default() -> Self {
        serde_json::from_value(json!({})).unwrap()
    }
}

/// Appelle le fournisseur et renvoie (octets du maillage, extension).
pub async fn generer(
    f: &Fournisseur,
    image: &[u8],
    mime: &str,
    nom_image: &str,
    o: &Options,
) -> Result<(Vec<u8>, String)> {
    if f.famille() != "image3d" {
        bail!(
            "« {} » est un modèle de langage : il ne produit pas de maillage 3D. \
             Choisis un serveur image→3D (Hunyuan3D, générique).",
            f.nom
        );
    }
    let c = client(Duration::from_secs(f.delai))?;
    let octets = match f.genre.as_str() {
        "hunyuan3d" => {
            let url = format!("{}/generate", f.url);
            let corps = json!({
                "image": B64.encode(image),
                "remove_background": o.retirer_fond,
                "texture": false,
                "seed": o.graine,
                "octree_resolution": o.resolution.clamp(64, 512),
                "num_inference_steps": o.etapes.clamp(1, 100),
                "guidance_scale": o.guidage.clamp(0.1, 20.0),
                "type": "glb",
            });
            let r = avec_cle(c.post(&url), f)
                .json(&corps)
                .send()
                .await
                .map_err(|e| message_reseau(e, &url))?;
            verifier(r).await?.bytes().await?.to_vec()
        }
        _ => {
            let partie = reqwest::multipart::Part::bytes(image.to_vec())
                .file_name(nom_image.to_string())
                .mime_str(mime)?;
            let formulaire = reqwest::multipart::Form::new()
                .part("image", partie)
                .text("seed", o.graine.to_string())
                .text("remove_background", o.retirer_fond.to_string())
                .text("resolution", o.resolution.to_string())
                .text("steps", o.etapes.to_string())
                .text("guidance_scale", o.guidage.to_string());
            let r = avec_cle(c.post(&f.url), f)
                .multipart(formulaire)
                .send()
                .await
                .map_err(|e| message_reseau(e, &f.url))?;
            let r = verifier(r).await?;
            let json_ct = r
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.contains("json"));
            let b = r.bytes().await?.to_vec();
            if json_ct { depuis_json(&c, f, &b).await? } else { b }
        }
    };
    let ext = deviner_format(&octets)
        .ok_or_else(|| anyhow!("Réponse du fournisseur non reconnue (ni GLB, OBJ, STL ni PLY)."))?;
    Ok((octets, ext.to_string()))
}

/// Réponse JSON d'un serveur générique : maillage en base64 ou adresse à télécharger.
async fn depuis_json(c: &reqwest::Client, f: &Fournisseur, b: &[u8]) -> Result<Vec<u8>> {
    let j: Value = serde_json::from_slice(b).context("JSON illisible")?;
    for cle in [
        "model_base64",
        "glb_base64",
        "mesh_base64",
        "base64",
        "glb",
        "mesh",
        "model",
    ] {
        if let Some(s) = j.get(cle).and_then(|v| v.as_str()) {
            let s = s.split_once("base64,").map(|x| x.1).unwrap_or(s);
            if let Ok(o) = B64.decode(s.trim()) {
                return Ok(o);
            }
        }
    }
    for cle in ["url", "model_url", "glb_url", "mesh_url", "file"] {
        if let Some(u) = j.get(cle).and_then(|v| v.as_str()) {
            let u = if u.starts_with("http") {
                u.to_string()
            } else {
                // adresse relative au serveur
                let base = reqwest::Url::parse(&f.url)?;
                base.join(u)?.to_string()
            };
            let r = avec_cle(c.get(&u), f).send().await.map_err(|e| message_reseau(e, &u))?;
            return Ok(verifier(r).await?.bytes().await?.to_vec());
        }
    }
    bail!("Le JSON reçu ne contient ni « model_base64 » ni « url ».")
}

fn deviner_format(o: &[u8]) -> Option<&'static str> {
    if o.starts_with(b"glTF") {
        return Some("glb");
    }
    if o.starts_with(b"ply") {
        return Some("ply");
    }
    if o.len() >= 84 {
        let n = u32::from_le_bytes([o[80], o[81], o[82], o[83]]) as usize;
        if 84 + n * 50 == o.len() {
            return Some("stl");
        }
    }
    let debut = String::from_utf8_lossy(&o[..o.len().min(4096)]);
    if debut.trim_start().starts_with("solid") {
        return Some("stl");
    }
    if debut.lines().any(|l| l.starts_with("v ")) {
        return Some("obj");
    }
    if debut.starts_with('{') && debut.contains("\"asset\"") {
        return Some("gltf");
    }
    None
}

/// Convertit le maillage reçu : réparation, orientation, mise à l'échelle, socle optionnel,
/// puis écriture du STL et de l'aperçu GLB.
pub fn convertir(source: &Path, dossier: &Path, nom: &str, o: &Options) -> Result<Value> {
    // Le socle ajoute 2,4 mm sous le modèle : la hauteur totale reste celle demandée.
    let h = o.hauteur.clamp(5.0, 2000.0) - if o.socle { 2.4 } else { 0.0 };
    let (mut tm, axe) = mo::charger(source, &o.axe, Some(h))?;
    if o.socle {
        tm = ajouter_socle(&tm)?;
    }
    let stl = dossier.join(format!("{nom}.stl"));
    ml::ecrire_stl(&tm, &stl)?;
    ml::ecrire_glb(&[(nom.to_string(), tm.clone())], &dossier.join("apercu.glb"))?;
    let e = tm.etendue();
    let etanche = ml::vers_manifold(&tm).is_ok();
    Ok(json!({
        "axe": axe,
        "triangles": tm.faces.len(),
        "dims": e.map(|v| (v * 10.0).round() / 10.0),
        "etanche": etanche,
        "stl": format!("{nom}.stl"),
    }))
}

/// Socle cylindrique de 3 mm sous le modèle, fusionné (si le maillage est étanche).
fn ajouter_socle(tm: &ml::Maillage) -> Result<ml::Maillage> {
    let m = ml::vers_manifold(tm).context("Le maillage n'est pas étanche : socle impossible.")?;
    let e = tm.etendue();
    let r = (e[0].max(e[1]) * 0.6).max(10.0);
    let socle = mo::cylindre(r, 3.0, 0.6);
    let socle = ml::appliquer(&socle, &ml::translation([0.0, 0.0, -2.4]));
    let mut res = ml::vers_maillage(&m.union(&socle));
    let (lo, _) = res.bornes();
    res.translater([0.0, 0.0, -lo[2]]);
    Ok(res)
}

// ------------------------------------------------------------------ tâches

#[derive(Serialize, Clone)]
pub struct Tache {
    pub id: String,
    pub nom: String,
    pub fournisseur: String,
    pub etat: String,
    pub message: String,
    #[serde(skip)]
    pub debut: Option<Instant>,
    pub duree: f64,
    pub resultat: Option<Value>,
    #[serde(skip)]
    pub dossier: PathBuf,
}

pub type Taches = Arc<Mutex<HashMap<String, Tache>>>;

pub fn maj(t: &Taches, id: &str, etat: &str, message: &str) {
    if let Some(x) = t.lock().unwrap().get_mut(id) {
        x.etat = etat.into();
        x.message = message.into();
        if let Some(d) = x.debut {
            x.duree = (d.elapsed().as_secs_f64() * 10.0).round() / 10.0;
        }
    }
}

/// Exécute une génération complète dans une tâche tokio.
pub fn lancer(taches: Taches, id: String, f: Fournisseur, image: Vec<u8>, mime: String, nom_image: String, o: Options) {
    tokio::spawn(async move {
        maj(&taches, &id, "generation", &format!("Génération par {}…", f.nom));
        let (dossier, nom) = {
            let t = taches.lock().unwrap();
            let x = &t[&id];
            (x.dossier.clone(), x.nom.clone())
        };
        let res: Result<Value> = async {
            let (octets, ext) = generer(&f, &image, &mime, &nom_image, &o).await?;
            let source = dossier.join(format!("brut.{ext}"));
            std::fs::write(&source, &octets)?;
            maj(&taches, &id, "conversion", "Réparation et conversion en STL…");
            let o2 = o.clone();
            let d2 = dossier.clone();
            tokio::task::spawn_blocking(move || convertir(&source, &d2, &nom, &o2)).await?
        }
        .await;
        match res {
            Ok(v) => {
                if let Some(x) = taches.lock().unwrap().get_mut(&id) {
                    x.resultat = Some(v);
                }
                maj(&taches, &id, "termine", "Modèle prêt.");
            }
            Err(e) => maj(&taches, &id, "erreur", &e.to_string()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(deviner_format(b"glTF\x02\0\0\0"), Some("glb"));
        assert_eq!(deviner_format(b"ply\nformat ascii 1.0\n"), Some("ply"));
        assert_eq!(deviner_format(b"solid x\nfacet normal 0 0 1\n"), Some("stl"));
        assert_eq!(deviner_format(b"# obj\nv 0 0 0\nv 1 0 0\n"), Some("obj"));
        let mut stl = vec![0u8; 84 + 50];
        stl[80] = 1;
        assert_eq!(deviner_format(&stl), Some("stl"));
        assert_eq!(deviner_format(b"<html>"), None);
    }

    #[test]
    fn options_defaut() {
        let o = Options::default();
        assert_eq!(o.resolution, 256);
        assert!(o.retirer_fond);
    }
}
