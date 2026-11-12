# devtool-auto 0.2.0

Een zelfstandige Rust CLI voor opgeslagen automatiseringsprofielen, commandstappen, timers en filesystemwatchers. Eigen Cargo-project, `main.rs`, `lib.rs` en ELF-executable; geen librarykoppeling met devtool. Een profiel kan devtool, Bash en andere beschikbare executables aanroepen.

## Bouwen

```sh
cargo build --release --offline
./target/release/devtool-auto --help
cargo test --release --offline
```

De meegeleverde `dist/devtool-auto` is voor Android/Termux ARM64 gebouwd.

## Eigen automatiseringsregels

De eigen taal en canonical JSON volgen `mode → matcher → action`, met modes before, after, error, timer en change. Zie [RULES.md](RULES.md) voor de volledige grammatica, matchers, CLI-flags en voorbeelden. Importeren: `devtool-auto rule import automation.rules`. Losse commands met regels uitvoeren: `devtool-auto exec -- devtool --canonical-tree`.

## Commands

| Command | Functie en flags |
| --- | --- |
| `profile create NAAM` | Opslaan met `--cwd`, `--repeat`, `--interval-ms`, herhaalde `--env KEY=VALUE` |
| `profile set NAAM` | Dezelfde instellingen wijzigen; `--unset-env KEY` verwijdert een profielvariabele |
| `profile list`, `profile show NAAM` | Opgeslagen profielen of volledige JSON tonen |
| `profile export NAAM -o BESTAND` | Profiel exporteren |
| `profile import BESTAND --name NAAM` | Importeren, optioneel met een andere naam |
| `profile remove NAAM` | Profiel verwijderen |
| `step add NAAM --name STAP` | Executable met `--program` en herhaalde `--arg`, of Bash met `--command`; daarnaast `--cwd`, `--env`, `--delay-ms` |
| `step list NAAM` | Stappen in uitvoervolgorde tonen |
| `step remove NAAM --index NUMMER` | Eén stap verwijderen, nummering vanaf 1 |
| `step clear NAAM` | Alle stappen uit dit profiel verwijderen |
| `run NAAM` | Profiel uitvoeren; tijdelijke overrides `--cwd`, `--repeat`, `--interval-ms` |
| `timer set NAAM` | Timer opslaan met `--after-ms`, `--interval-ms`, `--runs` |
| `timer start NAAM` | Opgeslagen timer uitvoeren; dezelfde drie flags als tijdelijke overrides |
| `timer show NAAM`, `timer clear NAAM` | Timerinstellingen tonen of verwijderen |
| `watch set NAAM` | Watcher opslaan: herhaalde `--path`, `--recursive true/false`, `--poll-ms`, `--debounce-ms`, `--initial true/false`, `--runs` |
| `watch start NAAM` | Opgeslagen watcher uitvoeren; dezelfde flags als tijdelijke overrides |
| `watch show NAAM`, `watch clear NAAM` | Watcherinstellingen tonen of verwijderen |
| `lab init --path PAD` | Fake Linux-boom en profielen lab-build, lab-timer, lab-watch maken |
| `schema` | Profiel-JSON-schema tonen |

Elke command heeft `--help`. Globaal: `--profiles-dir PAD`, `--help`, `--version`. Zonder expliciete opslagmap: `$XDG_CONFIG_HOME/devtool-auto/profiles`, anders `$HOME/.config/devtool-auto/profiles`.

## Profiel met devtool en Bash

```sh
devtool-auto profile create mijn-flow --cwd /pad/naar/werkmap --repeat 1
devtool-auto step add mijn-flow --name hoofdmap --program devtool --arg=-D
devtool-auto step add mijn-flow --name templates --program devtool --arg=--canonical-tree
devtool-auto step add mijn-flow --name lijst --command 'find . -type f | sort > bestanden.txt'
devtool-auto run mijn-flow
```

Bij `--program` blijven argumenten losse argv-elementen, inclusief spaties en flags. Gebruik bijvoorbeeld `--arg=--format --arg=7z`. `--command` gebruikt `bash -c` voor shellsyntaxis, pipes, redirects, variabelen en gewone Linux-commands. Processen erven de terminalinvoer en uitvoer.

Profielen voeren stappen op volgorde uit. Een mislukte stap beëindigt de uitvoering met diens exitcode. De profielwerkmap is absoluut opgeslagen. Een relatieve stapwerkmap wordt vanuit die profielwerkmap bepaald. Shellstappen zijn afzonderlijke processen; een `cd` geldt binnen die shellstap. Profielvariabelen worden door stapvariabelen overschreven. `--repeat` herhaalt het complete profiel; `--delay-ms` wacht vóór een stap, `--interval-ms` tussen complete profielruns.

## Timers

```sh
devtool-auto timer set mijn-flow --after-ms 5000 --interval-ms 60000 --runs 10
devtool-auto timer start mijn-flow
```

De startvertraging geldt eenmaal. Het interval telt vanaf het einde van de vorige activatie. Elke activatie voert het gehele profiel met zijn eigen herhalingsinstellingen uit. `--runs 0` blijft uitvoeren tot onderbreking. `timer start --runs 1` wijzigt de opgeslagen timer niet.

## Watchers

```sh
devtool-auto watch set mijn-flow --path inbox --recursive true --poll-ms 100 --debounce-ms 300 --initial false --runs 0
devtool-auto watch start mijn-flow
```

De watcher vergelijkt pad, type, bestandsgrootte en wijzigingstijd per controle-interval. Toevoegen, wijzigen en verwijderen veroorzaken een activatie na de ingestelde rustige debounceperiode. Meerdere wijzigingen worden samengevoegd. Wijzigingen die volledig tussen twee controles verdwijnen worden niet geobserveerd. Recursie volgt submappen, geen symlinkdoelen. Relatieve watchpaden zijn relatief aan de profielwerkmap. `--initial true` voert het profiel direct eenmaal uit. Het aantal runs telt activaties, inclusief die eerste run.

Timer en watcher draaien in het actieve terminalproces; Ctrl C stopt ze. Startmeldingen en stapnamen gaan naar stderr; uitvoer van commands blijft zichtbaar. Tijdens een profielactivatie voert dezelfde watcher geen tweede activatie parallel uit.

## Demo-lab

```sh
devtool-auto lab init --path linux-fs
devtool-auto profile list
devtool-auto run lab-build
devtool-auto timer start lab-timer
devtool-auto watch start lab-watch --runs 1
```

De fake boom bevat onder meer bin, sbin, etc, home, usr, var, tmp, opt, dev, proc, sys, run, mnt, media en root. Het zijn gewone testmappen binnen de demo-werkmap. Commands gebruiken de beschikbare Termux-programma's. `lab-build` maakt een hoofdmap, schrijft alle 17 canonical files, schrijft een bestandenlijst en maakt zowel 7z als tar.gz. `lab-timer` schrijft drie logregels. `lab-watch` bewaakt `tmp/incoming`; maak daar in een tweede terminal een bestand om de activatie te zien. De log verschijnt in `var/log/watch.log`.

Het lab gebruikt eigen HOME/XDG-paden onder de fake boom voor de aangeroepen devtool-commands. De automatiseringsprofielen zelf staan in de gekozen profielenmap.
