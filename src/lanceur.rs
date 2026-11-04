//! Lancement de l'application de bureau : un seul serveur par utilisateur,
//! port préféré 8790 (ou un port libre), ouverture dans le navigateur.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::serveur::{self, APP};

pub const PORT_PREFERE: u16 = 8790;

fn dossier_etat() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    let d = base.join(APP);
    let _ = std::fs::create_dir_all(&d);
    d
}

fn lire(nom: &str) -> Option<String> {
    std::fs::read_to_string(dossier_etat().join(nom))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Vrai seulement si c'est bien gs3Dcreator qui répond sur ce port.
fn notre(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    if s.write_all(b"GET /api/sante HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut r = String::new();
    let _ = s.read_to_string(&mut r);
    r.contains(&format!("\"app\":\"{APP}\""))
}

fn avis(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["-i", APP, "gs3Dcreator", msg])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Ouvre l'URL en mode application (fenêtre sans barre d'adresse) si un navigateur
/// Chromium est présent, sinon avec le navigateur par défaut.
pub fn navigateur(url: &str) {
    let chromiums = [
        "google-chrome",
        "google-chrome-stable",
        "chromium-browser",
        "chromium",
        "brave-browser",
        "microsoft-edge",
    ];
    for b in chromiums {
        let ok = Command::new(b)
            .arg(format!("--app={url}"))
            .arg(format!("--class={APP}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
        if ok {
            return;
        }
    }
    if Command::new("xdg-open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_err()
    {
        println!("Ouvre {url} dans ton navigateur.");
    }
}

/// Action par défaut : réutilise le serveur existant ou en démarre un.
pub fn ouvrir() -> Result<()> {
    if let Some(p) = lire("port").and_then(|p| p.parse::<u16>().ok()) {
        if notre(p) {
            navigateur(&format!("http://127.0.0.1:{p}"));
            return Ok(());
        }
    }
    let port = if TcpListener::bind(("127.0.0.1", PORT_PREFERE)).is_ok() {
        PORT_PREFERE
    } else {
        0
    };
    servir("127.0.0.1", port, true)
}

pub fn arreter() -> Result<()> {
    let d = dossier_etat();
    let arrete = lire("serveur.pid")
        .map(|pid| {
            Command::new("kill")
                .arg(pid)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);
    let _ = std::fs::remove_file(d.join("serveur.pid"));
    let _ = std::fs::remove_file(d.join("port"));
    let msg = if arrete {
        "Serveur arrêté."
    } else {
        "Le serveur ne tournait pas."
    };
    println!("{msg}");
    avis(msg);
    Ok(())
}

pub fn servir(hote: &str, port: u16, ouvrir: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let ecoute = tokio::net::TcpListener::bind((hote, port))
            .await
            .with_context(|| format!("Impossible d'écouter sur {hote}:{port}"))?;
        let port = ecoute.local_addr()?.port();
        let d = dossier_etat();
        let _ = std::fs::write(d.join("port"), port.to_string());
        let _ = std::fs::write(d.join("serveur.pid"), std::process::id().to_string());
        let hote_url = if hote == "0.0.0.0" { "127.0.0.1" } else { hote };
        let url = format!("http://{hote_url}:{port}");
        println!(
            "\n  gs3Dcreator {} : {url}\n  (Ctrl+C pour arrêter)\n",
            serveur::VERSION
        );
        if ouvrir {
            navigateur(&url);
        }
        axum::serve(ecoute, serveur::routeur())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        let _ = std::fs::remove_file(d.join("port"));
        let _ = std::fs::remove_file(d.join("serveur.pid"));
        Ok(())
    })
}
