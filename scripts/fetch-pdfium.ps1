<#
  Descarga pdfium (binarios de https://github.com/bblanchon/pdfium-binaries) a vendor/bin/pdfium.
  Verifica el SHA-256 publicado por GitHub para la versión fijada; si no coincide, aborta.
  Para cambiar de versión: actualizar $Tag y $Sha256 (el hash sale de la API de releases).
#>
param([string]$OutDir = (Join-Path $PSScriptRoot '..\vendor\bin\pdfium'))

$ErrorActionPreference = 'Stop'
$Tag    = 'chromium/8100'
$Asset  = 'pdfium-win-x64.tgz'
$Sha256 = '51621bcc1d9be9ea448f4b84b0516862b928e3968d3fca899af1a6d6e2848d6d'

if (Test-Path (Join-Path $OutDir 'pdfium.dll')) { Write-Host "pdfium ya está en $OutDir"; return }

New-Item -ItemType Directory -Force $OutDir | Out-Null
$tmp = Join-Path ([IO.Path]::GetTempPath()) "pdfium-$([guid]::NewGuid()).tgz"
$url = "https://github.com/bblanchon/pdfium-binaries/releases/download/$($Tag -replace '/', '%2F')/$Asset"
Write-Host "Descargando $url"
Invoke-WebRequest $url -OutFile $tmp -UseBasicParsing

$actual = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $Sha256) { Remove-Item $tmp -Force; throw "SHA-256 distinto: esperado $Sha256, obtenido $actual" }

$extract = Join-Path ([IO.Path]::GetTempPath()) "pdfium-x-$([guid]::NewGuid())"
New-Item -ItemType Directory $extract | Out-Null
# tar.exe de Windows (bsdtar): el GNU tar de Git Bash toma 'C:' por un host remoto.
& (Join-Path $env:SystemRoot 'System32\tar.exe') -xzf $tmp -C $extract
if ($LASTEXITCODE -ne 0) { throw "tar falló con código $LASTEXITCODE" }
Copy-Item (Join-Path $extract 'bin\pdfium.dll') $OutDir
Copy-Item (Join-Path $extract 'LICENSE') (Join-Path $OutDir 'LICENSE-pdfium') -ErrorAction SilentlyContinue
Remove-Item $tmp -Force
Remove-Item $extract -Recurse -Force
Write-Host "pdfium $Tag instalado en $OutDir"
