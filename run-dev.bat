@echo off
REM Spusti Weave ve vyvojovem rezimu s vestavenou CUDA GPU inferenci (llama.cpp).
REM
REM Predpoklady overene na tomto stroji:
REM   - CUDA Toolkit 13.2  (12.6 odmita novejsi MSVC/VS -- viz poznamka nize)
REM   - CMake, Visual Studio (MSVC) s C++ workloadem
REM   - NVIDIA GPU (RTX 3090, sm_86)
REM
REM Pokud mas jinou architekturu GPU, uprav CMAKE_CUDA_ARCHITECTURES nize:
REM   RTX 30xx = 86, RTX 40xx = 89, RTX 20xx = 75, GTX 10xx = 61
REM
REM Model (.gguf) a pocet GPU vrstev se nastavuji v aplikaci:
REM   Nastaveni -> AI model -> Vestavena GPU inference

setlocal

REM --- CUDA 13.2 (12.6 selze na "unsupported Visual Studio version") ---
set "CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2"
set "CUDACXX=%CUDA_PATH%\bin\nvcc.exe"
set "PATH=%CUDA_PATH%\bin;%PATH%"

REM --- Cilova GPU architektura (RTX 3090 = 86) ---
set "CMAKE_CUDA_ARCHITECTURES=86"

REM --- sqlx pouziva commitnutou offline cache, DB se neni potreba pripojovat pri buildu ---
set "SQLX_OFFLINE=true"

echo.
echo === Weave dev (CUDA build) ===
echo CUDA_PATH=%CUDA_PATH%
echo CMAKE_CUDA_ARCHITECTURES=%CMAKE_CUDA_ARCHITECTURES%
echo.

where pnpm >nul 2>&1
if errorlevel 1 (
    echo CHYBA: 'pnpm' nebyl nalezen v PATH.
    echo Zavri toto okno, zavri VSECHNY terminaly/okna a otevri novy terminal
    echo ^(PATH se nacita jen pri otevreni okna^), pak to zkus znovu.
    pause
    endlocal
    exit /b 1
)

call pnpm tauri dev --features llm-cuda
if errorlevel 1 (
    echo.
    echo === Build/spusteni selhalo ^(kod %errorlevel%^) — viz vypis vyse ===
)

echo.
pause
endlocal
