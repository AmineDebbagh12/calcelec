# Régime permanent déséquilibré — réseau radial ABC/ABCN

Moteur : `radial-four-wire-0.4`, dans `src/unbalanced.rs`.
Il couvre les réseaux radiaux BT à quatre conducteurs et un premier franchissement
HTA/BT Dyn depuis un réseau amont ABC alimenté par une source équilibrée idéale.
Le neutre BT est représenté sans élimination, avec ses impédances propres et ses
couplages aux phases.
Ce jalon est une première partie du moteur HTA/BT déséquilibré prévu pour la V1.

## Domaine et données requises

- Données détaillées A/B/C ou A/B/C/N sur tous les jeux de barres et branches actifs ;
  les conducteurs d'une ligne ou d'un interrupteur correspondent à ses deux bus.
- Source active unique. Sur une frontière BT ABCN, son neutre est directement
  mis à la terre ; sur le côté HTA ABC, aucun conducteur N n'est inventé.
- Aux autres bus, `isolated` signifie aucune prise de terre locale : leur
  conducteur N reste connecté au neutre amont par les tronçons. Une déclaration
  `impedance` ajoute une prise de terre, éventuellement idéale (R = X = 0).
- Matrices primitives série 3×3 ou 4×4, avec ordre des conducteurs explicite. Elles
  peuvent être dissymétriques entre phases et posséder des couplages mutuels,
  sous réserve des contrôles de réciprocité et de passivité du modèle.
- Interrupteurs fermés ABC ou ABCN idéaux ; les interrupteurs ouverts sont exclus.
- Transformateur Dyn orienté HTA vers BT, côté HTA delta ABC, côté BT étoile
  ABCN avec neutre exposé et directement mis à la terre. Tensions assignées
  cohérentes avec les bus, pertes fer et courant à vide explicitement nuls,
  impédance homopolaire renseignée et prise fixe autorisée.
- Charges et productions P/Q constantes avec connexion détaillée phase-neutre,
  entre phases, étoile triphasée avec neutre ou triangle triphasé.
- Réseau en service connecté, sans boucle ni branche parallèle fermée.

La présence de matrices est obligatoire : le moteur ne reconstruit pas une
impédance de neutre depuis les seuls R/X du modèle historique.
Les phases manquantes, les tronçons monophasés, impédances de terre non nulles
à la source, sources multiples, points étoile flottants, shunts non nuls et autres
couplages de transformateurs ne sont pas encore pris en charge.
Ils sont refusés explicitement. Les objets hors service et les branchements
à des bus hors service sont exclus du calcul, comme pour le moteur équilibré.

Pour étudier un départ BT sans poste explicite, placer la frontière amont à
l'équivalent source du secondaire. Le modèle Dyn permet sinon de partir du bus
HTA amont. Les impédances de séquence de la source et les contributions
au défaut sont conservées comme données mais ne sont pas utilisées ici.
Les harmoniques, courants de fuite et potentiels locaux du sol ne sont pas
calculés. Dans chaque zone galvanique, les courants de retour par les impédances
de terre déclarées sont calculés vers une référence équipotentielle, sans
couplage entre électrodes ni entre zones. Le modèle
ne remplace pas une représentation détaillée du sol ou des noyaux magnétiques.

## Conventions numériques

Les calculs utilisent directement les volts, ampères, ohms, watts et vars.
L'ordre interne et l'ordre des tableaux exportés sont **A, B, C, N**. Une matrice
stockée dans un autre ordre est réordonnée avec ses coefficients ; aucune phase
n'est permutée physiquement.

Les tensions des conducteurs sont référencées à la terre source. Les tensions
qui alimentent les injections sont les différences entre leurs bornes :

```text
Iphase = conj(S / (Vphase − Vneutre))          charge phase-neutre
Ifrom  = conj(S / (Vfrom − Vto))              charge entre phases
Ito    = −Ifrom
```

La puissance des injections triphasées est un total : chaque élément étoile
ou triangle reçoit un tiers de P/Q. Une production est comptée négativement dans
la demande nette. Aucun regroupement équilibré des charges monophasées n'est fait.

La source impose les angles A = 0°, B = −120°, C = +120° et VN = 0. Sa tension
phase-neutre est `Unominale_composée × voltage_pu / sqrt(3)`. Les résultats par
phase en pu utilisent `Unominale_composée / sqrt(3)` comme base.

Le moteur utilise un balayage arrière/avant par sommation des courants :

1. Calculer les courants des injections d'après leurs tensions aux bornes.
2. Sommer les courants de chaque conducteur depuis les feuilles vers la source.
3. Recalculer les tensions vers l'aval par la matrice de ligne ou la relation
   de transformation en composantes symétriques.
4. Recalculer les courants puis effectuer un balayage de contrôle.

