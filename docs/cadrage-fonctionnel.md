# CalcElec — Cadrage fonctionnel

Statut : proposition initiale à affiner, sans engagement de calendrier.

## 1. Objectif et décisions acquises

Construire une application de bureau en Rust, utilisable sous Windows et Linux,
pour modéliser et étudier des réseaux électriques HTB, HTA et BT.

La première version utilisable vise les **réseaux de distribution publique HTA/BT** et
comprend **la répartition de puissance et les calculs de courts-circuits**.
Ces priorités ont été choisies par le porteur du projet. Les réseaux industriels
ne constituent pas la cible initiale.

Le territoire de référence est la **France**, avec **Enedis** comme gestionnaire
de réseau de référence. Sa documentation technique publique guidera les cas
d'étude et les hypothèses métier ; ce choix n'implique aucune homologation.

Le cas d'exemple retenu est un départ HTA rural alimentant plusieurs postes
HTA/BT, dont un départ BT détaillé avec production photovoltaïque.
Les caractéristiques numériques restent à définir.

Le format des objets et des modifications du réseau doit être compact,
documenté, facilement partageable et compatible avec un stockage en base de
données pour échanger des versions et des modifications.

La **représentation géographique** fait partie de la V1 : chaque objet porte
une géométrie, ponctuelle pour les postes et équipements ponctuels, et une
polyligne pour les tronçons. Elle constitue la source de vérité spatiale. La
vue unifilaire est dérivée des raccordements et de l'agencement géographique ;
elle ne possède pas de géométrie globale indépendante à maintenir.

PowerFactory constitue une référence de catégories de fonctionnalités, sans
objectif de reproduire son étendue dès la première version.

## 2. Parcours utilisateur cible

1. Créer un projet et renseigner fréquence, unités et conventions.
2. Dessiner un réseau ou charger un exemple, puis paramétrer ses équipements.
3. Définir un cas d'étude : charges, production, prises et état des organes.
4. Vérifier les données et la connectivité.
5. Calculer le régime permanent, examiner tensions, courants et pertes.
6. Calculer des défauts sur les jeux de barres et examiner les contributions.
7. Comparer deux cas d'étude et exporter les résultats avec leurs hypothèses.

## 3. Périmètre proposé pour la V1

### Études prioritaires de distribution publique

Le périmètre de travail proposé part d'un équivalent du réseau amont alimentant
des départs HTA, des postes HTA/BT et leurs départs BT jusqu'aux points de
raccordement des consommateurs et producteurs. Le réseau HTB amont est représenté
par cet équivalent ; sa modélisation détaillée est reportée.

- Étudier les profils de tension, pertes et charges des lignes, câbles et
  transformateurs sur des situations de forte et de faible consommation.
- Évaluer l'effet d'un raccordement de consommation ou de production distribuée
  par comparaison de cas avant/après, notamment pour la production photovoltaïque.
- Étudier la répartition des charges monophasées entre phases et ses effets
  sur les tensions et le courant de neutre.
- Comparer des configurations d'exploitation par ouverture et fermeture des
  organes, avec signalement des portions non alimentées et des surcharges.
- Calculer les courants de défaut aux nœuds d'étude HTA et BT.

Les charges peuvent représenter un consommateur ou un agrégat documenté.
Pour la V1, les puissances des cas de pointe, de creux et de forte production
sont saisies explicitement ; le foisonnement automatique et la génération
statistique des consommations sont reportés.

Ces études constituent une aide à l'analyse ; une décision automatique de
raccordement selon les règles d'un gestionnaire de réseau est hors V1.

### Déclinaison France — Enedis

- Proposer un premier jeu de cas HTA à 20 kV et BT à 230/400 V ; conserver
  la tension nominale paramétrable, notamment pour les réseaux HTA à 15 kV.
- Privilégier les départs exploités radialement, avec points de coupure et
  liaisons de secours représentés dans la topologie.
- Placer la limite amont initiale au jeu de barres HTA du poste source :
  tension et équivalent de court-circuit y sont fournis comme données d'entrée.
- Décrire les postes HTA/BT, départs aériens et souterrains, branchements et
  points de raccordement avec des paramètres électriques traçables.
- Définir explicitement le régime de neutre HTA et les mises à la terre BT
  pour chaque cas de référence avant les calculs de défaut à la terre.
  Leur couverture numérique reste à spécifier, sans valeur universelle implicite.
