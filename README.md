<p align="center">
  <img src="branding/logo-copyright.svg" width="120" alt="Naute ©">
</p>

<h1 align="center">Naute</h1>

<p align="center">Notes, tableurs, designs et présentations — avec une dictée qui tourne entièrement sur l’appareil.<br>
Windows · macOS · Linux · Android</p>

---

## Ce que fait Naute

| | |
|---|---|
| **Notes** | Texte enrichi : titres, gras/italique/souligné, listes, listes à cocher, citations, tableaux, images, séparateurs. |
| **Tableurs** | Grille avec formules (`=SUM`, `AVERAGE`, `MIN`, `MAX`, `IF`, `ROUND`, `COUNT`, `CONCAT`…, séparateur `,` ou `;`), références `A1:B5`, somme automatique, gras, alignement, couleurs, colonnes redimensionnables. |
| **Designs** | Une toile **de n’importe quelle dimension** (A4, A3, Letter, carte de visite, post carré, story, bannière, affiche… ou taille libre en px, mm, cm, in). Texte, rectangles, ellipses, lignes, images, tableaux ; déplacer, redimensionner, pivoter, opacité, calques, verrouillage, guides magnétiques, annuler/rétablir, plusieurs pages. |
| **Présentations** | Les mêmes outils en diapositives (16:9, 4:3, carré, vertical), notes de l’orateur, mode présentation plein écran. |
| **Dictée locale** | Bouton micro dans chaque éditeur. La parole est transcrite **sur l’appareil**, rien n’est envoyé. |
| **Export & partage** | PDF, PNG, SVG, PowerPoint (`.pptx`, texte éditable), Excel (`.xlsx`, formules conservées), CSV, Word (`.docx`), Markdown, HTML, texte, et `.naute` (copie modifiable pour un autre utilisateur). L’enregistrement passe par le sélecteur du système : dossier, cloud (Drive, iCloud, OneDrive…) ou autre application. Import de `.naute`, `.md`, `.txt` et `.csv`. |
| **33 langues** | Les 24 langues officielles de l’UE, plus norvégien, islandais, catalan, ukrainien, russe, serbe, albanais, macédonien — et chinois simplifié. |

## Dictée : les modèles de Handy

Naute utilise le même moteur que [Handy](https://github.com/cjpais/Handy) — **transcribe.cpp** — et les mêmes fichiers GGUF publiés par Handy sur Hugging Face (téléchargés une fois, vérifiés par SHA-256) :

| Modèle | Taille | Langues |
|---|---|---|
| Parakeet V3 | 549 Mo | 25 langues européennes |
| Parakeet V2 | 539 Mo | anglais |
| Whisper Base | 85 Mo | ~99 langues |
| Whisper Small | 194 Mo | ~99 langues |
| Whisper Medium | 583 Mo | ~99 langues |
| Whisper Turbo | 620 Mo | ~99 langues |
| Whisper Large | 1,16 Go | ~99 langues |
| Canary 180M Flash | 218 Mo | anglais, allemand, espagnol, français |
| Moonshine Base | 77 Mo | anglais |
| SenseVoice | 253 Mo | chinois, cantonais, anglais, japonais, coréen |
| Qwen3-ASR 0.6B | 645 Mo | 30 langues dont le chinois |

Les quantifications (Q5_K_M / Q8_0) sont choisies pour rester confortables sur téléphone. Sur Mac Apple Silicon, le moteur utilise Metal.

## Architecture — 100 % Rust

```
core/        naute-core   Modèle de documents, moteur de formules, mise en page du texte,
                          rendu SVG des pages (partagé par l’interface et les exports)
ui/          naute-ui     Interface en Rust (Leptos) compilée en WebAssembly
src-tauri/   naute        Application Tauri 2 : stockage, exports (resvg, svg2pdf,
                          rust_xlsxwriter, docx-rs, PPTX), dictée (transcribe.cpp)
branding/                 Logo avec ©, icône source
```

L’interface mesure le texte avec les mêmes polices que l’exporteur (IBM Plex Sans, Literata, IBM Plex Mono, embarquées), si bien que les retours à la ligne sont identiques à l’écran et dans les fichiers. Les icônes sont les [Material Symbols](https://fonts.google.com/icons) (Rounded), embarquées pour fonctionner hors ligne.

## Télécharger

Chaque push déclenche **GitHub Actions** (`.github/workflows/build.yml`) qui compile :

- **Windows** : installateur `.exe` (NSIS) et `.msi`
- **macOS** : `.dmg` Apple Silicon et Intel
- **Linux** : `.AppImage`, `.deb`, `.rpm`
- **Android** : `.apk` (arm64)

Les fichiers sont dans l’onglet **Actions → le run → Artifacts**. Pousser un tag `v*` (par ex. `v0.1.0`) crée en plus une *release* brouillon avec tous les binaires.

> Les binaires ne sont pas signés : Windows SmartScreen et macOS Gatekeeper afficheront un avertissement (macOS : clic droit → Ouvrir). Pour l’APK, ajoutez les secrets `ANDROID_KEYSTORE_B64` et `ANDROID_KEYSTORE_PASSWORD` (alias `naute`) pour signer avec une clé stable ; sans eux la CI signe avec une clé jetable, suffisante pour tester.

## Compiler soi-même

Prérequis : Rust stable, la cible `wasm32-unknown-unknown`, [`trunk`](https://trunkrs.dev), CMake, et les [prérequis Tauri](https://tauri.app/start/prerequisites/) de votre système.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
npx @tauri-apps/cli@2 dev      # développement
npx @tauri-apps/cli@2 build    # installateurs
```

Android : `npx @tauri-apps/cli@2 android init` puis `android build --apk` (voir le workflow pour la configuration du NDK et la permission micro).

Tests : `cargo test -p naute-core`, `cargo test -p naute --no-default-features`, `python3 scripts/check_locales.py`.

## Licence

Code sous licence MIT (voir `LICENSE`). Le nom et le logo Naute sont © 2026 Kurodaki, tous droits réservés.
Polices : IBM Plex et Literata (SIL Open Font License), Material Symbols (Apache 2.0). Modèles de reconnaissance vocale : voir leurs licences respectives sur Hugging Face.
