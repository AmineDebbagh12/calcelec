# Catalogue technologique V0.20

Le catalogue appartient au projet autonome. Il ne dépend d'aucune base distante
pour relire une étude historique. Une entrée représente une révision immuable :
une correction crée un nouvel `id`, conserve le même `family_id` et augmente
`revision`.

Deux familles sont prises en charge :

- ligne/câble : pose aérienne ou souterraine, matériau, section, R, X et courant
  admissible ;
- transformateur : tensions assignées, puissance, uk et groupe de couplage.

Une entrée peut aussi embarquer un modèle `electrical` détaillé. Lors de
l'instanciation, les paramètres de la révision sont copiés dans l'ouvrage tandis
que ses raccordements et, pour une ligne, sa longueur sont conservés. Le lien
`technology` assure la traçabilité. Toute divergence ultérieure doit porter
`locally_modified: true`.

## Utilisation dans l'application

1. Déplier **Catalogue technologique** pour importer ou exporter un tableau JSON.
2. Sélectionner une ligne, un câble ou un transformateur.
3. Dans **Paramètres électriques et raccordements**, choisir une technologie.
4. Vérifier les valeurs résolues puis créer une version du réseau.
5. Utiliser **Réappliquer les paramètres du type** pour abandonner une variante
   locale et revenir exactement à la révision choisie.

`examples/catalog-technologies-demo.json` documente le format d'échange. Les
types pédagogiques intégrés 95AL, 150AL, 150CU, 148AA et transformateur 250 kVA
ne sont ni des références officielles ni des valeurs de catalogue Enedis.

## Limites

Le socle garantit l'identité, l'immutabilité, l'autonomie du projet et la mise à
jour explicite. Il ne fournit pas encore de base métier validée, de conditions
thermiques, de variantes de pose, de matrices complètes par conducteur pour les
types pédagogiques ni de synchronisation avec une base externe.
