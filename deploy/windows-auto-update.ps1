param(
    [string]$ApplicationTaskName = 'SPM Cloud',
    [string]$ExecutablePath = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe",
    [string]$HealthUrl = 'http://127.0.0.1:8787/health'
)

$ErrorActionPreference = 'Stop'
$repository = 'sdf123098/spm-cloud'
$release = Invoke-RestMethod -Headers @{ Accept = 'application/vnd.github+json' } `
    -Uri "https://api.github.com/repos/$repository/releases/latest"
$tag = [string]$release.tag_name
if ($release.draft -or $release.prerelease -or $tag -notmatch '^v\d+\.\d+\.\d+$') {
    throw "Latest GitHub release has an unexpected tag: $tag"
}

$archiveName = 'spm-cloud-x86_64-pc-windows-msvc.zip'
$archiveAsset = $release.assets | Where-Object name -eq $archiveName | Select-Object -First 1
$checksumAsset = $release.assets | Where-Object name -eq "$archiveName.sha256" | Select-Object -First 1
if (-not $archiveAsset -or -not $checksumAsset) {
    throw "Release $tag does not contain the Windows package and checksum."
}

$installDirectory = Split-Path -Parent $ExecutablePath
if (-not (Test-Path -LiteralPath $installDirectory -PathType Container)) {
    throw "Install directory does not exist: $installDirectory"
}
if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Installed executable not found: $ExecutablePath"
}

$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("spm-cloud-update-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null
$taskWasRunning = $false
$binaryReplaced = $false
$backupPath = "$ExecutablePath.previous"
$stagedPath = "$ExecutablePath.new"

try {
    $archivePath = Join-Path $temporaryDirectory $archiveName
    $checksumPath = "$archivePath.sha256"
    Invoke-WebRequest -Uri $archiveAsset.browser_download_url -OutFile $archivePath
    Invoke-WebRequest -Uri $checksumAsset.browser_download_url -OutFile $checksumPath

    $checksumLine = (Get-Content -LiteralPath $checksumPath -Raw).Trim()
    if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
        throw "Release checksum has an unexpected format: $checksumLine"
    }
    $expectedHash = $Matches[1].ToLowerInvariant()
    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw "SHA-256 verification failed for $archiveName"
    }

    $unpackDirectory = Join-Path $temporaryDirectory 'unpacked'
    Expand-Archive -LiteralPath $archivePath -DestinationPath $unpackDirectory
    $newExecutable = Get-ChildItem -LiteralPath $unpackDirectory -Filter 'spm-cloud.exe' -File -Recurse |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $newExecutable) {
        throw "Release archive does not contain spm-cloud.exe"
    }
    $expectedVersion = "spm-cloud $($tag.Substring(1))"
    $reportedVersion = (& $newExecutable --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $reportedVersion -ne $expectedVersion) {
        throw "Release executable reports '$reportedVersion'; expected '$expectedVersion'."
    }

    $task = Get-ScheduledTask -TaskName $ApplicationTaskName -ErrorAction Stop
    $taskWasRunning = $task.State -eq 'Running'
    if (-not $taskWasRunning) {
        throw "Scheduled task '$ApplicationTaskName' must be running before an automatic update."
    }

    Stop-ScheduledTask -TaskName $ApplicationTaskName
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 500
        $task = Get-ScheduledTask -TaskName $ApplicationTaskName
    } while ($task.State -eq 'Running' -and (Get-Date) -lt $deadline)
    if ($task.State -eq 'Running') {
        throw "Scheduled task '$ApplicationTaskName' did not stop; update cancelled."
    }

    Copy-Item -LiteralPath $ExecutablePath -Destination $backupPath -Force
    Copy-Item -LiteralPath $newExecutable -Destination $stagedPath -Force
    Move-Item -LiteralPath $stagedPath -Destination $ExecutablePath -Force
    $binaryReplaced = $true
    Start-ScheduledTask -TaskName $ApplicationTaskName

    $healthy = $false
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Seconds 1
        try {
            $null = Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3
            $healthy = $true
        }
        catch {
            $task = Get-ScheduledTask -TaskName $ApplicationTaskName
            if ($task.State -ne 'Running') { break }
        }
    } while (-not $healthy -and (Get-Date) -lt $deadline)

    if (-not $healthy) {
        throw "Updated service failed its health check."
    }

    Write-Output "Updated native SPM Cloud to $tag from the verified prebuilt Windows package."
}
catch {
    try {
        if ($binaryReplaced -and (Test-Path -LiteralPath $backupPath -PathType Leaf)) {
            Stop-ScheduledTask -TaskName $ApplicationTaskName -ErrorAction SilentlyContinue
            Copy-Item -LiteralPath $backupPath -Destination $stagedPath -Force
            Move-Item -LiteralPath $stagedPath -Destination $ExecutablePath -Force
        }
    }
    finally {
        if ($taskWasRunning) {
            Start-ScheduledTask -TaskName $ApplicationTaskName -ErrorAction SilentlyContinue
        }
    }
    if ($binaryReplaced) {
        throw "Update failed; restored the previous executable. $($_.Exception.Message)"
    }
    throw
}
finally {
    Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force -ErrorAction SilentlyContinue
}
