# Installs the latest caffeinate.exe release into %LOCALAPPDATA%\Programs\caffeinate
# and adds that folder to the user PATH.
#
#   irm https://raw.githubusercontent.com/OWNER/caffeinate-windows/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "OWNER/caffeinate-windows"
$dir = Join-Path $env:LOCALAPPDATA "Programs\caffeinate"
$exe = Join-Path $dir "caffeinate.exe"
$url = "https://github.com/$repo/releases/latest/download/caffeinate.exe"

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $exe -UseBasicParsing

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $dir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$dir", "User")
    $env:Path = "$env:Path;$dir"
    Write-Host "Added $dir to user PATH (restart other terminals to pick it up)"
}

Write-Host "Installed: $exe"
& $exe -h | Select-Object -First 1
