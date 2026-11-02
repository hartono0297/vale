# Builds the embedded fonts in assets\fonts: static, Latin-subset TTF files of
# DM Sans, DM Mono and JetBrains Mono, plus assets\fonts\OFL.txt.
# app.rc embeds the TTFs as RCDATA 203-210; keep the file names in sync.
#
# Sources are pinned by commit and SHA-256 below and downloaded into a fresh
# scratch directory (-WorkDir, default a new folder under %TEMP%), never into
# the repo. fontTools runs in a venv inside that directory. Variable fonts are
# instanced with fontTools.varLib.instancer, renamed, then subset with
# pyftsubset (fontTools.subset). Output is raw TTF: DirectWrite in-memory
# loading does not take WOFF/WOFF2.
#
# DM Sans comes from the upstream googlefonts/dm-fonts repo instead of
# google/fonts: the google/fonts build predates the tabular-figures update and
# has no tnum feature, which the clock and timers need.
#
# JetBrains Mono comes from the upstream JetBrains/JetBrainsMono repo (static
# TTFs at the v2.304 tag commit). Its programming ligatures are dropped in the
# subset (no calt, liga, dlig), so "->", "!=", "==" and "www" are drawn
# literally by DirectWrite and GDI alike.
#
# Requires Windows PowerShell 5.1+ and Python 3.13 (the py launcher or python
# on PATH). Re-run after changing a pin; commit the regenerated files.

