<#
.SYNOPSIS
  Recursively rename LYMA/LYBA paths and optionally update text references to LYMA/LYBA.

.DESCRIPTION
  Safe-by-default migration script. By default it performs a dry-run only.

  Renames names recursively:
    lyma  -> lyma
    lyba -> lyba
    LYMA  -> LYMA
    LYBA -> LYBA
    Lyma  -> Lyma
    Lyba -> Lyba

  Optional content updates are also recursive when -UpdateContent is used.

.EXAMPLES
  # Preview recursively
  .\rename-lyma-lyba-to-lyma-lyba-recursive.ps1 -Root .

  # Apply recursive path renames
  .\rename-lyma-lyba-to-lyma-lyba-recursive.ps1 -Root . -Apply

  # Apply recursive path renames and recursive content updates
  .\rename-lyma-lyba-to-lyma-lyba-recursive.ps1 -Root . -Apply -UpdateContent

  # Also rename the root folder itself if its name contains lyma/lyba
  .\rename-lyma-lyba-to-lyma-lyba-recursive.ps1 -Root . -Apply -RenameRoot
#>

[CmdletBinding()]
param(
    [string]$Root = ".",
    [switch]$Apply,
    [switch]$UpdateContent,
    [switch]$RenameRoot,
    [switch]$IncludeGenerated,
    [string[]]$ExcludeDirs = @(
        ".git", ".hg", ".svn",
        "node_modules", "target", "dist", "build", "out",
        ".next", ".turbo", ".cache", ".idea", ".vscode"
    ),
    [string[]]$TextExtensions = @(
        ".md", ".mdx", ".txt", ".rst", ".adoc",
        ".toml", ".json", ".jsonc", ".yaml", ".yml",
        ".lua", ".luau", ".lyma", ".lyba", ".lyma", ".lyba",
        ".rs", ".c", ".h", ".cpp", ".hpp", ".cs", ".go", ".java", ".kt", ".swift",
        ".js", ".jsx", ".ts", ".tsx", ".css", ".scss", ".html", ".xml",
        ".py", ".rb", ".php", ".sh", ".bash", ".zsh", ".ps1", ".psm1", ".bat", ".cmd",
        ".ron", ".ini", ".cfg", ".conf", ".env", ".gitignore", ".gitattributes"
    )
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$GeneratedDirs = @("target", "dist", "build", "out", ".next", ".turbo", ".cache")
if ($IncludeGenerated) {
    $ExcludeDirs = @($ExcludeDirs | Where-Object { $GeneratedDirs -notcontains $_ })
}

# Order matters: lyba must be replaced before lyma.
$NameRules = @(
    @{ From = "LYBA"; To = "LYBA" },
    @{ From = "Lyba"; To = "Lyba" },
    @{ From = "lyba"; To = "lyba" },
    @{ From = "LYMA";  To = "LYMA" },
    @{ From = "Lyma";  To = "Lyma" },
    @{ From = "lyma";  To = "lyma" }
)

$ContentRules = @(
    @{ From = "Lua YAML-like Binary Assembly"; To = "Lua YAML-like Binary Assembly" },
    @{ From = "Lua YAML-like Markup Assembly";        To = "Lua YAML-like Markup Assembly" },
    @{ From = "Lua YAML-like Binary Assembly";          To = "Lua YAML-like Binary Assembly" }
) + $NameRules

function Convert-NameText {
    param([string]$Text)
    $Result = $Text
    foreach ($Rule in $NameRules) {
        $Result = $Result -creplace [regex]::Escape($Rule.From), $Rule.To
    }
    return $Result
}

function Convert-ContentText {
    param([string]$Text)
    $Result = $Text
    foreach ($Rule in $ContentRules) {
        $Result = $Result -creplace [regex]::Escape($Rule.From), $Rule.To
    }
    return $Result
}

function Resolve-FullPath {
    param([string]$Path)
    return [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Path).Path)
}

function Get-RelativePath {
    param([string]$BasePath, [string]$FullPath)
    $BaseFull = [System.IO.Path]::GetFullPath($BasePath).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $TargetFull = [System.IO.Path]::GetFullPath($FullPath)
    $BaseUri = [System.Uri]::new($BaseFull + [System.IO.Path]::DirectorySeparatorChar)
    $TargetUri = [System.Uri]::new($TargetFull)
    return [System.Uri]::UnescapeDataString($BaseUri.MakeRelativeUri($TargetUri).ToString()).Replace('/', [System.IO.Path]::DirectorySeparatorChar)
}

function Test-ExcludedByPath {
    param([string]$RootPath, [System.IO.FileSystemInfo]$Item)
    $Relative = Get-RelativePath -BasePath $RootPath -FullPath $Item.FullName
    $Segments = @($Relative -split '[\\/]+' | Where-Object { $_ -ne "" })
    foreach ($Segment in $Segments) {
        if ($ExcludeDirs -contains $Segment) { return $true }
    }
    return $false
}

function Get-AllItemsRecursive {
    param([string]$RootPath)

    # Explicitly recursive. This intentionally excludes generated/vendor dirs by path segment.
    Get-ChildItem -LiteralPath $RootPath -Force -Recurse |
        Where-Object { -not (Test-ExcludedByPath -RootPath $RootPath -Item $_) }
}

function Test-TextCandidate {
    param([System.IO.FileInfo]$File)
    $KnownTextNames = @(".gitignore", ".gitattributes", ".editorconfig", "LICENSE", "NOTICE", "README", "CHANGELOG", "Makefile", "Dockerfile")
    if ($KnownTextNames -contains $File.Name) { return $true }
    if ($TextExtensions -contains $File.Extension.ToLowerInvariant()) { return $true }
    return $false
}

