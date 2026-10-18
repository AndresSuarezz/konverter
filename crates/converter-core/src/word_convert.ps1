# Convierte un documento con Microsoft Word por automatización COM.
# Lo ejecuta converter-core con: powershell -File word_convert.ps1 -InPath x -OutPath y -To pdf|docx
# Protocolo: imprime "WORD_PID=<n>" por cada proceso WINWORD que crea (para poder matarlo si se
# cuelga); los errores van a stderr con código de salida distinto de 0.
param(
    [Parameter(Mandatory = $true)][string]$InPath,
    [Parameter(Mandatory = $true)][string]$OutPath,
    [Parameter(Mandatory = $true)][ValidateSet('pdf', 'docx')][string]$To
)

$ErrorActionPreference = 'Stop'

$before = @(Get-Process WINWORD -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
$word = $null
$doc = $null
$created = $false
try {
    $word = New-Object -ComObject Word.Application
    $mine = @(Get-Process WINWORD -ErrorAction SilentlyContinue |
        ForEach-Object { $_.Id } | Where-Object { $before -notcontains $_ })
    foreach ($id in $mine) {
        [Console]::Out.WriteLine("WORD_PID=$id")
        [Console]::Out.Flush()
    }
    # Si no apareció un proceso nuevo, Word reutilizó una instancia del usuario: no se cierra.
    $created = $mine.Count -gt 0

    $word.Visible = $false
    $word.DisplayAlerts = 0                # wdAlertsNone
    $word.AutomationSecurity = 3           # msoAutomationSecurityForceDisable: sin macros

    # Open(FileName, ConfirmConversions, ReadOnly, AddToRecentFiles, PasswordDocument)
    # La contraseña ficticia evita el cuadro de diálogo si el documento está protegido: Word
    # devuelve un error en vez de quedarse esperando.
    $doc = $word.Documents.Open($InPath, $false, $true, $false, 'converter-sin-contrasena')

    $format = if ($To -eq 'pdf') { 17 } else { 16 }   # wdFormatPDF / wdFormatXMLDocument
    $doc.SaveAs2($OutPath, $format)
}
catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 2
}
finally {
    # Close/Quit esperan argumentos por referencia: con $false a secas fallan y Word se queda vivo.
    if ($doc) { try { $doc.Close([ref]$false) } catch { } }
    if ($word -and $created) { try { $word.Quit([ref]$false) } catch { } }
    if ($doc) { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($doc) }
    if ($word) { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) }
}
