@echo off
cd /d "%~dp0"
if not exist "dist\LocalBot.exe" (
  echo 尚未生成程序，正在编译...
  powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build.ps1"
  if errorlevel 1 exit /b 1
)
start "" "%~dp0dist\LocalBot.exe"