- Documenter séparément les seuils d'alerte choisis pour le logiciel et les
  critères d'étude issus des documents Enedis, avec référence et version.

La note Enedis-NMO-RES_011E, version 1 du 03/12/2024, sert de première
référence sur la structure des ouvrages. Les valeurs nominales et principes
d'architecture ci-dessus s'appuient sur cette note ; le choix du premier jeu
de cas et de sa limite amont est une proposition de périmètre CalcElec.
Les méthodes détaillées de raccordement et de calcul devront faire l'objet
d'une sélection documentaire complémentaire avant leur implémentation.

### Modélisation du réseau

| Élément | Couverture initiale proposée |
| --- | --- |
| Topologie | Réseaux radiaux et maillés ; équipements en/hors service ; détection des îlots |
| Jeux de barres et bornes | Tension nominale, phases disponibles, raccordements explicites |
| Réseau amont | Source équivalente ; tension imposée ; impédances de séquence ou données permettant de les déterminer |
| Lignes et câbles | Longueur, paramètres électriques par phase, couplages, neutre et limites de courant |
| Transformateurs | Deux enroulements, rapport, couplage, déphasage, impédances, mise à la terre et prise fixe |
| Charges | Triphasées et monophasées ; puissance active/réactive constante pour le premier modèle |
| Production distribuée | Injection P/Q en régime permanent ; contribution au défaut selon un modèle explicitement supporté |
| Organes de coupure | Interrupteurs, sectionneurs et disjoncteurs représentés par leur état ouvert/fermé |
| Compensation | Shunts fixes ; commande automatique reportée |
| Terre et neutre | Connexions du neutre, impédances de mise à la terre et chemins de retour nécessaires aux modèles supportés |

Prévoir des modèles paramétrés et réutilisables de câbles et transformateurs.
La présence d'un disjoncteur dans le schéma ne signifie pas que sa protection est simulée.

### Catalogue de technologies et de types d'ouvrages

La V1 doit permettre de définir et réutiliser plusieurs technologies : câbles
et lignes, par exemple 95AL, 150AL, 150CU et 148AA, ainsi que des types de
transformateurs HTA/BT. Ces désignations sont des exemples de références à
paramétrer ; leurs caractéristiques ne doivent pas être déduites du nom seul.

- Chaque type possède un identifiant stable, une version, une désignation,
  sa provenance, ses unités et un domaine d'emploi documenté.
- Pour les lignes/câbles : matériau et section des conducteurs, constitution,
  phases/neutre, matrices et limites de courant, avec conditions de référence
  et hypothèses thermiques quand elles sont disponibles.
- Pour les transformateurs : tensions et puissance assignées, couplage,
  essais, prises et données de neutre/homopolaires disponibles.
- Un ouvrage peut être instancié depuis un type et porter des variantes
  explicites. Les paramètres de calcul sont figés ou rattachés à une version
  immuable du type pour reconstruire les anciennes études.
- La modification d'un type ne modifie jamais silencieusement les ouvrages
  existants ; sa propagation est une action explicite, visible dans les lots.
- Un projet autonome exporté inclut ses types ou leurs paramètres résolus.
  L'absence d'une base de catalogue externe ne doit pas empêcher sa relecture.

Le catalogue est prévu au scope V1 ; il n'est pas encore implémenté. Les valeurs
provisoires de la toolbox ne constituent pas ce catalogue.

### Répartition de puissance

- Calcul AC équilibré pour les premiers jalons du moteur.
- Calcul AC déséquilibré en phases pour la V1 HTA/BT, avec neutre explicite
  lorsque le réseau le nécessite.
- Une source de référence par îlot alimenté dans le périmètre initial.
- Charges et injections P/Q ; prises de transformateurs fixées par le cas d'étude.
- Résultats : modules et angles de tension, tensions phase-neutre et entre phases,
  courants de phase et de neutre, puissances P/Q, pertes et taux de charge.
- Signalement des dépassements de seuils configurables, des îlots non alimentés,
  des données manquantes et de la non-convergence.
- Affichage du résidu, de la tolérance et du nombre d'itérations ; ne pas présenter
  comme valides les résultats d'un calcul ayant échoué.

### Courts-circuits

- Référentiel de calcul proposé : IEC 60909, avec édition et domaine
  d'application à préciser avant l'implémentation de cette fonction.
