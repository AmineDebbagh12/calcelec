# Résultats persistants et rapports — V0.21

Un résultat enregistré est une enveloppe immuable contenant : identifiant, nom
d'étude, projet, version exacte du réseau, type d'étude, version du moteur et
charge utile JSON complète. Les types actuels sont régime permanent équilibré,
régime permanent déséquilibré et vérification multi-situations.

L'enregistrement se fait depuis la vue résultats, dans la base SQLite indiquée
par l'interface. Enregistrer le même identifiant et le même contenu est sans
effet ; un contenu différent avec le même identifiant est refusé. Les résultats
d'une ancienne version restent conservés. Seul un screening lié à la version
courante peut être restauré dans l'interface.

La vérification multi-situations propose trois restitutions :

- JSON : contrat complet destiné à la réouverture et aux échanges logiciels ;
- CSV UTF-8 séparé par des points-virgules : situations, contraintes et cas
  dimensionnants, avec identifiants et noms d'ouvrages ;
- Markdown : synthèse lisible du verdict, des seuils, des scénarios, des cas
  dimensionnants et du volet IccBmin/3I0 lorsqu'il est présent.

Les exports refusent d'écraser un fichier existant. Le rapport Markdown constitue
un premier gabarit technique ; la mise en page métier, les traductions et un
format final imprimable restent à réaliser avant la V1.
