#!/usr/bin/env bash
# Compila wordcraft-cli (https://github.com/storytold/wordcraft, MIT OR Apache-2.0) a
# vendor/bin/wordcraft en Linux y macOS. Equivalente de scripts/build-wordcraft.ps1: misma versión
# fijada (etiqueta y commit). Tarda ~5 min. Copia también las licencias, obligatorias al redistribuir.
#
# Uso:   scripts/build-wordcraft.sh [--out DIR]
set -euo pipefail

REPO='https://github.com/storytold/wordcraft'
TAG='v0.8.0'
COMMIT='f2e15cf8f899a79899a2d9595ca4ce7df1e5ea22'
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/vendor/bin/wordcraft"

while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    -h | --help) sed -n '2,7p' "$0"; exit 0 ;;
    *) echo "argumento desconocido: $1" >&2; exit 2 ;;
  esac
done

if [ -f "$OUT/wordcraft-cli" ]; then
  echo "wordcraft-cli ya está en $OUT"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
echo "Clonando $REPO ($TAG)"
git clone --quiet --depth 1 --branch "$TAG" "$REPO" "$WORK/src"
ACTUAL="$(git -C "$WORK/src" rev-parse HEAD)"
if [ "$ACTUAL" != "$COMMIT" ]; then
  echo "el commit de $TAG es $ACTUAL y se esperaba $COMMIT: la etiqueta cambió" >&2
  exit 1
fi

echo "Compilando wordcraft-cli (release)…"
# Desde fuera del repo clonado (--manifest-path), para no heredar su .cargo/config.
CARGO_TARGET_DIR="$WORK/target" cargo build --release -p wordcraft-cli --locked --manifest-path "$WORK/src/Cargo.toml"

mkdir -p "$OUT"
cp "$WORK/target/release/wordcraft-cli" "$OUT/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do cp "$WORK/src/$f" "$OUT/$f"; done
echo "$TAG $COMMIT" >"$OUT/VERSION"
echo "wordcraft-cli $TAG instalado en $OUT"
