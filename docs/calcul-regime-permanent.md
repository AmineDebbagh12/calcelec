# Premier moteur de régime permanent

Identifiant : `radial-balanced-0.2`, implémenté dans `src/load_flow.rs`.
Ce jalon constitue une première partie du moteur équilibré du cadrage, sans
couvrir encore les réseaux maillés ni le calcul déséquilibré de la V1.

## Domaine de calcul

- Réseau AC radial connecté, une source active unique imposant sa tension.
- Lignes représentées par leur impédance série R + jX ; pas de shunts.
- Charges P/Q constantes et productions P/Q constantes, triphasées équilibrées.
- Interrupteurs fermés idéaux, interrupteurs ouverts exclus des branches.
- Équipements hors service et raccordements à un jeu de barres hors service
  exclus du calcul. Un jeu de barres restant en service doit être alimenté.
- Fréquence du réseau conservée comme donnée ; les impédances fournies sont
  supposées valables à cette fréquence, sans correction automatique.

Les boucles, branches parallèles fermées, îlots non alimentés et sources multiples
sont refusés. Le moteur ne choisit pas automatiquement une configuration radiale.
Les références aux objets et les paramètres du réseau sont validés avant le calcul.

## Simplifications optionnelles

Les deux options suivantes sont désactivées par défaut et autorisées ensemble
par **Autoriser les modèles simplifiés** dans l'interface, ou `--simplified`
en ligne de commande. L'API Rust permet de les choisir séparément.

| Option | Hypothèse et conséquence |
| --- | --- |
| `allow_balanced_equivalent` | La puissance totale de chaque charge ou production monophasée est répartie sur trois phases ; sa puissance totale reste inchangée, son déséquilibre n'est pas évalué |
| `allow_reactive_transformers` | Impédance purement réactive de module uk, rapport issu des tensions nominales des jeux de barres, prise 1 pu, déphasage nul ; résistance, pertes fer et magnétisation omises |

Les hypothèses effectivement utilisées sont enregistrées dans le résultat.
Le modèle simplifié des transformateurs ne calcule pas leurs pertes actives.
Les courants primaire et secondaire sont distincts et respectent le rapport
des tensions de base. Ce modèle ne représente ni les couplages ni les chemins
homopolaires et ne doit pas servir de modèle de court-circuit.

## Exploitation des données détaillées

Les matrices déclarées peuvent être utilisées lorsqu'elles admettent un
équivalent direct équilibré exact : `Z1 = Zpropre − Zmutuelle`. L'ordre des
conducteurs est explicite ; un neutre déclaré n'est pas éliminé à l'import.
Les transformateurs détaillés utilisent R et X calculés depuis uk et les pertes
cuivre, à rapport nominal, horaire 0 et excitation explicitement nulle.
Les autres configurations détaillées sont refusées même avec `--simplified` ;
le moteur ne remplace pas des données fournies par ses anciennes approximations.
Les nouvelles validations et cas analytiques sont décrits dans le
[modèle électrique détaillé](modele-electrique.md).

## Conventions et algorithme

