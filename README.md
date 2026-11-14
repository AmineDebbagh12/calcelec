# CalcElec

Application de bureau Rust pour l'étude des réseaux de distribution publique
HTA/BT, avec la France et Enedis comme référence métier.

**État : premier prototype de modélisation et de partage. Les calculs de
répartition de puissance et de courts-circuits ne sont pas encore implémentés.**
Le périmètre de la V1 reste décrit dans le [cadrage](docs/cadrage-fonctionnel.md).

## Lancer l'application

Installer Rust stable (1.95 minimum), puis :

```sh
cargo run
```

L'application utilise une interface native egui/eframe et SQLite embarqué.
Les cibles prévues sont Windows et Linux. Sous Linux, un environnement graphique
X11 ou Wayland avec OpenGL est nécessaire. Les dépendances système de compilation
varient selon la distribution. La compilation Windows reste à vérifier sur Windows.

L'écran initial présente un réseau rural synthétique : départ HTA 20 kV, trois
postes HTA/BT, départ BT détaillé, charges monophasées et production photovoltaïque.
Les valeurs sont pédagogiques et ne proviennent pas d'un catalogue Enedis.

1. Sélectionner un équipement dans le tableau ou la vue géographique.
2. Modifier le nom, l'état en service ou les puissances d'une charge/production.
3. Cliquer sur **Créer une version avec ces changements**.
4. Exporter le projet ou l'enregistrer dans SQLite pour conserver le travail.

La vue géographique s'ouvre par défaut : molette pour zoomer, glisser pour
déplacer la vue, bouton **Cadrer le réseau** pour revenir à l'ensemble du réseau.
Les tronçons suivent leurs polylignes et les autres équipements ont une position
ponctuelle. Des clics successifs parcourent les objets superposés à un même endroit.
Les coordonnées X/Y et les sommets se modifient dans les propriétés, puis se
valident par la création d'une version. La longueur électrique reste indépendante
du tracé. L'exemple utilise un repère métrique local fictif ; le modèle permet
aussi de déclarer des coordonnées Lambert-93, sans reprojection. Aucun fond de
carte externe n'est chargé.

Le bouton **Vue topologique** conserve la vue de connectivité des jeux de barres
et des branches. Il ne constitue pas encore un éditeur unifilaire complet.
Le diagnostic d'alimentation vérifie uniquement
les connexions, sans calcul électrique ni prise en compte d'une production îlotée.

Les projets sont initialement en mémoire. Avant de fermer, ouvrir ou relire un
projet, sauvegarder les versions à conserver. Les champs de l'inspecteur doivent
être validés en créant une version avant sauvegarde. Les exports refusent
d'écraser un fichier existant ; utiliser un nouveau nom. SQLite permet des
enregistrements successifs sans recopier l'historique déjà stocké.
Après redémarrage, renseigner le chemin de la base, lister ses projets puis charger
celui souhaité, ou ouvrir un fichier projet exporté.

## Partager une modification

1. Exporter le projet de base A et le transmettre au destinataire.
2. Modifier un objet puis créer une version B.
3. Exporter le dernier lot et transmettre ce fichier.
4. Le destinataire ouvre A et importe le lot pour reconstruire B.

Un lot déjà reçu est reconnu. Un lot créé depuis une autre version ou un autre
projet est refusé sans modification partielle. Deux branches d'une même base
peuvent être conservées dans des fichiers distincts ; leur fusion n'est pas
encore implémentée. L'export du dernier lot ne remplace pas l'envoi des lots
intermédiaires si le destinataire possède une version plus ancienne.

Le [contrat de données initial](docs/format-donnees.md) décrit le format JSON,
les opérations et le stockage SQLite.

## Ligne de commande

Ces commandes fonctionnent sans ouvrir de fenêtre :

```sh
cargo run -- --example /tmp/rural.json
cargo run -- --check /tmp/rural.json
cargo run -- --apply base.json modification.json resultat.json
```

Utiliser un chemin adapté sous Windows. Le premier appel crée un nouveau projet
avec ses propres identifiants ; pour partager une même base, transmettre son
export plutôt que générer un autre exemple.

## Vérification

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Les tests couvrent le partage et la reconstruction, l'atomicité des lots,
les conflits, les références invalides, les historiques SQLite, les exports
sans écrasement, la connectivité, les géométries, la migration des anciens
projets et le rendu des deux vues sans serveur graphique.

## Suite du développement

- Compléter les modèles électriques : phases et neutre, matrices d'impédances,
  couplages des transformateurs, mises à la terre et équivalents de défaut.
- Définir les cas numériques de validation puis développer les moteurs.
- Étendre les opérations à toutes les propriétés électriques et les exposer
  dans l'éditeur, avec création, raccordement et suppression d'équipements.
- Ajouter cas d'étude, annuler/rétablir, résultats, exports de rapports et
  conditionnement compressé des échanges.
- Vérifier l'application native et préparer les distributions Windows/Linux.
