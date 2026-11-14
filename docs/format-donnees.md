# Contrat de données — prototype, schéma 4

Ce document décrit l'implémentation actuelle. Le cadrage fonctionnel reste la
cible ; tous ses modèles et mécanismes ne sont pas encore réalisés.

## Projet autonome

Un projet JSON contient :

| Champ | Sens |
| --- | --- |
| `schema_version` | Version du format, actuellement `4` |
| `id` | UUID stable du projet |
| `root_version` | UUID de la version initiale |
| `root` | Réseau initial, incluant tous les paramètres disponibles |
| `changes` | Historique ordonné des lots depuis la racine |

La version courante est la version du dernier lot ou `root_version` si
l'historique est vide. Le réseau courant est reconstruit en appliquant les lots.
Les exports JSON suppriment les espaces superflus et les propriétés inchangées
ne sont pas retransmises dans les opérations de modification. Il n'y a pas encore
de compression ni de format binaire.

Le réseau contient `name`, `frequency_hz`, `coordinate_reference` et une table `objects`, indexée par
UUID. Chaque objet porte le même `id`, un `name`, un état `in_service` et un
`equipment` dont le champ `type` précise le modèle : `bus`, `source`, `line`,
`transformer`, `load`, `generator` ou `switch`.

Chaque objet porte également un champ `geometry`. Les coordonnées sont des
couples `[x, y]` en mètres, X vers l'est et Y vers le nord. Le référentiel commun
au réseau est `local_meters` (repère local) ou `lambert93` (EPSG:2154).
L'identification Lambert-93 reprend un code encore largement utilisé, décrit
par l'[IGN](https://geodesie.ign.fr/actualites/evolution-venir-des-codes-epsg-du-rgf93).
Le logiciel ne réalise pas de conversion entre référentiels ; changer une étiquette
de référentiel n'est pas une reprojection.

```json
{"type":"point","coordinates":[700000.0,6600000.0]}
```

```json
{"type":"polyline","coordinates":[[700000.0,6600000.0],[700030.0,6600040.0],[700090.0,6600040.0]]}
```

Un tronçon utilise une polyligne d'au moins deux sommets, les autres équipements
un point. Les sommets consécutifs identiques, coordonnées non finies et coordonnées
hors du domaine métrique pris en charge (±10⁹ m sur chaque axe) sont refusés.
`{"type":"unlocated"}` exprime l'absence de localisation ; les objets concernés
sont comptés dans la vue et ne sont pas placés arbitrairement à l'origine.
Les géométries absentes sont lues comme non localisées, notamment à l'import ancien.

La géométrie est indépendante des identifiants de raccordement : deux équipements
superposés ou deux tracés qui se croisent ne sont pas automatiquement connectés.
Les jeux de barres HTA et BT d'un poste peuvent partager le même point. La longueur
plane d'une polyligne est informative et ne remplace pas `length_m` électrique.
La disposition du schéma topologique reste calculée séparément.

Les références entre objets sont des UUID. Les quantités sont en unités SI
explicites dans les noms de champs : V, A, W, var, m et ohm/km pour les paramètres
linéiques. La tension nominale d'un jeu de barres triphasé est composée.
`voltage_pu` est sans dimension et `uk_percent` est exprimé en pourcentage.
`phase` vaut `Abc`, `A`, `B` ou `C` pour une charge ou une production.
La consommation et la production ont chacune P positive ; leur type détermine
le sens physique. Aucune équivalence numérique des modèles incomplets n'est supposée.

## Lot de modifications

Exemple illustratif, avec identifiants à remplacer par ceux du projet concerné :

```json
{
  "schema_version": 4,
  "project_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
  "base_version": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  "version": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
  "author": "Études réseau",
  "message": "Évolution de la consommation",
  "operations": [
    {
      "op": "set_power",
      "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
      "p_w": 12500.0,
      "q_var": 2000.0
    }
  ]
}
```

Opérations disponibles :

| `op` | Champs spécifiques |
| --- | --- |
| `create` | `object` : objet complet |
| `delete` | `id` |
| `rename` | `id`, `name` |
| `set_service` | `id`, `in_service` |
| `set_power` | `id`, `p_w`, `q_var` pour charge ou producteur |
| `set_switch` | `id`, `closed` pour interrupteur |
| `set_geometry` | `id`, `geometry` : nouveau point ou nouveau tracé complet |
| `set_equipment` | `id`, `equipment` : paramètres électriques et bornes, à type d'équipement inchangé |
| `set_electrical` | `id`, `electrical` : modèle détaillé typé, ou `null` pour le retirer |

