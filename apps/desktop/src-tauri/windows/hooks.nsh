; Riptide NSIS hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; Clean uninstall, per user, no elevation:
;   * the three HKCU Native Messaging host keys (Chrome, Edge, Firefox) and the manifests in
;     %LOCALAPPDATA%\Riptide\native-messaging are always removed;
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

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current

    DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\${RIPTIDE_HOST_NAME}"
    DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\${RIPTIDE_HOST_NAME}"
    DeleteRegKey HKCU "Software\Mozilla\NativeMessagingHosts\${RIPTIDE_HOST_NAME}"
    RMDir /r "${RIPTIDE_DATA}\native-messaging"

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
