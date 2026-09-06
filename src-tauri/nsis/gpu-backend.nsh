; Výběr GPU backendu při instalaci.
;
; llama.cpp má backend zakompilovaný napevno, takže jedna binárka neumí
; přepnout CUDA/Vulkan za běhu. Instalátor proto nese obě verze a tenhle hook
; po rozbalení souborů nechá na disku jen tu, která na stroji dává smysl:
;
;   NVIDIA >= 20 GB VRAM   -> CUDA (3090, 4090, 5090 - model se vejde celý do
;                             VRAM a CUDA tam znatelně zrychlí prompt)
;   NVIDIA 12-19 GB VRAM   -> zeptat se (u karet, kde se velký model do VRAM
;                             nevejde, je přínos CUDY menší než 485 MB navíc)
;   cokoli jiného          -> Vulkan (AMD, Intel, slabší NVIDIA)
;
; Volba se pamatuje v registru, aby se aktualizace (updater instaluje tiše)
; neptaly znovu a nepřepsaly CUDA verzi zpátky Vulkanem.

!include LogicLib.nsh

!define WEAVE_REGKEY "Software\Weave"
!define WEAVE_CUDA_AUTO_MB 20000   ; >= 20 GB -> CUDA bez ptaní
!define WEAVE_CUDA_ASK_MB  12000   ; >= 12 GB -> nabídnout volbu

!macro NSIS_HOOK_POSTINSTALL
  ; Instalátor bez CUDA větve (release.yml staví jen Vulkan) nemá co vybírat.
  ; Bez téhle pojistky by hook smazal weave-app.exe a nahradil ho souborem,
  ; který v balíčku není — tedy rozbil instalaci.
  IfFileExists "$INSTDIR\cuda\weave-app.exe" 0 weave_no_payload

  Push $0
  Push $1
  Push $2

  StrCpy $2 ""

  ; 1) Volba z předchozí instalace má přednost — updater běží tiše a nesmí
  ;    uživateli CUDA verzi potichu vyměnit za Vulkan.
  ReadRegStr $0 HKCU "${WEAVE_REGKEY}" "GpuBackend"
  ${If} $0 == "cuda"
    StrCpy $2 "cuda"
  ${ElseIf} $0 == "vulkan"
    StrCpy $2 "vulkan"
  ${EndIf}
  ${If} $2 != ""
    Goto weave_apply
  ${EndIf}

  ; 2) Detekce karty. nvidia-smi je součástí ovladače NVIDIE (System32),
  ;    takže jeho úspěšné spuštění je zároveň důkaz, že NVIDIA v stroji je.
  nsExec::ExecToStack '"nvidia-smi" --query-gpu=memory.total --format=csv,noheader,nounits'
  Pop $0   ; návratový kód
  Pop $1   ; VRAM v MiB, u víc karet víc řádků
  ${If} $0 != "0"
    StrCpy $2 "vulkan"
    Goto weave_apply
  ${EndIf}

  ; IntOp přečte číslo ze začátku řetězce, takže odřízne "\r\n" i další karty.
  IntOp $1 $1 + 0

  ${If} $1 >= ${WEAVE_CUDA_AUTO_MB}
    StrCpy $2 "cuda"
  ${ElseIf} $1 >= ${WEAVE_CUDA_ASK_MB}
    ; Tichá instalace (aktualizace) se ptát nemůže — bereme bezpečnější Vulkan.
    IfSilent 0 weave_ask
      StrCpy $2 "vulkan"
      Goto weave_apply
    weave_ask:
    IntOp $0 $1 / 1024
    MessageBox MB_YESNO|MB_ICONQUESTION \
      "Nalezena NVIDIA s $0 GB VRAM.$\n$\nPoužít akceleraci CUDA? Je na NVIDII rychlejší, ale zabere o 485 MB víc místa.$\nNe = Vulkan (menší, funguje všude).$\n$\nFound an NVIDIA GPU with $0 GB VRAM. Use CUDA (faster, 485 MB larger)? No = Vulkan." \
      IDYES weave_pick_cuda
      StrCpy $2 "vulkan"
      Goto weave_apply
    weave_pick_cuda:
      StrCpy $2 "cuda"
  ${Else}
    StrCpy $2 "vulkan"
  ${EndIf}

weave_apply:
  ${If} $2 == "cuda"
    ; CUDA verze i s cuBLAS DLL se přesune nad Vulkan build.
    Delete "$INSTDIR\weave-app.exe"
    Rename "$INSTDIR\cuda\weave-app.exe" "$INSTDIR\weave-app.exe"
    ; Wildcard, aby změna verze CUDA (cublas64_13 -> _14) nevyžadovala zásah.
    CopyFiles /SILENT "$INSTDIR\cuda\*.dll" "$INSTDIR"
    DetailPrint "Weave: nainstalována CUDA verze (NVIDIA)"
  ${Else}
    DetailPrint "Weave: nainstalována Vulkan verze"
  ${EndIf}
  RMDir /r "$INSTDIR\cuda"
  WriteRegStr HKCU "${WEAVE_REGKEY}" "GpuBackend" "$2"

  Pop $2
  Pop $1
  Pop $0

weave_no_payload:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; CUDA DLL se při instalaci přesunuly z cuda\ do kořene, takže je seznam
  ; souborů odinstalátoru nezná — bez tohohle by po odinstalaci zůstalo
  ; skoro půl giga.
  Delete "$INSTDIR\cudart64_*.dll"
  Delete "$INSTDIR\cublas64_*.dll"
  Delete "$INSTDIR\cublasLt64_*.dll"
  RMDir /r "$INSTDIR\cuda"
  DeleteRegValue HKCU "${WEAVE_REGKEY}" "GpuBackend"
  DeleteRegKey /ifempty HKCU "${WEAVE_REGKEY}"
!macroend