- Défauts triphasés, biphasés, monophasés à la terre et biphasés à la terre,
  sur les jeux de barres ; impédance de défaut paramétrable.
- Cas maximal et minimal avec hypothèses enregistrées.
- Résultats ciblés : courant initial symétrique I''k, courant de crête ip,
  contributions des sources et courants dans les branches.
- Autres grandeurs, notamment courants de coupure et équivalent thermique :
  inclusion à décider selon les besoins de dimensionnement.
- Prise en compte des couplages de transformateurs et des chemins homopolaires.
- Contributions des convertisseurs, moteurs et sources particulières : chaque
  modèle doit avoir un domaine documenté ; rejeter les configurations non
  couvertes au lieu de supposer silencieusement une contribution nulle.
- Les calculs de défaut et de régime permanent restent deux études distinctes,
  avec des hypothèses et paramètres propres.

L'implémentation d'une méthode inspirée d'une norme ne suffit pas à revendiquer
la conformité : celle-ci demande un périmètre défini et une validation dédiée.

### Interface et gestion des études

- Vue géographique : tracé réel des tronçons, position des postes, zoom,
  déplacement et sélection synchronisée avec les propriétés et le tableau.
- Coordonnées dans un référentiel explicite ; repère métrique local pour les
  exemples synthétiques et prise en charge initiale de Lambert-93 pour les
  données géoréférencées. Aucune reprojection implicite.
- Géométries persistées et versionnées, avec opérations de modification des
  coordonnées et des sommets. Un objet non encore localisé doit être signalé
  explicitement, sans position fictive inventée à l'import.
- Distinguer topologie électrique et géométrie géographique. Un croisement de
  tracés ne crée pas une connexion. La disposition unifilaire est recalculée
  depuis ces deux informations et n'est pas stockée comme une troisième source
  de vérité éditable.
- Distinguer longueur géométrique du tracé et longueur électrique renseignée ;
  toute éventuelle mise en cohérence doit être une action explicite et traçable.
- Fond de carte facultatif : la consultation et l'édition du réseau doivent
  fonctionner hors ligne sans service externe.
- Vue unifilaire dérivée : consulter et sélectionner les ouvrages, modifier
  leurs propriétés électriques, de charge ou d'état, et afficher les résultats.
  Les raccordements, positions et tracés globaux se modifient exclusivement
  depuis la vue géographique puis sont répercutés dans l'unifilaire.
- Organisation visuelle inspirée du principe présenté pour **Erable**, surcouche
  Enedis de PowerFactory, sans objectif de reproduction à l'identique : la vue
  géographique montre les câbles/lignes, organes de coupure et postes ; un poste
  ou ouvrage complexe peut ouvrir un **monde interne** contenant ses objets
  techniques, représentés localement en unifilaire.
- Conserver trois niveaux distincts : géométrie géographique externe, unifilaire
  global dérivé de cette géométrie et de la topologie, puis unifilaire local du
  monde interne. Modifier l'agencement interne ne déplace jamais le poste sur la
  carte et modifier la carte ne change pas silencieusement sa topologie interne.
- Les objets internes portent un rattachement explicite et versionné à leur
  conteneur (poste, tableau ou ouvrage complexe). Le passage carte → poste →
  monde interne conserve la sélection et fournit un fil d'Ariane. Les frontières
  électriques entre monde externe et interne utilisent des bornes identifiées,
  sans connexion implicite fondée sur la seule proximité graphique.
- Le premier rendu interne pourra être calculé depuis la connectivité. Si des
  ajustements locaux de présentation deviennent nécessaires, ils resteront des
  métadonnées schématiques propres au monde interne, sans devenir une seconde
  géométrie géographique du réseau.
- Inspecteur des propriétés avec unités et validation des saisies.
- Repérage des phases et du neutre dans les propriétés des raccordements.
- Annuler/rétablir les modifications de modèle.
- Enregistrement local dans un format documenté et versionné ; ouverture et
  sauvegarde sans perte des paramètres et de la disposition graphique.
- Plusieurs cas d'étude partageant un réseau de référence, avec variations
  de charge, production, prises et états de coupure.
- Tableaux de résultats filtrables et affichage des valeurs sur le schéma.
- Export CSV et rapport synthétique exportable, avec modèle, cas d'étude,
  options, hypothèses et version du moteur.
