# ccvv for Windows — cleans clipboard text in place.
#
# Bind to a keyboard shortcut via:
#   - AutoHotkey: ^!c::Run, powershell -WindowStyle Hidden -File "ccvv.ps1"
#   - Or create a shortcut (.lnk) and assign a hotkey in its properties.

function Test-ListItem($line) {
    return $line -match '^- ' -or $line -match '^\* ' -or $line -match '^[•◦▪]\s+' -or $line -match '^\d+[.)\]] '
}

function Test-ListItemWithIndent($line) {
    return Test-ListItem ($line.TrimStart())
}

function Normalize-BulletMarker($line) {
    return ($line -replace '^[•◦▪]\s+', '- ')
}

function Test-CodeFenceLine($line) {
    return $line.TrimStart().StartsWith('```')
}

function Get-LeadingWhitespace($line) {
    $match = [regex]::Match($line, '^[ \t]*')
    return $match.Value
}

function Remove-Prefix($line, $prefix) {
    if ($prefix -and $line.StartsWith($prefix)) {
        return $line.Substring($prefix.Length)
    }
    return $line
}

function Join-ParagraphLines($lines) {
    $outLines = @()
    $buf = ''
    $baseIndent = ($lines | Measure-Object -Property Indent -Minimum).Minimum
    foreach ($entry in $lines) {
        $line = $entry.Text
        $indent = [int]$entry.Indent
        $relIndent = [Math]::Max(0, $indent - $baseIndent)
        if (Test-ListItem $line) {
            if ($buf -ne '') {
                $outLines += $buf
                $buf = ''
            }
            $outLines += ((' ' * $relIndent) + $line)
        } elseif ($outLines.Count -gt 0 -and (Test-ListItemWithIndent $outLines[-1]) -and $buf -eq '' -and $relIndent -ge 2) {
            $outLines[-1] = "$($outLines[-1]) $line"
        } else {
            $buf = if ($buf -eq '') { $line } else { "$buf $line" }
        }
    }
    if ($buf -ne '') {
        $outLines += $buf
    }
    return $outLines -join "`n"
}

function Invoke-ccvv($text) {
    $text = $text -replace "`r`n", "`n" -replace "`r", "`n"

    $result = @()
    $paragraph = @()
    $codeBlock = @()
    $inCodeFence = $false
    $codeFenceIndent = ''

    foreach ($rawLine in ($text -split "`n")) {
        $line = $rawLine -replace '\s+$', ''

        if ($inCodeFence) {
            if (Test-CodeFenceLine $line) {
                $codeBlock += $line.Trim()
                $result += ($codeBlock -join "`n")
                $codeBlock = @()
                $inCodeFence = $false
                $codeFenceIndent = ''
            } else {
                $codeBlock += (Remove-Prefix $line $codeFenceIndent)
            }
            continue
        }

        if (Test-CodeFenceLine $line) {
            if ($paragraph.Count -gt 0) {
                $result += (Join-ParagraphLines $paragraph)
                $paragraph = @()
            }
            $inCodeFence = $true
            $codeFenceIndent = Get-LeadingWhitespace $line
            $codeBlock = @($line.Trim())
            continue
        }

        $indentMatch = [regex]::Match($line, '^[ \t]+')
        $indent = if ($indentMatch.Success) { $indentMatch.Value.Length } else { 0 }
        $s = $line.TrimStart()
        if ($s.StartsWith([char]0x23FA)) {  # ⏺
            $s = $s.Substring(1).TrimStart()
        }
        if ($s -match '^[◦▪]\s+') {
            $indent += 2
        }
        $s = Normalize-BulletMarker $s

        if ($s -eq '') {
            if ($paragraph.Count -gt 0) {
                $result += (Join-ParagraphLines $paragraph)
                $paragraph = @()
            }
        } else {
            $paragraph += [pscustomobject]@{
                Text = $s
                Indent = $indent
            }
        }
    }

    if ($paragraph.Count -gt 0) {
        $result += (Join-ParagraphLines $paragraph)
    }
    if ($codeBlock.Count -gt 0) {
        $result += ($codeBlock -join "`n")
    }

    return $result -join "`n`n"
}

# Main
$text = Get-Clipboard -Raw
if (-not $text) { exit }

$cleaned = Invoke-ccvv $text
Set-Clipboard $cleaned

# Toast notification (best-effort)
try {
    [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null
    $notify = New-Object System.Windows.Forms.NotifyIcon
    $notify.Icon = [System.Drawing.SystemIcons]::Information
    $notify.Visible = $true
    $notify.ShowBalloonTip(2000, 'ccvv', 'Clipboard cleaned', 'Info')
    Start-Sleep -Seconds 3
    $notify.Dispose()
} catch {}
