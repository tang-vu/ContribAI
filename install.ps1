$Version = "v6.8.0"
$ErrorActionPreference = "Stop"
$Repo = "tang-vu/ContribAI"
$Binary = "contribai-$Version-windows-x86_64.exe"
$ExpectedSha256 = "7450ac2fe313193fd68c313ac3b736c86bb4ddbe3df59334ef4ee56f4f4013a2"
$InstallDir = "$env:USERPROFILE\.contribai\bin"
$Url = "https://github.com/$Repo/releases/download/$Version/$Binary"

Write-Host "Installing ContribAI $Version for Windows..." -ForegroundColor Cyan
$OutPath = Join-Path $InstallDir "contribai.exe"
$TempPath = Join-Path ([IO.Path]::GetTempPath()) "contribai-$([Guid]::NewGuid()).tmp"

try {
    Write-Host "  Downloading: $Url"
    Invoke-WebRequest -Uri $Url -OutFile $TempPath -UseBasicParsing

    $ActualSha256 = (Get-FileHash -Path $TempPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ActualSha256 -ne $ExpectedSha256) {
        throw "Checksum verification failed; refusing to install. Expected $ExpectedSha256, got $ActualSha256."
    }
    Write-Host "  SHA256 checksum verified." -ForegroundColor Green

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    Move-Item -Path $TempPath -Destination $OutPath -Force
} finally {
    if (Test-Path $TempPath) {
        Remove-Item -LiteralPath $TempPath -Force
    }
}

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
