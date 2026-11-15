# Scénarios de charge et de production

Un scénario est un petit fichier JSON indépendant du réseau. Il référence la
version de réseau dans le résultat de calcul, mais ne modifie ni les ouvrages ni
l'historique partagé.

Il contient :

- un identifiant et un nom ;
- un type `maximum_consumption`, `maximum_generation` ou `custom` ;
- un coefficient global pour les charges et un pour les productions ;
- une table optionnelle d'exceptions indexée par identifiant d'injection.

Une exception peut imposer P, Q et/ou l'état en service. Elle est appliquée après
le coefficient global. Les valeurs absentes héritent donc du réseau mis à
l'échelle. Seules les charges et productions peuvent être ciblées.

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

La collection reste locale à la session ; chaque scénario à conserver doit être
exporté avant de quitter. Les prochains ajouts seront sa persistance groupée,
les états de prises/interrupteurs par scénario et la comparaison graphique de
plusieurs résultats.
