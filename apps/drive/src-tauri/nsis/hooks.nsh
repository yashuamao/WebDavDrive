/*
  WebDAV Drive - NSIS installer hooks.

  ASCII only on purpose: makensis reads this file with the ANSI code page, so
  non-ASCII comments would arrive as mojibake.

  Why this file exists: the Tauri NSIS template restores the remembered install
  directory in .onInit (RestorePreviousInstallLocation) but never checks whether
  that directory still exists, and it knows nothing about installs made by
  earlier versions. MUI calls MUI_CUSTOMFUNCTION_GUIINIT from .onGUIInit, which
  runs after .onInit and before the first page is shown, so $INSTDIR can still
  be corrected here - the directory page then opens on the previous location.

  The !define below must be seen before !include "MUI2.nsh" consumes it; the
  bundler includes this hook file before the template body.
*/

!define MUI_CUSTOMFUNCTION_GUIINIT WebDavDriveGuiInit

Var WebDavDriveDir
Var WebDavDriveHit

; Input: $WebDavDriveDir. Output: $WebDavDriveHit = "1" when that directory
; holds an installed copy of the app.
Function WebDavDriveCheckDir
  StrCpy $WebDavDriveHit "0"
  IfFileExists "$WebDavDriveDir\drive.exe" 0 +2
  StrCpy $WebDavDriveHit "1"
FunctionEnd

Function WebDavDriveGuiInit
  ; Unattended installs (/S, used by the in-app updater) pass /D= explicitly.
  IfSilent webdav_drive_done

  StrCpy $WebDavDriveDir "$INSTDIR"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_done

  ; Location remembered by the app/installer (HKCU, currentUser install mode).
  ClearErrors
  ReadRegStr $WebDavDriveDir HKCU "Software\foundation\WebDAV Drive" ""
  StrCmp $WebDavDriveDir "" webdav_drive_next1
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  ; Default currentUser location used by the template.
webdav_drive_next1:
  StrCpy $WebDavDriveDir "$LOCALAPPDATA\WebDAV Drive"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  StrCpy $WebDavDriveDir "$LOCALAPPDATA\Programs\WebDAV Drive"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  ; Per-machine installs from earlier versions.
  StrCpy $WebDavDriveDir "$PROGRAMFILES64\WebDAV Drive"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  StrCpy $WebDavDriveDir "$PROGRAMFILES\WebDAV Drive"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  StrCpy $WebDavDriveDir "$PROGRAMFILES32\WebDAV Drive"
  Call WebDavDriveCheckDir
  StrCmp $WebDavDriveHit "1" webdav_drive_use

  Goto webdav_drive_done

webdav_drive_use:
  StrCpy $INSTDIR "$WebDavDriveDir"

webdav_drive_done:
FunctionEnd
