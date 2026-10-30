@echo off
setlocal
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
set "SDK=%LOCALAPPDATA%\LocalBot\winsdk\pkg\c"
if exist "%SDK%\um\x64" set "LIB=%SDK%\um\x64;%SDK%\ucrt\x64;%LIB%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
cargo %*
exit /b %ERRORLEVEL%
