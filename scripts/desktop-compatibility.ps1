# Opt-in metadata probe for Windows. Run without elevation in the tester session.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][switch]$ConfirmDisposable,
    [Parameter(Mandatory)][string]$Executable
)
$ErrorActionPreference = 'Stop'

function Protect-Path([string]$Path) {
    if ($Path -match '^[A-Za-z]:[\\/][^:\x00-\x1f\x7f<>"|?*]+\z') {
        $Path -replace '^([A-Za-z]:[\\/](?:.*?[\\/])?Users[\\/])[^\\/]+', '$1<user>'
    }
}

function Read-ExecutableArchitecture([string]$Path) {
    try {
        $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
        try {
            $reader = [IO.BinaryReader]::new($stream)
            if ($reader.ReadUInt16() -ne 0x5a4d) { return }
            $stream.Position = 0x3c
            $stream.Position = $reader.ReadUInt32()
            if ($reader.ReadUInt32() -ne 0x4550) { return }
            switch ($reader.ReadUInt16()) {
                0x8664 { 'x64' }
                0xaa64 { 'arm64' }
                0x014c { 'x86' }
                0xa641 { 'arm64ec' }
                0xa64e { 'arm64x' }
            }
        } finally { $stream.Dispose() }
    } catch {
        # An unreadable or truncated PE file leaves the architecture unknown.
    }
}

if (-not $ConfirmDisposable -or -not (Protect-Path $Executable)) {
    throw 'Supply -ConfirmDisposable and an absolute executable path.'
}
try {
    $exe = Get-Item -LiteralPath $Executable
} catch {
    throw 'Supply a readable installed .exe file.'
}
if ($exe.PSIsContainer -or $exe.Extension -ine '.exe') { throw 'Supply the installed .exe file.' }
$protectedExecutable = Protect-Path $exe.FullName
if (-not $protectedExecutable) { throw 'Supply the installed .exe file.' }

$os = Get-CimInstance -ClassName Win32_OperatingSystem -Property Version,BuildNumber
$processor = Get-CimInstance -ClassName Win32_Processor -Property Architecture | Select-Object -First 1
$hostArchitecture = switch ($processor.Architecture) {
    0 { 'x86' }
    5 { 'arm' }
    9 { 'x64' }
    12 { 'arm64' }
}
$package = $null
$packageLookup = 'unavailable'
try {
    # Scope to the current OS user. Match a directory boundary and prefer the most specific root.
    $package = Get-AppxPackage | Where-Object {
        $_.InstallLocation -and
        $exe.FullName.StartsWith($_.InstallLocation.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
    } | Sort-Object { $_.InstallLocation.Length } -Descending | Select-Object -First 1
    $packageLookup = if ($package) { 'matched-install-root' } else { 'no-match' }
} catch {
    # Missing cmdlet or inaccessible package metadata.
}
$signature = $null
try {
    $signature = Get-AuthenticodeSignature -LiteralPath $exe.FullName
} catch {
    # Package signing and individual PE signing can differ.
}

$processes = @()
$unresolved = 0
$discovery = 'complete'
try {
    # Request only these columns. Never retrieve CommandLine or process environment.
    $snapshot = Get-CimInstance -ClassName Win32_Process -Property Name,ProcessId,ExecutablePath
    $processes = @(
        foreach ($process in ($snapshot | Where-Object { $_.Name -ieq $exe.Name } | Sort-Object ProcessId)) {
            $identity = 'same-name-different-path'
            $protectedPath = Protect-Path $process.ExecutablePath
            if (-not $protectedPath) {
                $unresolved++
                $identity = 'path-unavailable'
            } elseif ($process.ExecutablePath -ieq $exe.FullName) {
                $identity = 'selected-executable'
            }
            [ordered]@{
                pid = [long]$process.ProcessId
                identity = $identity
                executable_path = $protectedPath
                profile_attribution = 'unverified'
            }
        }
    )
    if ($unresolved -gt 0) { $discovery = 'partial' }
} catch {
    $discovery = 'unavailable'
}

$operations = [ordered]@{
    installed_metadata = 'supported'
    process_inventory = if ($discovery -eq 'complete') { 'supported' } else { 'unverified' }
    managed_profile_attribution = 'unverified'
    separate_instances = 'unverified'
    activation_smoke = 'unverified'
    dock_production_launch = 'unsupported'
    dock_production_activation = 'unsupported'
}
[ordered]@{
    os = [ordered]@{
        name = 'windows'
        version = $os.Version
        build = $os.BuildNumber
        architecture = $hostArchitecture
    }
    application = [ordered]@{
        executable_path = $protectedExecutable
        architectures = @(Read-ExecutableArchitecture $exe.FullName)
        declared_name = if ($exe.VersionInfo.ProductName) { $exe.VersionInfo.ProductName } else { $null }
        application_id = $package.PackageFamilyName
        package_lookup = $packageLookup
        package_id = $package.PackageFullName
        publisher = $package.Publisher
        signer = $signature.SignerCertificate.Subject
        signature_status = if ($signature) { $signature.Status.ToString() } else { $null }
        version = if ($exe.VersionInfo.ProductVersion) { $exe.VersionInfo.ProductVersion } else { $null }
        package_or_build_version = if ($package) { $package.Version.ToString() } else { $null }
    }
    discovery = [ordered]@{
        scope = 'selected-executable-basename'
        completeness = $discovery
        unresolved_paths = if ($discovery -eq 'unavailable') { $null } else { $unresolved }
        processes = $processes
    }
    activation_permission = [ordered]@{
        kind = 'windows-foreground-policy'
        availability = 'unverified'
    }
    operations = $operations
} | ConvertTo-Json -Depth 6
