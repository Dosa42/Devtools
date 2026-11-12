# Eigen automatiseringstaal

`devtool-auto` 0.2.0 gebruikt `mode → matcher → action`. De canonical opslag is JSON met `schema: devtool-auto.rules/1`. De leesbare taal gebruikt woorden, regels en inspringing; accolades, komma's, prefixes en aanhalingstekens zijn niet nodig. Inspringing is vrij. Lege regels zijn toegestaan. Eén waarde loopt tot het einde van de regel, zodat een pad met spaties ook werkt.

```text
rule archive-after-tree
  mode after
  matcher command devtool
  matcher flag canonical-tree
  action profile archive
end
```

Een regel heeft een naam, één mode en één actieprofiel. Matchers zijn optioneel voor commandmodes en timers. Alle matchers moeten overeenkomen. Zonder matchers reageert een regel op iedere gebeurtenis van zijn mode. Regels worden uitgevoerd in opgeslagen volgorde.

## Modes

| Mode | Moment |
| --- | --- |
| `before` | Vóór een uitvoerbare commandstap |
| `after` | Na afloop van een command, ook bij een fout |
| `error` | Na `after`, wanneer het command niet slaagt |
| `timer` | Volgens de opgeslagen startvertraging en het interval |
| `change` | Bij waargenomen filesystemwijzigingen |

`run`, bestaande `timer start` en `watch start` activeren commandregels rond hun profielstappen. Voor losse terminalcommands gebruik je:

```sh
devtool-auto exec -- devtool --canonical-tree
devtool-auto exec -- bash -c 'find linux-fs -type f | sort'
```

De tweede uitvoering is één command `bash` met argumenten `-c` en de shelltekst. De afzonderlijke commands binnen die shelltekst worden niet apart geïnstrumenteerd. Rechtstreeks buiten devtool-auto uitgevoerde commands publiceren geen events.

Een vóóractie voert eerst zijn profiel uit; bij een mislukte actie stopt die commanduitvoering. Een commandfout behoudt zijn oorspronkelijke exitstatus, ook wanneer een na- of foutactie een andere status teruggeeft. Na-actiefouten na een geslaagd command geven hun eigen foutstatus terug. Niet-startbare executables genereren `after` en `error` met exitcode 1 en melden de startfout.

Actieprofielen voeren gewone profielstappen uit; ook daarop werken de regels. Een cyclus binnen dezelfde uitvoerengine geeft een fout met de betrokken regelnamen. Profiel- en stapmatchers laten je precies aangeven op welke oorspronkelijke stap een regel betrekking heeft.

## Matchers

| Notatie | Betekenis |
| --- | --- |
| `matcher command devtool` | Executablenaam of volledig executablepad |
| `matcher flag canonical-tree` | Flag, met of zonder voorafgaande streepjes |
| `matcher profile mijn-flow` | Naam van het uitvoerende profiel |
| `matcher step templates` | Naam van de uitvoerende stap |
| `matcher path linux-fs/tmp/incoming` | Gewijzigd pad binnen deze map; bij commandevents de commandwerkmap |
| `matcher exit 7` | Afgesloten command met exitstatus 7 |

Tekstmatchers ondersteunen exact vergelijken en globtekens `*` en `?`. Een flagmatcher vergelijkt bijvoorbeeld zowel `--format` als `--format=7z` met `format`. Flags zijn hoofdlettergevoelig; `D` en `d` blijven verschillend. Gegroepeerde korte flags worden niet opgesplitst. Argumenten na `--` gelden niet als flags.

Letterlijke padmatchers omvatten het pad zelf en de onderliggende boom. `cwd` bepaalt de basis voor relatieve matcherpaden en wordt bij import of `rule set` absoluut vastgelegd. Zonder expliciete `cwd` is dit de werkmap van de importerende command. Er wordt geen pad via bestaande symlinkdoelen omgezet. Voor `change` zijn de padmatchers concrete bewaakte paden; globtekens zijn daar niet van toepassing. Meerdere matchers blijven AND, ook bij meerdere paden; de gewone profielwatcher ondersteunt onafhankelijk meerdere bewaakte paden.

## Timerregel

```text
rule hourly-backup
  mode timer
  interval 1h
  after 5s
  runs 0
  action profile backup
end
```

