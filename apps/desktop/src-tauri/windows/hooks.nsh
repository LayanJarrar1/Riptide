; Riptide NSIS hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; Browser bridge, per user, no elevation (HKCU only):
;   * after the files are copied (fresh install AND upgrade) `riptide-desktop.exe --register-host`
;     writes the Native Messaging manifests to %LOCALAPPDATA%\Riptide\native-messaging and sets
;     the default value of the three HKCU keys (Chrome, Edge, Firefox) to them. The ids and paths
;     come from the same Rust code the app uses at start, so nothing is duplicated here and the
;     extension works before the app was ever opened. Idempotent; an integration the user turned
;     off in Settings stays off.
;   * on uninstall `--unregister-host` removes only the keys and manifests that point at THIS
;     install's riptide-host.exe; another install's registration is left alone.
;
; Magnet and .torrent handlers (Settings, Integration), per user, no elevation:
;   * on uninstall `--unregister-handlers` removes the keys that point at THIS executable under
;     HKCU\Software\Classes (magnet, Riptide.Torrent, the .torrent entries) and gives back the
;     magnet handler another app had before Riptide took it over. Keys that point at another
;     program are never touched. It runs before the files are deleted, for silent (/S) and normal
;     uninstalls alike; nothing is removed during an update.
;
; Clean uninstall, per user, no elevation:
;   * the manifest folder is removed only when it is empty;
;   * the autostart Run value is removed (Tauri's own uninstaller does this too; kept here so it
;     survives template changes);
;   * the user is asked whether to keep the download history and settings (database, logs,
;     integration.json, and the webview storage that holds the UI preferences). Silent
;     uninstalls (/S) keep them.
; The user's downloaded files are never touched: nothing here deletes recursively outside the
; folders Riptide itself creates, and the data folder is only removed when it is empty.
; Nothing runs during an update ($UpdateMode = 1): the new version takes over the same state.

!define RIPTIDE_HOST_NAME "app.riptide.host"
!define RIPTIDE_DATA "$LOCALAPPDATA\Riptide"

!macro NSIS_HOOK_POSTINSTALL
  SetShellVarContext current
  DetailPrint "Registering the browser bridge (Chrome, Edge, Firefox)..."
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-host' $0
  ${If} $0 <> 0
    ; not fatal: the app registers again on its first start
    DetailPrint "Browser bridge registration returned $0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    ; conditional: keys and manifests of another install are not touched
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-host' $0
    ; the app is absent or half installed: nothing to ask, and nothing to fail
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-handlers' $0
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current

    ; only when empty: a manifest of another install keeps its folder
    RMDir "${RIPTIDE_DATA}\native-messaging"

    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${MAINBINARYNAME}"

    MessageBox MB_YESNO|MB_ICONQUESTION "Keep your Riptide download history and settings?$\r$\nYour downloaded files are not affected either way.$\r$\n$\r$\nهل تريد الاحتفاظ بسجل التنزيلات وإعدادات ريبتايد؟$\r$\nلن تتأثر الملفات التي نزّلتها في كلتا الحالتين." /SD IDYES IDYES riptide_keep_data
      Delete "${RIPTIDE_DATA}\riptide.sqlite"
      Delete "${RIPTIDE_DATA}\riptide.sqlite-wal"
      Delete "${RIPTIDE_DATA}\riptide.sqlite-shm"
      Delete "${RIPTIDE_DATA}\integration.json"
      RMDir /r "${RIPTIDE_DATA}\logs"
      ; webview storage (UI preferences); Tauri keeps it under the bundle identifier
      RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
      ; only when empty: a fallback downloads folder or anything else the user put there stays
      RMDir "${RIPTIDE_DATA}"
    riptide_keep_data:
  ${EndIf}
!macroend
