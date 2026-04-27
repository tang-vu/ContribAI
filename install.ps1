$Version = "v6.5.0"
$Repo = "tang-vu/ContribAI"
$Binary = "contribai-windows-x86_64.exe"
$InstallDir = "$env:USERPROFILE\.contribai\bin"
$Url = "https://github.com/$Repo/releases/download/$Version/$Binary"

Write-Host "Installing ContribAI $Version for Windows..." -ForegroundColor Cyan
Write-Host "  Downloading: $Url"

if (-not (Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null }

$OutPath = Join-Path $InstallDir "contribai.exe"
Invoke-WebRequest -Uri $Url -OutFile $OutPath -UseBasicParsing

# Add to user PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$UserPath;$InstallDir", "User")
    Write-Host "  Added $InstallDir to PATH" -ForegroundColor Green
}

Write-Host ""
Write-Host "ContribAI installed successfully!" -ForegroundColor Green
Write-Host "  Location: $OutPath"
Write-Host "  Restart your terminal, then run: contribai init" -ForegroundColor Yellow
