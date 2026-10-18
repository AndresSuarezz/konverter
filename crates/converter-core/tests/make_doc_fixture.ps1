<#
  Genera tests/fixtures/sample.doc (formato Word 97-2003) abriendo sample.docx con Microsoft
  Word y guardándolo como .doc. Es un fixture producido por Word, no por el código del motor.
  Requiere Word instalado. Uso (desde la raíz del repo):
    powershell -File crates/converter-core/tests/make_doc_fixture.ps1
#>
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'fixtures'
$docx = (Resolve-Path (Join-Path $dir 'sample.docx')).Path
$doc97 = Join-Path $dir 'sample.doc'

$word = New-Object -ComObject Word.Application
try {
    # Visible: guardar como .doc con Word oculto se queda colgado (un diálogo invisible bloquea la
    # llamada y PowerShell reintenta sin parar). Con la ventana visible funciona en un par de segundos.
    $word.Visible = $true
    $word.DisplayAlerts = 0
    $document = $word.Documents.Open($docx, $false, $true)
    $document.SaveAs2($doc97, 0)   # wdFormatDocument (97-2003)
    $document.Close([ref]$false)
}
finally {
    $word.Quit([ref]$false)
}
Write-Host "Generado $doc97"
