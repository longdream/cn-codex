@echo off
chcp 65001 >nul 2>&1
setlocal

echo ============================================
echo   CN-Codex UI Test Runner
echo ============================================
echo.

cd /d D:\rustwork\cn-codex
if not exist "logs" mkdir logs

REM Set proxy from Windows system settings (127.0.0.1:7897)
REM Change this if your proxy address is different
if not defined HTTPS_PROXY (
    set "HTTP_PROXY=http://127.0.0.1:7897"
    set "HTTPS_PROXY=http://127.0.0.1:7897"
    echo [INFO] Proxy set to 127.0.0.1:7897
)

REM Check Python
python --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Python not found. Install Python 3.10+ first.
    pause
    exit /b 1
)

REM Check Playwright
python -c "import playwright" >nul 2>&1
if errorlevel 1 (
    echo [INFO] Installing Playwright (via proxy)...
    pip install playwright
    if errorlevel 1 (
        echo [ERROR] Failed to install Playwright.
        pause
        exit /b 1
    )
)

REM Test uses system Chrome first, only install Playwright Chromium as fallback
echo [INFO] Test will use system Chrome/Edge if available.
echo [INFO] Proxy: %HTTPS_PROXY%
echo.

echo [1/3] Starting Vite dev server...
start /B "vite-dev" cmd /c "pnpm dev > logs\vite-test.log 2>&1"

echo Waiting for dev server on http://localhost:1420...
set RETRIES=0
:wait_loop
if %RETRIES% GEQ 30 (
    echo [ERROR] Dev server did not start within 30 seconds.
    taskkill /FI "WINDOWTITLE eq vite-dev" /F >nul 2>&1
    pause
    exit /b 1
)
timeout /t 1 /nobreak >nul
curl -s -o nul http://localhost:1420 >nul 2>&1
if errorlevel 1 (
    set /A RETRIES+=1
    goto wait_loop
)
echo Dev server is ready.

echo.
echo [2/3] Running UI tests...
echo.
python tests\ui\test_ui.py --url http://localhost:1420
set TEST_EXIT=%ERRORLEVEL%

echo.
echo [3/3] Stopping dev server...
taskkill /FI "WINDOWTITLE eq vite-dev" /F >nul 2>&1
for /f "tokens=5" %%a in ('netstat -aon ^| findstr :1420 ^| findstr LISTENING') do (
    taskkill /PID %%a /F >nul 2>&1
)

echo.
if %TEST_EXIT% EQU 0 (
    echo ============================================
    echo   ALL TESTS PASSED
    echo ============================================
) else (
    echo ============================================
    echo   SOME TESTS FAILED
    echo ============================================
)
echo.
echo Screenshots: tests\ui\screenshots\
echo Log:         tests\ui\test_results.log
echo.
pause
exit /b %TEST_EXIT%
