@echo off
chcp 65001 >nul 2>&1
setlocal enabledelayedexpansion
set "DEV_PORT=1420"

echo ============================================
echo   CN-Codex - Dev Mode
echo ============================================
echo.

cd /d "%~dp0"

if not exist "logs" mkdir logs

echo [%date% %time%] Starting dev mode...>logs\dev.log
echo [%date% %time%] Starting dev mode...

where codex.exe >nul 2>&1
if errorlevel 1 (
    if defined CN_CODEX_EXE (
        echo [INFO] Using CN_CODEX_EXE=%CN_CODEX_EXE%
        echo [%date% %time%] Using CN_CODEX_EXE=%CN_CODEX_EXE%>>logs\dev.log
    ) else if defined CODEX_CLI_PATH (
        echo [INFO] Using CODEX_CLI_PATH=%CODEX_CLI_PATH%
        echo [%date% %time%] Using CODEX_CLI_PATH=%CODEX_CLI_PATH%>>logs\dev.log
    ) else (
        echo [WARN] codex.exe not found in PATH.
        echo [WARN] Set CN_CODEX_EXE or CODEX_CLI_PATH if needed.
        echo [%date% %time%] WARN: codex.exe not in PATH>>logs\dev.log
    )
) else (
    echo [INFO] codex.exe found in PATH.
    echo [%date% %time%] codex.exe found in PATH>>logs\dev.log
)

echo.
echo Checking Vite port %DEV_PORT%...
echo [%date% %time%] Checking Vite port %DEV_PORT%>>logs\dev.log
set "PORT_WAS_BUSY=0"
for /f "tokens=5" %%p in ('netstat -aon ^| findstr /R /C:":%DEV_PORT% .*LISTENING"') do (
    set "PORT_WAS_BUSY=1"
    echo [WARN] Port %DEV_PORT% is already in use by PID %%p. Killing it...
    echo [%date% %time%] WARN: Port %DEV_PORT% in use by PID %%p. Killing it...>>logs\dev.log
    taskkill /PID %%p /F /T>>logs\dev.log 2>&1
    if errorlevel 1 (
        echo [WARN] Failed to kill PID %%p. It may have already exited.
        echo [%date% %time%] WARN: Failed to kill PID %%p>>logs\dev.log
    )
)
if "%PORT_WAS_BUSY%"=="0" (
    echo [INFO] Port %DEV_PORT% is free.
    echo [%date% %time%] Port %DEV_PORT% is free>>logs\dev.log
) else (
    timeout /t 1 /nobreak >nul
    for /f "tokens=5" %%p in ('netstat -aon ^| findstr /R /C:":%DEV_PORT% .*LISTENING"') do (
        echo [ERROR] Port %DEV_PORT% is still in use by PID %%p.
        echo [%date% %time%] ERROR: Port %DEV_PORT% still in use by PID %%p>>logs\dev.log
        pause
        exit /b 1
    )
)

:: Ensure embedded Node.js is available for online tools (hyperframes etc.)
echo.
echo Checking embedded Node.js for online tools...
echo [%date% %time%] Checking embedded Node.js>>logs\dev.log
if not exist "codey\node\node.exe" (
    set "NODE_VERSION=22.16.0"
    set "NODE_ARCHIVE=node-v22.16.0-win-x64.zip"
    set "NODE_URL=https://nodejs.org/dist/v22.16.0/node-v22.16.0-win-x64.zip"
    set "NODE_CACHE=tools\node-cache"
    if not exist "!NODE_CACHE!" mkdir "!NODE_CACHE!"
    if not exist "!NODE_CACHE!\!NODE_ARCHIVE!" (
        echo [INFO] Downloading Node.js v22.16.0 portable...
        echo [%date% %time%] Downloading Node.js v22.16.0>>logs\dev.log
        powershell -Command "Invoke-WebRequest -Uri '!NODE_URL!' -OutFile '!NODE_CACHE!\!NODE_ARCHIVE!'" 2>nul
        if errorlevel 1 (
            echo [WARN] Failed to download Node.js. Online tools will use system Node if available.
            echo [%date% %time%] WARN: Node.js download failed>>logs\dev.log
            goto :skip_node_setup
        )
    ) else (
        echo [INFO] Using cached Node.js archive.
    )
    echo [INFO] Extracting Node.js to codey\node\...
    set "NODE_TMP=codey\_node_tmp"
    powershell -Command "Expand-Archive -Path '!NODE_CACHE!\!NODE_ARCHIVE!' -DestinationPath '!NODE_TMP!' -Force" 2>nul
    if errorlevel 1 (
        echo [WARN] Failed to extract Node.js archive.
        echo [%date% %time%] WARN: Node.js extraction failed>>logs\dev.log
        if exist "!NODE_TMP!" rmdir /s /q "!NODE_TMP!"
        goto :skip_node_setup
    )
    set "NODE_EXTRACTED=!NODE_TMP!\node-v22.16.0-win-x64"
    mkdir "codey\node" 2>nul
    copy "!NODE_EXTRACTED!\node.exe" "codey\node\" >nul
    copy "!NODE_EXTRACTED!\npm" "codey\node\" >nul 2>&1
    copy "!NODE_EXTRACTED!\npm.cmd" "codey\node\" >nul 2>&1
    copy "!NODE_EXTRACTED!\npx" "codey\node\" >nul 2>&1
    copy "!NODE_EXTRACTED!\npx.cmd" "codey\node\" >nul 2>&1
    if exist "!NODE_EXTRACTED!\node_modules" (
        xcopy "!NODE_EXTRACTED!\node_modules" "codey\node\node_modules\" /E /I /Q /Y >nul
    )
    if exist "!NODE_TMP!" rmdir /s /q "!NODE_TMP!"
    echo [INFO] Embedded Node.js v22.16.0 ready at codey\node\
    echo [%date% %time%] Embedded Node.js ready>>logs\dev.log
) else (
    echo [INFO] Embedded Node.js already present at codey\node\node.exe
    echo [%date% %time%] Embedded Node.js already present>>logs\dev.log
)
:skip_node_setup

echo.
echo Configuring Rust low-memory dev profile...
if not defined CARGO_BUILD_JOBS set "CARGO_BUILD_JOBS=1"
if not defined CARGO_PROFILE_DEV_CODEGEN_UNITS set "CARGO_PROFILE_DEV_CODEGEN_UNITS=4"
if not defined CARGO_PROFILE_DEV_DEBUG set "CARGO_PROFILE_DEV_DEBUG=1"
echo [INFO] CARGO_BUILD_JOBS=%CARGO_BUILD_JOBS%, CARGO_PROFILE_DEV_CODEGEN_UNITS=%CARGO_PROFILE_DEV_CODEGEN_UNITS%, CARGO_PROFILE_DEV_DEBUG=%CARGO_PROFILE_DEV_DEBUG%
echo [%date% %time%] Rust dev profile: jobs=%CARGO_BUILD_JOBS% codegen-units=%CARGO_PROFILE_DEV_CODEGEN_UNITS% debug=%CARGO_PROFILE_DEV_DEBUG%>>logs\dev.log

echo.
echo Starting Tauri dev server (Vite HMR on http://localhost:%DEV_PORT%)...
echo Log: logs\dev.log
echo Press Ctrl+C to stop.
echo.

echo [%date% %time%] Running pnpm tauri dev>>logs\dev.log
call pnpm tauri dev
if errorlevel 1 (
    echo.
    echo [ERROR] Dev server failed. Check logs\dev.log for details.
    echo [%date% %time%] ERROR: Dev server failed>>logs\dev.log
    pause
    exit /b 1
)
