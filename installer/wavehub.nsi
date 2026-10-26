; Wave Hub - Windows x64 setup (NSIS 3, cross-built with makensis on Linux)
; Per-machine install: C:\Program Files\Wave Hub, Start menu shortcut, desktop shortcut.
; 0.2.x installed per-user (%LOCALAPPDATA%\Programs\Wave Hub): an old copy there is removed
; on upgrade, and its HKCU registry entries and shortcuts with it. What Wave Hub installed for
; you and its settings (%LOCALAPPDATA%\WaveHub) are data, and stay.
; Build: scripts/package-windows.sh 0.2.0
; Silent install: WaveHub-<ver>-setup-x64.exe /S     Silent uninstall: Uninstall.exe /S
Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef VIVERSION
  !define VIVERSION "${VERSION}.0"
!endif
!ifndef PKGDIR
  !define PKGDIR "..\dist\pkg"
!endif
!ifndef OUTFILE
  !define OUTFILE "..\dist\WaveHub-${VERSION}-setup-x64.exe"
!endif

!define APPNAME "Wave Hub"
!define EXE "WaveHub.exe"
!define PUBLISHER "Del (postmanpat)"
!define REGKEY "Software\WaveHub"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WaveHub"
!define ICON "..\assets\icons\wavehub.ico"
; Where 0.2.x put a per-user copy
!define OLDUSERDIR "$LOCALAPPDATA\Programs\${APPNAME}"

Name "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "${REGKEY}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${APPNAME} ${VERSION} - free for Windows and Linux"

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APPNAME} setup"
VIAddVersionKey "LegalCopyright" "MIT, (c) 2026 Del (postmanpat)"
VIAddVersionKey "CompanyName" "${PUBLISHER}"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_WELCOMEFINISHPAGE_BITMAP "welcome.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "welcome.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "header.bmp"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "Wave Hub installs, updates and opens the Wave apps: BigWave Radio, ChipWave Studio and StreamWave.$\r$\n$\r$\nIt's free. It installs for everyone on this PC, so Windows asks for an administrator password.$\r$\n$\r$\nClick Next to continue."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start Wave Hub"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${PKGDIR}\LICENSE.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Close a running Wave Hub so its program file can be replaced.
!macro CLOSE_RUNNING UN
Function ${UN}CloseRunning
  nsExec::ExecToStack 'tasklist /FI "IMAGENAME eq ${EXE}" /NH /FO CSV'
  Pop $0
  Pop $1
  StrCpy $2 $1 11 1
  ${If} $2 == "${EXE}"
    MessageBox MB_OKCANCEL|MB_ICONINFORMATION "Wave Hub is open. It will be closed to continue." /SD IDOK IDOK +2
    Abort
    nsExec::Exec 'taskkill /IM "${EXE}"'
    Sleep 1500
    nsExec::Exec 'taskkill /F /IM "${EXE}"'
    Sleep 500
  ${EndIf}
FunctionEnd
!macroend
!insertmacro CLOSE_RUNNING ""
!insertmacro CLOSE_RUNNING "un."

; Remove the per-user copy 0.2.x installed, so there is only one Wave Hub.
; Data (%LOCALAPPDATA%\WaveHub: the apps it installed, its settings) is left alone.
Function RemoveOldUserInstall
  ; old per-user shortcuts
  SetShellVarContext current
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  SetShellVarContext all

  ReadRegStr $0 HKCU "${REGKEY}" "InstallDir"
  ${If} $0 == ""
    StrCpy $0 "${OLDUSERDIR}"
  ${EndIf}
  ${If} $0 == "$INSTDIR"
    Return
  ${EndIf}
  ${IfNot} ${FileExists} "$0\${EXE}"
    Return
  ${EndIf}
  Delete "$0\${EXE}"
  Delete "$0\README.txt"
  Delete "$0\LICENSE.txt"
  Delete "$0\OFL-Poppins.txt"
  Delete "$0\Uninstall.exe"
  RMDir "$0"
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${REGKEY}"
FunctionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Wave Hub needs 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext all
  Call CloseRunning
  ; InstallDirRegKey ran before SetRegView 64: read the 64-bit key again
  ReadRegStr $0 HKLM "${REGKEY}" "InstallDir"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
  Call un.CloseRunning
FunctionEnd

!macro REFRESH_SHELL_ICONS
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

Section "Wave Hub (required)" SecCore
  SectionIn RO
  SetShellVarContext all
  Call RemoveOldUserInstall
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "${PKGDIR}\${EXE}"
  File "${PKGDIR}\README.txt"
  File "${PKGDIR}\LICENSE.txt"
  File "${PKGDIR}\OFL-Poppins.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk" "$INSTDIR\Uninstall.exe" "" "$INSTDIR\${EXE}" 0

  WriteRegStr HKLM "${REGKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKLM "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTKEY}" "URLInfoAbout" "https://github.com/postmanpat-303/WaveHub"
  WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
  !insertmacro REFRESH_SHELL_ICONS
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext all
  Delete "$DESKTOP\${APPNAME}.lnk"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  !insertmacro REFRESH_SHELL_ICONS
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore} "Wave Hub, its licence, a Start menu shortcut and the uninstaller."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Put a Wave Hub shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext all
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\OFL-Poppins.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINSTKEY}"
  DeleteRegKey HKLM "${REGKEY}"

  ; a per-user 0.2.x copy, if this install did not get the chance to remove it
  SetShellVarContext current
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  SetShellVarContext all
  ReadRegStr $0 HKCU "${REGKEY}" "InstallDir"
  ${If} $0 == ""
    StrCpy $0 "${OLDUSERDIR}"
  ${EndIf}
  ${If} $0 != "$INSTDIR"
    ${If} ${FileExists} "$0\${EXE}"
      Delete "$0\${EXE}"
      Delete "$0\README.txt"
      Delete "$0\LICENSE.txt"
      Delete "$0\OFL-Poppins.txt"
      Delete "$0\Uninstall.exe"
      RMDir "$0"
    ${EndIf}
    DeleteRegKey HKCU "${UNINSTKEY}"
    DeleteRegKey HKCU "${REGKEY}"
  ${EndIf}
  !insertmacro REFRESH_SHELL_ICONS
  ; The apps Wave Hub installed and its settings (%LOCALAPPDATA%\WaveHub) are left alone.
SectionEnd
