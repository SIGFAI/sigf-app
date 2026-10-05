; SIGF installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; The app keeps its data in %LOCALAPPDATA%\SIGF (SIGF_HOME): installed.json, privacy.json, the download cache, and
; snapshots\ with the original game files every install replaced. The program itself goes to
; %LOCALAPPDATA%\Programs\SIGF ("Install folder" below), so the two never share a folder. Tauri's uninstaller never
; deletes SIGF_HOME: it removes only its own files from the install folder (a non-recursive RMDir) and, when "Delete
; the application data" is ticked, %APPDATA%\ai.sigf.app and %LOCALAPPDATA%\ai.sigf.app (the webview profile). So the
; backups survive an uninstall. What an uninstall cannot do is put the originals back: only the app's Restore does.
; This hook says so when mods are still installed, and lets the player cancel.

; --- Install folder -----------------------------------------------------------------------------------------------
; Tauri's per-user default is $LOCALAPPDATA\<productName>, which is SIGF_HOME, and the config has no option to change
; it. So the default moves to $LOCALAPPDATA\Programs\SIGF, where per-user programs go: on the welcome page, before the
; directory page shows it, and again before the files are copied (silent and passive installs skip the pages). A
; folder the player picks is kept, except SIGF_HOME itself. The install saves $INSTDIR in the registry, so updates
; (/UPDATE restores that location) and the uninstaller (it runs from $INSTDIR) use the same folder.
!macro SIGF_FIX_INSTDIR
  StrCmp $INSTDIR "$LOCALAPPDATA\SIGF" 0 +2
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\SIGF"
!macroend

!macro NSIS_HOOK_PREINSTALL
  StrCmp $INSTDIR "$LOCALAPPDATA\SIGF" 0 sigf_instdir_ok
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\SIGF"
    SetOutPath $INSTDIR
    ; A test build that went straight into SIGF_HOME: remove its program files only, never the folder (it holds data).
    Delete "$LOCALAPPDATA\SIGF\${MAINBINARYNAME}.exe"
    Delete "$LOCALAPPDATA\SIGF\uninstall.exe"
  sigf_instdir_ok:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    IfFileExists "$LOCALAPPDATA\SIGF\installed.json" 0 sigf_no_mods
      FileOpen $R8 "$LOCALAPPDATA\SIGF\installed.json" r
      FileRead $R8 $R9 2
      FileClose $R8
      StrCmp $R9 "[]" sigf_no_mods
      StrCmp $R9 "" sigf_no_mods
      MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "SIGF still has mods installed in your games.$\r$\n$\r$\nUninstalling the app does not remove them. To get your games back to vanilla, click Cancel, open SIGF and use Restore vanilla on each mashup first.$\r$\n$\r$\nYour original game files stay backed up in $LOCALAPPDATA\SIGF\snapshots: the uninstaller never deletes that folder.$\r$\n$\r$\nClick OK to uninstall anyway." /SD IDOK IDOK sigf_no_mods
      Abort
    sigf_no_mods:
  ${EndIf}
!macroend

; --- Privacy (docs/PRIVACY.md) ------------------------------------------------------------------------------------
; SignPath Foundation's rule: software that sends data to systems the user did not pick shows its privacy policy during
; installation and offers options to turn it off. The license page carries the AGPL (bundle.licenseFile), and a hooks
; file cannot add a page in the middle of Tauri's list, so the privacy text goes on the welcome page: these defines are
; read by the template's MUI_PAGE_WELCOME, the first page it declares (nothing before it uses them). Its show callback
; swaps the welcome text for windows/privacy.txt in a scrolling box and a link to the full policy. The option is the
; question in NSIS_HOOK_POSTINSTALL below; the app asks again on its first start.
!define MUI_WELCOMEPAGE_TEXT "-"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW SigfPrivacyShow
!define SIGF_PRIVACY_URL "https://sigf.ai/privacy"

Var SigfPrivacyBox

