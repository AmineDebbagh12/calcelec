# Modèle électrique détaillé — schéma 4

Le champ optionnel `electrical` de chaque objet complète ses paramètres
historiques `equipment`. Son absence signifie **données détaillées inconnues**,
sans supposer de conducteurs, de mise à la terre ou de couplage.
Les paramètres communs restent dans `equipment` : bornes, tension nominale,
longueur électrique, puissance assignée, uk et puissances consommées/injectées.
Les géométries restent indépendantes.

Les objets détaillés sont typés, sérialisables et modifiables par une opération
`set_electrical`. Celle-ci participe aux validations, au partage, à SQLite et aux
lots d'annulation/rétablissement. Modifier simultanément une phase dans
`equipment` et sa connexion détaillée est possible dans le même lot.

## Conducteurs, lignes et neutre

Un jeu de barres déclare sa liste de conducteurs parmi `A`, `B`, `C`, `N`.
La liste doit être non vide, unique et contenir au moins une phase. Les
raccordements détaillés doivent utiliser des conducteurs présents sur leurs
jeux de barres lorsqu'ils sont déclarés. Un jeu de barres sans données détaillées
reste accepté comme modèle incomplet, signalé par le diagnostic.

Une ligne déclare l'ordre de ses conducteurs et sa matrice primitive complète
`series_ohm_per_km`. Chaque coefficient est `{ "r": ..., "x": ... }`.
Une ligne ABCN comporte quatre lignes et quatre colonnes ; le neutre reste
explicite. Aucune réduction de Kron n'est effectuée à l'import.
Les matrices doivent être réciproques (symétriques sans conjugaison), finies
et de la bonne taille. La partie résistive doit être positive semi-définie,
ce qui interdit une création de puissance active par le modèle série.
La vérification utilise une tolérance numérique de 10⁻¹² fois la plus grande
amplitude de coefficient, avec une échelle minimale de 1.

Les admittances shunt optionnelles utilisent la même dimension et l'ordre,
avec coefficients `{ "g": ..., "b": ... }` en S/km. Elles ont les mêmes
contrôles de réciprocité et de passivité de leur partie réelle.
`null` signifie que le modèle ne déclare pas de shunt, et non une mesure de
capacité nulle. Le moteur équilibré actuel refuse une matrice shunt non nulle.

La longueur physique électrique en mètres multiplie les valeurs par kilomètre.
Les coordonnées géographiques n'entrent pas dans ce calcul.
Les lignes ne disposent pas encore de permutation des phases entre leurs
extrémités ; les étiquettes A/B/C/N gardent le même sens aux deux bornes.

## Mise à la terre

Le régime d'un neutre ou d'un point étoile est décrit explicitement :

| `type` | Signification |
| --- | --- |
| `unspecified` | Information à renseigner |
| `not_applicable` | Aucun point neutre concerné, par exemple un triangle ou un bus sans N |
| `isolated` | Pas de mise à la terre locale déclarée |
| `impedance` | Liaison à la terre par `ohm: {r, x}` ; R et X nuls représentent une liaison idéale |

R doit être finie et non négative, X finie. Une réactance négative est possible
pour représenter une composante capacitive déclarée. Un bus avec N ne peut pas
utiliser `not_applicable`, et un bus sans N doit l'utiliser.
`isolated` sur un bus aval indique l'absence de prise de terre locale ; le
conducteur N peut rester relié au neutre amont par une ligne.

Le modèle conserve des impédances équivalentes de terre, sans réseau de sol,
sans conducteur PE distinct et sans calcul de tension de contact à ce stade.
Il ne prétend pas représenter l'intégralité d'une étude de sécurité des personnes.

## Transformateurs à deux enroulements

Chaque côté, `from` et `to`, décrit :

- sa tension composée assignée `rated_voltage_ll_v` ;
- son couplage `delta`, `star` ou `zigzag` ;
- la connexion d'une borne neutre au bus par `neutral_terminal` ;
- la mise à la terre de son point étoile, qui peut être interne même sans N distribué.

Un triangle ne possède pas de borne neutre et exige `not_applicable` pour
son point étoile. Une borne N déclarée exige N sur le bus quand ses conducteurs
sont renseignés. Les groupes horaires vont de 0 à 11. Leur parité est vérifiée
avec les familles de couplage ; cette vérification ne constitue pas une
validation exhaustive des variantes constructives.
La convention est le déphasage du côté `to` relatif à `from` : `−30 × clock`
degrés modulo 360. Ainsi l'indice 11 donne +30°.

Les données d'essais contiennent les pertes cuivre totales à courant assigné,
les pertes fer et le courant à vide en pourcentage du courant assigné. Les deux
dernières données peuvent rester inconnues (`null`) ; une valeur nulle doit
être déclarée explicitement pour être traitée comme telle par le moteur.
Sur la base de puissance triphasée assignée :

```text
Rpu = pertes_cuivre_W / puissance_assignée_VA
|Zpu| = uk_percent / 100
Xpu = sqrt(|Zpu|² − Rpu²)
```

Les pertes cuivre doivent être compatibles avec uk. Si les deux données à vide
sont connues, les pertes fer ne peuvent pas dépasser la puissance apparente à vide.

Le régleur optionnel porte son côté, ses positions minimale, maximale,
neutre et actuelle, ainsi que le pas en pourcentage. Les positions et facteurs
sur toute la plage sont vérifiés. Il ne représente qu'un réglage de rapport :

```text
facteur = 1 + (position − position_neutre) × pas_percent / 100
rapport = Uassignée_from / Uassignée_to
rapport_effectif = rapport × facteur  si prise côté from
rapport_effectif = rapport / facteur  si prise côté to
```

