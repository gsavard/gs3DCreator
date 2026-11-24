# API HTTP

Le serveur (`gs3dcreator serveur`) expose une petite API JSON, utilisée par l’interface web.
Les erreurs renvoient `{"detail": "message"}` avec un code HTTP 400, 404, 409 ou 422.

## `GET /api/sante`

```json
{"app": "gs3dcreator", "version": "2.2.1"}
```

Sert au lanceur à reconnaître l’application sur un port.

## `POST /api/modeles`

Formulaire `multipart/form-data` avec un champ `fichier` (STL, OBJ, GLB, glTF, PLY, 3MF, OFF;
1 Gio maximum).

```json
{"id": "3fa2c41b9e0d", "nom": "figurine"}
```

## `POST /api/modeles/{id}/preparer`

Corps : `{"axe": "auto" | "x" | "y" | "z", "hauteur": 150}`

Charge, répare, oriente et met à l’échelle le modèle, puis détecte les repères.

```json
{
  "axe": "z",
  "triangles": 2434,
  "dims": [52.8, 23.5, 150.0],
  "reperes": {
    "jambes_xy": [[-8.4, -0.8], [8.4, -0.8]],
    "H": 150.0, "z_cou": 128.8, "z_hanche": 65.1, "z_taille": 78.7,
    "axe_lat": "x", "mid_lat": 0.0, "demi_largeur": 17.8,
    "avertissements": []
  },
  "coupes": {
    "6": [{"label": "tete", "point": [0, 0.1, 128.8], "normale": [0, 0, 1]}, "…"],
    "5": ["…"], "4a": ["…"], "4b": ["…"], "3": ["…"], "2": ["…"]
  },
  "apercu": "/api/modeles/3fa2c41b9e0d/fichier/modele.glb"
}
```

## `POST /api/modeles/{id}/decouper`

```json
{
  "coupes": [{"label": "tete", "point": [0, 0, 128.8], "normale": [0, 0, 1]}],
  "connecteur": "rotule",
  "rayon": 2.5,
  "jeu": 0.2,
  "profondeur": 6.0,
  "volume": [250, 250, 250]
}
```

`connecteur` : `rotule`, `goujon`, `tenon` ou `aucun`. La normale pointe vers la pièce à
détacher. Les libellés connus (`tete`, `bras_1`, `bras_2`, `jambe_1`, `jambe_2`, `jambes`)
donnent leur nom aux pièces; tout autre libellé (`extra_1`…) est accepté.

```json
{
  "pieces": [
    {"nom": "torse", "label": "torse", "titre": "Torse", "dims": [50.7, 20.7, 64.6],
     "volume_cm3": 30.7, "tient": true}
  ],
  "goujons": 4,
  "type_connecteur": "rotule",
  "avertissements": [],
  "apercu": "/api/modeles/3fa2c41b9e0d/resultat/apercu.glb",
  "zip": "/api/modeles/3fa2c41b9e0d/resultat/figurine_pieces.zip",
  "mf": "/api/modeles/3fa2c41b9e0d/resultat/figurine.3mf"
}
```

## `GET /api/modeles/{id}/fichier/{nom}` et `GET /api/modeles/{id}/resultat/{nom}`

Téléchargent un fichier de travail (`modele.glb`) ou un résultat (STL, 3MF, ZIP, GLB).

## Exemple avec curl

```bash
ID=$(curl -s -F fichier=@figurine.stl http://127.0.0.1:8790/api/modeles | jq -r .id)
curl -s -H 'Content-Type: application/json' -d '{"axe":"auto","hauteur":180}' \
     http://127.0.0.1:8790/api/modeles/$ID/preparer > prep.json
jq '{coupes: .coupes["6"], connecteur: "rotule"}' prep.json |
  curl -s -H 'Content-Type: application/json' -d @- \
     http://127.0.0.1:8790/api/modeles/$ID/decouper | jq .
curl -s -O http://127.0.0.1:8790/api/modeles/$ID/resultat/figurine_pieces.zip
```

## Image → 3D

### `GET /api/fournisseurs`

```json
{
  "fournisseurs": [
    {"id": "e987c91d", "nom": "Hunyuan local", "type": "hunyuan3d", "famille": "image3d",
     "url": "http://127.0.0.1:8081", "modele": "", "delai": 900, "cle_definie": false}
  ],
  "types": [{"cle": "hunyuan3d", "nom": "…", "famille": "image3d", "url_defaut": "…", "cle_requise": false, "aide": "…"}, "…"]
}
```

Types : `hunyuan3d`, `generique` (famille `image3d`); `lmstudio`, `ollama`, `openrouter`,
`openai` (famille `llm`). La clé n’est jamais renvoyée.

