; Standalone per-user beta: never upgrades or removes the regular installation.
Unicode true
ManifestDPIAware true
SetCompressor /SOLID bzip2
RequestExecutionLevel user
Name "GitNexia Beta"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\GitNexia Beta"
InstallDirRegKey HKCU "Software\GitNexia Beta" "InstallLocation"
Icon "${ICON}"
UninstallIcon "${ICON}"
!include "MUI2.nsh"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_LICENSE "${STAGE}\BETA.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  ; A custom destination must not overwrite the regular CEF app or unrelated files.
  IfFileExists "$INSTDIR\GitNexia.exe" refuse
  IfFileExists "$INSTDIR\GitNex.exe" refuse
  IfFileExists "$INSTDIR\GitNexiaBeta.exe" 0 copy
  ClearErrors
  FileOpen $0 "$INSTDIR\GitNexiaBeta.exe" a
  IfErrors refuse
  FileClose $0
  copy:
  SetOutPath "$INSTDIR"
  File "${STAGE}\GitNexiaBeta.exe"
  File "${STAGE}\BETA.txt"
  SetOutPath "$INSTDIR\licenses"
  File "${STAGE}\licenses\*.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\GitNexia Beta.lnk" "$INSTDIR\GitNexiaBeta.exe"
  WriteRegStr HKCU "Software\GitNexia Beta" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitNexia Beta" "DisplayName" "GitNexia Beta"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitNexia Beta" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitNexia Beta" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitNexia Beta" "InstallLocation" "$INSTDIR"
  Goto done
  refuse:
  MessageBox MB_OK|MB_ICONSTOP "Choose a separate Beta folder and close any running Beta before installing."
  SetErrorLevel 2
  Abort
  done:
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\GitNexiaBeta.exe"
  IfFileExists "$INSTDIR\GitNexiaBeta.exe" 0 remove
  MessageBox MB_OK|MB_ICONSTOP "Close GitNexia Beta before uninstalling."
  SetErrorLevel 2
  Abort
  remove:
  Delete "$INSTDIR\BETA.txt"
  Delete "$INSTDIR\licenses\LICENSE.txt"
  Delete "$INSTDIR\licenses\THIRD-PARTY-NOTICES-rust.txt"
  Delete "$INSTDIR\licenses\THIRD-PARTY-NOTICES-ui.txt"
  RMDir "$INSTDIR\licenses"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\GitNexia Beta.lnk"
  DeleteRegKey HKCU "Software\GitNexia Beta"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitNexia Beta"
SectionEnd
