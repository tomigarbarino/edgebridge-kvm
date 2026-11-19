!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode == 1
    ; Tauri starts setup just before it exits. Let Windows finish releasing the
    ; old executable before the standard Restart Manager check runs.
    Sleep 2000
  ${EndIf}
!macroend
