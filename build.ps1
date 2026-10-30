$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$cargo = Join-Path $PSScriptRoot "scripts\cargo-msvc.bat"

cmd /c "`"$cargo`" test"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

cmd /c "`"$cargo`" build --release --features gui --bin LocalBot"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force -Path dist | Out-Null
Copy-Item -Force .\target\release\LocalBot.exe .\dist\LocalBot.exe
Write-Host "已生成 dist\LocalBot.exe"
