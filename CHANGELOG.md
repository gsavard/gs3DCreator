# Historique des versions

## 2.2.1 — 2026-11-24

Amélioration intelligente de la découpe et correction des chevauchements de plans.

- **Découpes bornées (rayon d’action)** : chaque plan de coupe dispose d’un rayon d’action
  local calculé automatiquement selon la morphologie du membre, évitant d'entamer les membres
  voisins (ex. l’épaule ou l’aile levée quand on coupe le cou).
- **Tri anatomique prioritaire** : les membres latéraux (bras, ailes, armes) sont détachés en
  priorité avant la tête et le torse, supprimant les fragments orphelins dus aux plans sécants.
- **Interface 3D enrichie** : les plans de coupe s’affichent sous forme de disques circulaires
  délimités dans Three.js, avec un champ de réglage du rayon et des boutons de priorité (▲ / ▼).

## 2.2.0 — 2026-11-16

Nouvelle section **Colorier**, inspirée de MelSplit : choisis les parties d’un modèle et
donne-leur une couleur, puis exporte en 3MF multicolore ou en STL par couleur.

- Onglet « Colorier » : ouvre un STL ou un 3MF (OBJ, GLB, PLY et OFF aussi). Les couleurs
  déjà présentes dans un 3MF (matériaux de base ou groupes de couleurs) sont conservées.
- **Détail** (pot de peinture) : un clic colorie la zone jusqu’aux arêtes vives, avec une
  taille maximale et une sensibilité aux arêtes réglables; aperçu de la zone au survol.
- **Pinceau** et **Gomme** à rayon réglable (clic droit ou Alt + glisser pour tourner la vue).
- Palette modifiable (sélecteur de couleur, couleurs rapides, part de la surface par couleur),
  Remplir tout, Tout effacer, Annuler / Rétablir (Ctrl+Z, Ctrl+Maj+Z).
- Export : un 3MF avec une couleur par triangle et un ZIP contenant un STL par couleur
  (mêmes coordonnées, donc superposables dans le trancheur).
- Nouvelles routes `/api/couleur*` (voir docs/API.md).

## 2.1.0 — 2026-11-04

Nouvelle section **Image → 3D**, inspirée de la génération 3D de Hi3D.

- Onglet « Image → 3D » : dépose (ou colle) une image PNG/JPG/WEBP, choisis le moteur,
  la qualité (Rapide, Standard, Détail), la hauteur et un socle optionnel, puis télécharge
  le STL réparé et mis à l’échelle. Historique des générations de la session avec aperçu 3D.
- Fournisseurs IA (bouton ⚙) :
  - **Image → 3D** : serveur API Hunyuan3D-2 / 2.1, ou serveur HTTP générique (TripoSR,
    Stable Fast 3D, TRELLIS…) qui renvoie un GLB, OBJ, STL ou PLY.
  - **Modèles de langage** : LM Studio, Ollama, OpenRouter et tout point d’accès compatible
    OpenAI. Servent à vérifier l’image avant la génération (LLM vision).
- Test de connexion et liste des modèles disponibles; clés API stockées dans
  `~/.config/gs3dcreator/fournisseurs.json` (droits 600) et jamais renvoyées au navigateur.
- Générations en tâche de fond : aucune limite de temps côté navigateur (délai réglable par
  fournisseur, 15 min par défaut).
- Nouvelles routes `/api/fournisseurs` et `/api/images` (voir docs/API.md).

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
