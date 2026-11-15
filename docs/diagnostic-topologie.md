# Diagnostic de la topologie d'exploitation

Le diagnostic `topology::analyze` décrit l'état électrique actif du réseau sans
lancer de calcul de puissance. Il tient compte des objets `in_service`, exclut
les interrupteurs ouverts et considère les lignes, transformateurs et
interrupteurs fermés comme des branches.

```sh
cargo run -- --topology examples/reference-rural-meshed.json
```

Le rapport JSON contient :

- `radial`, vrai lorsqu'aucune branche active ne ferme de cycle ;
- les sources actives et les bus alimentés ou non alimentés ;
- les interrupteurs ouverts et fermés ;
- les composants connexes, leurs bus, leurs sources et leur état d'alimentation ;
- `cycle_closures`, les branches qui ferment les cycles d'une forêt couvrante
  déterministe.

Une branche listée dans `cycle_closures` n'est pas nécessairement l'ouvrage que
l'exploitant désigne comme point de bouclage : elle dépend de l'arbre couvrant
déterministe. La liste prouve en revanche qu'une maille est effectivement fermée.
Une maille seulement dessinée, mais interrompue par un interrupteur ouvert, reste
radiale dans le rapport.

Le diagnostic accepte plusieurs composants et plusieurs sources afin de pouvoir
signaler leur état. Le moteur déséquilibré 0.7 reste plus restrictif : un unique
composant alimenté par une seule source, et des fermetures ligne/interrupteur dans
une même zone galvanique. Il refuse notamment un îlot non alimenté, une source
multiple et une boucle traversant un transformateur.

Le cas `reference-rural-meshed.json` donne un composant, huit bus alimentés et
une fermeture. Les tests basculent aussi un interrupteur HTA : ouvert, le réseau
est radial ; fermé, une fermeture apparaît ; après ouverture d'une branche de
départ, le diagnostic distingue le composant sans source.
