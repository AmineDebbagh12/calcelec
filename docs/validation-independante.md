# Validation indépendante avec pandapower

Le réseau rural radial est rejoué avec le moteur triphasé `runpp_3ph` de
pandapower 3.3.0. Cette comparaison est volontairement séparée de `cargo test` :
elle vérifie CalcElec avec une implémentation externe sans imposer Python et
pandapower aux utilisateurs de l'application.

## Exécution

Depuis la racine du dépôt, après avoir installé Python et créé un environnement
virtuel dédié :

```sh
python -m venv .venv-validation
. .venv-validation/bin/activate
python -m pip install -r validation/requirements.txt
cargo build
python validation/validate_rural_pandapower.py
```

Sous Windows, l'activation se fait avec
`.venv-validation\Scripts\activate`. Le script retourne 0 si toutes les
tolérances sont respectées, 1 en cas d'écart numérique et 2 si l'outil ou les
données ne permettent pas la comparaison. Il produit un rapport JSON sur la
sortie standard ; `--output rapport.json` permet de l'enregistrer.

## Correspondance des modèles

- La source est rendue pratiquement idéale dans pandapower, car CalcElec impose
  encore sa tension en régime permanent ; ses impédances internes sont désormais
  utilisées par le moteur radial IccBmin/3I0.
- Les matrices ABC symétriques sont converties en impédances directe et
  homopolaire. Les shunts sont nuls dans ce cas.
- Les Dyn11 reprennent tension assignée, uk, pertes cuivre, angle et impédance
  homopolaire du projet.
- Pandapower ne représente pas ici le conducteur N métallique avec les mêmes
  électrodes que CalcElec. La matrice ABCN du câble est donc projetée par
  réduction de Kron vers un modèle triphasé avec retour de terre. Les écarts du
  départ BT sont isolés dans une tolérance distincte ; ils ne doivent pas être
  interprétés comme une validation du partage neutre/terre propre à CalcElec.

Le solveur `runpp_3ph` de pandapower travaille en composantes de séquence et
considère le neutre des charges phase-terre confondu avec la terre. Sa
[documentation officielle](https://pandapower.readthedocs.io/en/v3.3.0/powerflow/ac_3ph.html)
précise également son domaine pour les transformateurs mis à la terre.

## Résultats de référence

Comparaison exécutée avec `meshed-four-wire-0.7` et pandapower 3.3.0 :

| Grandeur | Écart maximal | Tolérance |
| --- | ---: | ---: |
| Tension des bus HTA | 0,000000164 pu | 0,00005 pu |
| Tension des secondaires sans câble ABCN | 0,000000214 pu | 0,0003 pu |
| Tension dans la projection du départ ABCN | 0,006284 pu | 0,007 pu |
| Angle de phase, tous bus | 0,4052° | 0,5° |
| Puissance active source | 25,216 W | 30 W |
| Puissance réactive source | 1,577 var | 10 var |
| Pertes actives des lignes | 22,063 W | 30 W |
| Pertes actives des transformateurs | 0,366 W | 2 W |

Les écarts HTA et ceux des transformateurs sont très inférieurs aux seuils. Le
plus grand écart se situe, comme attendu, à l'extrémité du départ BT : CalcElec
y résout le conducteur N, la terre terminale de 1 ohm et leur partage de courant,
tandis que la projection pandapower les réduit à un retour de terre équivalent.

Cette comparaison complète les solutions analytiques et les bilans internes ;
elle ne valide pas encore les prises de terre multiples, un neutre source
impédant, le groupe Yzn ou les réseaux maillés dans un second moteur externe.
