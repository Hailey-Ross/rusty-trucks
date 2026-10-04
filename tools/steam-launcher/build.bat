@echo off
rem Builds SkateLauncher.exe next to this file with the C# compiler that ships with Windows (.NET Framework 4).
cd /d "%~dp0"
"%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /out:SkateLauncher.exe SkateLauncher.cs
if errorlevel 1 (echo Build failed. & pause & exit /b 1)
echo Built %~dp0SkateLauncher.exe
