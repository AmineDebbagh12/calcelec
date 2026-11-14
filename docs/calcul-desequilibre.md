# Régime permanent déséquilibré — premier jalon BT ABCN

Moteur : `radial-four-wire-0.1`, dans `src/unbalanced.rs`.
Il couvre les réseaux radiaux à quatre conducteurs, à un seul niveau de tension,
alimentés par une source de tension équilibrée idéale. Le neutre est représenté
sans élimination, avec ses impédances propres et ses couplages aux phases.
Ce jalon est une première partie du moteur HTA/BT déséquilibré prévu pour la V1.

## Domaine et données requises

- Données détaillées A/B/C/N sur tous les jeux de barres et tronçons actifs.
- Source active unique, raccordée à un bus dont le neutre est directement mis
  à la terre ; les deux déclarations de régime de neutre doivent être cohérentes.
- Aux autres bus, `isolated` signifie aucune prise de terre locale : leur
  conducteur N reste connecté au neutre amont par les tronçons.
- Matrices primitives série 4×4, avec ordre des conducteurs explicite. Elles
  peuvent être dissymétriques entre phases et posséder des couplages mutuels,
  sous réserve des contrôles de réciprocité et de passivité du modèle.
- Interrupteurs fermés ABCN idéaux ; les interrupteurs ouverts sont exclus.
- Charges et productions P/Q constantes avec connexion détaillée phase-neutre,
  entre phases, étoile triphasée avec neutre ou triangle triphasé.
- Réseau en service connecté, sans boucle ni branche parallèle fermée.

La présence de matrices est obligatoire : le moteur ne reconstruit pas une
impédance de neutre depuis les seuls R/X du modèle historique.
Les phases manquantes, les tronçons monophasés, prises de terre aval multiples,
impédances de terre non nulles à la source, sources multiples, points étoile
flottants, shunts non nuls et transformateurs ne sont pas encore pris en charge.
Ils sont refusés explicitement. Les objets hors service et les branchements
à des bus hors service sont exclus du calcul, comme pour le moteur équilibré.

Pour étudier un départ BT, placer la frontière amont à l'équivalent source du
secondaire du poste. Les impédances de séquence de la source et les contributions
au défaut sont conservées comme données mais ne sont pas utilisées ici.
Les harmoniques, courants de fuite et chemins de retour par le sol ne sont pas
calculés. Ce premier moteur ne remplace pas encore une étude complète HTA/BT
avec transformateurs et mises à la terre distribuées.

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

Le moteur utilise un balayage arrière/avant par sommation des quatre courants :

1. Calculer les courants des injections d'après leurs tensions aux bornes.
2. Sommer les courants de chaque conducteur depuis les feuilles vers la source.
3. Recalculer les quatre tensions vers l'aval par `Vaval = Vamont − Z4×4 × I4`.
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

## Résultats et traçabilité

Par bus : phaseurs A/B/C/N par rapport à la terre source, tensions et angles
A-N/B-N/C-N, tensions composées AB/BC/CA, potentiel du neutre et rapport de
séquence inverse/directe U2/U1. Ce dernier est calculé sur les tensions
phase-neutre ; ce n'est pas l'écart maximal entre trois modules de tension.

Par branche : phaseurs des quatre courants, flux P/Q amont et aval, pertes
actives et réactives. Les orientations sont de la source vers l'aval,
indépendamment de l'ordre `from/to` stocké dans l'objet.
La puissance d'une extrémité est `sum(Vk × conj(Ik))`, neutre inclus.
Les pertes sont calculées par `sum((Z×I)k × conj(Ik))` : elles incluent la
résistance du neutre et les couplages, sans les réduire à trois pertes de phase.

Le taux de charge est le maximum des quatre modules de courant, rapporté à
`max_current_a`. Cette limite unique est appliquée aux quatre conducteurs dans
ce jalon. Des limites distinctes selon les conducteurs et technologies devront
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
