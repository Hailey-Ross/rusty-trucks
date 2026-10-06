@echo off
rem Starts skate3rust.exe and saves its session log in the logs folder next to it.
rem Put this file in the folder that holds skate3rust.exe and double-click it.
cd /d "%~dp0"
if not exist "skate3rust.exe" (
    echo skate3rust.exe is not in this folder. Put PLAY-WITH-LOG.bat next to skate3rust.exe.
    pause
    exit /b 1
)
if not exist "logs" mkdir "logs"
for /f %%t in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd-HHmmss"') do set "STAMP=%%t"
set "LOG=logs\game-%STAMP%.log"
set "ERRLOG=logs\game-%STAMP%.stderr.log"
echo Playing with logging. The session log is saved to:
echo   %CD%\%ERRLOG%
powershell -NoProfile -Command "Start-Process -FilePath '.\skate3rust.exe' -RedirectStandardOutput '%LOG%' -RedirectStandardError '%ERRLOG%' -Wait"
echo.
echo Session finished. Attach this file to your report:
echo   %CD%\%ERRLOG%
echo Crash reports (if the game closed on its own) are in:
echo   %LOCALAPPDATA%\Skate3RustEngine\CrashReports
pause
