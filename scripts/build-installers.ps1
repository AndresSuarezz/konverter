<#
  Construye el instalador de Windows y lo deja en dist/:

    Converter_<versión>_x64-setup.exe            (normal, ~45 MiB)
        La aplicación, pdfium y WordCraft (motor de documentos, 35 MB). Convierte imágenes, datos,
        PDF y documentos de Word (DOCX/DOC) y Markdown a PDF sin internet. Solo PDF → Word necesita
        LibreOffice, que la aplicación ofrece descargar la primera vez (374 MB).

    Converter_<versión>_x64-setup-con-libreoffice.exe   (con -ConLibreOffice, ~155 MiB)
        Lo mismo y, además, LibreOffice podado dentro (PDF → Word sin internet). Este instalador
        muestra una página para elegir «Estándar» o «Con LibreOffice».

  Prepara lo necesario (pdfium, wordcraft-cli y, con -ConLibreOffice, LibreOffice en vendor/bin, que
  git ignora) si falta. Desde la raíz del repositorio:
      powershell -File scripts/build-installers.ps1                  # el normal
      powershell -File scripts/build-installers.ps1 -ConLibreOffice  # el que lleva LibreOffice

  El CLI de Tauri va FIJADO (2.12.3) porque crates/palette-app/nsis/installer.nsi es una copia de la
  plantilla de su tauri-bundler 2.10.3 con añadidos propios. Al subir el CLI, compara esa plantilla.
#>
param([switch]$ConLibreOffice)

$ErrorActionPreference = 'Stop'
$TauriCli = '@tauri-apps/cli@2.12.3'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
New-Item -ItemType Directory -Force dist | Out-Null

function Run([string]$what, [scriptblock]$block) {
    Write-Host "== $what"
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$what falló (código $LASTEXITCODE)" }
}

Run 'pdfium' { & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-pdfium.ps1 }
Run 'wordcraft-cli' { & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-wordcraft.ps1 }

# CVT_LO_DIR le dice a la plantilla NSIS qué carpeta de LibreOffice meter dentro. Sin ella, el
# instalador no lleva LibreOffice ni muestra la página de elección.
$bundle = Join-Path $root 'vendor/bin/bundle'
if ($ConLibreOffice) {
    Run 'LibreOffice podado' { cargo run -q -p components --example prepare_bundle -- vendor/bin/bundle }
    $lo = Get-ChildItem $bundle -Directory -Filter 'libreoffice-*' |
        Where-Object { $_.Name -notmatch '\.partial$' } | Select-Object -First 1
    if (-not $lo) { throw "no se encontró el LibreOffice preparado en $bundle" }
    $env:CVT_LO_DIR = $lo.FullName
    Write-Host "   LibreOffice incluido: $($lo.FullName)"
} else {
    Remove-Item Env:\CVT_LO_DIR -ErrorAction SilentlyContinue
}

$version = (Select-String -Path crates/palette-app/tauri.conf.json -Pattern '"version":\s*"([^"]+)"').Matches[0].Groups[1].Value
$env:CARGO_TARGET_DIR = Join-Path $root 'target/bundle'

Push-Location crates/palette-app
try {
    Run 'instalador' { npx --yes $TauriCli build --config tauri.bundle.conf.json }
} finally {
    Pop-Location
    Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
}

$built = Join-Path $root "target/bundle/release/bundle/nsis/Converter_${version}_x64-setup.exe"
$name = if ($ConLibreOffice) { "Converter_${version}_x64-setup-con-libreoffice.exe" } else { "Converter_${version}_x64-setup.exe" }
$out = Join-Path $root "dist/$name"
Copy-Item $built $out -Force
Remove-Item Env:\CVT_LO_DIR -ErrorAction SilentlyContinue
Write-Host ("-> {0} ({1:N0} MB)" -f $out, ((Get-Item $out).Length / 1MB))