Function SigfPrivacyShow
  !insertmacro SIGF_FIX_INSTDIR
  ; MUI's own variables and colors are declared after this file is included, so: the welcome text label is found by its
  ; text ("-", above) and hidden, and the colors are MUI2's welcome page defaults (text 000000 on FFFFFF).
  FindWindow $0 "#32770" "" $HWNDPARENT
  FindWindow $0 "Static" "-" $0
  ShowWindow $0 ${SW_HIDE}
  ${NSD_CreateLabel} 120u 45u 195u 18u "Before you install, please read how SIGF uses the network and what you can turn off."
  Pop $0
  SetCtlColors $0 "000000" "FFFFFF"
  nsDialogs::CreateControl EDIT ${DEFAULT_STYLES}|${WS_TABSTOP}|${WS_VSCROLL}|${ES_MULTILINE}|${ES_READONLY}|${ES_AUTOVSCROLL} ${WS_EX_CLIENTEDGE} 120u 66u 195u 106u ""
  Pop $SigfPrivacyBox
  SetCtlColors $SigfPrivacyBox "000000" "FFFFFF"
  ; The text ships inside the installer (ASCII, read line by line so no line meets NSIS's string limit).
  InitPluginsDir
  File "/oname=$PLUGINSDIR\sigf-privacy.txt" "${__FILEDIR__}\privacy.txt"
  FileOpen $1 "$PLUGINSDIR\sigf-privacy.txt" r
  sigf_privacy_line:
    FileRead $1 $2
    StrCmp $2 "" sigf_privacy_eof
    ; Whatever the checkout's line ends (LF or CRLF), the edit box wants CRLF.
    StrCpy $4 $2 1 -1
    StrCmp $4 "$\n" 0 +2
      StrCpy $2 $2 -1
    StrCpy $4 $2 1 -1
    StrCmp $4 "$\r" 0 +2
      StrCpy $2 $2 -1
    StrCpy $2 "$2$\r$\n"
    SendMessage $SigfPrivacyBox ${WM_GETTEXTLENGTH} 0 0 $3
    SendMessage $SigfPrivacyBox ${EM_SETSEL} $3 $3
    SendMessage $SigfPrivacyBox ${EM_REPLACESEL} 0 "STR:$2"
    Goto sigf_privacy_line
  sigf_privacy_eof:
  FileClose $1
  SendMessage $SigfPrivacyBox ${EM_SETSEL} 0 0
  ${NSD_CreateLink} 120u 176u 195u 10u "Read the full privacy policy: ${SIGF_PRIVACY_URL}"
  Pop $0
  SetCtlColors $0 "0000FF" "FFFFFF"
  ${NSD_OnClick} $0 SigfPrivacyOpen
FunctionEnd

Function SigfPrivacyOpen
  ExecShell "open" "${SIGF_PRIVACY_URL}"
FunctionEnd

; The first answer to the optional requests, for the app's first-start screen (<SIGF_HOME>\privacy.json, read by
; app/src-tauri/src/privacy.rs). "asked" stays false, so the app still shows each choice before it goes online. Not on
; an update, a silent or passive install, or when the file is already there (the player's own choices).
!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $PassiveMode <> 1
    IfSilent sigf_privacy_done
    IfFileExists "$LOCALAPPDATA\SIGF\privacy.json" sigf_privacy_done
      StrCpy $R7 "true"
      MessageBox MB_YESNO|MB_ICONQUESTION "Allow SIGF's optional online requests?$\r$\n$\r$\n- Game pictures from Steam, Epic Games and Modrinth image servers$\r$\n- The Steam store search, with the name of a game that has no picture$\r$\n- The games you own, sent to sigf.ai to list lobbies you can join$\r$\n$\r$\nYes: allow them. No: turn them all off.$\r$\nSIGF shows each choice again when it first starts, and under Privacy in the app." /SD IDYES IDYES sigf_privacy_write
      StrCpy $R7 "false"
    sigf_privacy_write:
      CreateDirectory "$LOCALAPPDATA\SIGF"
      FileOpen $R8 "$LOCALAPPDATA\SIGF\privacy.json" w
      FileWrite $R8 '{"asked":false,"storeArt":$R7,"artSearch":$R7,"lobbyGames":$R7,"lanAddress":"ask"}'
      FileClose $R8
    sigf_privacy_done:
  ${EndIf}
!macroend
