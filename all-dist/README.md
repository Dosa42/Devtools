# Platformdistributies

De workflow [Build all Devtools platforms](../.github/workflows/build-cli.yml) bouwt en test de vier zelfstandige CLI’s op Linux en macOS. Na een geslaagde run op main publiceert hij hier de pakketten. Elke build is ook beschikbaar als Actions-artifact.

| Map | Rust-target | Executableformaat |
| --- | --- | --- |
| `ubuntu-24.04-x86_64/` | `x86_64-unknown-linux-gnu` | ELF x86_64 |
| `ubuntu-24.04-arm64/` | `aarch64-unknown-linux-gnu` | ELF ARM64 |
| `macos-15-x86_64/` | `x86_64-apple-darwin` | Mach-O Intel |
| `macos-15-arm64/` | `aarch64-apple-darwin` | Mach-O Apple Silicon |

Elke platformmap bevat een complete `devtools-<platform>.tar.gz`, vier afzonderlijke projectpakketten, manifests met broncommit en toolversies en SHA-256-bestanden. De complete bundel bevat ook de Bash-PTY-demo met live telemetrie. Voor die demo zijn Python 3.11 of nieuwer en Bash nodig.

Pak een bundel uit, ga naar de uitgepakte map en start:

```sh
export PATH="$PWD/devtool/dist:$PATH"
python devtool/demo/server.py --port 8765
```

Open http://127.0.0.1:8765. De onderliggende bibliotheken van het gekozen besturingssysteem blijven van toepassing; een ELF-bestand voor Ubuntu is een andere build dan een Android/Termux-binary. De eerdere Android/Termux-binaries blijven onder de oorspronkelijke `dist/`-mappen beschikbaar.

De matrix gebruikt native runners zodat alle vier de executables en de verpakte demo werkelijk kunnen worden uitgevoerd. Het zijn afzonderlijke platformbuilds; de macOS-bestanden worden niet op Linux gelinkt.
