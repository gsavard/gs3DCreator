# Guide d’utilisation de gs3Dcreator

Ce guide suit le parcours de l’interface, de l’import du modèle à l’assemblage des pièces
imprimées, puis détaille la ligne de commande et les réglages.

## 1. Préparer le modèle

gs3Dcreator accepte **STL, OBJ, GLB/glTF, PLY, 3MF et OFF**. Le modèle doit être un solide
fermé (étanche). À l’import, l’application :

- fusionne les sommets confondus et retire les faces dégénérées ou en double;
- rend l’orientation des faces cohérente (normales vers l’extérieur);
- bouche les petits trous (boucles d’arêtes de bord).

Si le message « Le maillage n’est pas fermé (non manifold) » apparaît, répare le modèle dans
Blender (*3D Print Toolbox* → *Make Manifold*) ou Meshmixer, puis réessaie.

### Axe vertical

| Choix | Quand l’utiliser |
|---|---|
| Auto | Par défaut. Y pour les GLB/glTF et les modèles normalisés (≤ 1,5 unité), sinon Z |
| Y | Modèles générés par IA (Meshy, Tripo, Hunyuan…), exports glTF |
| Z | STL de CAO ou déjà orientés pour l’impression |
| X | Modèle couché sur le côté |

Si la figurine apparaît couchée, change l’axe puis clique **Appliquer l’axe et la hauteur**.

### Hauteur

La hauteur finale (mm) met le modèle à l’échelle. Le modèle est ensuite centré sur le
plateau, pieds à Z = 0.

## 2. Choisir un gabarit

L’analyse de la silhouette coupe le modèle en 96 tranches horizontales et repère :

- **l’entrejambe** : on suit les deux jambes depuis le bas jusqu’à leur fusion;
- **la taille** : un peu au-dessus de l’entrejambe;
- **le cou** : un vrai creux, avec la tête plus large au-dessus et les épaules plus larges
  en dessous;
- **les épaules** : la largeur du tronc au niveau de l’abdomen, sur l’axe latéral.

| Gabarit | Pièces |
|---|---|
| 6 | tête, torse, 2 bras, 2 jambes |
| 5 | torse avec tête, 2 bras, 2 jambes |
| 4 (bras) | torse avec tête, 2 bras, jambes ensemble |
| 4 (jambes) | tête, torse avec bras, 2 jambes |
| 3 | tête, torse avec bras, jambes ensemble |
| 2 | tête, reste du corps |

Si les jambes ou le cou ne sont pas détectés (robe, socle, pose fermée), un avertissement
s’affiche et des niveaux par défaut sont utilisés : ajuste les plans à la main.

## 3. Ajuster les coupes

Chaque coupe est un plan avec une **flèche blanche** qui pointe vers la pièce détachée.

- **Déplacer (T)** : tire les flèches du manipulateur, ou saisis X, Y, Z en mm.
- **Incliner (R)** : tire les anneaux (pas de 1°).
- **Tracer (D)** : dessine un trait à travers le membre, à l’angle voulu. Le plan contient
  le trait et la direction de vue, et il est centré au milieu de la matière sous le trait.
- **Inverser** : détache l’autre côté du plan.
- **Ajouter une coupe** : ajoute un plan libre (Pièce 1, Pièce 2…).
- **Coupes du gabarit** : revient aux plans proposés.
- **Échap** : désélectionne.

### Comment une coupe est appliquée

Les coupes s’appliquent dans l’ordre de la liste. Pour chaque coupe :

1. la pièce qui contient le centre du plan est coupée en deux;
2. du côté de la flèche, seule la partie qui touche la section la plus proche du centre
   est détachée; tout le reste (un bras qui croise le plan de la tête, par exemple) reste
   dans la pièce d’origine;
3. les connecteurs sont percés dans la face de coupe.

Une coupe dont le centre tombe dans une pièce déjà détachée la recoupe : c’est ainsi qu’on
sépare une botte d’une jambe ou un avant-bras d’un bras.

Les morceaux isolés qui n’appartiennent à aucune pièce deviennent des **fragments**
(à coller). Déplacer le plan de quelques millimètres les fait souvent disparaître.

## 4. Assemblage

