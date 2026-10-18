<#
  Compila wordcraft-cli (https://github.com/storytold/wordcraft, MIT OR Apache-2.0) a
  vendor/bin/wordcraft. Es el motor de documentos preferido de Converter (D-41): DOCX/DOC → PDF en
  décimas de segundo y sin LibreOffice.

  Se fija a una versión exacta (etiqueta y commit): al subir de versión hay que repetir las pruebas
  `wordcraft_*` de converter-core (ver CLAUDE.md) y cambiar $Tag y $Commit. Tarda ~4 min la primera vez.
  Copia también las licencias y avisos: son obligatorios al redistribuir el binario.
#>
param([string]$OutDir = (Join-Path $PSScriptRoot '..\vendor\bin\wordcraft'))

$ErrorActionPreference = 'Stop'
$Repo   = 'https://github.com/storytold/wordcraft'
$Tag    = 'v0.8.0'
$Commit = 'f2e15cf8f899a79899a2d9595ca4ce7df1e5ea22'
$exeName = if ($IsLinux -or $IsMacOS) { 'wordcraft-cli' } else { 'wordcraft-cli.exe' }

if (Test-Path (Join-Path $OutDir $exeName)) { Write-Host "wordcraft-cli ya está en $OutDir"; return }

$work = Join-Path ([IO.Path]::GetTempPath()) "wordcraft-$([guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Force $work | Out-Null
try {
    $src = Join-Path $work 'src'
    Write-Host "Clonando $Repo ($Tag)"
    git clone --quiet --depth 1 --branch $Tag $Repo $src
    if ($LASTEXITCODE -ne 0) { throw "git clone falló" }
    $actual = (git -C $src rev-parse HEAD).Trim()
    if ($actual -ne $Commit) { throw "el commit de $Tag es $actual y se esperaba ${Commit}: la etiqueta cambió" }

    Write-Host "Compilando wordcraft-cli (release)…"
    $env:CARGO_TARGET_DIR = Join-Path $work 'target'
    # Sin tocar los .cargo/config del repo clonado: se compila desde fuera con --manifest-path.
    cargo build --release -p wordcraft-cli --locked --manifest-path (Join-Path $src 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build falló" }

    New-Item -ItemType Directory -Force $OutDir | Out-Null
    Copy-Item (Join-Path $env:CARGO_TARGET_DIR "release\$exeName") $OutDir -Force
    foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') {
        Copy-Item (Join-Path $src $f) (Join-Path $OutDir $f) -Force
    }
    Set-Content (Join-Path $OutDir 'VERSION') "$Tag $Commit" -Encoding ASCII
    Write-Host "wordcraft-cli $Tag instalado en $OutDir"
} finally {
    Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
