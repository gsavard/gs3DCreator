# API HTTP

Le serveur (`gs3dcreator serveur`) expose une petite API JSON, utilisée par l’interface web.
Les erreurs renvoient `{"detail": "message"}` avec un code HTTP 400, 404, 409 ou 422.

## `GET /api/sante`

```json
{"app": "gs3dcreator", "version": "2.0.0"}
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
