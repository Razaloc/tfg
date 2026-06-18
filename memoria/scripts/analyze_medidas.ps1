param(
    [string]$InputDir = "Medidas",
    [string]$OutputDir = "figures"
)

$ErrorActionPreference = "Stop"

$seriesResistance = 10000.0
$parallelResistance = 20000.0
$parallelCapacitance = 10e-9

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

function Fmt {
    param(
        [string]$Format,
        [Parameter(ValueFromRemainingArguments = $true)]
        [object[]]$Args
    )
    return [string]::Format([Globalization.CultureInfo]::InvariantCulture, $Format, $Args)
}

function Get-FrequencyFromName([string]$name) {
    if ($name -match "_([0-9]+(?:\.[0-9]+)?)Hz") {
        return [double]::Parse($matches[1], [Globalization.CultureInfo]::InvariantCulture)
    }
    return $null
}

function Read-WaveformsCsv([string]$path) {
    $sampleRate = $null
    $rows = New-Object System.Collections.Generic.List[object]
    $headerSeen = $false

    foreach ($line in [IO.File]::ReadLines($path)) {
        if ($line.StartsWith("#Sample rate:")) {
            $value = $line.Substring("#Sample rate:".Length).Trim().Replace("Hz", "").Trim()
            $sampleRate = [double]::Parse($value, [Globalization.CultureInfo]::InvariantCulture)
            continue
        }
        if ($line.StartsWith("#") -or [string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if (-not $headerSeen) {
            $headerSeen = $true
            continue
        }

        $parts = $line.Split(",")
        if ($parts.Count -ge 3) {
            $rows.Add([pscustomobject]@{
                Time = [double]::Parse($parts[0], [Globalization.CultureInfo]::InvariantCulture)
                Ch1 = [double]::Parse($parts[1], [Globalization.CultureInfo]::InvariantCulture)
                Ch2 = [double]::Parse($parts[2], [Globalization.CultureInfo]::InvariantCulture)
            })
        }
    }

    return [pscustomobject]@{ SampleRate = $sampleRate; Rows = $rows }
}

function Get-Phasor($rows, [double]$frequency, [string]$field) {
    $sum = 0.0
    foreach ($row in $rows) {
        $sum += $row.$field
    }
    $mean = $sum / $rows.Count

    $real = 0.0
    $imag = 0.0
    foreach ($row in $rows) {
        $x = $row.$field - $mean
        $angle = -2.0 * [Math]::PI * $frequency * $row.Time
        $real += $x * [Math]::Cos($angle)
        $imag += $x * [Math]::Sin($angle)
    }

    $scale = 2.0 / $rows.Count
    return [pscustomobject]@{
        Real = $real * $scale
        Imag = $imag * $scale
        Magnitude = [Math]::Sqrt($real * $real + $imag * $imag) * $scale
        Phase = [Math]::Atan2($imag, $real)
        Mean = $mean
    }
}

function Get-MeasuredFrequency($rows, [string]$field, [double]$fallbackFrequency) {
    $sum = 0.0
    $minimum = [double]::PositiveInfinity
    $maximum = [double]::NegativeInfinity

    foreach ($row in $rows) {
        $value = [double]$row.$field
        $sum += $value
        if ($value -lt $minimum) { $minimum = $value }
        if ($value -gt $maximum) { $maximum = $value }
    }

    $mean = $sum / $rows.Count
    $hysteresis = ($maximum - $minimum) * 0.12
    $armed = $false
    $crossings = New-Object System.Collections.Generic.List[double]

    for ($i = 1; $i -lt $rows.Count; $i++) {
        $previous = [double]$rows[$i - 1].$field
        $current = [double]$rows[$i].$field

        if ($current -lt ($mean - $hysteresis)) {
            $armed = $true
        }

        if ($armed -and $previous -lt $mean -and $current -ge $mean -and $current -ne $previous) {
            $fraction = ($mean - $previous) / ($current - $previous)
            $time = [double]$rows[$i - 1].Time +
                $fraction * ([double]$rows[$i].Time - [double]$rows[$i - 1].Time)
            $crossings.Add($time)
            $armed = $false
        }
    }

    if ($crossings.Count -lt 3) {
        return $fallbackFrequency
    }

    $periods = New-Object System.Collections.Generic.List[double]
    for ($i = 1; $i -lt $crossings.Count; $i++) {
        $period = $crossings[$i] - $crossings[$i - 1]
        if ($period -gt 0.0) {
            $periods.Add($period)
        }
    }

    if ($periods.Count -eq 0) {
        return $fallbackFrequency
    }

    $orderedPeriods = @($periods | Sort-Object)
    $medianPeriod = $orderedPeriods[[int][Math]::Floor($orderedPeriods.Count / 2)]
    return 1.0 / $medianPeriod
}

function Divide-Complex($aRe, $aIm, $bRe, $bIm) {
    $den = $bRe * $bRe + $bIm * $bIm
    return [pscustomobject]@{ Re = ($aRe * $bRe + $aIm * $bIm) / $den; Im = ($aIm * $bRe - $aRe * $bIm) / $den }
}

function Add-Complex($aRe, $aIm, $bRe, $bIm) {
    return [pscustomobject]@{ Re = $aRe + $bRe; Im = $aIm + $bIm }
}

function Sub-Complex($aRe, $aIm, $bRe, $bIm) {
    return [pscustomobject]@{ Re = $aRe - $bRe; Im = $aIm - $bIm }
}

function Mul-Complex($aRe, $aIm, $bRe, $bIm) {
    return [pscustomobject]@{ Re = $aRe * $bRe - $aIm * $bIm; Im = $aRe * $bIm + $aIm * $bRe }
}

function Get-Mag($re, $im) {
    return [Math]::Sqrt($re * $re + $im * $im)
}

function Escape-Svg([string]$text) {
    return $text.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;")
}

function Write-LinePlotSvg($path, $rows, [string]$title, [string]$xLabel, [string]$yLabel, [string]$seriesAName, [string]$seriesBName, [string]$xField, [string]$aField, [string]$bField, [switch]$LogX) {
    $width = 900
    $height = 520
    $left = 78
    $right = 24
    $top = 46
    $bottom = 72
    $plotW = $width - $left - $right
    $plotH = $height - $top - $bottom

    $xs = @($rows | ForEach-Object { [double]$_.$xField })
    if ($LogX) { $xs = @($xs | ForEach-Object { [Math]::Log10($_) }) }
    $ys = @()
    $ys += @($rows | ForEach-Object { [double]$_.$aField })
    if ($bField) { $ys += @($rows | ForEach-Object { [double]$_.$bField }) }

    $minX = ($xs | Measure-Object -Minimum).Minimum
    $maxX = ($xs | Measure-Object -Maximum).Maximum
    $minY = ($ys | Measure-Object -Minimum).Minimum
    $maxY = ($ys | Measure-Object -Maximum).Maximum
    if ($minY -eq $maxY) { $minY -= 1; $maxY += 1 }
    $padY = ($maxY - $minY) * 0.08
    $minY -= $padY
    $maxY += $padY

    function PointList([string]$field) {
        $points = New-Object System.Collections.Generic.List[string]
        foreach ($row in $rows) {
            $xVal = [double]$row.$xField
            if ($LogX) { $xVal = [Math]::Log10($xVal) }
            $yVal = [double]$row.$field
            $px = $left + (($xVal - $minX) / ($maxX - $minX)) * $plotW
            $py = $top + (1.0 - (($yVal - $minY) / ($maxY - $minY))) * $plotH
            $points.Add((Fmt "{0:F1},{1:F1}" $px $py))
        }
        return [string]::Join(" ", $points)
    }

    $grid = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -le 5; $i++) {
        $y = $top + $i * $plotH / 5.0
        $value = $maxY - $i * ($maxY - $minY) / 5.0
        $grid.Add("<line x1='$left' y1='$y' x2='$($left+$plotW)' y2='$y' stroke='#d8dde6' stroke-width='1'/>")
        $grid.Add("<text x='$($left-10)' y='$($y+4)' text-anchor='end' font-size='12' fill='#344054'>$(Fmt "{0:F1}" $value)</text>")
    }

    foreach ($row in $rows) {
        $xValue = [double]$row.$xField
        $xPlot = if ($LogX) { [Math]::Log10($xValue) } else { $xValue }
        $px = $left + (($xPlot - $minX) / ($maxX - $minX)) * $plotW
        $grid.Add("<line x1='$px' y1='$top' x2='$px' y2='$($top+$plotH)' stroke='#eef1f5' stroke-width='1'/>")
        $grid.Add("<text x='$px' y='$($top+$plotH+22)' text-anchor='middle' font-size='11' fill='#344054'>$(Fmt "{0:G4}" $xValue)</text>")
    }

    $aPoints = PointList $aField
    $bPoints = if ($bField) { PointList $bField } else { "" }
    $titleEsc = Escape-Svg $title
    $xEsc = Escape-Svg $xLabel
    $yEsc = Escape-Svg $yLabel
    $aEsc = Escape-Svg $seriesAName
    $bEsc = Escape-Svg $seriesBName

    $svg = @"
<svg xmlns="http://www.w3.org/2000/svg" width="$width" height="$height" viewBox="0 0 $width $height">
  <rect width="100%" height="100%" fill="white"/>
  <text x="$($width/2)" y="24" text-anchor="middle" font-family="Arial, sans-serif" font-size="18" font-weight="700" fill="#111827">$titleEsc</text>
  $([string]::Join("`n  ", $grid))
  <line x1="$left" y1="$top" x2="$left" y2="$($top+$plotH)" stroke="#111827" stroke-width="1.4"/>
  <line x1="$left" y1="$($top+$plotH)" x2="$($left+$plotW)" y2="$($top+$plotH)" stroke="#111827" stroke-width="1.4"/>
  <polyline points="$aPoints" fill="none" stroke="#0066cc" stroke-width="2.8"/>
  $(if ($bField) { "<polyline points=""$bPoints"" fill=""none"" stroke=""#c2410c"" stroke-width=""2.8""/>" } else { "" })
  <text x="$($left+$plotW/2)" y="$($height-18)" text-anchor="middle" font-family="Arial, sans-serif" font-size="14" fill="#111827">$xEsc</text>
  <text transform="translate(18 $($top+$plotH/2)) rotate(-90)" text-anchor="middle" font-family="Arial, sans-serif" font-size="14" fill="#111827">$yEsc</text>
  <rect x="$($width-250)" y="44" width="222" height="50" fill="white" stroke="#d0d5dd"/>
  <line x1="$($width-236)" y1="62" x2="$($width-198)" y2="62" stroke="#0066cc" stroke-width="3"/>
  <text x="$($width-190)" y="66" font-family="Arial, sans-serif" font-size="13" fill="#111827">$aEsc</text>
  $(if ($bField) { "<line x1=""$($width-236)"" y1=""82"" x2=""$($width-198)"" y2=""82"" stroke=""#c2410c"" stroke-width=""3""/><text x=""$($width-190)"" y=""86"" font-family=""Arial, sans-serif"" font-size=""13"" fill=""#111827"">$bEsc</text>" } else { "" })
</svg>
"@
    Set-Content -Path $path -Value $svg -Encoding UTF8
}

function Get-TikzPoints($rows, [string]$xField, [string]$yField, [double]$minX, [double]$maxX, [double]$minY, [double]$maxY, [switch]$LogX) {
    $points = New-Object System.Collections.Generic.List[string]
    foreach ($row in $rows) {
        $xValue = [double]$row.$xField
        if ($LogX) { $xValue = [Math]::Log10($xValue) }
        $yValue = [double]$row.$yField
        $x = (($xValue - $minX) / ($maxX - $minX)) * 12.0
        $y = (($yValue - $minY) / ($maxY - $minY)) * 5.0
        $points.Add((Fmt "({0:F3},{1:F3})" $x $y))
    }
    return [string]::Join(" -- ", $points)
}

function Write-TikzPlotTex($path, $rows, [string]$title, [string]$yLabel, [string]$seriesAName, [string]$seriesBName, [string]$aField, [string]$bField) {
    $xValues = @($rows | ForEach-Object { [Math]::Log10([double]$_.Frequency) })
    $yValues = @($rows | ForEach-Object { [double]$_.$aField })
    if ($bField) { $yValues += @($rows | ForEach-Object { [double]$_.$bField }) }

    $minX = ($xValues | Measure-Object -Minimum).Minimum
    $maxX = ($xValues | Measure-Object -Maximum).Maximum
    $minY = ($yValues | Measure-Object -Minimum).Minimum
    $maxY = ($yValues | Measure-Object -Maximum).Maximum
    $padY = ($maxY - $minY) * 0.08
    if ($padY -eq 0) { $padY = 1.0 }
    $minY -= $padY
    $maxY += $padY

    $aPoints = Get-TikzPoints $rows "Frequency" $aField $minX $maxX $minY $maxY -LogX
    $bPoints = if ($bField) { Get-TikzPoints $rows "Frequency" $bField $minX $maxX $minY $maxY -LogX } else { "" }

    $ticks = New-Object System.Collections.Generic.List[string]
    foreach ($row in $rows) {
        $xValue = [double]$row.Frequency
        $x = (([Math]::Log10($xValue) - $minX) / ($maxX - $minX)) * 12.0
        $xText = Fmt "{0:F3}" $x
        $xLabel = Fmt "{0:G4}" $xValue
        $ticks.Add("\draw[gray!25] ($xText,0) -- ($xText,5);")
        $ticks.Add("\node[below, font=\scriptsize, rotate=45] at ($xText,0) {$xLabel};")
    }

    $yTicks = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -le 5; $i++) {
        $y = $i
        $value = $minY + ($i / 5.0) * ($maxY - $minY)
        $valueText = Fmt "{0:F1}" $value
        $yTicks.Add("\draw[gray!25] (0,$y) -- (12,$y);")
        $yTicks.Add("\node[left, font=\scriptsize] at (0,$y) {$valueText};")
    }

    $legendB = if ($bField) { "\draw[very thick, orange!80!black] (8.2,4.35) -- (9.0,4.35); \node[right, font=\scriptsize] at (9.0,4.35) {$seriesBName};" } else { "" }

    $tex = @"
\begin{figure}[H]
\centering
\begin{tikzpicture}[x=0.78cm,y=0.72cm]
\node[font=\bfseries] at (6,5.85) {$title};
$([string]::Join("`n", $yTicks))
$([string]::Join("`n", $ticks))
\draw[->] (0,0) -- (12.45,0) node[right] {Frecuencia (Hz)};
\draw[->] (0,0) -- (0,5.35) node[above] {$yLabel};
\draw[very thick, blue!70!black] $aPoints;
$(if ($bField) { "\draw[very thick, orange!80!black] $bPoints;" } else { "" })
\draw[very thick, blue!70!black] (8.2,4.7) -- (9.0,4.7); \node[right, font=\scriptsize] at (9.0,4.7) {$seriesAName};
$legendB
\end{tikzpicture}
\end{figure}
"@
    Add-Content -Path $path -Value $tex -Encoding ASCII
}