function Test-ProbablyBinary {
    param([System.IO.FileInfo]$File)
    $MaxBytes = [Math]::Min(8192, $File.Length)
    if ($MaxBytes -le 0) { return $false }
    $Buffer = New-Object byte[] $MaxBytes
    $Stream = [System.IO.File]::OpenRead($File.FullName)
    try { [void]$Stream.Read($Buffer, 0, $MaxBytes) }
    finally { $Stream.Dispose() }
    foreach ($Byte in $Buffer) {
        if ($Byte -eq 0) { return $true }
    }
    return $false
}

function Move-PathSafe {
    param([string]$OldPath, [string]$NewPath)
    if (Test-Path -LiteralPath $NewPath) {
        throw "Cannot rename '$OldPath' to '$NewPath' because the destination already exists."
    }
    Move-Item -LiteralPath $OldPath -Destination $NewPath
}

$RootPath = Resolve-FullPath -Path $Root

Write-Host "LYMA/LYBA recursive rename migration"
Write-Host "Root: $RootPath"
Write-Host "Mode: $(if ($Apply) { 'APPLY' } else { 'DRY-RUN' })"
Write-Host "Update content recursively: $($UpdateContent.IsPresent)"
Write-Host "Rename root folder: $($RenameRoot.IsPresent)"
Write-Host ""

$AllItems = @(Get-AllItemsRecursive -RootPath $RootPath)

if ($RenameRoot) {
    $RootInfo = Get-Item -LiteralPath $RootPath -Force
    $AllItems += $RootInfo
}

$RenamePlan = @()
foreach ($Item in $AllItems) {
    $NewName = Convert-NameText -Text $Item.Name
    if ($NewName -eq $Item.Name) { continue }

    $ParentPath = if ($Item.PSIsContainer) { $Item.Parent.FullName } else { $Item.DirectoryName }
    if ([string]::IsNullOrWhiteSpace($ParentPath)) { continue }

    $NewFullPath = Join-Path -Path $ParentPath -ChildPath $NewName
    $Depth = ($Item.FullName -split '[\\/]+').Count

    $RenamePlan += [pscustomobject]@{
        OldPath = $Item.FullName
        NewPath = $NewFullPath
        OldRelative = if ($Item.FullName -eq $RootPath) { "." } else { Get-RelativePath -BasePath $RootPath -FullPath $Item.FullName }
        NewRelative = if ($Item.FullName -eq $RootPath) { "../$NewName" } else { Get-RelativePath -BasePath $RootPath -FullPath $NewFullPath }
        Depth = $Depth
    }
}

# Deepest first prevents parent directory moves from invalidating child paths.
$RenamePlan = @($RenamePlan | Sort-Object -Property Depth, OldPath -Descending)

if ($RenamePlan.Count -eq 0) {
    Write-Host "No recursive file or directory names need renaming."
} else {
    Write-Host "Recursive file/directory rename plan:"
    foreach ($Step in $RenamePlan) {
        Write-Host "  $($Step.OldRelative) -> $($Step.NewRelative)"
    }

    if ($Apply) {
        Write-Host ""
        Write-Host "Applying recursive file/directory renames..."
        foreach ($Step in $RenamePlan) {
            Move-PathSafe -OldPath $Step.OldPath -NewPath $Step.NewPath
        }

        if ($RenameRoot) {
            $RootPath = Resolve-FullPath -Path (Split-Path -Parent $RenamePlan[-1].NewPath)
        }
    }
}

$ContentChangeCount = 0

if ($UpdateContent) {
    Write-Host ""
    Write-Host "Scanning text files recursively for content updates..."

    # Re-scan after renames, because paths may have changed.
    $ScanRoot = if ($RenameRoot -and $Apply) {
        $MovedRootStep = @($RenamePlan | Where-Object { $_.OldRelative -eq "." }) | Select-Object -First 1
        if ($null -ne $MovedRootStep) { $MovedRootStep.NewPath } else { $RootPath }
    } else {
        $RootPath
    }

    $AllFiles = @(Get-AllItemsRecursive -RootPath $ScanRoot | Where-Object { -not $_.PSIsContainer })
    $Utf8NoBom = [System.Text.UTF8Encoding]::new($false)

    foreach ($Item in $AllFiles) {
        $File = [System.IO.FileInfo]$Item
        if (-not (Test-TextCandidate -File $File)) { continue }
        if (Test-ProbablyBinary -File $File) { continue }

        try { $Original = [System.IO.File]::ReadAllText($File.FullName) }
        catch {
            Write-Warning "Skipping unreadable text candidate: $($File.FullName)"
            continue
        }

        $Updated = Convert-ContentText -Text $Original
        if ($Updated -ne $Original) {
            $Relative = Get-RelativePath -BasePath $ScanRoot -FullPath $File.FullName
            Write-Host "  content: $Relative"
            $ContentChangeCount++

            if ($Apply) {
                [System.IO.File]::WriteAllText($File.FullName, $Updated, $Utf8NoBom)
            }
        }
    }

    if ($ContentChangeCount -eq 0) {
        Write-Host "No recursive text content references need updating."
    }
}

Write-Host ""
Write-Host "Summary:"
Write-Host "  Recursive file/directory renames: $($RenamePlan.Count)"
Write-Host "  Recursive text files changed:    $ContentChangeCount"

if (-not $Apply) {
    Write-Host ""
    Write-Host "Dry-run only. Re-run with -Apply to make changes."
} else {
    Write-Host ""
    Write-Host "Migration complete. Review with: git status --short"
}