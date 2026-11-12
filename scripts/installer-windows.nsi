# -*- coding: utf-8 -*-
Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!searchparse /file "..\src\lib.rs" 'pub const BUILD: &str = "' APP_VERSION '";'
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja"

Name "Janja"
OutFile "..\dist\janja-setup-windows.pending.exe"
InstallDir "$LOCALAPPDATA\Programs\Janja"
RequestExecutionLevel user
ManifestSupportedOS Win10
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show
BrandingText "Janja ${APP_VERSION}"

!define MUI_ICON "..\assets\janja.ico"
!define MUI_UNICON "..\assets\janja.ico"
!define MUI_WELCOMEPAGE_TEXT "Instala o Janja e suas bibliotecas de áudio e vídeo para este usuário.$\r$\n$\r$\nFeche o Janja antes de instalar ou atualizar.$\r$\n$\r$\nNão é necessário instalar MSYS2 ou GStreamer separadamente."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\janja.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "PortugueseBR"

Function .onInit
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "O Janja exige Windows 10/11 de 64 bits." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

Section "Janja"
  ClearErrors
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File /oname=janja.exe "..\dist\janja-windows.exe"
  IfErrors install_error
  WriteUninstaller "$INSTDIR\uninstall.exe"
  IfErrors install_error
  CreateDirectory "$SMPROGRAMS\Janja"
  CreateShortcut "$SMPROGRAMS\Janja\Janja.lnk" "$INSTDIR\janja.exe"
  CreateShortcut "$DESKTOP\Janja.lnk" "$INSTDIR\janja.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Janja"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "InfoCore Tech"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\janja.exe",0'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  IfErrors install_error
  Goto installed
install_error:
  MessageBox MB_OK|MB_ICONSTOP "Não foi possível concluir a instalação. Verifique a permissão da pasta e tente novamente." /SD IDOK
  SetErrorLevel 1
  Abort
installed:
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "Uninstall"
  ClearErrors
  Delete "$INSTDIR\janja.exe"
  IfErrors uninstall_error
  Delete "$INSTDIR\uninstall.exe"
  Delete "$DESKTOP\Janja.lnk"
  Delete "$SMPROGRAMS\Janja\Janja.lnk"
  RMDir "$SMPROGRAMS\Janja"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  # Settings and runtime cache are shared with portable copies; preserve them.
  RMDir "$INSTDIR"
  Goto uninstalled
uninstall_error:
  MessageBox MB_OK|MB_ICONSTOP "Feche o Janja e tente desinstalar novamente." /SD IDOK
  SetErrorLevel 1
  Abort
uninstalled:
SectionEnd
