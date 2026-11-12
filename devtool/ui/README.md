# Devtools Rust/WASM UI

De interface behoudt de terminalwerkplaats: persistente command- en instellingenbalken, echte Bash-terminal, rechts live telemetry en een bestandenpaneel. Rust bestuurt de interface en verwerkt API-antwoorden. De browser laadt `devtools_ui_bg.wasm`; de native CLI’s en Bash draaien via de localhostserver.

| Bestand | Verantwoordelijkheid |
| --- | --- |
| `src/app.rs` | Browserstart, sessie, API, invoerwachtrij, polling en events |
| `src/render.rs` | Settings, processen, executables, tijdlijn, bestandsboom en commandcatalogus |
| `src/model.rs` | Commandopbouw, padweergave, letterlijke tekst en formattering |
| `src/shell.html` | In Rust ingebedde interfaceopbouw |
| `../demo/terminal.js` | xterm-renderer, terminalbytes, focus en afmetingen |
| `build.py` | Compilatie, browserbindings, optimalisatie, validatie en bouwmanifest |

## Lokaal bouwen

Benodigd: Rust/Cargo met `wasm32-unknown-unknown`, `wasm-bindgen-cli 0.2.130`, Binaryen, WABT of wasm-tools, Python 3.11+, Node en npm. De CLI-versie van wasm-bindgen is gelijk aan de Rust-dependency. De build installeert de vastgelegde npm-dependencies wanneer die lokaal ontbreken.

```sh
python devtool/ui/build.py
python devtool/demo/server.py --port 8765
```

Open http://127.0.0.1:8765. De reeds gecompileerde assets staan in `devtool/demo/wasm`; een normale start vereist geen compiler.

## Bouwen met Assembler-compiler-lib

Op Ubuntu 24.04 ARM64:

```sh
python devtool/ui/build.py --toolchain-ref COMMIT_SHA
```

De build haalt de manifests en archiefdelen van Rust, wasm-bindgen, Binaryen en wasm-tools op van die exacte commit, controleert hun SHA-256 en gebruikt de uitgepakte tools. `manifest.json` registreert de compiler, toolchaincommit, toolchainmanifests, bronhashes en uitvoerhashes. Cargo.lock legt de UI-dependencies vast. Dit is een vastgelegde bouwketen; byte-identieke builds over verschillende hosts zijn hiermee nog niet aangetoond.

De workflow `build-ui.yml` wordt door de vierplatformworkflow aangeroepen. Alle platformpakketten ontvangen dezelfde browserassets. `all-dist/wasm-ui` bevat daarnaast de afzonderlijke frontenddistributie.

## Verificatie

```sh
cargo test --locked --manifest-path devtool/ui/Cargo.toml
python -m unittest discover -s devtool/demo/tests -v
python devtool/ui/tests/browser.py --url http://127.0.0.1:8765 --evidence devtool/evidence
```

De browsertest gebruikt Python websockets en een geopende localhostpagina via CDP op poort 9224. Hij controleert WASM-laden, PTY-opdrachten, canonical tree, previews, CLI-tijdlijn, compressieopbouw, catalogus, vijf schermbreedtes, reload en offlineherstel. De Actions-job voert dezelfde test uit met een eigen headless Chromium-browser en native Linux ARM64-executables.
