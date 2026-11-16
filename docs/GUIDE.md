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

## 9. Image → 3D

L’onglet **Image → 3D** transforme une image en modèle 3D imprimable (STL), à la manière
de la génération 3D de Hi3D. gs3Dcreator ne contient pas de modèle d’IA : il se connecte au
serveur que tu fais tourner (sur ta machine ou un poste avec GPU).

### Deux familles de fournisseurs

| Famille | Exemples | Rôle |
|---|---|---|
| **Image → 3D** | Hunyuan3D-2 / 2.1, TripoSR, Stable Fast 3D, TRELLIS | Produit le maillage. **Obligatoire.** |
| **Modèle de langage** | LM Studio, Ollama, OpenRouter, vLLM, OpenAI… | Analyse l’image avant la génération. Facultatif. |

Un LLM (même vision) ne produit pas de maillage : il décrit l’image et signale les
problèmes (sujet coupé, fond chargé, éléments fins). La forme 3D vient toujours d’un
modèle image→3D dédié.

### Configurer un fournisseur

Bouton **⚙** à côté des onglets, puis **Ajouter un fournisseur** :

| Type | Adresse par défaut | Remarques |
|---|---|---|
| Hunyuan3D-2 / 2.1 | `http://127.0.0.1:8081` | Dans le dépôt Tencent : `python api_server.py --port 8081`. GPU NVIDIA requis (≈ 6 Go pour 2mini, plus pour 2.1). |
| Générique (HTTP) | `http://127.0.0.1:8000/generate` | Voir le contrat ci-dessous. |
| LM Studio | `http://127.0.0.1:1234/v1` | Active le serveur dans l’onglet Developer; charge un modèle vision (Qwen2.5-VL, Gemma 3…). |
| Ollama | `http://127.0.0.1:11434/v1` | `ollama pull qwen2.5vl` (ou `llava`, `gemma3`). |
| OpenRouter | `https://openrouter.ai/api/v1` | Clé API requise; choisis un modèle qui accepte les images. |
| Autre (compatible OpenAI) | — | Tout point d’accès `/v1/chat/completions`. |

**Tester** enregistre le fournisseur, vérifie la connexion et, pour un LLM, liste les
modèles disponibles. La configuration est dans `~/.config/gs3dcreator/fournisseurs.json`
(droits 600). Les clés ne sont jamais renvoyées au navigateur : laisse le champ vide pour
conserver la clé enregistrée.

Si le serveur d’IA tourne sur une autre machine (ex. un poste GPU), mets son adresse
réseau (`http://192.168.x.y:8081`) et assure-toi qu’il écoute sur `0.0.0.0`.

### Générer

1. **Image** : dépose, choisis ou colle (Ctrl+V) une image PNG, JPG ou WEBP (20 Mo max.).
   Idéalement un seul sujet entier, vu de face ou de trois quarts, sur un fond uni.
2. **Moteur 3D** : choisis le serveur. Facultatif : ouvre « Vérifier l’image avec un LLM
   vision » et clique **Analyser l’image**.
3. **Réglages** :
   - **Qualité** : Rapide (résolution 128, 5 étapes, pour les modèles « turbo »), Standard
     (256, 30), Détail (384, 50). Plus c’est élevé, plus c’est long et détaillé.
   - **Hauteur** finale en mm et **axe vertical** (Auto convient aux GLB produits par l’IA).
   - **Socle** : ajoute un disque de 3 mm sous le modèle (si le maillage est étanche).
   - **Avancé** : graine (change la variante), guidage, résolution, étapes, retrait du fond.
4. **Générer le modèle 3D**. La génération tourne en arrière-plan; l’historique à droite
   montre l’état. Une fois prêt, le modèle s’affiche et **Télécharger le STL** devient
   disponible.