- Fonctionnement local hors ligne, sans compte obligatoire.

### Langues et traduction

La V1 doit être disponible au minimum en **français et en anglais**. La
traduction complète et sa vérification sont prévues vers la fin de réalisation,
avec la phase d'ergonomie ; les textes devront être séparés progressivement
du code et des données pour préparer ce travail.

- Choix de langue dans les paramètres de l'application, mémorisé localement.
- Traduction des menus, outils, propriétés, aides, messages de validation et
  erreurs, tableaux de résultats et libellés des rapports générés.
- Terminologie électrique cohérente entre langues, avec un glossaire métier.
- Formats numériques d'affichage et de saisie cohérents avec la langue choisie ;
  unités physiques et précision des données conservées.
- Identifiants, clés des formats d'échange, codes d'erreur structurés et valeurs
  sérialisées indépendants de la langue. Changer de langue ne crée pas une
  nouvelle version du réseau et ne modifie pas les résultats numériques.
- Noms d'ouvrages, références de technologies et commentaires saisis par les
  utilisateurs conservés tels quels, sans traduction automatique.
- Vérification des deux langues sous Windows et Linux : absence de textes
  manquants, libellés lisibles et parcours d'édition utilisables dans les deux cas.

Le prototype actuel reste en français ; cet ajout fixe le périmètre de la V1,
sans implémenter immédiatement le système de traduction.

### Ergonomie de l'édition géographique

Prévoir une phase dédiée en fin de réalisation V1, avec essais sur les parcours
réels : créer un départ, placer/raccorder un poste, choisir une technologie,
modifier un tracé et comparer deux configurations.

Évaluer et améliorer l'accès aux outils, la lisibilité des symboles et étiquettes,
la sélection des objets superposés, l'accrochage aux bornes, les aperçus de tracé,
le déplacement groupé des postes, les raccourcis et l'annulation des gestes.
Les erreurs doivent guider leur correction sans exposer inutilement le format
interne ; les formulaires métier doivent remplacer la saisie JSON pour les
opérations courantes. Les parcours doivent être vérifiés sous Windows et Linux,
à plusieurs tailles de fenêtre et sur un réseau représentatif de la cible.

### Objets, versions et échanges de modifications

Cette capacité fait partie du socle de la V1. Le contrat de données doit être
indépendant de l'interface graphique, du moteur de calcul et du fournisseur
de base de données.

| Concept | Informations et comportement attendus |
| --- | --- |
| Objet réseau | Identifiant stable et unique, type explicite, propriétés typées, références aux autres objets par identifiant |
| Monde interne | Conteneur identifié, objets techniques rattachés par identifiant et bornes de frontière explicites vers le réseau externe ; aucune duplication de l'ouvrage entre les deux niveaux |
| Instantané | État complet d'une version du réseau, avec les données nécessaires à sa reconstruction |
| Modification | Création, mise à jour de propriétés ou suppression d'un objet identifié |
| Lot de modifications | Identifiant unique, version de base, opérations ordonnées, auteur déclaré et message ; application atomique |
| Version du réseau | Identifiant immuable, version parente et lien vers le lot appliqué ; date à titre informatif |
| Cas d'étude | Référence à une version précise du réseau et variations explicites, sans copie complète obligatoire |
| Résultat | Références à la version du réseau, au cas d'étude et aux options et version du moteur |

Un objet conserve son identifiant lorsqu'il est renommé, déplacé ou modifié.
La version du schéma de données est distincte de la version du réseau et de
celle du logiciel. Les paramètres de catalogue utilisés sont figés ou référencés
par une version immuable pour permettre de reconstruire un ancien calcul.

**Format et compacité**

- Définir un schéma logique unique avec unités canoniques et règles de validation.
- Prévoir une représentation textuelle lisible pour inspection et diagnostic,
  et un conditionnement compact pour stockage et transfert. Le choix des
  encodages et de la compression sera validé sur des exemples mesurés.
- Échanger uniquement les propriétés modifiées lorsque cela suffit ; ne pas
  imposer le transfert complet du réseau à chaque changement.
- Séparer données électriques, présentation graphique et résultats pour
  partager le modèle seul ou un projet complet selon le besoin.
