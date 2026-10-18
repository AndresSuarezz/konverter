; Ganchos del instalador de Windows (NSIS). Tauri los inserta en su plantilla.
;
; Al desinstalar hay que limpiar lo que la aplicación guardó FUERA de su carpeta de instalación:
; el LibreOffice que descargó (~690 MB) y sus ajustes. Sin esto, desinstalar Converter dejaba
; esos archivos huérfanos en %LOCALAPPDATA%\Converter.

!macro NSIS_HOOK_POSTUNINSTALL
  ; En una actualización el desinstalador se ejecuta antes de instalar la versión nueva: no se
  ; debe borrar nada (el usuario perdería LibreOffice y sus ajustes a cada actualización).
  ${If} $UpdateMode <> 1
    ; LibreOffice descargado: es una descarga repetible, así que se quita siempre.
    RmDir /r "$LOCALAPPDATA\Converter\components"

    ${If} $DeleteAppDataCheckboxState = 1
      ; El usuario marcó «borrar los datos de la aplicación»: también los ajustes.
      RmDir /r "$LOCALAPPDATA\Converter"
    ${Else}
      ; Si solo quedaba esa carpeta vacía, se retira; si hay ajustes, se conservan.
      RmDir "$LOCALAPPDATA\Converter"
    ${EndIf}
  ${EndIf}
!macroend
