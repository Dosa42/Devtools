# devtool

Native Rust CLI for creating directories, writing files, compressing archives and managing settings. The supplied release is an Android/Termux ARM64 ELF executable.

## Build and run

```sh
cargo build --release --locked
./target/release/devtool --help
./dist/devtool --version
```

`src/main.rs` parses CLI arguments and determines the exit code. `src/lib.rs` dispatches operations to the configuration, filesystem, archive, menu and schema modules. Archive compression runs inside the binary using sevenz-rust2 and tar/flate2. Rust and Cargo are required to build; the built executable uses the platform runtime.

## Main directories: -D

```sh
devtool -D
devtool -D --path /path/to/parent
devtool -D --json
```

`-D` uses an uppercase D and creates exactly one main directory. Without `--path`, the parent is the current working directory of the invoking process. `--path` selects a parent directory; it does not supply the new directory's name. Relative paths are resolved from the current working directory. The selected parent is an existing directory.

Names use the fixed format `YYYYMMDDTHHMMSSZ_run-nr.NNN`, for example `20261112T143000Z_run-nr.035`. The timestamp is the UTC time of execution, to the second. The run number starts at `001`, increases across separate invocations and parent directories, and continues past `999` as `1000`. Naming and the starting number are fixed in code.

The persistent counter is `$XDG_STATE_HOME/devtool/directory-counter.json`, or `$HOME/.local/state/devtool/directory-counter.json` when XDG_STATE_HOME is unset. It is independent of the selected configuration file. Concurrent processes share a file lock when reserving their next run number. The new run number and latest path are stored after the main directory is created. The same state records the absolute path of the latest successfully created main directory. Text output prints the created absolute path; JSON output includes `path`, `name`, `timestamp` and the numeric `run`. `-D` is an alternative to a subcommand; `--path` requires `-D` or `--canonical-tree`.

## Canonical files and subdirectories

```sh
devtool -D
devtool --canonical-tree
devtool --canonical-tree --path /chosen/main-directory
devtool --canonical-tree --json
```

`--canonical-tree` executes the embedded shell runner using the embedded JSON template. Without `--path`, its target is the latest successfully created main directory, including when invoked from another working directory or after a process restart. With `--path`, the path is the existing target directory itself. An explicit target does not change the remembered latest main directory or advance its run number. `-D`, `--canonical-tree` and subcommands are separate actions.

[templates/canonical-tree.json](templates/canonical-tree.json) lists the fixed tree and its prepared file assets. All 17 files already exist under `templates/files/` and are embedded as literal byte arrays with `include_bytes!` in `src/canonical.rs`. Their contents are compiled into the ELF.

[templates/canonical-tree.sh](templates/canonical-tree.sh) consists of four fixed `mkdir` commands and 17 fixed `cp` commands. It contains no `if`, loops, existence checks, skip logic or noclobber setting. Rust materializes the already embedded bytes into a temporary source directory and executes that embedded script through `sh`. The script directly copies the prepared files to their fixed destinations. The required runtime commands are `sh`, `mkdir` and `cp`.

The initial template is:

```text
main-directory/
├── document.pdf
├── README.md
├── manifest.json
├── state.txt
├── checksums.sha
├── docs/
│   ├── README.md
│   ├── outline.txt
│   └── metadata.json
├── data/
│   ├── README.md
│   ├── records.json
│   └── input.txt
├── logs/
│   ├── README.md
│   ├── run.log
│   └── events.jsonl
└── output/
    ├── result.txt
    ├── summary.md
    └── index.json
```

There are four subdirectories, five root files and three files per subdirectory: 17 files total. `document.pdf` is a minimal one-page PDF. Markdown and JSON files contain minimal templates. Text/log files and `checksums.sha` are empty placeholders; the SHA placeholder does not claim to contain computed checksums.

The runner executes those fixed creation and copy commands directly. JSON output lists the template directories and files. Shell command failures are reported as CLI errors. When no main-directory path has been recorded, select one with `--path` or create one with `-D`.

## Commands

```sh
devtool mkdir project/src tests assets --parents
devtool file create project/src/main.rs
devtool file create message.txt --content "Hallo"
devtool file create message.txt --append --content " wereld"
devtool file create message.txt --overwrite --content "Nieuw"
printf 'Hallo' | devtool file create stdin.txt --stdin
devtool compress project --format 7z --level 3 -o project.7z
devtool compress project --format tar.gz -o project.tar.gz
devtool config init
devtool config menu
devtool config set compression.format 7z
devtool config set compression.level 3
devtool config get compression.level
devtool config show
devtool config show --effective
devtool config validate
devtool config path
devtool config schema --output schemas/config.schema.json
devtool schema --output schemas/cli.schema.json
devtool --json mkdir result
devtool file create --help
```

