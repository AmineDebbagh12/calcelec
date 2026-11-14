# CalcElec

Application de bureau Rust pour l'étude des réseaux de distribution publique
HTA/BT, avec la France et Enedis comme référence métier.

**État : prototype avec toolbox géographique, modélisation, partage et répartition de puissance AC
équilibrée sur réseaux radiaux. Le calcul déséquilibré et les courts-circuits
restent à implémenter.**
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

## Toolbox géographique

La toolbox se trouve en haut du panneau droit. Les actions de carte concernent
la vue géographique ; le glisser droit déplace la vue et la molette zoome.

| Outil | Utilisation |
| --- | --- |
| Sélection | Cliquer un objet ; les clics répétés parcourent les objets superposés |
| Placer | Choisir un poste HTA/BT, nœud HTA/BT, charge, production ou source, puis cliquer la carte ; choisir explicitement le nœud de raccordement des injections |
| Raccorder | Choisir le type de liaison et ses deux bornes ; pour un tronçon, commencer le tracé, cliquer ses sommets intermédiaires puis terminer ; pour un interrupteur ou transformateur, créer la liaison avec le bouton |
| Déplacer | Sélectionner l'objet puis glisser son point ou son tracé avec le bouton gauche |
| Sommets | Sélectionner un tronçon puis glisser une de ses poignées ; ajouter/supprimer des sommets depuis les propriétés |

**Échap** abandonne le geste ou le tracé en cours. Les placements et gestes de
carte validés créent immédiatement une version en mémoire ; un déplacement ne
produit qu'un seul lot au relâchement de la souris. Les modifications saisies
dans les propriétés demandent toujours **Créer une version avec ces changements**.

Les paramètres électriques et bornes de raccordement sont modifiables dans
**Paramètres électriques et raccordements**. Les données invalides sont refusées
sans application partielle. Les nouveaux équipements reçoivent des paramètres
provisoires visibles dans l'inspecteur, qui ne constituent pas un catalogue Enedis.
La longueur électrique d'un nouveau tronçon est initialisée depuis son tracé.
Les déplacements ultérieurs ne changent ni cette longueur ni ses connexions.

Un poste placé crée ensemble deux nœuds HTA/BT et un transformateur. Ces trois
objets restent distincts et sélectionnables. Le déplacement agit sur l'objet
sélectionné ; le déplacement groupé d'un poste et l'ajustement automatique des
tracés incidents restent à ajouter. Une modification de borne ne déplace pas
non plus automatiquement les extrémités dessinées d'un tronçon.

La rubrique **Supprimer l'ouvrage** de l'inspecteur liste les objets qui le
référencent. Leur suppression doit être choisie explicitement ; aucune suppression
en cascade n'est cachée. **Annuler / Rétablir** restaure les objets et leurs
identifiants par de nouveaux lots, sans réécrire les versions précédentes.
Cette pile d'actions est locale à la session et se réinitialise lors d'un
changement externe d'historique ; l'historique de versions reste partageable
et persistant. Sauvegarder pour conserver les versions créées.

## Données électriques détaillées

L'inspecteur dispose d'une rubrique **Modèle électrique détaillé — édition avancée**
pour renseigner les phases et le neutre, les matrices de ligne, les enroulements,
prises et pertes des transformateurs, les mises à la terre et les paramètres de
source. Cette première édition utilise du JSON typé, validé à la création d'une
version. Les structures préremplies sont provisoires et doivent être vérifiées.
Les données inconnues des anciens projets restent absentes ; l'application les
signale dans **Données détaillées** et la commande `--check`.

Deux cas synthétiques ouvrables depuis le panneau Projet sont fournis :
[ligne quatre conducteurs](examples/reference-four-wire.json) et
[transformateur avec pertes cuivre](examples/reference-transformer.json).
Ils se calculent sans les options de simplification. Le moteur exploite leurs
matrices équilibrées et leurs résistances ; les déphasages, prises hors nominal
et matrices dissymétriques restent refusés explicitement.
La [description du modèle électrique](docs/modele-electrique.md) précise les
unités, conventions, limites et contrôles analytiques.

## Calculer le régime permanent

Sur l'exemple rural, cocher **Autoriser les modèles simplifiés**, puis cliquer
sur **Calculer le régime permanent**. Ce choix autorise explicitement les
équivalents triphasés équilibrés des puissances monophasées et les transformateurs
purement réactifs, sans déphasage, à rapport nominal. Il ne s'agit pas d'un calcul
déséquilibré : aucun courant de neutre ou résultat par phase n'est calculé.

Le calcul s'exécute en arrière-plan et peut être annulé. Les résultats apparaissent
dans l'onglet **Résultats du calcul** : tensions composées, angles, courants,
taux de charge et pertes. Les dépassements des seuils configurés sont signalés
dans les tableaux et en rouge sur la vue géographique ; les valeurs se consultent
aussi en survolant les objets de la carte. Les seuils par défaut sont indicatifs.

Les résultats JSON exportés conservent la version exacte du réseau, celle du
moteur, les options et les hypothèses. Ils concernent les données validées par
création d'une version ; les champs en cours d'édition ne sont pas inclus.
Un changement de version invalide l'affichage du calcul précédent. Les résultats
restent en mémoire jusqu'à leur export et ne sont pas enregistrés avec le projet
dans SQLite à ce stade.

Sans l'option de simplification, le moteur accepte les réseaux radiaux à source
unique avec charges/productions triphasées, lignes et interrupteurs. Les
transformateurs insuffisamment paramétrés, boucles, sources multiples et îlots
non alimentés sont refusés. Une non-convergence ne produit aucun tableau de résultats.
La [spécification numérique du premier moteur](docs/calcul-regime-permanent.md)
décrit ses conventions et sa validation.

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
cargo run -- --load-flow /tmp/rural.json --simplified
cargo run -- --apply base.json modification.json resultat.json
cargo run -- --reference-case four-wire /tmp/quatre-conducteurs.json
cargo run -- --load-flow examples/reference-transformer.json
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
Les tests d’édition vérifient le placement, le tracé, les dépendances, les
annulations et les gestes souris de déplacement et de modification des sommets.
Les tests numériques comparent les tensions, angles, courants et pertes à des
solutions analytiques, puis vérifient le bilan du réseau rural, les erreurs
attendues, l'annulation et l'invalidation des résultats après modification.

## Suite du développement

- Exploiter les phases, le neutre, les couplages, prises et mises à la terre dans
  les moteurs suivants ; compléter les modèles homopolaires et de convertisseurs.
- Étendre les cas de validation et les moteurs aux réseaux maillés,
  au régime déséquilibré et aux courts-circuits.
- Ajouter le déplacement groupé des postes, l’ajustement des tracés incidents
  et un éditeur unifilaire complet.
- Ajouter cas d'étude, annuler/rétablir, persistance des résultats, rapports et
  conditionnement compressé des échanges.
- Vérifier l'application native et préparer les distributions Windows/Linux.
