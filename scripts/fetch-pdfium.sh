#!/usr/bin/env bash
# Descarga pdfium (binarios de https://github.com/bblanchon/pdfium-binaries) a vendor/bin/pdfium en
# Linux y macOS. Es el equivalente de scripts/fetch-pdfium.ps1 (Windows): misma versión fijada y
# verificación del SHA-256 que publica GitHub; si no coincide, aborta.
#
# Uso:   scripts/fetch-pdfium.sh [--asset linux-x64|linux-arm64|mac-x64|mac-arm64] [--out DIR]
# Sin --asset se elige según el sistema. Para cambiar de versión: actualizar TAG y la tabla de
# hashes (salen de la API de releases: GET /repos/bblanchon/pdfium-binaries/releases/tags/<tag>).
set -euo pipefail

TAG='chromium/8100'
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/vendor/bin/pdfium"
ASSET=''

while [ $# -gt 0 ]; do
  case "$1" in
    --asset) ASSET="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h | --help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "argumento desconocido: $1" >&2; exit 2 ;;
  esac
done

if [ -z "$ASSET" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ASSET=linux-x64 ;;
    Linux-aarch64 | Linux-arm64) ASSET=linux-arm64 ;;
    Darwin-arm64) ASSET=mac-arm64 ;;
    Darwin-x86_64) ASSET=mac-x64 ;;
    *) echo "sistema no soportado: $(uname -s)-$(uname -m)" >&2; exit 2 ;;
  esac
fi

case "$ASSET" in
  linux-x64) SHA256='1c1894a12d4d96ee01400c3874444126254180c5c7d2ab628576b67f8120320a'; LIB='libpdfium.so' ;;
  linux-arm64) SHA256='58c87a95a40a24c914a9c755fae798843f46f06adf9e5e9f2c893302d4bdb040'; LIB='libpdfium.so' ;;
  mac-x64) SHA256='d513a0530a0f0c1ca9929ded06660e27edb9a108a7d18d9df55a5fb060b091c8'; LIB='libpdfium.dylib' ;;
  mac-arm64) SHA256='7ea647c7a7a9c1169be3f947000bc071ece2fade2cd6bd7e401e3675c6560cce'; LIB='libpdfium.dylib' ;;
  *) echo "asset desconocido: $ASSET" >&2; exit 2 ;;
esac

if [ -f "$OUT/$LIB" ]; then
  echo "pdfium ya está en $OUT"
  exit 0
fi

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${TAG//\//%2F}/pdfium-$ASSET.tgz"
echo "Descargando $URL"
curl -fsSL -o "$TMP/pdfium.tgz" "$URL"

ACTUAL="$(sha256_of "$TMP/pdfium.tgz")"
if [ "$ACTUAL" != "$SHA256" ]; then
  echo "SHA-256 distinto: esperado $SHA256, obtenido $ACTUAL" >&2
  exit 1
fi

mkdir -p "$TMP/x" "$OUT"
tar -xzf "$TMP/pdfium.tgz" -C "$TMP/x"
cp "$TMP/x/lib/$LIB" "$OUT/$LIB"
cp "$TMP/x/LICENSE" "$OUT/LICENSE-pdfium" 2>/dev/null || true
echo "pdfium $TAG ($ASSET) instalado en $OUT"