Le résidu est le maximum des écarts de phaseurs de tension du balayage de
contrôle, normalisé par la base phase-neutre de chaque bus. Les valeurs par
défaut sont 200 itérations et une tolérance de 10⁻⁹ pu. L'API permet de les
modifier. L'annulation est vérifiée pendant la construction, les injections
et les balayages. Non-convergence, limites numériques ou annulation ne publient
aucun résultat partiel.

Le principe de sommation est décrit par [MATPOWER](https://matpower.org/documentation/ref-manual/legacy/functions/calc_v_i_sum.html).
La distinction entre neutre explicite et terre est également présentée dans
les [conventions OpenDSS / EPRI](https://opendss.epri.com/NeutralRules.html).
L'implémentation et les validations de CalcElec sont propres au projet ; aucune
comparaison numérique avec ces outils n'a encore été effectuée.

## Transformateur Dyn — extensions 0.3 et 0.4

Le côté BT est ramené en composantes symétriques. Avec `a = exp(j120°)`, les
composantes sont calculées explicitement depuis A/B/C. Le rapport `r` inclut la
position fixe de prise ; l'angle `θ` vient du groupe horaire (`−clock × 30°`,
normalisé). Les séquences directe et inverse utilisent des rotations opposées :

```text
V1_BT = exp(jθ) / r × V1_HTA − Z1_BT × I1_BT
V2_BT = exp(−jθ) / r × V2_HTA − Z1_BT × I2_BT
V0_BT =                         − Z0_BT × I0_BT

I1_HTA = exp(−jθ) / r × I1_BT
I2_HTA = exp(jθ)  / r × I2_BT
I0_HTA = 0
```

`Z1_BT` provient de uk et des pertes cuivre sur la base triphasée assignée,
ramenée en ohms au secondaire. `Z0_BT` est l'impédance homopolaire déclarée côté
HTA divisée par `r²`. Le triangle bloque le courant homopolaire dans les lignes
HTA ; le modèle conserve son effet sur les tensions et pertes BT. Les courants
des deux enroulements sont donc distincts et exportés séparément.

Le secondaire constitue une frontière de terre idéale avec `VN = 0`. Le courant
de neutre métallique rejoint le point étoile ; il ne doit pas être confondu avec
un courant d'électrode. Depuis la version 0.4, les prises de terre supplémentaires
en aval sont résolues dans une zone galvanique ancrée à ce point étoile. Leurs
courants de neutre s'arrêtent à cet ancrage et ne traversent jamais le delta.

Le modèle reste un équivalent de séquences : pas de saturation, excitation,
couplage magnétique détaillé du noyau ni harmoniques. Le rôle du delta dans le
blocage homopolaire et l'importance des données de noyau sont documentés par le
[modèle généralisé OpenDSS / EPRI](https://opendss.epri.com/GeneralizedTransformermodel.html)
et sa note sur les [effets de noyau](https://opendss.epri.com/ModelingTransformerCoreEffectsin.html).
CalcElec ne revendique pas une équivalence complète avec ce modèle nodal général.

## Prises de terre du neutre — extensions 0.2 et 0.4

Les prises de terre aval sont connectées à une **référence de terre commune
équipotentielle dans leur zone galvanique**. Chacune porte son impédance complexe
déclarée, avec R non négative et X signée. La source ou le point étoile Dyn garde
une liaison idéale du neutre à cette référence. Les impédances et potentiels
propres au sol entre électrodes,
les tensions de contact et les couplages entre électrodes ne sont pas modélisés.
La tension VN publiée est relative à la référence commune, pas une mesure du
potentiel du sol local.

À courants d'injection momentanément fixés, le système des terres est résolu
exactement avant le balayage suivant. Soit V⁰ la tension sans les courants
supplémentaires des terres, et g le vecteur des courants du neutre vers la terre :

```text
H[i,j] = somme des Z[N,N] sur le chemin commun ancrage→i et ancrage→j
(diag(Zterre) + H) × g = V⁰neutre
Vneutre[i] = Zterre[i] × g[i]
```

Chaque électrode est affectée à l'ancrage idéal le plus proche en remontant le
réseau : neutre de la source ou point étoile d'un Dyn. Les termes `H[i,j]` sont
nuls entre deux zones différentes. La matrice globale est donc formée de blocs
galvaniquement indépendants, même si elle est factorisée en une seule opération.

Cette relation conserve les couplages phase-neutre : les quatre tensions sont
recalculées avec la matrice complète après ajout des courants g dans les
conducteurs N de leurs chemins vers la source. Les charges et productions sont
ensuite recalculées avec leurs nouvelles tensions aux bornes.
Le résultat publié utilise le dernier état linéaire : loi d'Ohm des branches et
conditions de terre sont satisfaites ensemble, et le résidu extérieur mesure
l'écart entre les deux derniers états de tension.

La matrice des terres est factorisée une seule fois avec mise à l'échelle des
lignes et pivot partiel ; la factorisation est réutilisée pendant les itérations.
Une liaison idéale ne reçoit aucune résistance artificielle. Si plusieurs
liaisons idéales redondantes rendent les courants individuels indéterminés, ou
si un pivot normalisé tombe sous 10⁻¹², le calcul retourne une erreur explicite.
Les contrôles de valeurs finies, de conditions de terre et d'annulation restent
actifs. Le solveur dense de ce jalon accepte au plus **256 prises de terre aval** ;
il ne s'agit pas d'une taille maximale annoncée pour la V1.

Le courant de chaque ancrage idéal est l'opposé de la somme des courants des
prises aval de sa zone. Il ferme localement le bilan des courants de terre, avec
une puissance dissipée nulle puisque `VN = 0` à l'ancrage.
Les impédances des terres aval consomment `Zterre × |g|²` : cette puissance est
incluse dans le bilan global, séparément des pertes des lignes. Une réactance
capacitive peut fournir une puissance réactive, qui reste signée dans le résultat.

Le modèle s'appuie sur des équations de circuit propres à CalcElec. Les
[conventions de neutre OpenDSS / EPRI](https://opendss.epri.com/NeutralRules.html)
et les travaux sur le [calcul quadrifilaire avec mise à la terre](https://research.manchester.ac.uk/en/publications/power-flow-in-four-wire-distribution-networks-general-approach-2/)
constituent des références de contexte ; aucune équivalence numérique avec
ces logiciels ou publications n'est revendiquée à ce stade.

## Résultats et traçabilité

Par bus : phaseurs A/B/C/N par rapport à la terre source, conducteurs présents,
tensions et angles A-N/B-N/C-N (phase-terre lorsqu'un bus ABC n'a pas de N),
tensions composées AB/BC/CA, potentiel du neutre et rapport de
séquence inverse/directe U2/U1. Ce dernier est calculé sur les tensions
phase-neutre ; ce n'est pas l'écart maximal entre trois modules de tension.

Par branche : phaseurs des quatre courants, flux P/Q amont et aval, pertes
actives et réactives. Les orientations sont de la source vers l'aval,
indépendamment de l'ordre `from/to` stocké dans l'objet.
La puissance d'une extrémité est `sum(Vk × conj(Ik))`, neutre inclus.
Les pertes des lignes sont calculées par `sum((Z×I)k × conj(Ik))` : elles
incluent la résistance du neutre et les couplages, sans les réduire à trois
pertes de phase. Le total inclut aussi les puissances des prises de terre.

Le résultat 0.2 ajoute les champs `line_loss_w`, `line_loss_var`,
`grounding_loss_w`, `grounding_loss_var` et `groundings`. Chaque prise de terre
indique le bus, le phaseur du courant vers la terre, celui de VN et ses puissances
active/réactive. Le tableau de l'application montre les modules, angles et pertes.
Le survol d'un bus sur la carte indique également son courant de terre.
La version 0.3 ajoute `conductors` sur les bus,
`currents_from_a` / `currents_to_a` sur les branches et
`transformer_loss_w/var`. `currents_a` reste le courant côté aval pour les
lecteurs existants. Les champs `total_loss_w/var` sont les sommes lignes +
transformateurs + terres. Les flux de
branche restent ceux des conducteurs métalliques ; le courant de terre peut
ainsi être différent du courant du conducteur N.
La version 0.4 conserve le format et publie, dans `groundings`, chaque ancrage
de zone avec `VN = 0` et le courant opposé à la somme de ses électrodes.

Le taux de charge d'une ligne est le maximum des modules de courant rapporté à
`max_current_a`. Celui d'un transformateur est le maximum des taux des phases
sur ses deux côtés, à partir de `Sn/(sqrt(3) Un)`. Des limites distinctes selon
les conducteurs et technologies devront
être ajoutées avec le catalogue ; aucun calcul thermique n'est réalisé ici.

Le résultat conserve le projet, sa version, la version du moteur, les options et
les hypothèses. L'erreur de bilan publiée est le module de
`Ssource − Sdemande nette − Spertes`, en VA.
Les résultats sont invalidés après modification de la version, et les réponses
tardives d'un calcul précédent sont écartées. L'export est distinct du projet
et ne modifie pas son schéma 4.

## Utilisation

Dans l'application, ouvrir `examples/reference-unbalanced-bt.json`, choisir
**Déséquilibré ABCN**, puis **Calculer le régime permanent**. Les tableaux
présentent tensions/angles par phase, courant de neutre et pertes. Le survol
sur la carte donne les valeurs par phase ; les dépassements des seuils de
tension et de courant renseignés sont signalés.

```sh
cargo run -- --unbalanced-load-flow examples/reference-unbalanced-bt.json
cargo run -- --reference-case unbalanced-bt nouveau-projet.json
```

L'exemple combine 12/8/5 kW de consommation sur A/B/C et 10 kW de production
photovoltaïque sur C, avec une demande réactive totale de 3,4 kvar. Les paramètres
sont synthétiques et ne constituent pas des références de technologies Enedis.
Le calcul initial donne environ 225,129 / 228,261 / 236,878 V phase-neutre,
5,329 V de potentiel du neutre et 68,120 A dans le conducteur N. Il converge en
5 itérations, avec environ 536,926 W de pertes et une erreur de bilan inférieure
à 0,001 VA.

Les tests vérifient notamment la solution analytique d'une charge phase-neutre
avec impédance de boucle phase + neutre et couplage mutuel, une charge entre
phases, l'accord avec le moteur équilibré, les connexions triangle, les flux
inverses, les matrices dissymétriques, la conservation des courants à une
bifurcation, la conservation de puissance et le refus des domaines non couverts.

## Exemple Dyn11 HTA/BT

Ouvrir `examples/reference-dyn-transformer.json`, choisir **Déséquilibré ABCN**
et lancer le calcul. Le cas synthétique contient un Dyn11 20 kV/400 V, 100 kVA,
uk 4 %, pertes cuivre assignées 1 kW et trois charges BT de 12/8/5 kW avec une
demande réactive totale de 3,4 kvar.

```sh
cargo run -- --unbalanced-load-flow examples/reference-dyn-transformer.json
cargo run -- --reference-case dyn-transformer nouveau-projet.json
```

Le calcul initial converge en 4 itérations. Les tensions A-N/B-N/C-N sont proches
de 229,608 / 230,087 / 230,435 V, avec environ 71,972 W de pertes actives et
278,251 var de pertes réactives dans le transformateur. Le courant homopolaire
des lignes HTA et leur conducteur N restent nuls ; le bilan est inférieur à
0,001 VA. Le cas à vide vérifie séparément 400/sqrt(3) V et le déphasage +30°.
Les valeurs sont destinées à la validation logicielle, pas à un catalogue Enedis.

## Exemple Dyn11 avec terre BT aval

Ouvrir `examples/reference-dyn-grounded.json` pour tester le franchissement
HTA/BT et la zone de terre secondaire dans un même calcul. Le Dyn11 est suivi
d'un câble ABCN de 50 m et d'une terre terminale synthétique de 1 ohm.

```sh
cargo run -- --unbalanced-load-flow examples/reference-dyn-grounded.json
cargo run -- --reference-case dyn-grounded nouveau-projet.json
```

Le calcul initial converge en 5 itérations. Le courant de l'électrode vaut
environ 1,042 A et son potentiel de neutre 1,042 V ; le point étoile publie le
courant exactement opposé avec `VN = 0`. Les pertes sont proches de 114,212 W
dans la ligne, 72,819 W dans le transformateur et 1,086 W dans la terre. L'erreur
de bilan reste inférieure à 0,001 VA.

## Exemple avec terres intermédiaire et terminale

Ouvrir `examples/reference-grounded-bt.json`, conserver **Déséquilibré ABCN**
et lancer le calcul. Il reprend les charges et le PV du premier exemple,
avec le câble partagé en deux tronçons de 50 m. Des impédances de terre purement
résistives de 0,5 ohm au point intermédiaire et 1 ohm au point terminal sont
choisies à titre synthétique, sans prescription de valeurs pour un réseau réel.

```sh
cargo run -- --unbalanced-load-flow examples/reference-grounded-bt.json
cargo run -- --reference-case grounded-bt nouveau-projet.json
```

Résultat initial : 5 itérations, environ 467,055 W de pertes des lignes et
33,294 W dans les terres, soit 500,349 W au total. Les courants des terres
intermédiaire et terminale sont respectivement proches de 4,588 A et 4,772 A ;
le retour à la source a un module de 9,360 A. Les potentiels de neutre sont
proches de 2,294 V et 4,772 V. L'erreur de bilan est inférieure à 0,001 VA.

Les tests supplémentaires comparent une charge monophasée avec retour parallèle
neutre/terre à sa solution analytique complexe, puis deux prises de terre à leur
équivalent parallèle résistif. Ils vérifient aussi le neutre fixé idéalement,
les connexions idéales redondantes, les impédances capacitives, les lois de
Kirchhoff avec couplages mutuels, la séparation des pertes et l'affichage des
résultats. Un test matriciel vérifie aussi que deux zones séparées forment des
blocs indépendants, et le cas Dyn vérifie les lois de Kirchhoff et les pertes.
Les autres couplages de transformateurs, le neutre source raccordé par une
impédance non nulle et le modèle de sol détaillé
restent des extensions à réaliser.
