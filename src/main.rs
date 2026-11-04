//! gs3Dcreator — découpe un modèle 3D (figurine, statue…) en pièces imprimables
//! avec rotules, goujons ou tenons d'assemblage.

mod cli;
mod lanceur;
mod maillage;
mod moteur;
mod sections;
mod serveur;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use crate::moteur::Config;

#[derive(Parser)]
#[command(
    name = "gs3dcreator",
    version,
    about = "Découpe un modèle 3D en pièces imprimables, avec connecteurs d'assemblage.",
    long_about = "Sans sous-commande, gs3dcreator démarre l'interface web locale (si elle ne \
                  tourne pas déjà) et l'ouvre dans le navigateur."
)]
struct Args {
    #[command(subcommand)]
    commande: Option<Commande>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Connecteur {
    Rotule,
    Goujon,
    Tenon,
    Aucun,
}

impl Connecteur {
    fn nom(self) -> &'static str {
        match self {
            Connecteur::Rotule => "rotule",
            Connecteur::Goujon => "goujon",
            Connecteur::Tenon => "tenon",
            Connecteur::Aucun => "aucun",
        }
    }
}

#[derive(clap::Args)]
struct Assemblage {
    /// Rayon du connecteur (mm)
    #[arg(long, default_value_t = 2.5)]
    rayon: f64,
    /// Profondeur par côté (mm)
    #[arg(long, default_value_t = 6.0)]
    profondeur: f64,
    /// Jeu radial (mm)
    #[arg(long, default_value_t = 0.2)]
    jeu: f64,
    /// Paroi minimale autour du trou (mm)
    #[arg(long, default_value_t = 1.6)]
    paroi: f64,
    /// Connecteurs max par section (2 = anti-rotation)
    #[arg(long = "par-ilot", default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=2))]
    par_ilot: u8,
    /// Volume d'impression X Y Z (mm)
    #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], default_values_t = [250.0, 250.0, 250.0])]
    volume: Vec<f64>,
}

impl Assemblage {
    fn config(&self, c: Connecteur) -> Config {
        Config {
            connecteur: c.nom().into(),
            rayon: self.rayon,
            jeu: self.jeu,
            profondeur: self.profondeur,
            paroi: self.paroi,
            par_ilot: self.par_ilot as usize,
            volume: [self.volume[0], self.volume[1], self.volume[2]],
        }
    }
}

#[derive(Subcommand)]
enum Commande {
    /// Ouvre l'interface (démarre le serveur au besoin) — action par défaut
    Ouvrir,
    /// Démarre seulement le serveur web, au premier plan
    Serveur {
        /// Adresse d'écoute
        #[arg(long, default_value = "127.0.0.1")]
        hote: String,
        /// Port (0 = port libre choisi par le système)
        #[arg(long, default_value_t = lanceur::PORT_PREFERE)]
        port: u16,
        /// Ouvre aussi le navigateur
        #[arg(long)]
        ouvrir: bool,
    },
    /// Arrête le serveur lancé par « gs3dcreator ouvrir »
    Arreter,
    /// Affiche les dimensions et l'étanchéité d'un modèle
    Info { modele: PathBuf },
    /// Profil de sections tranche par tranche (îlots et aires) le long d'un axe
    Profil {
        modele: PathBuf,
        #[arg(long, default_value = "z", value_parser = ["x", "y", "z"])]
        axe: String,
        /// Pas entre les tranches (mm)
        #[arg(long, default_value_t = 5.0)]
        pas: f64,
    },
    /// Découpe par plans libres et/ou automatiquement pour entrer dans le volume d'impression
    Decouper {
        modele: PathBuf,
        /// Dossier de sortie
        #[arg(short, long, default_value = "pieces")]
        sortie: PathBuf,
        /// Plan de coupe : « z=120 » ou « nx,ny,nz,px,py,pz » (répétable)
        #[arg(long = "plan", allow_hyphen_values = true)]
        plans: Vec<String>,
        /// Coupe automatiquement jusqu'à entrer dans --volume
        #[arg(long)]
        auto: bool,
        /// Facteur d'échelle appliqué au modèle
        #[arg(long, default_value_t = 1.0)]
        echelle: f64,
        #[arg(long, value_enum, default_value = "goujon")]
        connecteur: Connecteur,
        #[command(flatten)]
        assemblage: Assemblage,
    },
    /// Découpe un personnage selon un gabarit (6, 5, 4a, 4b, 3, 2 pièces) détecté automatiquement
    Gabarit {
        modele: PathBuf,
        /// Gabarit : 6, 5, 4a, 4b, 3 ou 2
        #[arg(short, long, default_value = "6")]
        gabarit: String,
        #[arg(short, long, default_value = "pieces")]
        sortie: PathBuf,
        /// Axe vertical du modèle : auto, x, y ou z
        #[arg(long, default_value = "auto", value_parser = ["auto", "x", "y", "z"])]
        axe: String,
        /// Hauteur finale de la figurine (mm)
        #[arg(long, default_value_t = 150.0)]
        hauteur: f64,
        #[arg(long, value_enum, default_value = "rotule")]
        connecteur: Connecteur,
        #[command(flatten)]
        assemblage: Assemblage,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    let r = match args.commande.unwrap_or(Commande::Ouvrir) {
        Commande::Ouvrir => lanceur::ouvrir(),
        Commande::Serveur { hote, port, ouvrir } => lanceur::servir(&hote, port, ouvrir),
        Commande::Arreter => lanceur::arreter(),
        Commande::Info { modele } => {
            cli::charger_et_reparer(&modele, 1.0).map(|tm| cli::afficher_info(&modele.display().to_string(), &tm))
        }
        Commande::Profil { modele, axe, pas } => cli::charger_et_reparer(&modele, 1.0).and_then(|tm| {
            cli::afficher_info(&modele.display().to_string(), &tm);
            cli::afficher_profil(&tm, &axe, pas)
        }),
        Commande::Decouper {
            modele,
            sortie,
            plans,
            auto,
            echelle,
            connecteur,
            assemblage,
        } => cli::decouper_plans(&cli::OptionsDecoupe {
            modele,
            sortie,
            plans,
            auto,
            echelle,
            cfg: assemblage.config(connecteur),
        }),
        Commande::Gabarit {
            modele,
            gabarit,
            sortie,
            axe,
            hauteur,
            connecteur,
            assemblage,
        } => cli::decouper_gabarit(
            &modele,
            &sortie,
            &gabarit,
            &axe,
            Some(hauteur),
            &assemblage.config(connecteur),
        ),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}
