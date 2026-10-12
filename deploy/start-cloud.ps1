param([string]$ConfigPath)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$cloudBinary = Join-Path $PSScriptRoot 'spm-cloud.exe'
if (-not (Test-Path -LiteralPath $cloudBinary)) {
    throw 'Run this launcher from the extracted Windows prebuilt package.'
}
if (-not $ConfigPath) { $ConfigPath = Join-Path $PSScriptRoot 'config.json' }
if (-not (Test-Path -LiteralPath $ConfigPath)) {
    & $cloudBinary --init-config $ConfigPath
    if ($LASTEXITCODE -ne 0) { throw 'Configuration initialization failed.' }
    Write-Host "Configuration created: $ConfigPath"
    Write-Host 'Set the instance origin and bind address in this JSON before public deployment.'
}
& $cloudBinary --config $ConfigPath --check-config
if ($LASTEXITCODE -ne 0) { throw 'Configuration validation failed.' }
& $cloudBinary --config $ConfigPath
exit $LASTEXITCODE