function Get-WaveformTikzBlock($case, [double]$yLimit = 1.8, [int]$maximumPoints = 140) {
    $rows = $case.Rows
    $frequency = [double]$case.Frequency

    $meanCh1 = ($rows | Measure-Object -Property Ch1 -Average).Average
    $meanCh2 = ($rows | Measure-Object -Property Ch2 -Average).Average

    $startIndex = [int][Math]::Floor($rows.Count * 0.35)
    $endSearchIndex = [int][Math]::Floor($rows.Count * 0.65)
    for ($i = $startIndex + 1; $i -le $endSearchIndex; $i++) {
        if ([double]$rows[$i - 1].Ch1 -lt $meanCh1 -and [double]$rows[$i].Ch1 -ge $meanCh1) {
            $startIndex = $i
            break
        }
    }

    $startTime = [double]$rows[$startIndex].Time
    $endTime = $startTime + 2.0 / $frequency
    $endIndex = $startIndex
    while ($endIndex -lt ($rows.Count - 1) -and [double]$rows[$endIndex].Time -le $endTime) {
        $endIndex++
    }

    $availablePoints = [Math]::Max(1, $endIndex - $startIndex)
    $step = [Math]::Max(1, [int][Math]::Ceiling($availablePoints / [double]$maximumPoints))
    $pointsCh1 = New-Object System.Collections.Generic.List[string]
    $pointsCh2 = New-Object System.Collections.Generic.List[string]

    for ($i = $startIndex; $i -lt $endIndex; $i += $step) {
        $cycle = (([double]$rows[$i].Time - $startTime) * $frequency)
        $ch1 = [double]$rows[$i].Ch1 - $meanCh1
        $ch2 = [double]$rows[$i].Ch2 - $meanCh2
        $pointsCh1.Add((Fmt "({0:F4},{1:F4})" $cycle $ch1))
        $pointsCh2.Add((Fmt "({0:F4},{1:F4})" $cycle $ch2))
    }

    $frequencyText = if ($frequency -lt 1000.0) {
        Fmt "{0:F2}~Hz" $frequency
    } else {
        Fmt "{0:F2}~kHz" ($frequency / 1000.0)
    }
    $gainText = Fmt "{0:F3}" ([double]$case.Gain)
    $phaseText = Fmt "{0:F1}" ([double]$case.Phase)
    $lowerTick = Fmt "{0:F1}" (-$yLimit)
    $upperTick = Fmt "{0:F1}" $yLimit
    $ch1Path = [string]::Join(" -- ", $pointsCh1)
    $ch2Path = [string]::Join(" -- ", $pointsCh2)

    return @"
\begin{minipage}[t]{0.48\textwidth}
\centering
\begin{tikzpicture}[x=2.25cm,y=1.0cm]
\draw[gray!20] (0,-$yLimit) grid[xstep=0.5,ystep=0.6] (2,$yLimit);
\draw[gray!55] (0,0) -- (2,0);
\draw[->] (0,-$yLimit) -- (2.08,-$yLimit) node[right, font=\scriptsize] {ciclos};
\draw[->] (0,-$yLimit) -- (0,$($yLimit + 0.18)) node[above, font=\scriptsize] {V};
\node[left, font=\tiny] at (0,-$yLimit) {$lowerTick};
\node[left, font=\tiny] at (0,0) {0};
\node[left, font=\tiny] at (0,$yLimit) {$upperTick};
\foreach \x in {0.5,1,1.5,2} {
    \node[below, font=\tiny] at (\x,-$yLimit) {\x};
}
\draw[very thick, blue!70!black] $ch1Path;
\draw[very thick, orange!85!black] $ch2Path;
\node[font=\scriptsize, align=center] at (1,$($yLimit + 0.48))
{\(f=$frequencyText,\quad |H|=$gainText,\quad \Delta\phi=$phaseText^\circ\)};
\draw[very thick, blue!70!black] (0.08,$($yLimit - 0.18)) -- (0.30,$($yLimit - 0.18));
\node[right, font=\tiny] at (0.30,$($yLimit - 0.18)) {\(V_A\)};
\draw[very thick, orange!85!black] (0.72,$($yLimit - 0.18)) -- (0.94,$($yLimit - 0.18));
\node[right, font=\tiny] at (0.94,$($yLimit - 0.18)) {\(V_B\)};
\end{tikzpicture}
\end{minipage}
"@
}

