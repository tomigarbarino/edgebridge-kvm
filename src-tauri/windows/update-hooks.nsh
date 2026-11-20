!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode == 1
    Push $0
    Push $1
    Push $2
    Push $3
    Push $R4
    Push $R5

    ; Antivirus can retain the old executable after the app exits. Wait for
    ; Restart Manager to report that it is free, then run Tauri's usual check.
    StrCpy $R5 0
    ${Do}
      ; Use a fresh session so Restart Manager re-enumerates file holders.
      !insertmacro RestartManager_StartSession $R4
      ${If} $R4 == ""
        ${ExitDo}
      ${EndIf}
      !insertmacro RestartManager_RegisterFile $R4 "$INSTDIR\${MAINBINARYNAME}.exe"
      ${If} $0 = 0
        System::Call 'RSTRTMGR::RmGetList(i R4, *i .r1, *i 0 .r2, p 0, *i .r3) i .r0'
      ${EndIf}
      !insertmacro RestartManager_EndSession $R4
      ${If} $0 <> ${ERROR_MORE_DATA}
      ${OrIf} $R5 >= 120
        ${ExitDo}
      ${EndIf}
      ${If} $R5 = 0
        DetailPrint "Waiting for Windows to release EdgeBridge KVM."
      ${EndIf}
      Sleep 500
      IntOp $R5 $R5 + 1
    ${Loop}

    Pop $R5
    Pop $R4
    Pop $3
    Pop $2
    Pop $1
    Pop $0
  ${EndIf}
!macroend