La méthode choisie est un balayage arrière/avant par sommation des courants,
une famille de méthodes présentée dans la [documentation MATPOWER](https://matpower.org/documentation/ref-manual/legacy/functions/calc_v_i_sum.html).
L'implémentation CalcElec et ses tests sont propres au projet ; aucune comparaison
numérique avec MATPOWER n'a encore été réalisée.

Les puissances sont des totaux triphasés. Les tensions nominales sont composées.
La base de puissance commune est 1 MVA ; chaque jeu de barres utilise sa tension
nominale comme base. Ainsi :

```text
Zbase = Unom² / Sbase
Ibase = Sbase / (sqrt(3) × Unom)
Zligne,pu = (R/km + jX/km) × longueur_km / Zbase
Ztransfo,pu = j × (uk% / 100) × Sbase / Sassignée
```

Les charges sont positives dans la demande nette ; les productions sont
soustraites. L'export vers le réseau amont donne une puissance source négative.
L'angle de la source est nul et les angles calculés sont relatifs à cette source.

1. Construire et orienter l'arbre depuis la source, indépendamment de l'ordre
   des bornes `from` et `to` dans les objets stockés.
2. Initialiser les tensions à celle de la source, en pu.
3. Calculer le courant demandé par chaque nœud : `conj(Sdemande,pu / Vpu)`.
4. Sommer les courants de l'aval vers la source.
5. Recalculer les tensions vers l'aval par `Vaval = Vamont − Zpu × Ipu`.
6. Recalculer les courants et effectuer un balayage de contrôle ; le résidu est
   le maximum du module complexe de l'écart de tension de ce contrôle.

La convergence exige un résidu inférieur ou égal à la tolérance, par défaut
10⁻⁹ pu, dans un maximum de 200 itérations. L'API permet de régler ces valeurs.
Une tension quasi nulle, un résultat non fini, une annulation ou une
non-convergence provoquent un échec explicite, sans résultat partiel publié.

## Grandeurs publiées

- Jeux de barres : tension composée en V et en pu, angle en degrés, alerte
  hors de l'intervalle configuré (par défaut 0,9–1,1 pu, à titre indicatif).
- Branches : courants amont/aval en A, P/Q amont/aval, pertes actives et
  réactives, taux de charge quand une limite est disponible.
- Lignes : charge rapportée au courant admissible renseigné.
- Transformateurs : charge rapportée à la puissance assignée, en retenant
  la plus grande puissance apparente de leurs deux côtés.
- Source : puissance P/Q fournie, demande nette et pertes totales.
- Bilan : module de `Ssource − Sdemande nette − Spertes`, en VA.

Les flux de branche sont orientés de la source vers l'aval. `p_to_w` et
`q_to_var` sont les puissances livrées à l'extrémité aval dans ce même sens,
et non des injections entrantes de signe opposé. Une production aval peut
inverser le signe du flux. Les pertes sont obtenues par `3 × Z × |Iphase|²`,
ou l'expression équivalente en pu.

L'export `StudyResult` inclut `project_id`, `network_version`, `solver_version`,
`options` et `result`. Les identifiants des jeux de barres et branches permettent
de retrouver leurs objets dans le projet correspondant. Le JSON de résultats
ne contient pas une copie autonome de ce projet.

## Intégration et validité des résultats

Le calcul utilise une copie de la version courante, dans un fil d'exécution
distinct de l'interface. L'annulation est vérifiée pendant la construction et
entre les balayages. Une annulation reste prioritaire à la réception d'un
résultat qui aurait fini juste avant sa prise en compte.

Les résultats affichés sont invalidés après changement de version ou de projet.
Un résultat reçu tardivement pour une autre version est écarté. Les changements
de champs non encore validés ne font pas partie du réseau calculé. Changer les
options du prochain calcul ne modifie pas les options consignées dans le résultat
existant ; elles restent consultables dans ses hypothèses et son export.

## Validation du jalon

Les tests automatisés incluent :

- Réseau à deux nœuds résistif : solution analytique de tension, courant et pertes.
- Réseau à deux nœuds R/X et P/Q : solution analytique complexe indépendante.
- Réseau à vide, injection photovoltaïque avec flux inverse et élévation de tension.
- Transformateur simplifié : rapport des courants et consommation réactive.
- Invariance physique lors de l'inversion des bornes stockées d'une ligne.
- Réseau rural : bilan de puissance et cohérence des flux/pertes de chaque branche.
- Boucles, îlots, sources multiples, options invalides, non-convergence,
  limites numériques et annulation.
- Dépassements de seuils, traçabilité de version et invalidation dans l'interface.

Pour l'exemple rural initial et les options simplifiées par défaut : convergence
en 4 itérations, demande nette 192 kW + j46 kvar, puissance source voisine de
192,196 kW + j48,898 kvar, pertes actives des lignes voisines de 196,254 W.
L'erreur de bilan est inférieure à 0,001 VA. Ces chiffres concernent les données
synthétiques et les hypothèses ci-dessus.

La validation indépendante sur jeux de référence externes et les essais natifs
sous Windows restent à réaliser. Les modèles de neutre, transformateurs complets,
réglages, réseaux maillés et courts-circuits constituent les jalons suivants.