- Représenter le rattachement des mondes internes dans le modèle métier, pas
  seulement dans l'interface. Un delta peut modifier un objet interne ou une
  borne de frontière sans recopier tout le poste ; les références invalides et
  cycles de conteneurs sont rejetés atomiquement.
- Un export autonome doit inclure les dépendances nécessaires, notamment les
  paramètres de catalogue, ou signaler explicitement qu'il s'agit d'un delta
  nécessitant une version de base.
- Documenter la compatibilité et les migrations ; refuser explicitement un
  schéma non supporté plutôt que perdre silencieusement des informations.

**Historique, intégrité et conflits**

- Conserver les versions publiées dans l'historique ; une correction produit
  une nouvelle version. Distinguer le brouillon local de la version enregistrée.
- Vérifier la version de base avant application d'un lot ; en cas de divergence,
  signaler un conflit sans écraser les changements existants.
- Permettre deux variantes issues d'une même base. Pour la V1, la fusion peut
  demander une résolution explicite ; aucune fusion automatique n'est requise.
- Valider le lot complet, les références et les préconditions avant de le
  rendre visible. Une suppression doit traiter explicitement les références
  restantes ; aucun raccordement ne doit devenir pendant silencieusement.
- Reconnaître un lot déjà reçu par son identifiant et vérifier son contenu
  pour éviter une double application ou une réutilisation incohérente d'identifiant.
- Prévoir une empreinte d'intégrité du contenu et une sérialisation canonique
  si cette empreinte sert d'identifiant. Une empreinte ne prouve pas l'identité de l'auteur.
- Les dates et noms d'auteur ne servent pas à résoudre automatiquement l'ordre
  des modifications ; les liens de versions et préconditions font référence.

**Stockage local et bases de données**

- Assurer sauvegarde, historique et échange par fichiers sans serveur.
- Définir un contrat de persistance commun pour les objets, instantanés,
  versions et lots, permettant ensuite un adaptateur local ou distant.
- Prévoir les transactions et le contrôle de concurrence dans ce contrat,
  ainsi que la récupération d'une version et des lots manquants depuis une base connue.
- La V1 doit démontrer l'aller-retour via un adaptateur de base de données
  local et la conservation de l'historique. Le choix de la base reste ouvert.
- Le service distant partagé, l'authentification, les droits d'accès et
  l'exploitation du serveur sont une extension ; la collaboration simultanée
  en temps réel n'est pas nécessaire pour partager des versions et des deltas.

Exemple de parcours attendu : exporter la version A, modifier la puissance
d'une charge et ajouter un producteur, puis partager le lot A → B. Un autre
poste disposant de A doit reconstruire B à l'identique. Si sa base a divergé,
il doit recevoir un conflit exploitable, sans modification partielle du réseau.

## 4. Hors V1 et extensions

| Étape ultérieure | Fonctionnalités envisagées |
| --- | --- |
| Distribution avancée | Profils temporels, charges ZIP, réglage automatique des prises, lois de commande des convertisseurs |
| Protections | Courbes temps-courant, réglages, sélectivité et coordination |
| Études HTB dédiées | Nœuds PV et limites réactives, transformateurs trois enroulements, réglages et contingences N-1 |
| Qualité de l'énergie | Harmoniques, résonances, indicateurs de qualité de tension |
| Dynamique | Stabilité RMS puis, si nécessaire, transitoires électromagnétiques EMT |
| Échanges et automatisation | Formats externes choisis selon les utilisateurs, API et calculs par lots |

Sont également reportés : optimisation économique, estimation d'état,
fonds de carte externes, reprojections et échanges SIG avancés,
collaboration multiutilisateur en temps réel, hébergement du
service distant de partage et import natif PowerFactory. Les échanges de versions
et de modifications par fichiers et le contrat de persistance sont inclus en V1.
Un réseau comportant plusieurs tensions sera possible dès la V1 ; cela ne
constitue pas encore une couverture complète des études de transport HTB.

## 5. Exigences de qualité et limites

- Même modèle et mêmes options donnent des résultats reproductibles dans les
  tolérances numériques documentées sous Windows et Linux.
- Séparer modèle métier, moteurs de calcul, persistance et interface pour
  tester les calculs sans interface graphique.
- Conserver des identifiants stables et distinguer schéma graphique et topologie.
- Documenter unités, conventions de signe, bases per-unit et conversions.
- Une erreur doit identifier les équipements concernés et une cause exploitable.
- Garder l'interface réactive pendant le calcul et permettre son annulation.
- Mesurer les performances sur des cas de référence ; ne pas promettre de
  taille maximale avant le prototype numérique.