### `POST /api/fournisseurs`

Crée (sans `id`) ou met à jour (avec `id`) un fournisseur. Une `cle` vide conserve la clé
existante.

```json
{"id": "", "type": "ollama", "nom": "Ollama", "url": "http://127.0.0.1:11434/v1",
 "cle": "", "modele": "qwen2.5vl", "delai": 900}
```

### `DELETE /api/fournisseurs/{id}`

### `POST /api/fournisseurs/{id}/tester`

```json
{"message": "Connexion réussie : 2 modèle(s).", "modeles": ["llava", "qwen2.5vl"]}
```

### `POST /api/images/analyser`

Formulaire : `image`, `fournisseur` (un LLM), `question` (facultatif).

```json
{"texte": "1) Une figurine de gobelin…", "modele": "qwen2.5vl"}
```

### `POST /api/images`

Formulaire : `image` (PNG, JPG, WEBP; 20 Mo max.), `fournisseur` (image→3D), `options`
(JSON, tous les champs facultatifs) :

```json
{"hauteur": 150, "axe": "auto", "socle": false, "graine": 1234, "resolution": 256,
 "etapes": 30, "guidage": 5.0, "retirer_fond": true}
```

Réponse immédiate : `{"id": "efb20be66c6f"}`. La génération continue en arrière-plan.

### `GET /api/images/{id}` et `GET /api/images`

`etat` : `attente`, `generation`, `conversion`, `termine` ou `erreur` (détail dans `message`).

```json
{
  "id": "efb20be66c6f", "nom": "gobelin", "fournisseur": "Hunyuan local",
  "etat": "termine", "message": "Modèle prêt.", "duree": 74.2,
  "resultat": {"axe": "y", "dims": [61.3, 48.0, 150.0], "triangles": 412880, "etanche": true, "stl": "gobelin.stl"},
  "image": "/api/images/efb20be66c6f/fichier/image.png",
  "apercu": "/api/images/efb20be66c6f/fichier/apercu.glb",
  "stl": "/api/images/efb20be66c6f/fichier/gobelin.stl"
}
```

`GET /api/images` renvoie `{"generations": [...]}`, la plus récente en premier.

### Exemple avec curl

```bash
F=$(curl -s -H 'Content-Type: application/json' \
     -d '{"type":"hunyuan3d","url":"http://127.0.0.1:8081"}' \
     http://127.0.0.1:8790/api/fournisseurs | jq -r .id)
ID=$(curl -s -F image=@gobelin.png -F fournisseur=$F -F 'options={"hauteur":120}' \
     http://127.0.0.1:8790/api/images | jq -r .id)
until [ "$(curl -s http://127.0.0.1:8790/api/images/$ID | jq -r .etat)" = termine ]; do sleep 5; done
curl -s -O http://127.0.0.1:8790/api/images/$ID/fichier/gobelin.stl
```

## Colorier

### `POST /api/couleur`

Envoi multipart (champ `fichier` : STL, 3MF, OBJ, GLB, PLY ou OFF). Réponse :

```json
{"id": "a1b2c3d4e5f6", "nom": "dragon", "triangles": 14160, "sommets": 7080,
 "dims": [40, 40, 40], "min": [-20, -20, 0], "max": [20, 20, 40],
 "palette": ["#BDB8AE"], "faces": "<base64>", "colore": false}
```

`palette` commence toujours par la couleur de base (indice 0). `faces` contient, en base64,
un octet par triangle : son indice dans la palette.

### `GET /api/couleur/{id}/maillage`

Maillage binaire petit-boutiste : nombre de sommets (u32), nombre de triangles (u32),
positions (f32 × 3 par sommet), indices (u32 × 3 par triangle). L’ordre des triangles est
celui de `faces`.

### `POST /api/couleur/{id}/exporter`

Corps : `{"palette": ["#BDB8AE", "#E5484D"], "faces": "<base64>"}`. Crée un 3MF (un matériau de
base par couleur utilisée, `pid`/`p1` par triangle) et un ZIP de STL (un par couleur utilisée).

```json
{"mf": "/api/couleur/a1b2c3d4e5f6/fichier/dragon_couleur.3mf",
 "zip": "/api/couleur/a1b2c3d4e5f6/fichier/dragon_couleurs_stl.zip",
 "couleurs": [{"hex": "#E5484D", "triangles": 536, "pct": 3.8, "fichier": "dragon_02_E5484D.stl"}]}
```

### `GET /api/couleur/{id}/fichier/{nom}`

Télécharge un fichier produit par l’export.