Le champ optionnel `electrical` conserve conducteurs, matrices, enroulements,
prises, mises à la terre et paramètres de défaut. Son absence signifie que ces
données ne sont pas renseignées. Il ne remplace pas `equipment` ; leurs références
et phases doivent rester cohérentes. Voir le [modèle détaillé](modele-electrique.md).

`set_equipment` remplace uniquement les paramètres métier de l'objet ciblé,
sans retransmettre son nom, son état ou sa géométrie. Les éditions de puissance
seule continuent d'utiliser `set_power`. Les changements de bornes sont explicites
et indépendants des positions géographiques.

Les commandes de toolbox utilisent les mêmes opérations. Un poste HTA/BT est
créé par trois `create` dans un lot atomique. Annuler/rétablir produit un nouveau
lot de compensation, sans effacer les lots précédents ni changer les identifiants.
Les piles locales d'annulation ne sont pas sérialisées et ne s'appliquent qu'à
la version attendue de la session.

Une modification du tracé transmet la géométrie de l'objet concerné, sans
retransmettre ses paramètres électriques ou les autres objets. Les différences
sommet par sommet ne sont pas encore implémentées.

Le lot est validé sur une copie du réseau avant ajout à l'historique. Les
opérations sont ordonnées ; les références sont contrôlées sur l'état final,
ce qui permet de supprimer un objet et ses dépendances dans un même lot.
Un échec annule le lot entier. Il n'y a pas de suppression implicite en cascade.

La base et le projet doivent correspondre exactement à la version courante.
Les identifiants de lot et de version ne peuvent pas être réutilisés. La réception
répétée d'un lot identique est sans effet ; un contenu différent sous le même
identifiant provoque un conflit. Les schémas non supportés et champs inconnus
sont refusés. L'auteur est une déclaration, sans authentification.

## Base SQLite

L'adaptateur local conserve la racine dans `projects` et les lots dans `changes`,
avec leur ordre et leur identifiant. Il reconstruit le même objet `Project` que
l'import JSON. Les enregistrements successifs ajoutent uniquement les nouveaux
lots après vérification que l'historique existant est un préfixe identique.

L'écriture utilise une transaction immédiate : deux connexions concurrentes ne
peuvent pas remplacer silencieusement leurs historiques. Une base plus récente
ou une branche divergente provoque un conflit. L'interface peut lister et
recharger les projets présents dans une base existante.

## Migration depuis les schémas 1, 2 et 3

Les projets JSON de schémas 1, 2 et 3 et leurs lots sont convertis explicitement vers
le schéma 4 à la lecture. Les identifiants d'objets, projets, lots et versions
restent inchangés. Les anciennes données sans coordonnées deviennent
`unlocated`, dans un repère `local_meters`. Aucune localisation n'est inventée.
Un ancien lot de schéma 1 reste importable, avec les mêmes règles de conflit
et de réception répétée, et ne peut pas prétendre contenir des géométries.
Les coordonnées existantes du schéma 2 sont conservées. `set_equipment` exige
un lot de schéma 3 au minimum ; un lot de schéma 1 ou 2 contenant cette opération est refusé.
Le schéma 4 ajoute `electrical` et `set_electrical`. Aucun modèle détaillé n'est
inventé lors de la migration. Ces champs ou opérations sont refusés dans les
racines et lots portant une ancienne version de schéma.

Les anciennes bases SQLite restent lisibles. Leur représentation est migrée
dans la transaction lors de l'enregistrement d'un projet migré, après contrôle
de son historique. Les exports suivants sont au schéma 4 et nécessitent cette
version de l'application ; les anciennes versions de l'application ne peuvent
pas les ouvrir. Les autres versions de schéma restent refusées.

## Limites et évolutions prévues

Un premier export de résultats de régime permanent existe séparément du projet.
Il inclut les identifiants du projet et de sa version, la version du moteur,
les options, les hypothèses et les grandeurs calculées. Il ne modifie pas le
schéma 4 du réseau et n'est pas encore persisté dans SQLite. Voir le
[contrat du moteur](calcul-regime-permanent.md).

- Pas encore d'empreinte cryptographique, de signature ou d'authentification :
  les UUID identifient les versions mais ne certifient pas leur contenu.
- Les migrations futures devront être explicites, comme les migrations 1/2/3 → 4.
- Historique linéaire par document/projet stocké ; pas encore de graphe de
  branches persistant, de fusion, d'export d'une plage de lots ni de points de reprise.
- Pas de serveur partagé ; le contrat de données est indépendant de SQLite,
  mais un adaptateur distant et son protocole restent à implémenter.
- Pas encore de disposition graphique persistée, de cas d'étude, de catalogue
  ni de résultats de simulation dans ce schéma initial.
- Avant de figer le format V1, compléter le modèle électrique et mesurer les
  tailles, les coûts de reconstruction et les besoins de compression.
