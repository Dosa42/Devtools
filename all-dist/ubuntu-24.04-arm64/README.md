# Devtools — ubuntu-24.04-arm64

Vier native ELF-executables voor `aarch64-unknown-linux-gnu`.

Pak `devtools-ubuntu-24.04-arm64.tar.gz` uit en start vanuit de uitgepakte map:

```sh
export PATH="$PWD/devtool/dist:$PATH"
python devtool/demo/server.py --port 8765
```

Open http://127.0.0.1:8765. Python 3.11 of nieuwer en Bash zijn nodig voor de demo.

De vier submappen bevatten ook afzonderlijke pakketten. `devtool` bevat de complete demo en vier binaries; de overige pakketten bevatten hun eigen executable.

Broncommit: `f0145b19ae9668a752f94c1d6a947d86b562ed19`.
