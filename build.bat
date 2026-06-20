@echo off
chcp 65001 >nul 2>&1
setlocal

echo ============================================
echo   CN-Codex Build Script
echo ============================================
echo.

cd /d D:\rustwork\cn-codex

if not exist "logs" mkdir logs
set "LOGFILE=logs\build_%date:~0,4%%date:~5,2%%date:~8,2%.log"

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
echo [1/4] Building frontend...
echo [%date% %time%] [1/4] Building frontend >> "%LOGFILE%"
call pnpm build >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [ERROR] Frontend build failed! See %LOGFILE%
    echo [%date% %time%] ERROR: Frontend build failed >> "%LOGFILE%"
    pause
    exit /b 1
)
echo       OK

echo.
echo [2/4] Building Tauri app...
echo [%date% %time%] [2/4] Building Tauri app >> "%LOGFILE%"
call pnpm tauri build >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [ERROR] Tauri build failed! See %LOGFILE%
    echo [%date% %time%] ERROR: Tauri build failed >> "%LOGFILE%"
    pause
    exit /b 1
)
echo       OK

echo.
echo [3/4] Copying artifacts to build/...
echo [%date% %time%] [3/4] Copying artifacts >> "%LOGFILE%"
if not exist "build" mkdir build
copy /Y src-tauri\target\release\cn-codex.exe build\ >nul 2>&1
copy /Y src-tauri\target\release\bundle\nsis\*.exe build\ >nul 2>&1
copy /Y src-tauri\target\release\bundle\msi\*.msi build\ >nul 2>&1
echo       OK

echo.
echo [4/4] Launching cn-codex.exe...
echo [%date% %time%] [4/4] Launching cn-codex.exe >> "%LOGFILE%"
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