- Les alertes de tension ou de charge ne valent pas vérification réglementaire
  globale d'une installation ni dimensionnement automatique des conducteurs.

## 6. Jalons et critères d'acceptation

Ces jalons découpent la réalisation ; la V1 comprend leur ensemble.

État d'implémentation : le prototype dispose du socle de versions, des géométries,
d'une toolbox de placement, raccordement, déplacement et édition des sommets,
de la modification des paramètres électriques, de la suppression explicite
des dépendances, d'annuler/rétablir par lots de compensation et d'un premier
moteur équilibré radial. Ce dernier couvre une partie du jalon B,
avec des simplifications explicitement optionnelles pour l'exemple rural.
Les données détaillées de phases, neutre, matrices, couplages, prises et mises
à la terre sont désormais représentées et validées dans le format versionné,
avec deux cas synthétiques à solution analytique. Le moteur équilibré exploite
les matrices symétriques et les pertes cuivre dans son domaine actuel.
Un premier moteur déséquilibré ABC/ABCN couvre désormais les départs radiaux
avec source équivalente secondaire, neutre explicite et prises de terre aval
sur une référence équipotentielle propre à chaque zone galvanique. Il franchit
aussi les transformateurs Dyn et Yzn
HTA/BT à secondaire directement mis à la terre, en appliquant rapport, prise
fixe, déphasage, impédances directe/inverse et homopolaire. Les courants et pertes
des lignes, transformateurs et terres sont séparés.
Il est validé par des solutions analytiques monophasées et par comparaison au
moteur équilibré sur un cas symétrique. Les terres BT sont désormais découpées
en zones galvaniques ancrées à la source ou au secondaire Dyn/Yzn. L'ancrage
source accepte désormais une impédance complexe non nulle. Les modèles de sol
détaillés, les calculs maillés et les
courts-circuits restent à développer.
Voir le [premier jalon ABCN](calcul-desequilibre.md) pour ses limites précises.
Voir le [modèle électrique détaillé](modele-electrique.md) pour les conventions
et les données encore à compléter.
Voir la [spécification du premier moteur](calcul-regime-permanent.md) pour son
domaine et les cas analytiques déjà vérifiés.
Une première vue unifilaire dérivée est disponible : ses colonnes suivent la
connectivité et son ordre vertical l'agencement géographique ; elle partage la
sélection et l'inspecteur mais verrouille la géométrie. Le déplacement groupé
des postes et l'ajustement automatique des tracés incidents restent à réaliser
pour le jalon d'interface V1. La navigation dans les mondes internes des postes,
leurs frontières électriques et leur vue unifilaire locale sont ajoutées au
périmètre d'un futur jalon ; elles ne sont pas encore représentées dans le format.

| Jalon | Livrable | État / critère de sortie |
| --- | --- | --- |
| A — Spécification numérique | Modèles, conventions et cas de référence | Socle établi ; à compléter pour chaque nouveau calcul avec résultats attendus et tolérances |
| A bis — Contrat de données | Objets, instantanés, versions et lots documentés | Socle opérationnel ; compacité et montée en charge encore à mesurer |
| B — Régime permanent équilibré | Moteur utilisable sans interface | Radial opérationnel ; cas maillés et validation indépendante restant à couvrir |
| C — Distribution déséquilibrée | Phases, neutre et transformateurs | ABCN radial, terres multi-zones, neutre source impédant et groupes Dyn/Yzn disponibles ; cas rural indépendant et mailles à compléter |
| D — Courts-circuits | Méthode et modèles du périmètre retenu | Défauts supportés validés, cas min/max et contributions vérifiés |
| E — Application V1 | Cas d'étude, résultats, comparaisons et exports | Parcours complet réalisé sur Windows et Linux sur un réseau HTA/BT de référence |
| E bis — Technologies | Catalogue versionné et instanciation des ouvrages | Choix de plusieurs types, variantes explicites et reconstruction d'une ancienne étude après évolution du catalogue |
| E ter — Vues réseau et mondes internes | Carte externe, unifilaire global dérivé et unifilaires locaux des postes | Navigation sans ambiguïté, rattachements et bornes persistés, aucune divergence silencieuse entre géographie externe et contenu interne |
| F — Ergonomie, langues et livraison | Édition géographique affinée, français/anglais et distributions | Parcours métier testés avec des utilisateurs cibles dans les deux langues ; corrections d'ergonomie et essais natifs Windows/Linux |