L'impédance homopolaire optionnelle est en ohms, ramenée au côté `from`, et
exclut les impédances externes de neutre. Le noyau, la magnétisation homopolaire
et la répartition des impédances de fuite entre enroulements ne sont pas encore
détaillés. Cette donnée seule ne permet pas de calculer tous les défauts.

## Sources, consommations et productions

Une source peut déclarer ses impédances internes directe, inverse et homopolaire
en ohms, à la tension nominale de son bus, ainsi que son régime de neutre.
Ces impédances excluent la mise à la terre du neutre déclarée séparément, pour
éviter son double comptage dans les futurs calculs de défaut. Une impédance de
séquence inconnue est `null` ; une impédance interne explicitement nulle est refusée.
Le moteur de régime permanent utilise encore la tension idéale imposée au bus
source ; ces impédances sont des entrées pour le futur moteur de défaut.

Les injections déclarent leur connexion : phase-neutre, entre deux phases,
étoile triphasée avec ou sans N, ou triangle triphasé. La connexion doit être
cohérente avec la phase historique et les conducteurs disponibles. La connexion
entre phases utilise le repère historique `Abc`, le détail étant porté par ses
bornes de phase ; elle reste une injection monophasée au sens du calcul.

La contribution au défaut est déclarée inconnue, explicitement nulle, ou limitée
par une valeur de courant. Cette dernière est une donnée initiale pour les
convertisseurs, sans encore décrire leur loi dynamique ou dépendance à la tension.
Aucun calcul de contribution au défaut n'est réalisé dans ce jalon.

## Diagnostic et moteur équilibré

L'application et `--check` listent les données détaillées absentes : modèles,
mises à la terre inconnues, essais à vide, paramètres homopolaires ou contributions
au défaut. Ce diagnostic est un inventaire ; zéro information manquante ne
prouve ni la conformité des données ni leur couverture par un moteur.

Le moteur `radial-balanced-0.2` exploite une matrice de ligne équilibrée exacte :
diagonales de phase identiques, couplages mutuels identiques et couplages au
neutre identiques. Dans ce cas, `Z1 = Zpropre − Zmutuelle`, sans courant de neutre
dans l'équivalent équilibré. Les champs R/X historiques deviennent un repli
pour les objets sans matrice ; ils ne remplacent pas une matrice fournie.

Pour les transformateurs détaillés, le moteur utilise R/X issus des essais,
à rapport nominal et horaire 0, avec excitation explicitement nulle. Il refuse
les prises hors nominal, déphasages, excitations non nulles ou inconnues, lignes
dissymétriques et interrupteurs non triphasés, même si les anciennes options de
simplification sont activées. Il ne supprime pas silencieusement un paramètre
nouvellement fourni. Les données non encore calculables restent éditables et
partageables pour préparer les prochains moteurs.

L'édition initiale de ces données se fait dans la rubrique **Modèle électrique
détaillé — édition avancée** de l'inspecteur, sous forme de JSON typé. Le bouton
de préremplissage fournit une structure provisoire, à vérifier et compléter ;
il n'est pas une identification automatique d'un équipement réel. La validation
se fait par **Créer une version avec ces changements**. Les formulaires dédiés
aux matrices et aux enroulements restent à améliorer.

## Cas de référence et validation

Quatre projets autonomes ciblent les modèles détaillés dans `examples/` :

| Cas | Modèle et contrôle analytique |
| --- | --- |
| `reference-four-wire.json` | Ligne ABCN de 100 m, matrice 4×4 réciproque et passive ; réduction directe 0,38 + j0,06 ohm/km ; charge triphasée 30 kW ; solution analytique complexe à deux nœuds |
| `reference-transformer.json` | Transformateur Yyn0 20 kV/400 V, 100 kVA, uk 4 %, pertes cuivre assignées 1 kW, excitation explicitement nulle ; charge 30 kW + j9 kvar ; solution analytique par l'impédance série ramenée au secondaire |
| `reference-dyn-transformer.json` | Transformateur Dyn11 20 kV/400 V avec trois charges monophasées différentes ; rapport, déphasage, séquences directe/inverse/homopolaire, courants des deux côtés et pertes vérifiés |
| `reference-dyn-grounded.json` | Même Dyn11 suivi d'un câble ABCN de 50 m et d'une prise de terre terminale ; courant d'électrode, compensation au point étoile, tensions et pertes vérifiés dans la zone BT |

Ces valeurs sont synthétiques et ne décrivent pas des ouvrages Enedis types.
Les tests vérifient tensions, angles, courants et pertes face aux solutions
analytiques, et le refus des modèles incompatibles ou non supportés. Ils
couvrent aussi les permutations d'ordre de conducteurs, les données invalides,
les migrations et les échanges/annulations/stockages des modifications détaillées.
Le moteur déséquilibré exploite les matrices ABCN et un premier modèle Dyn HTA/BT
sur des départs radiaux, avec neutre BT explicitement représenté ; voir le
[jalon déséquilibré](calcul-desequilibre.md). Son extension aux autres couplages,
au neutre source non directement à la terre et aux modèles de sol détaillés,
la comparaison à des outils indépendants et la validation
normative des calculs de courts-circuits restent à réaliser.

Références de formulation consultées : [matrices de ligne OpenDSS / EPRI](https://opendss.epri.com/Properties14.html),
[modèle généralisé de transformateur OpenDSS / EPRI](https://opendss.epri.com/GeneralizedTransformermodel.html),
[effets de noyau et séquence homopolaire OpenDSS / EPRI](https://opendss.epri.com/ModelingTransformerCoreEffectsin.html)
et [paramètres d'essai des transformateurs pandapower](https://pandapower.readthedocs.io/en/stable/elements/trafo.html).
Le schéma et les validations ci-dessus sont ceux de CalcElec, sans revendication
d'équivalence complète à ces outils.
