# Devtools Rust/WASM-werkplaats

Open http://127.0.0.1:8765 op deze telefoon. De server draait lokaal; alle browserassets zijn meegeleverd.

De interface wordt uit Rust gecompileerd naar WebAssembly en gebruikt een echte Bash-PTY. De Rust-broncode staat in `../ui/src/`; `app.js` laadt de module en `terminal.js` koppelt xterm aan WASM. De sessiestarter in `server.py` en `session_runtime.py` stelt vier afzonderlijke executables uit `../dist/` beschikbaar: `devtool`, `devtool-auto`, `devtool-proof` en `devtools-watch`. Elke sessie heeft een eigen werkmap, configuratie en teller onder `workspaces/<sessie>/`. De huidige Rust-functies worden rechtstreeks uitgevoerd.

De starter start eerst Watch en bevestigt het actieve proces via zijn CLI. Daarna opent hij de Unix98 PTY en start Bash. Watch draait los van de controlling terminal; foreground commands en Ctrl C in Bash stoppen Watch niet. Watch gebruikt de configuratie van de sessie. Voor een nieuwe sessie is de werkmap de standaard zoekmap; een bestaande Watch-configuratie blijft van toepassing. `runtime/session.json` registreert het Watch-PID, Bash-PID en de startvolgorde; `/api/state` toont dezelfde gegevens onder `session_runtime`.

Bij het verlaten van Bash of het sluiten van de server stopt de starter de Watch van die sessie. Een browserreload houdt beide processen actief. `devtools-watch --off`, `--on` en `--config` blijven rechtstreeks bruikbaar in Bash. De starter stelt geen CPU-prioriteiten in en herstart een bewust gestopte Watch niet automatisch.

De vaste COMMANDS- en SETTINGS-balken blijven boven de terminal staan. Een commandknop zet tekst op de opdrachtregel; Enter voert uit. Je kunt rechtstreeks in de terminal typen of de invoerbalk gebruiken, ook voor antwoorden in `devtool config menu`. Een map in het bestandenpaneel selecteren vult de bron van het compressieformulier in. Het formulier zet het command klaar. Bestanden openen toont de gemaakte inhoud.

`-D`, `--canonical-tree`, `compress`, het interactieve configuratiemenu, help en alle overige huidige commands gebruiken dezelfde executable. De getoonde settings zijn de standaardconfiguratie van de demosessie. Een expliciet `--config` bij een command werkt volgens de CLI en verandert deze standaardweergave niet.

Herladen behoudt de sessie. Nieuwe sessie opent een eigen werkmap en Bash-terminal; eerdere werkmappen blijven bewaard.

Het rechterpaneel opent op Telemetrie. Het toont het huidige Bash-command, de vier executables, de gemeten sessieprocessen met PID, voorgrond/achtergrond en werkgeheugen, en de commandtijdlijn met duur en Bash-exitcode. De tab Bestanden houdt de bestaande bestandsboom en voorbeelden bereikbaar. Paneel klapt de zijbalk in of uit. Onder 761 CSS-pixels staat het paneel onder de terminal.

`shell_hooks.sh` registreert interactieve Bash-opdrachten via DEBUG en PROMPT_COMMAND in `runtime/commands.bin`; menu-antwoorden blijven invoer van het actieve programma. `telemetry.py` volgt Bash, Watch en hun zichtbare subprocessen via `/proc` op Linux/Android of `ps` op macOS elke 150 ms. De frontend haalt `/api/telemetry` elke 350 ms op. Daardoor verschijnen ook korte commands in de tijdlijn; een kort proces kan tussen procesmetingen vallen. Bash-exitcodes gelden voor de ingevoerde commandregel volgens de gewone shellregels. De laatste 100 commandregels blijven in het API-overzicht; de frontend toont de nieuwste 30. Browserreload behoudt de tijdlijn. De telemetrie schrijft in de sessieruntime, buiten de werkmap.

De browserverificatie voor het meekijkpaneel draait via `python tests/telemetry_browser.py`; resultaten en screenshots staan in `../evidence/telemetry-*`.

Start vanuit deze map:

```sh
python server.py --port 8765
```

De actieve server schrijft zijn PID en URL in `.runtime/server.json`. Stop een voorgrondserver met Ctrl C. Voor opnieuw opbouwen van de meegeleverde browserbundels:

```sh
npm ci --ignore-scripts
npm run vendor
npm test
```

`tests/browser_check.py` is een afzonderlijke controle via de CDP-verbinding op poort 9224 van de geopende demo in Chrome. Hiervoor is Python `websockets` nodig. De controle maakt een eigen demosessie, voert de CLI-functies uit en bewaart screenshots en resultaten in `../evidence/`.

De Rust/WASM-build uitvoeren vanuit de repository:

```sh
python devtool/ui/build.py
```

De actuele browsertest is `python devtool/ui/tests/browser.py --url http://127.0.0.1:8765 --evidence devtool/evidence`. Deze gebruikt een geopende localhostpagina via CDP op poort 9224. De oudere demoscripts blijven als historische controles beschikbaar.
