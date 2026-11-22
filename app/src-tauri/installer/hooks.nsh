; Installer hooks of SYNTRAC (Tauri NSIS, per machine). Each hook runs ONE script of the payload with explicit arguments and checks its answer; the
; logic lives in scripts/deploy, where it is tested. docs/RELEASE.md explains the behaviour.
;
; The payload is in $INSTDIR\setup (the service program, its config template and the deploy scripts). The service's own folders, $INSTDIR\bin and
; $INSTDIR\config, are siblings of it and are never installed or removed by the installer itself.

; NSIS is a 32-bit program: on 64-bit Windows its "System32" is really SysWOW64, where PowerShell would see the x86 Program Files. Sysnative reaches
; the real one. The path is left in $R9.
!macro SYNTRAC_FIND_POWERSHELL
  ${If} ${FileExists} "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy $R9 "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${Else}
    StrCpy $R9 "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

; Before any file is copied. The service runs as SYSTEM, so its folder must not be one ordinary users can write to: only Program Files is accepted.
!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  StrLen $R0 "$PROGRAMFILES64\"
  StrCpy $R1 "$INSTDIR" $R0
  ${If} "$R1" != "$PROGRAMFILES64\"
    MessageBox MB_OK|MB_ICONSTOP "SYNTRAC must be installed inside $PROGRAMFILES64.$\r$\n$\r$\nIts background service runs with the highest rights, so its folder must not be writable by ordinary users." /SD IDOK
    Abort "SYNTRAC must be installed inside Program Files."
  ${EndIf}
  Pop $R1
  Pop $R0
!macroend

; After the app's files are in place: install or upgrade the service. A failure puts the previous service back (installer-install.ps1) and stops the
; installer. The app's own files are not rolled back.
!macro NSIS_HOOK_POSTINSTALL
  Push $R0
  Push $R8
  Push $R9
  !insertmacro SYNTRAC_FIND_POWERSHELL
  ; A silent install can name the person who runs the app: setup.exe /S /USERSID=S-1-5-21-...
  StrCpy $R8 ""
  ${GetOptions} $CMDLINE "/USERSID=" $R0
  ${IfNot} ${Errors}
    StrCpy $R8 ' -UserSid "$R0"'
  ${EndIf}
  DetailPrint "Setting up the SYNTRAC background service..."
  nsExec::ExecToLog '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\setup\scripts\deploy\installer-install.ps1" -InstallDir "$INSTDIR" -LogPath "$TEMP\SYNTRAC-setup.log"$R8'
  Pop $R0
  ${If} "$R0" != "0"
    MessageBox MB_OK|MB_ICONSTOP "SYNTRAC could not set up its background service.$\r$\n$\r$\nAn earlier version of the service, if there was one, was put back.$\r$\nThe details are in:$\r$\n$TEMP\SYNTRAC-setup.log" /SD IDOK
    Abort "The SYNTRAC service could not be set up. See $TEMP\SYNTRAC-setup.log"
  ${EndIf}
  Pop $R9
  Pop $R8
  Pop $R0
!macroend

; Before anything is removed. The service is taken out only by a REAL uninstall. When an installer replaces this version it runs this uninstaller in
; place (the uninstaller then runs from its own folder, $EXEDIR = $INSTDIR) or with /UPDATE: the service is left alone then, because the new
; installer upgrades it and can roll it back. The configuration and the data are kept either way.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} "$EXEDIR" != "$INSTDIR"
    Push $R0
    Push $R9
    !insertmacro SYNTRAC_FIND_POWERSHELL
    DetailPrint "Removing the SYNTRAC background service (your data and configuration are kept)..."
    nsExec::ExecToLog '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\setup\scripts\deploy\installer-uninstall.ps1" -InstallDir "$INSTDIR" -LogPath "$TEMP\SYNTRAC-uninstall.log"'
    Pop $R0
    ${If} "$R0" != "0"
      MessageBox MB_OK|MB_ICONSTOP "SYNTRAC could not remove its background service, so nothing was uninstalled.$\r$\n$\r$\nThe details are in:$\r$\n$TEMP\SYNTRAC-uninstall.log" /SD IDOK
      Abort "The SYNTRAC service could not be removed. See $TEMP\SYNTRAC-uninstall.log"
    ${EndIf}
    Pop $R9
    Pop $R0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