The executable must be on PATH to use `devtool` directly; otherwise use `./dist/devtool` or its absolute path. No installation outside this project is performed.

`--config FILE`, `--json` and `--color auto|always|never` work before or after a subcommand. `mkdir` and `file create` accept `--parents` and `--no-parents` to select parent-directory behavior. With parents enabled, mkdir accepts directories that already exist. File creation accepts byte streams via `--stdin`, UTF-8 strings via `--content`, or creates an empty file when neither is given. Existing files follow the configured error/overwrite/append policy, overridden by the explicit `--overwrite` or `--append` flag.

Compression accepts one file or directory tree and writes `.7z` or `.tar.gz` using the Rust archive libraries:

```sh
devtool compress hoofdmap --format 7z -o archief.7z
devtool compress hoofdmap --format tar.gz -o archief.tar.gz
```

`--format` selects the codec; omitting it uses `compression.format`. `-o` selects the output path; omitting it uses the source basename plus the selected format extension next to the source. `--level` selects 0–9, or uses `compression.level`. The output parent is an existing directory. The selected output is written when compression completes.

The archive contains the source basename as its root, including empty directories and file bytes. Source entries are collected before writing the archive. The complete collected source is passed to the archive backend. Directory, UTF-8 text and binary-file roundtrips are tested for both formats, including all 17 files of the canonical tree.

## Settings

Settings are reloaded on every invocation. No rebuild or background service is needed. The configuration path is selected by `--config`, then `DEVTOOL_CONFIG`, then `$XDG_CONFIG_HOME/devtool/config.toml`, then `$HOME/.config/devtool/config.toml`. An explicitly selected missing file is an error for operations; `config init`, `config set` and the menu can create it. An absent default configuration uses built-in defaults.

Priority from highest to lowest:

1. CLI flags.
2. Environment variables.
3. The selected TOML configuration file.
4. Built-in defaults.

| Setting | Default | Environment variable |
| --- | --- | --- |
| `schema_version` | `1` | `DEVTOOL_SCHEMA_VERSION` |
| `filesystem.create_parents` | `true` | `DEVTOOL_FILESYSTEM_CREATE_PARENTS` |
| `filesystem.existing_file` | `error` | `DEVTOOL_FILESYSTEM_EXISTING_FILE` |
| `compression.format` | `tar.gz` | `DEVTOOL_COMPRESSION_FORMAT` |
| `compression.level` | `6` | `DEVTOOL_COMPRESSION_LEVEL` |
| `output.format` | `text` | `DEVTOOL_OUTPUT_FORMAT` |
| `output.color` | `auto` | `DEVTOOL_OUTPUT_COLOR` |

`config show` displays stored settings with defaults. `config show --effective` includes overrides and each setting's source. `config set` and the menu persist file settings without saving environment or CLI overrides. The menu accepts a setting number, then a new value; `s` saves and exits, while `q` exits without saving. Invalid values are reported in the menu. EOF before save/quit exits with an error. `config init --force` restores defaults, including when the current file is malformed.

`config validate` validates the selected file itself. Unknown fields, unsupported schema versions, wrong types and compression levels outside 0–9 are rejected. Partial TOML files inherit defaults. `schemas/config.schema.json` is a JSON Schema generated from the settings types. `schemas/cli.schema.json` describes commands, arguments, flags, conflicts, arities and enumerated values generated from clap; it is a versioned CLI inventory.

Output is text by default. `--json` produces JSON on stdout for operations. Help and version use clap's text output; the interactive menu uses text and rejects `--json`. Errors go to stderr. Exit codes are 0 for success/help/version, 1 for operation/configuration failures and 2 for invalid CLI arguments. Automatic color applies to terminal text unless NO_COLOR is set.

## Verification

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

The integration suite executes the actual CLI in isolated temporary directories and checks main-directory naming, default and explicit parent paths, persistent and concurrent run allocation, the fixed shell commands, embedded file bytes, latest-directory selection, file contents, settings precedence, menu persistence, schema generation, archive replacement and extraction of both archive formats. Release evidence is retained under `evidence/`.

Canonical templateversie 2 gebruikt `state.txt` als vooraf ingebedde machinechecklist in plaats van `notes.txt`. Root `manifest.json` declareert `derived_outputs: ["state.txt"]`. De vaste runner blijft vier mkdir- en zeventien cp-opdrachten uitvoeren. `devtools-watch` werkt de machinewaarden bij nadat de template is aangemaakt. Met `devtool-proof --derived state.txt` worden bewijsbestanden voor deze meetopzet gemaakt.