function Write-WaveformCasesTikz($path, $cases) {
    Set-Content -Path $path -Value "% Formas de onda generadas automaticamente por scripts/analyze_medidas.ps1" -Encoding UTF8

    $groups = @(
        [pscustomobject]@{ Start = 0; Count = 4; Caption = "Formas de onda medidas a baja frecuencia. Se representan las componentes alternas de \(V_A\) y \(V_B\) durante dos periodos, manteniendo la misma escala vertical."; Label = "fig:ondas-medidas-baja" },
        [pscustomobject]@{ Start = 4; Count = 4; Caption = "Formas de onda medidas a frecuencias intermedias. La escala común permite comparar directamente la amplitud de entrada y salida."; Label = "fig:ondas-medidas-media" },
        [pscustomobject]@{ Start = 8; Count = 3; Caption = "Formas de onda medidas a las frecuencias más elevadas. La captura de \(5.23~kHz\) presenta un comportamiento anómalo respecto al resto del barrido."; Label = "fig:ondas-medidas-alta" }
    )

    foreach ($group in $groups) {
        $blocks = New-Object System.Collections.Generic.List[string]
        $lastIndex = [Math]::Min($cases.Count, $group.Start + $group.Count)
        for ($i = $group.Start; $i -lt $lastIndex; $i++) {
            $blocks.Add((Get-WaveformTikzBlock $cases[$i]))
        }

        $body = New-Object System.Collections.Generic.List[string]
        for ($i = 0; $i -lt $blocks.Count; $i++) {
            $body.Add($blocks[$i])
            if (($i % 2) -eq 0 -and $i -lt ($blocks.Count - 1)) {
                $body.Add("\hfill")
            } elseif (($i % 2) -eq 1 -and $i -lt ($blocks.Count - 1)) {
                $body.Add("\par\medskip")
            }
        }

        $figure = @"
\begin{figure}[H]
\centering
$([string]::Join("`n", $body))
\caption{$($group.Caption)}
\label{$($group.Label)}
\end{figure}
"@
        Add-Content -Path $path -Value $figure -Encoding UTF8
    }
}

