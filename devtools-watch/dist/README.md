# devtools-watch 0.1.0

Vierde zelfstandige ARM64 ELF-executable. Een achtergrondmonitor met exact drie bedieningsacties:

```sh
devtools-watch --on
devtools-watch --off
devtools-watch --config
```

`--on` start een los achtergrondproces en geeft direct een JSON-resultaat met PID terug. Nogmaals `--on` verwijst naar hetzelfde proces. `--off` stopt die monitor en wacht op beëindiging. `--config` toont de opgeslagen configuratie, configuratiepaden, versie en processtatus. Er zijn geen aanvullende command-, help- of versionflags.

## Machinebestand

Canonical templateversie2 van devtool vervangt notes.txt door het vooraf ingebedde state.txt. De root behoudt vijf bestanden en vier submappen; totaal17 bestanden. Watch herkent de template via manifest.json met template_version2 en derived_outputs state.txt.

```text
schema_version: 1
canonical_tree: false
sha_present: false
sha_scope: false
sha_checksum: false
sha_signature: false
provenance_present: false
provenance_scope: false
provenance: false
provenance_signature: false
```

De sleutels en volgorde zijn vast. Alle resultaten zijn booleans. Geen tijdstempels, vrije tekst, procesgegevens of gegenereerde beschrijvingen in de rootchecklist. Dezelfde waarneming geeft dezelfde bytes. Na een externe wijziging van state.txt schrijft Watch zijn vaste machinewaarden terug.

| Veld | True betekent |
| --- | --- |
| canonical_tree | Alle17 canonical bestanden en vier submappen bestaan |
| sha_present | Het gekozen checksum-bestand bestaat |
| sha_scope | Het manifest bevat entries en bindt state.txt niet |
| sha_checksum | De genoemde bestanden slagen voor devtool-proof manifest verify binnen deze scope |
| sha_signature | Het checksum-bestand heeft een geldige handtekening onder de gekozen publieke sleutel |
| provenance_present | Het gekozen provenance-bestand bestaat |
| provenance_scope | De provenance-snapshot is gevuld en snapshot/input/output binden state.txt niet |
| provenance | devtool-proof provenance verify slaagt binnen deze scope |
| provenance_signature | Het provenance-bestand heeft een geldige handtekening onder de gekozen publieke sleutel |

False omvat ontbrekend, oningevuld of ongeldig bewijs. De handtekening op een opgeslagen bewijsbestand en de geldigheid van de actuele bronbestanden zijn afzonderlijke meetwaarden. Een gewijzigde bron kan bijvoorbeeld sha_checksum false geven terwijl sha_signature true blijft.

Watch schrijft alleen state.txt in de geselecteerde hoofdmap. Bewijsberekening en ondertekening blijven expliciete opdrachten van devtool-proof. Er worden geen takenprofielen of shellacties uitgevoerd. De verify-commands van devtool-proof leveren de controle-uitkomsten.

## Afgeleid meetbestand

De derde executable heeft hiervoor de expliciete optie --derived gekregen:

```sh
devtool-proof manifest create hoofdmap --derived state.txt
devtool-proof provenance create hoofdmap --derived state.txt -o bewijs/provenance.json
```

Zo bindt een checksum of provenance-snapshot de veranderlijke checklist niet. Zonder die optie behoudt devtool-proof zijn gewone bestandsinventaris, inclusief state.txt. Watch rapporteert een scope die zijn eigen meetbestand bindt als false en gaat niet wisselen tussen true en false door zichzelf te herschrijven. Het SHA-bestand controleert de opgenomen entries; Watch beweert daarmee geen controle van ongenoemde bestanden.

Handtekeningen, sleutels en provenance mogen via configuratie elders staan. De monitor maakt of vernieuwt deze bewijzen niet. Nieuwe resultaten ontstaan wanneer de gekozen bewijsbestanden worden aangeboden of veranderen.

## Configuratie

```sh
devtools-watch --config watch-config.json
```

Dit leest en valideert het JSON-bestand en slaat de configuratie op. Een actieve monitor neemt de nieuwe configuratie tijdens zijn volgende controle over. `--config` zonder bestand toont de effectieve instellingen en processtatus. Alle CLI-resultaten zijn JSON.

```json
{
  "schema": "devtools-watch.config/1",
  "parents": ["/pad/naar/werkmap"],
  "targets": [],
  "proof_executable": "devtool-proof",
  "poll_ms": 250,
  "verify_interval_ms": 1000
}
```

Onder parents worden uitsluitend directe submappen met de vaste naam YYYYMMDDTHHMMSSZ_run-nr.NNN gevolgd. Een map gaat pas de checklistfase in zodra zijn canonical manifest de nieuwe template identificeert. Daardoor kan Watch al draaien vóór -D en --canonical-tree.

Een expliciet target kan ook een zelfgekozen mapnaam en externe bewijsbestanden gebruiken:

```json
{
  "schema": "devtools-watch.config/1",
  "parents": [],
  "targets": [
    {
      "root": "/pad/naar/hoofdmap",
      "checksums": "checksums.sha",
      "checksum_signature": "/pad/naar/bewijs/checksums.sig",
      "provenance": "/pad/naar/bewijs/provenance.json",
      "provenance_signature": "/pad/naar/bewijs/provenance.json.sig",
      "public_key": "/pad/naar/sleutels/author.public.json"
    }
  ],
  "proof_executable": "devtool-proof",
  "poll_ms": 250,
  "verify_interval_ms": 1000
}
```

Relatieve parent- en rootpaden worden bij import absoluut gemaakt vanuit de huidige werkmap. Relatieve bewijspaden zijn relatief aan hun root. De default bewijsnamen zijn checksums.sha, checksums.sha.sig, provenance.json en provenance.json.sig. Zonder gekozen publieke sleutel blijven de handtekeningvelden false. Expliciete targets hebben voorrang op een gevonden root met hetzelfde pad.

Standaard volgt parents de werkmap waarin de configuratie voor het eerst wordt aangemaakt. Configuratie staat onder XDG_CONFIG_HOME/devtools-watch of HOME/.config/devtools-watch. Procesmetadata, locks en machinerapporten staan onder XDG_STATE_HOME/devtools-watch of HOME/.local/state/devtools-watch. Ze staan buiten de gemeten hoofdmap. De monitor bepaalt dit uit dezelfde omgeving als de aanroepende terminal.

De poll controleert structuur, relevante bestandsmetadata en externe bewijspaden. Verificatie gebeurt na wijzigingen en uiterlijk weer na verify_interval_ms. Alleen gewijzigde checklistbytes worden geschreven. --off beëindigt ook een lopend verify-childproces. De laatste checklist blijft staan na uitschakelen.

## Bouwen en testen

```sh
cargo build --release --locked
cargo test --release --locked
cargo clippy --all-targets --locked -- -D warnings
```

De monitor is een zelfstandig ELF-programma. Voor de bewijscontroles gebruikt hij de apart geïnstalleerde devtool-proof. De integratietests gebruiken die echte executable, standaard via PATH. DEVTOOLS_WATCH_TEST_PROOF kan voor tests een expliciet executablepad aanwijzen. Het programma zelf ondersteunt Linux/Android-procesbeheer via /proc en Unix-signalen. De meegeleverde binary is voor Android/Termux ARM64 gebouwd.

Schema's staan onder schemas/. Het state-schema beschrijft de getypeerde sleutel/waarderepresentatie van state.txt. Runtimegegevens en gegenereerde templates bevatten machinegegevens.
