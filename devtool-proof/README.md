# devtool-proof 0.2.0

Derde zelfstandig Rust-project en executable voor hashes, checksum-manifesten, Ed25519-handtekeningen, provenance en AES-256-archieven. ZIP en 7z zijn beide native ingebouwd. De executable roept geen externe archiver, OpenSSL of andere devtool-executable aan.

De meegeleverde `dist/devtool-proof` is een Android/Termux ARM64 ELF-binary. Andere platforms krijgen een eigen buildtarget.

## Bouwen

```sh
cargo build --release --locked
cargo test --release --locked
cargo clippy --all-targets --locked -- -D warnings
```

De integratietests gebruiken ook `sha256sum`, `sha512sum` en OpenSSL voor onafhankelijke controles. De executable zelf heeft die programma's niet nodig.

## Hash en SHA-bestand

```sh
devtool-proof hash document.pdf
devtool-proof hash document.pdf --algorithm sha512
devtool-proof hash --stdin --algorithm sha256
devtool-proof manifest create hoofdmap
devtool-proof manifest verify hoofdmap/checksums.sha
```

`manifest create hoofdmap` schrijft `hoofdmap/checksums.sha`, met SHA-256, relatieve paden en vaste sortering. Het manifestbestand neemt zijn eigen outputpad niet op in zijn berekening. Bestanden worden volledig gelezen; inhoud wordt gestreamd. SHA-512 is een expliciete algoritmekeuze. De SHA-notatie ondersteunt GNU-checksumregels, inclusief escaped backslashes en nieuwe regels in UTF-8-bestandsnamen.

```sh
devtool-proof manifest create hoofdmap --format json -o manifest.json
devtool-proof manifest verify manifest.json --root hoofdmap
devtool-proof manifest compare oud.sha nieuw.sha
```

JSON-manifesten bevatten schema, algoritme, relatieve paden, hashes en bestandsgroottes. `verify` controleert de opgenomen bestanden en rapporteert afwijkingen en ontbrekende bestanden. `compare` toont toegevoegde, verwijderde en gewijzigde paden. De standaardbasis voor verificatie is de map van het manifest; met `--root` geef je de bronmap aan voor een manifest dat elders staat.

De bronboom omvat gewone bestanden en mappen. Symlinks volgen hun doelen; een directorycyclus wordt als fout gemeld. Speciale filesystemobjecten en niet-UTF-8-bestandsnamen vallen buiten dit manifestformaat. De uitvoermap van een manifest, sleutel of archief moet bestaan.

## Sleutels en ondertekening

```sh
devtool-proof key generate --type ed25519 --private auteur.private.json --public auteur.public.json
devtool-proof sign hoofdmap/checksums.sha --key auteur.private.json
devtool-proof verify-signature hoofdmap/checksums.sha --key auteur.public.json
```

`sign` schrijft standaard `BESTAND.sig`; `--output` kiest een ander pad. `verify-signature --signature PAD` kiest een afwijkend signaturebestand. Verificatie gebruikt de door jou gekozen publieke sleutel. `key generate` toont de SHA-256-fingerprint van de publieke sleutel. Nieuwe private sleutelbestanden krijgen Unix-modus 0600.

Het signaturedocument bevat schema, Ed25519-algoritme, SHA-256-digest, publieke sleutel en detached signature als hex. Het exact ondertekende bericht is de byteprefix `devtool-proof.signature/1`, NUL, `sha256`, NUL, gevolgd door de 32 digestbytes. Dit is Ed25519 over dat bericht. Verificatie berekent de actuele bestandshash opnieuw en controleert de handtekening met de gekozen publieke sleutel. De inhoud van een JSON-bestand wordt als bestandbytes ondertekend; wijzigingen aan de serialisatie wijzigen ook de bestandshash.

## Provenance

```sh
devtool-proof provenance create hoofdmap -o provenance.json --run run-nr.001 --command 'devtool --canonical-tree' --tool devtool=0.4.0 --input data/input.txt --artifact output/result.txt
devtool-proof provenance show provenance.json
devtool-proof provenance verify provenance.json --root hoofdmap
devtool-proof sign provenance.json --key auteur.private.json
```