Duren gebruiken gehele getallen met `ms`, `s`, `m`, `h` of `d`. Zonder eenheid is de waarde milliseconden. `interval` is verplicht en groter dan nul. `after` is standaard 0. `runs 0` blijft actief tot onderbreking. Het interval begint na afloop van de vorige actie. Iedere activatie voert het complete actieprofiel uit, inclusief diens eigen herhalingen.

```sh
devtool-auto rule start hourly-backup
devtool-auto rule start hourly-backup --runs 2
```

Alleen de gestarte timerregel wordt door die timer geactiveerd. Andere timerregels starten via hun eigen `rule start`.

## Changeregel

```text
rule incoming-files
  mode change
  cwd /pad/naar/demo-werkmap
  matcher path linux-fs/tmp/incoming
  recursive true
  poll 100ms
  debounce 300ms
  initial false
  runs 0
  action profile process-incoming
end
```

Minstens één padmatcher is verplicht. Standaard: recursief, poll 250ms, debounce 300ms, geen directe eerste activatie en onbeperkt runs. Polling vergelijkt pad, type, grootte en wijzigingstijd. De actie ontvangt de verzamelde gewijzigde paden, inclusief waargenomen verwijderingen. De bestaande pollingbeperkingen en seriële uitvoering gelden ook hier. `initial true` gebruikt de bewaakte paden voor de eerste gebeurtenis.

Timers en changeregels draaien in de actieve terminal. Ctrl C stopt ze. `rule start` activeert de opgeslagen regel; commandmodes worden door commandevents gestart.

## CLI

```sh
devtool-auto rule check automation.rules
devtool-auto rule import automation.rules
devtool-auto rule list
devtool-auto rule show archive-after-tree
devtool-auto rule export --format json -o automation.json
devtool-auto rule export --format text -o automation.rules
devtool-auto rule schema
```

Import accepteert leesbare tekst en canonical JSON. Gelijknamige regels worden vervangen; overige opgeslagen regels blijven staan. Het hele importbestand wordt vóór opslag gevalideerd. `rule check` controleert taal/schema en instellingen; het voert geen acties uit en vereist geen al bestaande actieprofielen. Een genoemd actieprofiel wordt bij uitvoering geladen.

Via flags kun je dezelfde regel aanmaken of vervangen:

```sh
devtool-auto rule set archive-after-tree --mode after --matcher 'command devtool' --matcher 'flag canonical-tree' --action archive
devtool-auto rule set hourly-backup --mode timer --interval 1h --after 5s --runs 0 --action backup
devtool-auto rule set incoming-files --mode change --matcher 'path linux-fs/tmp/incoming' --recursive true --poll 100ms --debounce 300ms --initial false --action process-incoming
```

Daarnaast: `--cwd`, `--enabled true/false`, `rule enable NAAM`, `rule disable NAAM` en `rule remove NAAM`. `rule set` schrijft de volledige regel met de meegegeven instellingen en de gedocumenteerde defaults. `rule start --runs` is een tijdelijke override.

Matchercontrole zonder uitvoering:

```sh
devtool-auto rule match --mode after --command devtool --arg=--canonical-tree --exit-code 0
```

Een expliciete testgebeurtenis uitvoeren:

```sh
devtool-auto rule emit --mode after --command devtool --arg=--canonical-tree --exit-code 0
```

Eventflags: `--mode`, `--command`, herhaalde `--arg`, `--profile`, `--step`, `--cwd`, herhaalde `--path` en `--exit-code`. De globale `--profiles-dir` kiest ook de regelopslag: `PROFIELENMAP/rules/rules.json`. Profiel- en regelopslag blijven afzonderlijk leesbaar.

## Gegevens voor acties

Acties krijgen `DEVTOOL_AUTO_RULE`, `DEVTOOL_AUTO_MODE`, `DEVTOOL_AUTO_COMMAND`, `DEVTOOL_AUTO_PROFILE`, `DEVTOOL_AUTO_STEP`, `DEVTOOL_AUTO_CWD` en `DEVTOOL_AUTO_EXIT`. Niet-beschikbare velden zijn leeg. `DEVTOOL_AUTO_EVENT` bevat het complete event als JSON met mode, command, args, profile, step, cwd, paths en exit_code. De eigen werkmap van het actieprofiel blijft gelden.
