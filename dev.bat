@echo off
chcp 65001 >nul 2>&1
setlocal
set "DEV_PORT=1420"

echo ============================================
echo   CN-Codex - Dev Mode
echo ============================================
echo.

cd /d D:\rustwork\cn-codex

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