### Ordre proposé des prochains jalons

1. **Achever C — modèle déséquilibré de distribution** : cas rural HTA/BT
   complet avec comparaison indépendante.
2. **Achever B/C — topologies d'exploitation** : calcul des réseaux maillés dans
   le domaine retenu, îlots et configurations avec points de coupure, sans
   confondre une maille dessinée et une maille effectivement fermée.
3. **Réaliser D — courts-circuits** : figer l'édition et le sous-ensemble IEC
   60909, développer défauts triphasés, biphasés et à la terre, cas min/max,
   contributions et cas analytiques HTA/BT.
4. **Réaliser E bis — catalogue technologique** : types versionnés de câbles,
   lignes et transformateurs, instanciation explicite, variantes locales et
   mise à jour contrôlée sans modifier les études historiques.
5. **Compléter E — études et restitutions** : cas de charge/production, comparaison
   avant-après, états de coupure, persistance des résultats, exports CSV et rapport.
6. **Réaliser E ter — mondes internes** : ajouter le modèle de conteneur et ses
   bornes, puis la navigation carte → poste → unifilaire interne et le retour
   synchronisé vers la carte, selon l'inspiration fonctionnelle d'Erable.
7. **Finaliser F — usage et livraison** : formulaires métier, ergonomie géographique,
   français/anglais, performances, essais natifs et paquets Windows/Linux.

Cet ordre pourra faire avancer en parallèle le catalogue et les maquettes de
mondes internes, mais les calculs de défaut doivent rester prioritaires pour
atteindre la promesse fonctionnelle de la V1.

Pour le contrat de données, vérifier aussi les échanges entre deux projets locaux,
la réception répétée d'un lot, les variantes concurrentes et la sauvegarde puis
relecture de l'historique via l'adaptateur de base de données. Mesurer la taille
des instantanés et des deltas et les temps de lecture/écriture sur le cas rural
retenu puis sur un réseau plus grand ; fixer ensuite les objectifs de compacité.

Pour chaque moteur, utiliser des cas analytiques simples et des jeux de référence
traçables. Comparer, lorsque disponible, avec un outil indépendant en alignant
les modèles et options. Définir les tolérances par grandeur avant les comparaisons.
Vérifier aussi les échecs attendus : réseau isolé, paramètres invalides, modèle
non supporté et non-convergence.

## 7. Questions à trancher avant le développement des moteurs

1. Paramètres et dimensions du cas rural HTA/BT retenu, avec production photovoltaïque ?
2. Schémas de liaison à la terre et couplages à couvrir dans les premiers cas ?
3. Place des moteurs et des productions à convertisseurs dans les réseaux cibles ?
4. Grandeurs de court-circuit nécessaires au-delà de I''k et ip ?
5. Réseaux de référence disponibles et possibilités de comparaison indépendante ?
6. Taille typique des réseaux et temps de calcul acceptable ?
7. Distribution et licence du logiciel, formats d'échange prioritaires et éventuelles langues supplémentaires au-delà du français et de l'anglais ?
8. Granularité de l'historique : enregistrement explicite d'un lot, fréquence des
   instantanés et présentation des différences à l'utilisateur ?

Le choix de la bibliothèque graphique et des bibliothèques numériques sera fait
après ces arbitrages et un prototype ciblé, sans imposer de dépendances à ce stade.

## 8. Références de cadrage

Référence métier France — Enedis :

- [Enedis-NMO-RES_011E — Structure des réseaux et des ouvrages composant le Réseau Public de Distribution géré par Enedis](https://www.enedis.fr/media/4383/download), version 1 du 03/12/2024.

Ces pages décrivent les catégories de fonctions du produit de référence ; elles
ne constituent ni une spécification normative ni une validation de CalcElec.

- [PowerFactory — Vue d'ensemble des fonctions](https://www.digsilent.de/en/functions.html)
- [PowerFactory — Répartition de puissance](https://www.digsilent.de/en/load-flow-analysis.html)
- [PowerFactory — Courts-circuits](https://www.digsilent.de/en/short-circuit-analysis.html)