$results = New-Object System.Collections.Generic.List[object]
$waveformCases = New-Object System.Collections.Generic.List[object]
foreach ($file in Get-ChildItem -Path $InputDir -Filter "MedidaZ1_*Hz.csv" | Sort-Object Name) {
    $nominalFrequency = Get-FrequencyFromName $file.Name
    $data = Read-WaveformsCsv $file.FullName
    $frequency = Get-MeasuredFrequency $data.Rows "Ch1" $nominalFrequency
    $phasorIn = Get-Phasor $data.Rows $frequency "Ch1"
    $phasorOut = Get-Phasor $data.Rows $frequency "Ch2"

    $h = Divide-Complex $phasorOut.Real $phasorOut.Imag $phasorIn.Real $phasorIn.Imag
    $oneMinusH = Sub-Complex 1.0 0.0 $h.Re $h.Im
    $zMeasured = Divide-Complex ($seriesResistance * $h.Re) ($seriesResistance * $h.Im) $oneMinusH.Re $oneMinusH.Im

    $omega = 2.0 * [Math]::PI * $frequency
    $yTheoretical = [pscustomobject]@{ Re = 1.0 / $parallelResistance; Im = $omega * $parallelCapacitance }
    $zTheoretical = Divide-Complex 1.0 0.0 $yTheoretical.Re $yTheoretical.Im
    $hTheoretical = Divide-Complex $zTheoretical.Re $zTheoretical.Im ($seriesResistance + $zTheoretical.Re) $zTheoretical.Im

    $phaseDeg = (($phasorOut.Phase - $phasorIn.Phase) * 180.0 / [Math]::PI)
    while ($phaseDeg -gt 180.0) { $phaseDeg -= 360.0 }
    while ($phaseDeg -lt -180.0) { $phaseDeg += 360.0 }
    $phaseTheoreticalDeg = [Math]::Atan2($hTheoretical.Im, $hTheoretical.Re) * 180.0 / [Math]::PI

    $waveformCases.Add([pscustomobject]@{
        Frequency = $frequency
        Gain = (Get-Mag $h.Re $h.Im)
        Phase = $phaseDeg
        Rows = $data.Rows
    })

    $results.Add([pscustomobject]@{
        NominalFrequency = $nominalFrequency
        Frequency = $frequency
        VinAmplitude = $phasorIn.Magnitude
        VoutAmplitude = $phasorOut.Magnitude
        GainMeasured = (Get-Mag $h.Re $h.Im)
        GainTheoretical = (Get-Mag $hTheoretical.Re $hTheoretical.Im)
        PhaseMeasuredDeg = $phaseDeg
        PhaseTheoreticalDeg = $phaseTheoreticalDeg
        ZMeasuredAbs = (Get-Mag $zMeasured.Re $zMeasured.Im)
        ZMeasuredRe = $zMeasured.Re
        ZMeasuredIm = $zMeasured.Im
        ZTheoreticalAbs = (Get-Mag $zTheoretical.Re $zTheoretical.Im)
        ZTheoreticalRe = $zTheoretical.Re
        ZTheoreticalIm = $zTheoretical.Im
    })
}

