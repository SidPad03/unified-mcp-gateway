# MCP Gateway Agent Installer (Windows)
# Usage: irm https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$InstallDir = "$env:USERPROFILE\.mcp-gateway-agent\bin"
$BinaryName = "mcp-gateway-agent.exe"
$Target = "x86_64-pc-windows-gnu"

Write-Host "[INFO] Detected: Windows x86_64 ($Target)" -ForegroundColor Cyan

# Get gateway URL
if (-not $env:GATEWAY_URL) {
    Write-Host "Enter your MCP Gateway URL (e.g., https://mcp-gateway.example.com):" -ForegroundColor Cyan
    $GatewayUrl = Read-Host
} else {
    $GatewayUrl = $env:GATEWAY_URL
}

if (-not $GatewayUrl) {
    Write-Host "[ERROR] Gateway URL is required." -ForegroundColor Red
    exit 1
}

# Normalize URL
$GatewayUrl = $GatewayUrl.TrimEnd('/')
$GatewayUrl = $GatewayUrl -replace '/agent/ws$', ''
$GatewayUrl = $GatewayUrl.TrimEnd('/')
$GatewayUrl = $GatewayUrl -replace '^wss://', 'https://'
$GatewayUrl = $GatewayUrl -replace '^ws://', 'http://'
if ($GatewayUrl -notmatch '^https?://') {
    $GatewayUrl = "https://$GatewayUrl"
}

Write-Host "[INFO] Using gateway: $GatewayUrl" -ForegroundColor Cyan

# Get API key
if (-not $env:API_KEY) {
    Write-Host "Enter your API key (starts with mcpgw_):" -ForegroundColor Cyan
    $ApiKey = Read-Host -AsSecureString
    $ApiKey = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
        [Runtime.InteropServices.Marshal]::SecureStringToBSTR($ApiKey))
} else {
    $ApiKey = $env:API_KEY
}

if (-not $ApiKey) {
    Write-Host "[ERROR] API key is required." -ForegroundColor Red
    exit 1
}

# Fetch latest release info
Write-Host "[INFO] Fetching latest release info..." -ForegroundColor Cyan
$Headers = @{ "Authorization" = "Bearer $ApiKey" }

try {
    $Release = Invoke-RestMethod -Uri "$GatewayUrl/api/v1/agent/releases/latest" -Headers $Headers
} catch {
    $StatusCode = $_.Exception.Response.StatusCode.value__
    switch ($StatusCode) {
        401 { Write-Host "[ERROR] Authentication failed — check your API key." -ForegroundColor Red }
        403 { Write-Host "[ERROR] Authentication failed — check your API key." -ForegroundColor Red }
        404 { Write-Host "[ERROR] No agent releases found. Push a tag like 'agent-v0.1.0' to trigger the CI release build." -ForegroundColor Red }
        default { Write-Host "[ERROR] Failed to fetch release info (HTTP $StatusCode): $_" -ForegroundColor Red }
    }
    exit 1
}

$Version = $Release.version
if (-not $Version) {
    Write-Host "[ERROR] Could not determine latest version." -ForegroundColor Red
    exit 1
}

Write-Host "[INFO] Latest version: v$Version" -ForegroundColor Cyan

# Create install directory
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# Download binary
$DownloadUrl = "$GatewayUrl/api/v1/agent/releases/v$Version/download?arch=$Target"
$BinaryPath = Join-Path $InstallDir $BinaryName
$TempPath = "$BinaryPath.download"

Write-Host "[INFO] Downloading $BinaryName for $Target..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $DownloadUrl -Headers $Headers -OutFile $TempPath
} catch {
    Remove-Item -Force -ErrorAction SilentlyContinue $TempPath
    Write-Host "[ERROR] Download failed — no binary for $Target in release v$Version." -ForegroundColor Red
    exit 1
}

Move-Item -Force $TempPath $BinaryPath
Write-Host "[OK] Downloaded $BinaryName v$Version to $BinaryPath" -ForegroundColor Green

# Add to user PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -and -not $UserPath.Contains($InstallDir)) {
    [Environment]::SetEnvironmentVariable("PATH", "$InstallDir;$UserPath", "User")
    Write-Host "[INFO] Added $InstallDir to user PATH" -ForegroundColor Cyan
} elseif (-not $UserPath) {
    [Environment]::SetEnvironmentVariable("PATH", $InstallDir, "User")
    Write-Host "[INFO] Added $InstallDir to user PATH" -ForegroundColor Cyan
}

Write-Host ""
Write-Host "[OK] MCP Gateway Agent v$Version installed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Open a new terminal (to pick up PATH changes)"
Write-Host "  2. Run the setup wizard:  mcp-gateway-agent setup"
Write-Host ""
