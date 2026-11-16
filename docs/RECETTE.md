# Grille de recette RadarVector (J1 à J5)

État au 15 novembre 2026. « Auto » = vérifié par un test automatisé ; « Headless » = vérifié par
les tests d'UI `egui_kittest` avec rendu wgpu (captures dans `target/visual/`) ; « À faire » =
nécessite une vérification manuelle à l'écran (session macOS verrouillée pendant le développement).

## Critères de jalon

| Jalon | Critère | Résultat |
| --- | --- | --- |
| J1 | 100 % des lignes de `VATSpy.dat` acceptées | ✅ 19 888 / 19 888 (v2611.2), `radarvector data validate` |
| J1 | Taux de résolution des callsigns sur 50 instantanés réels | ✅ voir section « Corpus » |
| J2 | NF-03 : 60 i/s, 3 000 avions, toutes les FIR | ✅ headless 1600×900 : p95 ≈ 1,9 ms (3 000 avions), ≈ 2,1 ms (5 000) |
| J2 | Captures de référence, dont l'antiméridien | ✅ 4 vues (`crates/radarvector-app/tests/visual/*.png`), tolérance 2/255 |
| J3 | Toutes les exigences P0 cochables | ✅ tableau ci-dessous (interactions souris à confirmer à l'écran) |
| J4 | Installation sur les 3 OS, mise à jour N→N+1 signée, refus d'une signature invalide | ◐ `.app`/`.dmg` macOS produits localement ; signature cargo-packager vérifiée et paquet altéré refusé ; Windows/Linux à valider en CI |
| J5 | Exigences P1 cochées | ✅ tableau ci-dessous |

## Exigences P0

| ID | Vérification |
| --- | --- |
| MAP-01 | Zoom vers le curseur : test `roundtrip_and_zoom_anchor` (Auto). Glisser/molette : À faire |
| MAP-02 | Styles actif/inactif, UIR, option « masquer les non dotés » : captures + `builds_activity` (Auto) |
| MAP-03 | Sous-secteurs (ex. `LON_DK_CTR` → `EGTT-D`, masqués inactifs) : tests `enroute`, `subsectors` (Auto) |
| MAP-04 | Libellé FIR à `label_lat/lon` : capture `ui_main` (Headless) |
| MAP-05 | Symboles DEL/GND/TWR/APP/ATIS + cercle d'approche ou TRACON : captures `ui_airport` (Headless) |
| MAP-06 | Icône orientée, étiquettes, désencombrement : captures `ui_airport`, `ui_pilot` (Headless) |
| MAP-07 | Infobulle au survol : À faire ; clic → fiche (onglet d'aperçu réutilisé) : `ui_smoke` (Headless) |
| MAP-08 | Antiméridien, IDL, bouclage : capture `antimeridian_nzzo`, tests `wraps`, `antimeridian_edges_skipped` (Auto) |
| MAP-09 | Distance NM + cap vrai : tests geo (Auto) ; tracé à la souris : À faire |
| MAP-10 | Signets nommés + Ctrl+Alt+0..9 / Ctrl+0..9 (Cmd sur macOS) : persistance testée (`toml_roundtrip`) ; raccourcis : À faire |
| MAP-11 | Couleurs, police, infobulles, inversion des boutons : capture `ui_settings` (Headless) |
| LST-01 | Liste des contrôleurs (ATC uniquement) : capture `ui_main` (Headless) |
| LST-02 | Liste des vols, type sans codes d'équipement : `ui_flights`, test `types` (Auto) |
| DET-01 | Fiche FIR (contrôleurs, aéroports, trafic dans la zone) : code + `point_in_multipolygon` (Auto) ; à l'écran : À faire |
| DET-02 | Fiche aéroport (contrôleurs, ATIS, départs/arrivées, METAR) : `ui_airport` (Headless) |
| DET-03 | Fiche vol (plan complet, position, transpondeur) : `ui_pilot` (Headless) |
| DET-04 | Fiche contrôleur : à l'écran : À faire |
| FLT-01 | Filtres nommés, jokers : tests `wildcards`, `filter` (Auto) |
| FLT-02 | Onglet de résultats de filtre : À faire |
| FAV-01 | Favoris avec confirmation, centrage : persistance testée ; à l'écran : À faire |
| JMP-01 | Boîte de saut OACI / FIR / préfixe pays + suggestions : test `jump`, `ui_smoke` (Auto/Headless) |
| REF-01..04 | Bouton désactivé pendant le rafraîchissement, Arrêter (annulation), auto 15 s min / 60 s, retour de minimisation, bandeau d'erreur + lien statut : code + `sequence` (backoff) ; à l'écran : À faire |
| WX-01 | METAR multi-stations : client testé en live (fiche KMIA) ; onglet METAR : À faire |
| CFG-01 | Fenêtre unique, restaurer les défauts, sauvegarde auto : `restore_keeps_user_content`, `ui_settings` |
| UPD-01 | Mise à jour des données depuis GitHub : `radarvector data update` (live, v2611.2 installée) |
| UPD-02 | Dossiers utilisateur de l'OS : `~/Library/Application Support/RadarVector` vérifié |

## Exigences P1 (J5) et P2 livrées

| ID | Vérification |
| --- | --- |
| MAP-12 | Thèmes clair et sombre : `ui_fr_light_search` (Headless) |
| MAP-13 | TRACON SimAware : 90 % des postes APP/DEP du corpus dessinés en polygone ; tests `approach_uses_tracon_prefixes` |
| MAP-14 | Tracé orthodromique + trace : `ui_pilot` (Headless) |
| LST-03, LST-04 | Serveurs ; tri, filtre texte, colonnes : code ; à l'écran : À faire |
| LST-05 (P2) | Onglet des plans déposés |
| FLT-03 | Couleur de surbrillance par filtre |
| JMP-02 | Recherche unifiée Ctrl+K : `unified`, `ui_fr_light_search` |
| ALR-01 | Notifications : `followed_and_favorites` (Auto) ; affichage système : À faire |
| UPD-03 | Mise à jour signée de l'application : `signature_roundtrip_and_tamper`, `packager_signature_verifies` |
| I18N-01 | Français / anglais : `switch`, `ui_fr_light_search` |

## Exigences non fonctionnelles mesurées

| ID | Mesure |
| --- | --- |
| NF-02 | Géométrie préparée en 31 ms ; données statiques chargées en 148 ms (debug) |
| NF-03 | Voir J2 |
| NF-05 | Binaire 24,7 Mo ; RSS 174 Mo ; CPU au repos 0 % (fenêtre non affichée : à remesurer à l'écran) |
| NF-06 | proptest sur les parseurs ; cibles `cargo fuzz` dans `fuzz/` |
| NF-10 | CI 3 OS : fmt, clippy `-D warnings`, tests, `cargo deny` |

## Reste à faire hors code

- Vérification manuelle à l'écran des lignes « À faire » (souris, raccourcis, notifications).
- Secrets de signature Apple / Windows et clé minisign de mise à jour (voir `RELEASING.md`).
- Nom de dépôt GitHub définitif (champ `repository` du `Cargo.toml`).
- Politique VATSIM sur l'affichage des noms : le flux v3 les fournit ; la décision reste ouverte.

## Corpus

Instantanés du flux v3 enregistrés toutes les 60 s le 15 novembre 2026, noms retirés
(`radarvector snapshot record --anonymize`), dossier `corpus/` (non versionné).
    50 snapshot(s): 11297 ATC stations, 0 unresolved, resolution rate 100.00%
    APP/DEP stations: 1393, drawn with a TRACON polygon: 1244 (89.3%), circle otherwise
