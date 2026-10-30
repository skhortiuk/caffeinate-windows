#Requires -Version 5.1
# Installs the latest caffeinate.exe release into %LOCALAPPDATA%\Programs\caffeinate
# and adds that folder to the user PATH. No admin rights, no other tools needed.
#
#   irm https://raw.githubusercontent.com/skhortiuk/caffeinate-windows/main/install.ps1 | iex
#
# Wrapped in a script block so nothing leaks into the calling shell when run via iex.
& {
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue"

    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "caffeinate.exe is 64-bit only; this is a 32-bit Windows."
    }

    $repo = "skhortiuk/caffeinate-windows"
    $dir = Join-Path $env:LOCALAPPDATA "Programs\caffeinate"
    $exe = Join-Path $dir "caffeinate.exe"
    $tmp = "$exe.tmp"
    $base = "https://github.com/$repo/releases/latest/download"

    # Windows PowerShell 5.1 may still default to TLS 1.0/1.1, which GitHub rejects.
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    New-Item -ItemType Directory -Force -Path $dir | Out-Null

    # Resolve the latest tag from the redirect, so an up-to-date install is a no-op.
    $latest = $null
    try {
        $resp = [Net.WebRequest]::Create("https://github.com/$repo/releases/latest")
        $resp.Method = "HEAD"
        $resp.AllowAutoRedirect = $false
        $loc = $resp.GetResponse().Headers["Location"]
        if ($loc -match '/tag/v?([^/]+)$') { $latest = $Matches[1] }
    } catch { }
    if ($latest -and (Test-Path $exe)) {
        $current = (& $exe --version 2>$null) -replace '^caffeinate\s+', ''
        if ($current -eq $latest) {
            Write-Host "caffeinate $current is already the latest version ($exe)"
            return
        }
        Write-Host "Updating caffeinate $current -> $latest"
    }

    Write-Host "Downloading $base/caffeinate.exe"
    try {
        Invoke-WebRequest -Uri "$base/caffeinate.exe" -OutFile $tmp -UseBasicParsing
        Invoke-WebRequest -Uri "$base/caffeinate.exe.sha256" -OutFile "$tmp.sha256" -UseBasicParsing
    } catch {
        Remove-Item $tmp, "$tmp.sha256" -ErrorAction SilentlyContinue
        throw "Download failed: $($_.Exception.Message)`nIs there a published release at https://github.com/$repo/releases ?"
    }

    $expected = ((Get-Content "$tmp.sha256" -Raw) -split '\s+')[0].ToLower()
    Remove-Item "$tmp.sha256" -ErrorAction SilentlyContinue
    $actual = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        Remove-Item $tmp -ErrorAction SilentlyContinue
        throw "SHA256 mismatch.`n expected $expected`n got      $actual"
    }

    # A running caffeinate.exe cannot be overwritten, but it can be renamed.
    if (Test-Path $exe) {
        Move-Item $exe "$exe.old" -Force
    }
    Move-Item $tmp $exe -Force
    Remove-Item "$exe.old" -ErrorAction SilentlyContinue

    # Edit the registry value directly so %VAR% entries in PATH stay unexpanded.
    $envKey = "HKCU:\Environment"
    $userPath = (Get-ItemProperty -Path $envKey -Name Path -ErrorAction SilentlyContinue).Path
    $entries = @()
    if ($userPath) { $entries = $userPath -split ";" | Where-Object { $_ } }
    $present = $entries | Where-Object { $_.TrimEnd("\") -ieq $dir.TrimEnd("\") }
    if (-not $present) {
        $newPath = if ($userPath) { "$userPath;$dir" } else { $dir }
        Set-ItemProperty -Path $envKey -Name Path -Value $newPath -Type ExpandString
        # Tell Explorer and new shells that the environment changed.
        $sig = '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'
        $w = Add-Type -MemberDefinition $sig -Name WinEnv -Namespace Caffeinate -PassThru
        [UIntPtr]$r = [UIntPtr]::Zero
        $w::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$r) | Out-Null
        Write-Host "Added $dir to user PATH. Other open terminals need a restart to see it."
    }
    if (($env:Path -split ";") -notcontains $dir) {
        $env:Path = "$env:Path;$dir"
    }

    $v = & $exe --version
    Write-Host "Installed $v at $exe"
}