Le maillage reçu est réparé (sommets fusionnés, normales, petits trous), redressé, mis à
l’échelle et posé sur le plateau. Pour le découper ensuite, télécharge le STL et ouvre-le
dans l’onglet **Découpe**.

Avec Hunyuan3D-2.1, seule l’image est prise en compte par le serveur : les réglages de
qualité sont ignorés (limite du serveur de Tencent, pas de gs3Dcreator).

### Contrat du serveur générique

gs3Dcreator envoie un `POST multipart/form-data` à l’adresse configurée :

| Champ | Contenu |
|---|---|
| `image` | le fichier image |
| `seed`, `resolution`, `steps`, `guidance_scale`, `remove_background` | réglages (texte) |

Réponses acceptées :

- le fichier 3D directement (GLB, OBJ, STL ou PLY; le format est détecté);
- du JSON avec `model_base64` (ou `glb_base64`, `mesh_base64`) contenant le fichier;
- du JSON avec `url` (absolue ou relative au serveur) vers le fichier à télécharger.

Un petit serveur FastAPI autour de TripoSR ou Stable Fast 3D suffit pour respecter ce
contrat. Un en-tête `Authorization: Bearer <clé>` est envoyé si une clé est définie.

### Dépannage Image → 3D

| Symptôme | Piste |
|---|---|
| « Impossible de joindre … » | Le serveur d’IA n’est pas démarré, ou il écoute sur `127.0.0.1` d’une autre machine. |
| « Délai dépassé » | Augmente le délai du fournisseur (⚙) ou baisse la qualité. |
| « … est un modèle de langage » | Un LLM ne génère pas de 3D : choisis un serveur image→3D comme moteur. |
| Le LLM ne répond rien | Le modèle choisi n’accepte pas les images : prends un modèle vision. |
| « Le maillage n’est pas étanche » | Le STL reste utilisable : le trancheur le répare à l’import. Le socle n’est alors pas ajouté. |

## 10. Colorier

L’onglet **Colorier** attribue une couleur à chaque partie d’un modèle, à la manière de
MelSplit, puis exporte le résultat pour l’impression multicolore.

1. **Modèle** : dépose un STL ou un 3MF. Un STL ne porte pas de couleur : tout est « Base ».
   Un 3MF déjà coloré garde ses couleurs (matériaux de base ou groupes de couleurs de la
   spécification 3MF).
2. **Choisis les parties** :
   - **Détail** (D) : un clic colorie la zone sous le curseur, en s’arrêtant aux arêtes vives.
     *Taille maximale du détail* limite l’étendue autour du clic (en % de la diagonale du
     modèle); *Sensibilité aux arêtes* est l’angle à partir duquel deux triangles voisins sont
     séparés. Ça déborde : réduis les deux. Ça ne prend pas tout : augmente-les.
   - **Pinceau** (P) : glisse pour peindre; le rayon se règle avec le curseur. Clic droit
     ou Alt + glisser pour tourner la vue.
   - **Gomme** (G) : remet la couleur de base.
3. **Couleurs** : clique sur une couleur pour la choisir, sur sa pastille pour la modifier.
   Les pastilles rapides ajoutent une couleur; ✕ la retire (ses triangles reviennent à la
   base et l’historique est vidé). *Remplir tout* et *Tout effacer* agissent sur tout le modèle.
   Ctrl+Z annule, Ctrl+Maj+Z (ou Ctrl+Y) rétablit.
4. **Exporter** :
   - un **3MF** avec une couleur par triangle (matériaux de base du 3MF);
   - un **ZIP de STL**, un par couleur utilisée, avec les mêmes coordonnées : importe-les
     ensemble dans le trancheur pour les superposer et affecte un filament à chacun.

Limites : les couleurs peintes dans le trancheur (Bambu Studio, OrcaSlicer : attribut
`paint_color`) ne sont pas lues; la sélection se fait par triangle, sans lissage du bord;
au-delà de 500 000 triangles, les clics deviennent lents.
