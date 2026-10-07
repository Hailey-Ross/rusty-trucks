@echo off
rem Skate launcher from a keyboard (Steam uses SkateLauncher.exe instead; see README.md).
rem   No argument: the menu (arrows / Enter / Esc, or a controller's D-pad / A / B).
rem   With arguments: run an entry directly, e.g.  launcher.bat rust-play -Version main
rem   launcher.bat list  prints every entry; launcher.bat versions  lists the versions.
title Skate launcher
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0launcher.ps1" %*
