@echo off
chcp 65001 >nul 2>&1
setlocal

echo ============================================
echo   CN-Codex Build Script
echo ============================================
echo.

cd /d "%~dp0"

if not exist "logs" mkdir logs
for /f %%i in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "BUILD_TS=%%i"
if not defined BUILD_TS set "BUILD_TS=fallback_%RANDOM%"
set "LOGFILE=logs\build_%BUILD_TS%.log"

echo [%date% %time%] === Build started === > "%LOGFILE%"

REM Check codex availability
where codex.exe >nul 2>&1
if errorlevel 1 (
    if defined CN_CODEX_EXE (
        echo [INFO] Using CN_CODEX_EXE=%CN_CODEX_EXE%
        echo [%date% %time%] Using CN_CODEX_EXE=%CN_CODEX_EXE% >> "%LOGFILE%"
    ) else if defined CODEX_CLI_PATH (
        echo [INFO] Using CODEX_CLI_PATH=%CODEX_CLI_PATH%
        echo [%date% %time%] Using CODEX_CLI_PATH=%CODEX_CLI_PATH% >> "%LOGFILE%"
    ) else (
        echo [WARN] codex.exe not found in PATH.
        echo [WARN] Set CN_CODEX_EXE or CODEX_CLI_PATH for runtime.
        echo [%date% %time%] WARN: codex.exe not in PATH >> "%LOGFILE%"
    )
) else (
    echo [INFO] codex.exe found in PATH.
    echo [%date% %time%] codex.exe found in PATH >> "%LOGFILE%"
)

echo.
if defined TAURI_BUNDLES (
    set "TAURI_BUILD_ARGS=--bundles %TAURI_BUNDLES%"
    echo [INFO] Bundle targets: %TAURI_BUNDLES%
    echo [%date% %time%] Bundle targets=%TAURI_BUNDLES% >> "%LOGFILE%"
) else (
    set "TAURI_BUILD_ARGS=--no-bundle"
    if not defined CARGO_PROFILE_RELEASE_LTO set "CARGO_PROFILE_RELEASE_LTO=thin"
    if not defined CARGO_PROFILE_RELEASE_CODEGEN_UNITS set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8"
    echo [INFO] Bundle targets: none ^(fast mode^).
    echo [INFO] Set TAURI_BUNDLES=nsis or msi if installer packages are needed.
    echo [INFO] Fast release profile defaults: lto=thin, codegen-units=8 ^(override via env^).
    echo [%date% %time%] Bundle targets=none ^(fast mode^) >> "%LOGFILE%"
    echo [%date% %time%] Fast release profile defaults: lto=thin, codegen-units=8 >> "%LOGFILE%"
)

echo.
echo [1/3] Building Tauri app (includes frontend build)...
echo [%date% %time%] [1/3] Building Tauri app >> "%LOGFILE%"
call pnpm tauri build %TAURI_BUILD_ARGS% >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [ERROR] Tauri build failed! See %LOGFILE%
    echo [HINT] Fast mode uses --no-bundle. Use TAURI_BUNDLES only when you need installers.
    echo [%date% %time%] ERROR: Tauri build failed ^(args=%TAURI_BUILD_ARGS%^) >> "%LOGFILE%"
    pause
    exit /b 1
)
echo       OK

echo.
echo [2/3] Copying artifacts to build/...
echo [%date% %time%] [2/3] Copying artifacts >> "%LOGFILE%"
if not exist "build" mkdir build
copy /Y src-tauri\target\release\cn-codex.exe build\ >nul 2>&1
if defined TAURI_BUNDLES (
    if exist "src-tauri\target\release\bundle\nsis\*.exe" copy /Y src-tauri\target\release\bundle\nsis\*.exe build\ >nul 2>&1
    if exist "src-tauri\target\release\bundle\msi\*.msi" copy /Y src-tauri\target\release\bundle\msi\*.msi build\ >nul 2>&1
)
echo       OK

echo.
echo [3/3] Launching cn-codex.exe...
echo [%date% %time%] [3/3] Launching cn-codex.exe >> "%LOGFILE%"
if exist "build\cn-codex.exe" (
    echo [%date% %time%] === Build complete, launching app === >> "%LOGFILE%"
    start "" "build\cn-codex.exe"
    echo       Launched!
) else (
    echo [ERROR] build\cn-codex.exe not found after build.
    echo [%date% %time%] ERROR: cn-codex.exe not found >> "%LOGFILE%"
)

echo.
echo ============================================
echo   Build complete!
echo   EXE:  build\cn-codex.exe
echo   Log:  %LOGFILE%
echo ============================================
echo.
dir build\*.exe 2>nul
echo.
pause
