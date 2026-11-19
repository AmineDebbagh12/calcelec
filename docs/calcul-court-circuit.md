# Premier moteur de court-circuit — IccBmin et 3I0

Moteur : `radial-sequence-fault-0.1`, dans `src/short_circuit.rs`.

Ce premier sous-ensemble calcule, à chaque jeu de barres d'un réseau radial :

- le courant minimal de défaut biphasé `IccBmin` ;
- le courant résiduel d'un défaut monophasé-terre, égal à `3I0`, lorsque le
  chemin homopolaire est représentable ;
- les impédances de Thévenin directe, inverse et homopolaire utilisées ;
- les minima/maxima du réseau et les marges par rapport aux exigences de
  protection configurées.

Il prépare une implémentation IEC 60909 mais ne revendique pas encore la
conformité complète à l'IEC 60909-0:2026. Il manque notamment les corrections
normatives d'impédance, les contributions des machines et convertisseurs, les
défauts triphasés, le courant de choc, les effets thermiques et plusieurs cas
min/max. La norme 2026 couvre les courts-circuits équilibrés et déséquilibrés
des réseaux triphasés BT et HT à 50 ou 60 Hz ; son domaine public est décrit par
la [fiche IEC officielle](https://webstore.iec.ch/en/publication/68454).

## Formules du jalon 0.1

Avec `Un` la tension nominale composée, `cmin` le facteur de tension minimal,
`Zf` l'impédance du défaut et les impédances de Thévenin au point étudié :

```text
IccBmin = cmin × Un / |Z1 + Z2 + Zf|
3I0     = sqrt(3) × cmin × Un / |Z1 + Z2 + Z0 + 3 Zf|
```

La valeur par défaut de `cmin` est 0,95. Elle reste une option explicite et ne
doit pas être interprétée comme une valeur universelle Enedis ou IEC. Les
impédances de source fournies doivent correspondre au cas minimal que l'étude
cherche à représenter.

Les matrices de lignes ABC transposées sont converties en séquences :
`Z1 = Zpropre − Zmutuelle` et `Z0 = Zpropre + 2 Zmutuelle`. Les transformateurs
Dyn/Yzn propagent Z1/Z2 selon leur rapport et ouvrent un nouveau réseau
homopolaire sur leur secondaire mis à la terre. Leur Z0 déclarée est ramenée au
secondaire et complétée par trois fois l'impédance de mise à la terre du point
étoile.

Pour une ligne ABCN, Z1/Z2 restent calculables et donc `IccBmin` est publié. En
revanche `3I0` est marqué non évalué : réduire silencieusement le conducteur N,
les électrodes et la terre donnerait un courant résiduel trompeur. L'intégration
du solveur quadrifilaire et des admittances shunt homopolaires constitue la
prochaine extension importante.

## Exigences de protection

Une exigence peut définir :

- `minimum_icc_b_a` : `IccBmin` doit être supérieur ou égal à ce seuil ;
- un seuil résiduel `minimum_detection` : `3I0` doit atteindre le seuil de prise
  en compte de la protection ;
- un seuil résiduel `maximum_admissible` : `3I0` doit rester inférieur ou égal
  au plafond retenu dans l'étude.

Cette distinction évite de confondre le seuil de détection d'un défaut avec une
limite maximale de courant résiduel. La marge publiée est toujours positive
lorsque le critère sélectionné est satisfait. Si `3I0` n'est pas calculable,
le contrôle reste `null` et n'est jamais déclaré conforme.

## Utilisation

```sh
cargo run -- --short-circuit examples/reference-rural-unbalanced.json
cargo run -- --short-circuit examples/reference-rural-unbalanced.json examples/fault-study-rural.json
```

Le fichier de démonstration utilise des seuils pédagogiques non issus d'une
doctrine Enedis. Sur le cas rural courant, le minimum global `IccBmin` vaut
environ 2 466,86 A au troisième bus HTA. À cet emplacement, `3I0` vaut environ
2 235,63 A. Avec les seuils de démonstration 2 000 A et 2 500 A, les marges sont
respectivement +466,86 A et +264,37 A.

Le cas analytique à une source vérifie directement les deux formules. Les tests
du réseau rural contrôlent l'accumulation des impédances de ligne, la séparation
homopolaire des Dyn et le refus explicite des mailles dans cette première version.

## Limites immédiates

- réseau radial connecté, source active unique ;
- lignes triphasées transposées et shunts absents ;
- transformateurs orientés de l'amont vers l'aval ;
- contributions des producteurs et charges négligées ;
- pas encore de courant capacitif homopolaire, donc pas de calcul représentatif
  des réseaux HTA à neutre compensé ou impédant avec départs capacitifs ;
- pas de coordination temps-courant ni de réglage automatique des protections.

Ces limites sont portées dans chaque résultat afin qu'une valeur 0.1 ne puisse
pas être confondue avec une étude de protection finalisée.

## Icctri G — jalon ultérieur

La documentation technique Enedis emploie `Icctri G` dans deux contrôles du plan
de protection. Pour la protection générale C 13-100, le défaut triphasé est au
Point de Livraison et le courant remonte vers cette protection. Pour la
protection du départ producteur — le cas visé ici — le défaut est en tête de
câble d'un **autre départ** du même poste source et le courant remonte au niveau
de la protection du départ producteur. Le producteur étudié et les autres
producteurs couplés sur son départ contribuent au défaut. Il ne s'agit donc pas,
en général, du courant d'un producteur placé sur l'autre départ en défaut.

Source publique : [Enedis-NMO-RES_039E — impact d'une production décentralisée
HTA sur le plan de protection](https://www.enedis.fr/media/2161/download).

Ce calcul est placé après la V1. Il demande auparavant : un poste source avec
plusieurs départs explicites, des protections de tête orientées, un modèle de
contribution de court-circuit des producteurs HTA et BT, les schémas normal et
secourant, ainsi que le calcul triphasé. Le futur jalon couvrira aussi `Iccbi G`
et les marges de réglage associées afin de ne pas isoler Icctri G de son usage
dans le plan de protection.
