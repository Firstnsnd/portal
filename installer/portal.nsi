; Portal NSIS installer.
;
; Invoked by scripts/build-windows.ps1:
;   makensis /DARCH=x86_64 /DVERSION=0.19.0 /DTARGET=x86_64-pc-windows-msvc installer/portal.nsi
;
; ARM64 note: NSIS always produces an x86/x64 installer shell; on ARM64
; Windows it runs fine under emulation and simply copies the aarch64
; portal.exe into $PROGRAMFILES64 as a plain file — no LIBRARYARCHITECTURE
; switch needed.

Unicode true

!define APPNAME "Portal"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Portal"

Name "${APPNAME} ${VERSION}"
OutFile "..\Portal-windows-${ARCH}-v${VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\${APPNAME}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\portal.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; English is the only bundled NSIS language file on a stock install.
!insertmacro MUI_LANGUAGE "English"

Section "Install"
    SetOutPath $INSTDIR
    File "/oname=portal.exe" "..\target\${TARGET}\release\portal.exe"

    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\${APPNAME}"
    CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\portal.exe"
    CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
    ; Desktop shortcut
    CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\portal.exe"

    ; Add/Remove Programs entry
    WriteRegStr HKLM "${UNINSTKEY}" "DisplayName" "${APPNAME}"
    WriteRegStr HKLM "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\portal.exe"
    WriteRegStr HKLM "${UNINSTKEY}" "Publisher" "Portal"
    WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
    WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    Delete "$INSTDIR\portal.exe"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
    Delete "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk"
    RMDir "$SMPROGRAMS\${APPNAME}"
    Delete "$DESKTOP\${APPNAME}.lnk"
    DeleteRegKey HKLM "${UNINSTKEY}"
SectionEnd