Het document bevat de aanmaaktijd in Unix-milliseconden, een optionele runnaam, de gedeclareerde opdracht, toolversies, een bestandsinventaris met hashes en gekozen input-/outputbestanden. Input- en artifactpaden zijn relatief aan de bronmap. `--input`, `--artifact` en `--tool` kunnen herhaald worden. De eigen executableversie wordt rechtstreeks opgenomen. Andere versies en de opdracht zijn gegevens die je expliciet meegeeft; dit command beweert geen waargenomen uitvoering van die opdracht. Ondertekenen bindt deze recordinhoud aan de gekozen sleutel.

## ZIP en 7z met AES-256

```sh
devtool-proof archive encrypt hoofdmap --format zip --cipher aes256 --password-prompt -o pakket.zip
devtool-proof archive encrypt hoofdmap --format 7z --cipher aes256 --encrypt-names --password-prompt -o pakket.7z
```

`encrypt` maakt een nieuw archief van een bronbestand of bronmap. De naam van de bron staat als bovenste archiefentry in de uitvoer. `--level 0..9` kiest het compressieniveau, standaard 6. ZIP gebruikt Deflate en WinZip AES-256. 7z gebruikt LZMA2 en AES-256/SHA-256 met een KDF cycles power van 19. Salt/IV worden door de libraries willekeurig aangemaakt; file- en headerencoders krijgen afzonderlijke encryptieparameters.

`--encrypt-names` is een 7z-optie. ZIP houdt zijn entrynamen in de ZIP-directory. Bij 7z zonder deze flag blijven headers leesbaar en zijn de bestandsgegevens versleuteld. Het cipher is expliciet AES-256, niet ZipCrypto.

Wachtwoordinvoer, kies één:

| Flag | Invoer |
| --- | --- |
| `--password-prompt` | Verborgen prompt op de terminal |
| `--password-stdin` | Eén regel uit stdin |
| `--password-file PAD` | Bestand met het wachtwoord |

Een afsluitende LF, of CRLF, wordt verwijderd. Andere spaties blijven onderdeel van het wachtwoord. De promptwaarde gaat niet in de uitvoerrapportage.

```sh
devtool-proof archive test pakket.zip --password-prompt
devtool-proof archive decrypt pakket.zip --password-prompt -o uitgepakt
devtool-proof archive test pakket.7z --password-stdin
devtool-proof archive decrypt pakket.7z --password-file wachtwoordbestand -o uitgepakt
```

De leescommands herkennen ZIP of 7z aan de bestandsheader. `test` leest alle entries tot het einde en gebruikt de controles van het archiefformaat; het schrijft geen uitgepakte bestanden. `decrypt` pakt de entries uit in de gekozen map. Het checksum-manifest kan daarna de inhoud opnieuw controleren. Encryptie van een bestaande ZIP als bronbestand neemt die ZIP als bestand op in een nieuw archief; het wijzigt niet de interne entries van de oorspronkelijke ZIP.

## Schema's, help en exitcodes

```sh
devtool-proof schema --kind manifest
devtool-proof schema --kind key
devtool-proof schema --kind signature
devtool-proof schema --kind provenance
devtool-proof --help
devtool-proof archive encrypt --help
```

Alle rapporten zijn leesbare JSON. Globaal `--json` maakt ze compact. De schema's staan ook onder `schemas/`. Runtime-validatie controleert onder meer schema-identiteit, digestlengtes, relatieve paden en unieke manifestpaden.

Exitcodes: 0 voor geslaagde uitvoering/verificatie, 1 voor een inhouds- of handtekeningmismatch, 2 voor ongeldige invoer, I/O-, archief- of commandfouten. `manifest compare` rapporteert verschillen en geeft 0 wanneer het vergelijken slaagt.

## Afgeleide machinebestanden

Met `--derived state.txt` bij `manifest create` of `provenance create` declareer je een afgeleid bestand dat buiten de inhoudsmeting valt. De optie kan herhaald worden en gebruikt relatieve paden onder de bronroot. Een opgegeven map omvat zijn inhoud. Zonder deze optie blijven alle bestanden behalve het eigen outputpad in de inventaris.

```sh
devtool-proof manifest create hoofdmap --derived state.txt
devtool-proof provenance create hoofdmap --derived state.txt -o provenance.json
```

Dit ondersteunt de vaste checklist van devtools-watch: het wijzigen van het meetresultaat wijzigt de gemeten inhoud dan niet. De overige hash-, signature- en archiefcommands zijn ongewijzigd.
