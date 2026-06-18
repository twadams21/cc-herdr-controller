# Downloads the SDL2 MinGW development libraries into rust/vendor/ so the Rust
# build can link SDL2. Run once after cloning (the libs are gitignored).
#
#   powershell -ExecutionPolicy Bypass -File rust\setup-sdl2.ps1
#
# Requires the GNU Rust toolchain:
#   rustup default stable-x86_64-pc-windows-gnu

$ErrorActionPreference = "Stop"
$ver = "2.28.5"
$dest = Join-Path $PSScriptRoot "vendor"
$marker = Join-Path $dest "SDL2-$ver\x86_64-w64-mingw32\lib\libSDL2.dll.a"

if (Test-Path $marker) {
    Write-Host "SDL2 $ver dev libs already present at $dest"
    exit 0
}

New-Item -ItemType Directory -Force -Path $dest | Out-Null
$url = "https://github.com/libsdl-org/SDL/releases/download/release-$ver/SDL2-devel-$ver-mingw.zip"
$zip = Join-Path $env:TEMP "SDL2-devel-$ver-mingw.zip"

Write-Host "Downloading $url ..."
Invoke-WebRequest -Uri $url -OutFile $zip
Write-Host "Extracting to $dest ..."
Expand-Archive -Path $zip -DestinationPath $dest -Force
Remove-Item $zip -Force

Write-Host "Done. Now build with: cargo build --release --manifest-path rust\Cargo.toml"