$ordered = @($results | Sort-Object Frequency)
$orderedWaveformCases = @($waveformCases | Sort-Object Frequency)
$ordered | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir "medidas_resumen.csv")
Write-WaveformCasesTikz (Join-Path $OutputDir "medidas_ondas_tikz.tex") $orderedWaveformCases

Write-LinePlotSvg (Join-Path $OutputDir "medidas_ganancia.svg") $ordered "Respuesta en amplitud del circuito de prueba" "Frecuencia (Hz)" "|Vout/Vin|" "Medida por Fourier" "Modelo teorico" "Frequency" "GainMeasured" "GainTheoretical" -LogX
Write-LinePlotSvg (Join-Path $OutputDir "medidas_impedancia.svg") $ordered "Impedancia equivalente de la rama R || C" "Frecuencia (Hz)" "|Z| (ohm)" "Estimada desde medidas" "Modelo teorico" "Frequency" "ZMeasuredAbs" "ZTheoreticalAbs" -LogX
Write-LinePlotSvg (Join-Path $OutputDir "medidas_fase.svg") $ordered "Desfase de la salida respecto a la entrada" "Frecuencia (Hz)" "Fase (grados)" "Medida por Fourier" "Modelo teorico" "Frequency" "PhaseMeasuredDeg" "PhaseTheoreticalDeg" -LogX