| Connecteur | Description | À imprimer en plus |
|---|---|---|
| Rotules | Une cavité sphérique de chaque côté, centrée au point le plus épais de la section. La boule a une base aplatie pour s’imprimer sans support. Son rayon s’adapte à la section et à l’épaisseur réelle du modèle pour ne jamais percer la surface. | `boule_rX.X.stl` (une par joint) |
| Goujons | Un ou deux trous cylindriques par section (deux = anti-rotation), profondeur limitée à 70 % de l’épaisseur de chaque côté. | `goujon.stl` (×N) |
| Tenons | Un tenon dépasse du corps, la pièce détachée a le trou correspondant. Rien d’autre à imprimer, mais la face avec tenon ne peut pas être posée à plat. | — |
| Aucun | Faces planes, à coller. | — |

- **Rayon** : rayon du goujon ou du tenon (la boule fait 1,6 × ce rayon, au moins 3 mm).
- **Jeu** : jeu radial ajouté aux trous. 0,2 mm convient à la plupart des imprimantes FDM
  bien calibrées; monte à 0,3 mm si l’assemblage force.

## 5. Résultats

- Chaque pièce est **posée sur sa plus grande face de coupe** (sans tenon) pour s’imprimer
  sans support à cet endroit.
- Le **3MF** contient toutes les pièces et connecteurs disposés en rangées sur un plateau de
  250 mm. Il s’ouvre directement dans Anycubic Slicer Next, Bambu Studio, PrusaSlicer,
  OrcaSlicer ou Cura.
- Le **ZIP** contient un STL par pièce et par type de connecteur, plus le 3MF.
- « Trop grande » signale une pièce qui n’entre pas dans le volume 250 × 250 × 250 mm, même
  en la tournant.

Les vues **Pièces** et **Vue éclatée** montrent le résultat en couleur.

## 6. Ligne de commande

Toutes les fonctions sont aussi accessibles sans interface. Exemples :

```bash
# Découpe d'un personnage en 4 pièces (bras séparés, jambes ensemble), goujons de 3 mm
gs3dcreator gabarit perso.glb -g 4a --hauteur 200 --connecteur goujon --rayon 3 -s perso/

# Trois plans horizontaux et un plan incliné, sans connecteur
gs3dcreator decouper vase.stl -s vase/ --plan z=60 --plan z=120 --plan z=180 \
    --plan "0.2,0,1,0,0,240" --connecteur aucun

# Agrandir 2× puis couper automatiquement pour un plateau de 220 × 220 × 250
gs3dcreator decouper statue.stl --echelle 2 --auto --volume 220 220 250 -s statue/
```

`profil` aide à choisir les niveaux de coupe : une ligne avec 2 îlots dans le bas indique
les jambes, une aire minimale indique un cou, une taille ou un poignet.

```
Profil selon Z (haut en bas) — îlots = morceaux séparés dans la tranche
       z | îlots | aire mm² |
   172.5 |     1 |      388 | ███████████████
   157.5 |     1 |      296 | ████████████
   142.5 |     1 |      980 | ████████████████████████████████████████
```

## 7. Serveur et lanceur

- `gs3dcreator` (ou `gs3dcreator ouvrir`) réutilise le serveur s’il tourne déjà, sinon en
  démarre un sur le port 8790 (ou un port libre) et ouvre l’interface.
- L’action **Arrêter le serveur** du lanceur (clic droit sur l’icône) ou
  `gs3dcreator arreter` l’arrête.
- `gs3dcreator serveur --hote 0.0.0.0 --port 8790` sert l’interface aux autres postes du
  réseau local. Aucune authentification n’est prévue : ne l’expose pas sur Internet.
- Les fichiers de travail sont dans `/tmp/gs3dcreator/` (effacés au redémarrage).
- L’état du lanceur (port, PID) est dans `~/.local/state/gs3dcreator/`.

## 8. Dépannage

| Symptôme | Piste |
|---|---|
| « Section vide au niveau du bassin » | Le modèle n’est pas debout : change l’axe vertical. |
| Une coupe est « ignorée » | Le plan ne traverse pas le modèle, ou la section est vide : déplace-le. |
| Pas de rotule sur un membre fin | La section est trop petite pour une boule de 2 mm + jeu + paroi : prends des goujons plus fins ou aucun connecteur. |
| L’interface ne s’ouvre pas | Lance `gs3dcreator serveur --ouvrir` dans un terminal pour voir les messages. |
| Le port 8790 est pris | Un port libre est choisi automatiquement. |
