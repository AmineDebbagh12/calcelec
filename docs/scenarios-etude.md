# Scénarios de charge et de production

Un scénario est un petit fichier JSON indépendant du réseau. Il référence la
version de réseau dans le résultat de calcul, mais ne modifie ni les ouvrages ni
l'historique partagé.

Il contient :

- un identifiant et un nom ;
- un type `maximum_consumption`, `maximum_generation` ou `custom` ;
- un coefficient global pour les charges et un pour les productions ;
- une table optionnelle d'exceptions indexée par identifiant d'injection ;
- une table optionnelle et clairsemée d'états d'exploitation par ouvrage.

Une exception peut imposer P, Q et/ou l'état en service. Elle est appliquée après
le coefficient global. Les valeurs absentes héritent donc du réseau mis à
l'échelle. Seules les charges et productions peuvent être ciblées.

Un état d'exploitation peut modifier `in_service`, `switch_closed` pour un
interrupteur, et `tap_position` pour un transformateur dont le régleur est
explicitement décrit. Le type d'ouvrage et les bornes de prise sont validés
avant toute application. Le réseau versionné reste inchangé : le solveur reçoit
une copie d'étude. Le fichier
`examples/scenario-transformer-tap.json` fournit un exemple compact.

Depuis la V0.18, un état de source peut en plus surcharger `voltage_pu` et les
impédances de séquence `positive_sequence_ohm`, `negative_sequence_ohm` et
`zero_sequence_ohm`. Les valeurs sont validées en ohms et appliquées à la copie
d'étude avant le calcul. La tension en pu est utilisée en régime permanent ; les
impédances sont utilisées par le moteur Icc, dont `voltage_factor_min` reste une
option propre. Le fichier
`examples/scenario-minimum-short-circuit.json` illustre une hypothèse amont plus
faible produisant des courants de défaut minimaux plus bas.

```sh
cargo run -- --scenario-short-circuit \
  examples/reference-rural-unbalanced.json \
  examples/scenario-minimum-short-circuit.json \
  examples/fault-study-rural.json
```

```json
{
  "id": "10000000-0000-4000-8000-000000000001",
  "name": "Consommation maximale — démonstration",
  "kind": "maximum_consumption",
  "load_scale": 1.2,
  "generation_scale": 0.0
}
```

Les deux scénarios fournis sont pédagogiques : leurs coefficients ne constituent
pas des profils Enedis. Leur intérêt est de valider le format et la traçabilité.

```sh
cargo run -- --scenario-unbalanced-load-flow \
  examples/reference-rural-unbalanced.json \
  examples/scenario-maximum-consumption.json

cargo run -- --scenario-unbalanced-load-flow \
  examples/reference-rural-unbalanced.json \
  examples/scenario-maximum-generation.json
```

Sur le cas rural, le premier scénario donne une demande nette de 252 kW et le
second 45 kW. Le résultat exporte l'intégralité du scénario, l'identifiant du
projet et la version exacte du réseau.

Le même format fonctionne avec le moteur équilibré via `--scenario-load-flow`.

## Interface graphique

Le panneau **Scénarios d'étude** propose le réseau de base et la collection de
la session. Il permet de créer, dupliquer et supprimer un scénario, de choisir
son type, de modifier ses coefficients et d'activer une exception pour chaque
charge ou production. Les exceptions éditent P, Q et l'état en service sans
modifier l'ouvrage de référence.

Le scénario sélectionné est appliqué par le worker de calcul équilibré ou
déséquilibré. Une modification du scénario annule le calcul en cours et invalide
le résultat précédent. La traçabilité affichée et le JSON exporté contiennent le
scénario complet, les options et la version du moteur. Les fichiers scénario
peuvent être importés et exportés depuis ce même panneau.

## Collection et persistance

La collection complète possède un schéma propre, l'identifiant du projet et la
version exacte du réseau. Elle peut être importée ou exportée dans un seul JSON.
Une collection visant un autre projet ou une autre version est refusée, tout
comme deux scénarios réutilisant le même identifiant.

Lors de **Enregistrer en base**, SQLite sauvegarde atomiquement l'historique du
réseau et la collection dans une table dédiée. **Relire ce projet** et le
chargement depuis la liste restaurent les deux. Les anciennes bases sont mises
à niveau automatiquement par création de la table ; en l'absence de collection,
le chargement retourne une liste vide.

## Vérification de toutes les situations

Le dimensionnement ne repose pas sur une comparaison deux à deux. La commande
**Vérifier toutes les situations** calcule chaque scénario, avec le réseau de
base en option, puis applique les mêmes seuils à chaque résultat : tension
minimale, tension maximale et taux de charge maximal. Le rapport contient :

- un verdict propre à chaque situation et la liste de ses contraintes ;
- un verdict global, conforme uniquement si toutes les situations sont calculées
  et conformes ;
- une enveloppe par ouvrage et par grandeur, avec la valeur la plus défavorable
  et le scénario qui l'a produite ;
- la version du projet, du réseau et du moteur pour assurer la traçabilité.

L'état `unable` est distinct d'une contrainte : une situation invalide ou non
convergente empêche de démontrer la conformité du réseau, sans masquer les
résultats obtenus pour les autres situations. Le rapport JSON peut être partagé
indépendamment du projet.

En mode déséquilibré, l'enveloppe publie aussi U2/U1 et le déplacement du neutre.
Ces grandeurs deviennent contraignantes seulement lorsqu'un seuil est configuré
par le modèle appelant ; l'interface expose actuellement les seuils de tension
et de charge.

Depuis la V0.17, l'interface peut joindre au verdict l'étude IccBmin/3I0 et ses
marges de protection. Les paramètres sont chargés depuis le JSON d'options du
moteur de défaut. Le réseau de base couvre les scénarios P/Q purs, puisque le
moteur néglige encore charges et contributions des producteurs. Depuis la V0.17.1,
chaque scénario modifiant un interrupteur, l'état de service ou une prise est en
revanche recalculé séparément pour le défaut. Un critère 3I0 demandé mais non
calculable rend le verdict global incomplet, jamais conforme.

Les prochains ajouts porteront sur la persistance des rapports et les exports.
La fusion concurrente de deux collections modifiées séparément reste également
à définir.