$tikzPath = Join-Path $OutputDir "medidas_resultados_tikz.tex"
Set-Content -Path $tikzPath -Value "% Figuras generadas automaticamente por scripts/analyze_medidas.ps1" -Encoding ASCII
Write-TikzPlotTex $tikzPath $ordered "Ganancia de salida obtenida mediante Fourier" "\(|V_B/V_A|\)" "Medida" "Modelo" "GainMeasured" "GainTheoretical"
Write-TikzPlotTex $tikzPath $ordered "Modulo de impedancia equivalente" "\(|Z|~(\Omega)\)" "Estimada" "Modelo" "ZMeasuredAbs" "ZTheoreticalAbs"
Write-TikzPlotTex $tikzPath $ordered "Desfase de salida respecto a entrada" "\(\Delta\phi~(^{\circ})\)" "Medida" "Modelo" "PhaseMeasuredDeg" "PhaseTheoreticalDeg"

$tableRows = New-Object System.Collections.Generic.List[string]
foreach ($row in $ordered) {
    $tableRows.Add((Fmt "{0:F2} & {1:F3} & {2:F3} & {3:F0} & {4:F0} & {5:F1} \\" $row.Frequency $row.GainMeasured $row.GainTheoretical $row.ZMeasuredAbs $row.ZTheoreticalAbs $row.PhaseMeasuredDeg))
}
Set-Content -Path (Join-Path $OutputDir "medidas_tabla.tex") -Value ([string]::Join("`n", $tableRows)) -Encoding ASCII

$ordered | Format-Table Frequency, GainMeasured, GainTheoretical, ZMeasuredAbs, ZTheoreticalAbs, PhaseMeasuredDeg
