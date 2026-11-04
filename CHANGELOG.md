# Historique des versions

## 2.0.0 — 2026-11-04

Réécriture complète en Rust sous le nom **gs3Dcreator**.

- Un seul binaire (`gs3dcreator`) : plus d’environnement Python ni de `pip install`.
- Mêmes algorithmes que la 1.2 : repères anatomiques, gabarits 6/5/4a/4b/3/2, coupes
  localisées, rotules adaptatives, goujons, tenons, orientation à plat, 3MF disposé.
- Interface web identique, aux couleurs du nouveau logo; three.js 0.160 intégré au binaire
  (fonctionne hors ligne).
- Ligne de commande unifiée : `ouvrir`, `serveur`, `arreter`, `info`, `profil`,
  `decouper`, `gabarit` (nouveau : gabarit de personnage sans interface).
- Nouveaux formats d’import : 3MF, OFF et PLY en plus de STL, OBJ, GLB/glTF.
- Paquets RPM et DEB avec lanceur, icônes, métadonnées AppStream et page de manuel.
- Publication automatique des paquets par GitHub Actions.

## 1.2 — Python

- Interface web (FastAPI + three.js), gabarits de personnage, coupes indépendantes,
  outil de tracé, rotules.

## 1.0 — Python

- Module en ligne de commande `decoupe3d.py` : plans de coupe, découpe automatique,
  goujons et tenons, export STL et 3MF.