param(
    [string]$WorkDir,
    [switch]$NoHinting
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path $Repo 'assets\fonts'

$GoogleFonts = 'https://raw.githubusercontent.com/google/fonts'
$DmFonts = 'https://raw.githubusercontent.com/googlefonts/dm-fonts'
$JetBrainsMono = 'https://raw.githubusercontent.com/JetBrains/JetBrainsMono'
$Sources = @(
    @{ Name = 'DMSans-VF.ttf'; Sha256 = 'A023F465D2F26D8F56C6E3CAC6AECE08B7A66EB75B4D75EEEF9DF838C53C81F6'
       Url = "$DmFonts/4412393b7d2de9fe7a92064c2dce9b5af5d7fd26/Sans/fonts/variable/DMSans%5Bopsz,wght%5D.ttf" }
    @{ Name = 'DMSans-OFL.txt'; Sha256 = '9AF36190332437F5ECD09974DE43C1F7C77A310A996CDD8CEB25628B458840E1'
       Url = "$DmFonts/4412393b7d2de9fe7a92064c2dce9b5af5d7fd26/Sans/OFL.txt" }
    @{ Name = 'DMMono-Regular.ttf'; Sha256 = '55B4C98F123DAEBB3ED27947BA47B2AF00554FC6284D639A540BCEF5E6258AD2'
       Url = "$GoogleFonts/bfb7c046dea572dca9fdd435481ba2f36b0a1ebc/ofl/dmmono/DMMono-Regular.ttf" }
    @{ Name = 'DMMono-Medium.ttf'; Sha256 = 'FD327DAF461DB87B44A87DEF475D251BF03B997F7C07D9680592D75DBBFAAD0B'
       Url = "$GoogleFonts/bfb7c046dea572dca9fdd435481ba2f36b0a1ebc/ofl/dmmono/DMMono-Medium.ttf" }
    @{ Name = 'DMMono-OFL.txt'; Sha256 = '2BADA5EA45C3C63B7F1EA1F88CE9672C9E4F0C42B2C3B7378949084FE55A3066'
       Url = "$GoogleFonts/bfb7c046dea572dca9fdd435481ba2f36b0a1ebc/ofl/dmmono/OFL.txt" }
    @{ Name = 'JetBrainsMono-Regular.ttf'; Sha256 = 'A0BF60EF0F83C5ED4D7A75D45838548B1F6873372DFAC88F71804491898D138F'
       Url = "$JetBrainsMono/cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9/fonts/ttf/JetBrainsMono-Regular.ttf" }
    @{ Name = 'JetBrainsMono-Medium.ttf'; Sha256 = '31C92D01A8A08528B718A43ADDF0AD3DF0AF2CA4B7B3290A452F70F358E14D3D'
       Url = "$JetBrainsMono/cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9/fonts/ttf/JetBrainsMono-Medium.ttf" }
    @{ Name = 'JetBrainsMono-SemiBold.ttf'; Sha256 = '1B3BFA1ED5665A4CE3F9FEB68D2D4E40E70BF8B4B7D9A3EDD418F321B4E166A0'
       Url = "$JetBrainsMono/cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9/fonts/ttf/JetBrainsMono-SemiBold.ttf" }
    @{ Name = 'JetBrainsMono-OFL.txt'; Sha256 = '30F0C136E3C88E422D0791ACD97238870F9054A9729BC34CF2FF0D4ED8CAC4AD'
       Url = "$JetBrainsMono/cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9/OFL.txt" }
)
$PipPackages = @('fonttools==4.67.0', 'brotli==1.2.1')

# Fixed head.modified so rebuilding from the same pins gives identical bytes.
$env:SOURCE_DATE_EPOCH = '1792972800'

$BuildPy = @'
import os
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

src_dir, work_dir, out_dir, hinting = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == "1"

UNICODES = ("U+0020-007E,U+00A0-00FF,U+0100-017F,U+2010-2027,U+2030-203A,"
            "U+20AC,U+2122,U+2190-2193,U+2212,U+2713")
FEATURES = "kern,liga,calt,ccmp,locl,mark,mkmk,tnum,pnum,lnum"
# JetBrains Mono: the same list without calt, liga and dlig (ligatures dropped).
FEATURES_NO_LIGA = "kern,ccmp,locl,mark,mkmk,tnum,pnum,lnum"
NAME_IDS = "0,1,2,3,4,5,6,13,14,16,17"
WEIGHT_NAMES = {400: "Regular", 500: "Medium", 600: "SemiBold"}

# (source, axis location or None for a static source, output, family, weight,
# layout features)
JOBS = [
    ("DMSans-VF.ttf", {"wght": 400, "opsz": 12}, "DMSans-400.ttf", "DM Sans", 400, FEATURES),
    ("DMSans-VF.ttf", {"wght": 500, "opsz": 12}, "DMSans-500.ttf", "DM Sans", 500, FEATURES),
    ("DMSans-VF.ttf", {"wght": 600, "opsz": 12}, "DMSans-600.ttf", "DM Sans", 600, FEATURES),
    ("DMMono-Regular.ttf", None, "DMMono-400.ttf", "DM Mono", 400, FEATURES),
    ("DMMono-Medium.ttf", None, "DMMono-500.ttf", "DM Mono", 500, FEATURES),
    ("JetBrainsMono-Regular.ttf", None, "JetBrainsMono-400.ttf", "JetBrains Mono", 400, FEATURES_NO_LIGA),
    ("JetBrainsMono-Medium.ttf", None, "JetBrainsMono-500.ttf", "JetBrains Mono", 500, FEATURES_NO_LIGA),
    ("JetBrainsMono-SemiBold.ttf", None, "JetBrainsMono-600.ttf", "JetBrains Mono", 600, FEATURES_NO_LIGA),
]


def set_names(font, family, weight):
    # Contract with the loader: typographic and legacy family are both the bare
    # family name; weight comes from usWeightClass. Every face is upright.
    typo_sub = WEIGHT_NAMES[weight]
    legacy_sub = "Regular"
    ps_name = f"{family.replace(' ', '')}-{typo_sub.replace(' ', '')}"
    name = font["name"]
    version = (name.getDebugName(5) or "Version 1.000").split(";")[0]
    values = {
        1: family,
        2: legacy_sub,
        3: f"{version.replace('Version ', '')};{ps_name};dynamic-island-subset",
        4: f"{family} {typo_sub}",
        6: ps_name,
        16: family,
        17: typo_sub,
    }
    for name_id, value in values.items():
        name.removeNames(nameID=name_id)
        name.setName(value, name_id, 3, 1, 0x409)

    os2 = font["OS/2"]
    os2.usWeightClass = weight
    sel = os2.fsSelection & ~((1 << 0) | (1 << 5) | (1 << 6))
    if weight == 400:
        sel |= 1 << 6
    os2.fsSelection = sel
    font["head"].macStyle = font["head"].macStyle & ~0b11
    if "STAT" in font:
        del font["STAT"]


os.makedirs(work_dir, exist_ok=True)
os.makedirs(out_dir, exist_ok=True)
for src, location, out, family, weight, features in JOBS:
    font = TTFont(os.path.join(src_dir, src))
    if location is not None:
        font = instancer.instantiateVariableFont(font, location, static=True)
    set_names(font, family, weight)
    tmp = os.path.join(work_dir, out)
    font.save(tmp)
    args = [
        tmp,
        f"--output-file={os.path.join(out_dir, out)}",
        f"--unicodes={UNICODES}",
        f"--layout-features={features}",
        f"--name-IDs={NAME_IDS}",
    ]
    if not hinting:
        args.append("--no-hinting")
    subset.main(args)  # pyftsubset
    print(f"built {out} from {src} {location or ''}")
'@

$VerifyPy = @'
import os
import sys

from fontTools.ttLib import TTFont

out_dir = sys.argv[1]
# file -> (family, typographic subfamily, legacy subfamily, weight); all upright
EXPECT = {
    "DMSans-400.ttf": ("DM Sans", "Regular", "Regular", 400),
    "DMSans-500.ttf": ("DM Sans", "Medium", "Regular", 500),
    "DMSans-600.ttf": ("DM Sans", "SemiBold", "Regular", 600),
    "DMMono-400.ttf": ("DM Mono", "Regular", "Regular", 400),
    "DMMono-500.ttf": ("DM Mono", "Medium", "Regular", 500),
    "JetBrainsMono-400.ttf": ("JetBrains Mono", "Regular", "Regular", 400),
    "JetBrainsMono-500.ttf": ("JetBrains Mono", "Medium", "Regular", 500),
    "JetBrainsMono-600.ttf": ("JetBrains Mono", "SemiBold", "Regular", 600),
}
LIMIT = 370 * 1024

errors, total = [], 0
for file, (family, typo_sub, legacy_sub, weight) in EXPECT.items():
    path = os.path.join(out_dir, file)
    size = os.path.getsize(path)
    total += size
    font = TTFont(path)
    name = font["name"]
    ids = {i: name.getDebugName(i) for i in (1, 2, 16, 17)}
    os2, head = font["OS/2"], font["head"]
    gsub = sorted({r.FeatureTag for r in font["GSUB"].table.FeatureList.FeatureRecord}) if "GSUB" in font else []
    gpos = sorted({r.FeatureTag for r in font["GPOS"].table.FeatureList.FeatureRecord}) if "GPOS" in font else []
    print(f"{file}: {size} bytes ({size / 1024:.1f} KB), {len(font.getGlyphOrder())} glyphs")
    print(f"  name 1={ids[1]!r} 2={ids[2]!r} 16={ids[16]!r} 17={ids[17]!r}")
    print(f"  usWeightClass={os2.usWeightClass} fsSelection={os2.fsSelection:#011b} macStyle={head.macStyle:#06b}")
    print(f"  GSUB={gsub} GPOS={gpos}")
    want = {1: family, 2: legacy_sub, 16: family, 17: typo_sub}
    for i, value in want.items():
        if ids[i] != value:
            errors.append(f"{file}: name {i} is {ids[i]!r}, expected {value!r}")
    if os2.usWeightClass != weight:
        errors.append(f"{file}: usWeightClass {os2.usWeightClass}, expected {weight}")
    if os2.fsSelection & 1 or head.macStyle & 2:
        errors.append(f"{file}: italic bit set")
    if os2.fsSelection & (1 << 5) or head.macStyle & 1:
        errors.append(f"{file}: bold bit set")
    if "fvar" in font:
        errors.append(f"{file}: still variable")
    if file.startswith("DMSans") and "tnum" not in gsub:
        errors.append(f"{file}: tnum feature missing")
    cmap = font.getBestCmap()
    for cp in (0x20, 0x7E, 0xE9, 0x2013, 0x2026, 0x20AC):
        if cp not in cmap:
            errors.append(f"{file}: U+{cp:04X} missing")
    if file.startswith("JetBrainsMono"):
        for tag in ("calt", "liga", "dlig"):
            if tag in gsub:
                errors.append(f"{file}: ligature feature {tag} still in GSUB")
        hmtx = font["hmtx"]
        widths = {cp: hmtx[cmap[cp]][0] for cp in range(0x30, 0x3A)}
        if len(set(widths.values())) != 1:
            errors.append(f"{file}: digit advances differ {widths}")
print(f"total {total} bytes ({total / 1024:.1f} KB), limit {LIMIT // 1024} KB")
if total > LIMIT:
    errors.append("total size over limit")
if errors:
    print("\n".join(errors))
    sys.exit(1)
print("verify ok")
'@

function Invoke-Native {
    param([string]$Exe, [string[]]$ArgList)
    & $Exe @ArgList
    if ($LASTEXITCODE -ne 0) { throw "$Exe exited with $LASTEXITCODE" }
}

function Write-Utf8Lf([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText($Path, ($Text -replace "`r`n", "`n"), (New-Object Text.UTF8Encoding($false)))
}

if (-not $WorkDir) {
    $WorkDir = Join-Path ([IO.Path]::GetTempPath()) ('dynamic-island-fonts-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
}
# Make WorkDir absolute against $PWD. Windows PowerShell 5.1 does not move the
# process working directory on Set-Location, so a relative path would resolve
# to one place for cmdlets and another for py/python and their arguments.
$WorkDir = [IO.Path]::GetFullPath($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($WorkDir))
$repoPrefix = [IO.Path]::GetFullPath($Repo).TrimEnd('\') + '\'
if (($WorkDir.TrimEnd('\') + '\').StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "WorkDir '$WorkDir' is inside the repo; pass a directory outside $Repo"
}
if ((Test-Path $WorkDir) -and (Get-ChildItem -Force $WorkDir | Select-Object -First 1)) {
    throw "WorkDir '$WorkDir' is not empty; pass a fresh directory"
}
$SrcDir = Join-Path $WorkDir 'src'        # downloads only
$ToolDir = Join-Path $WorkDir 'tool'      # our scripts
$InstDir = Join-Path $WorkDir 'instances'
$VenvDir = Join-Path $WorkDir 'venv'
New-Item -ItemType Directory -Force $SrcDir, $ToolDir | Out-Null
Write-Host "work dir: $WorkDir"

foreach ($s in $Sources) {
    $dest = Join-Path $SrcDir $s.Name
    Invoke-WebRequest -UseBasicParsing -Uri $s.Url -OutFile $dest
    $hash = (Get-FileHash -Algorithm SHA256 $dest).Hash
    if ($hash -ne $s.Sha256) { throw "$($s.Name): SHA-256 $hash does not match pinned $($s.Sha256)" }
    Write-Host "ok  $($s.Name)  $hash"
}

if (Get-Command py -ErrorAction SilentlyContinue) {
    Invoke-Native 'py' @('-3.13', '-I', '-m', 'venv', $VenvDir)
} else {
    Invoke-Native 'python' @('-I', '-m', 'venv', $VenvDir)
}
$Py = Join-Path $VenvDir 'Scripts\python.exe'
Invoke-Native $Py (@('-I', '-m', 'pip', 'install', '--quiet', '--disable-pip-version-check') + $PipPackages)

$build = Join-Path $ToolDir 'build_fonts.py'
$verify = Join-Path $ToolDir 'verify_fonts.py'
Write-Utf8Lf $build $BuildPy
Write-Utf8Lf $verify $VerifyPy
$hint = if ($NoHinting) { '0' } else { '1' }
Invoke-Native $Py @('-I', $build, $SrcDir, $InstDir, $OutDir, $hint)

$ofl = @(
    @{ File = 'DMSans-OFL.txt'; Title = 'DM Sans (Colophon Foundry, Google Fonts): DMSans-400.ttf, DMSans-500.ttf, DMSans-600.ttf' }
    @{ File = 'DMMono-OFL.txt'; Title = 'DM Mono (Colophon Foundry, Google Fonts): DMMono-400.ttf, DMMono-500.ttf' }
    @{ File = 'JetBrainsMono-OFL.txt'; Title = 'JetBrains Mono (The JetBrains Mono Project Authors): JetBrainsMono-400.ttf, JetBrainsMono-500.ttf, JetBrainsMono-600.ttf' }
)
$text = "The fonts in this folder are subset static instances of the families below,`n" +
        "built by scripts/make-fonts.ps1. Each is licensed under the SIL Open Font`n" +
        "License, Version 1.1; the upstream licence text with its copyright line follows.`n"
foreach ($o in $ofl) {
    $body = [IO.File]::ReadAllText((Join-Path $SrcDir $o.File)).TrimEnd()
    $text += "`n" + ('=' * 78) + "`n" + $o.Title + "`n" + ('=' * 78) + "`n`n" + $body + "`n"
}
Write-Utf8Lf (Join-Path $OutDir 'OFL.txt') $text

Invoke-Native $Py @('-I', $verify, $OutDir)
Write-Host "fonts written to $OutDir"
