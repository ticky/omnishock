# Install SDL2

$SRC_DIR = $PWD.Path

Set-Location $ENV:Temp
$ZIP_FILE = (Join-Path $PWD.Path "SDL2-devel-${SDL_VERSION}-VC.zip")
$SDL_VERSION = "2.0.6"

Invoke-WebRequest -OutFile "$ZIP_FILE" "https://www.libsdl.org/release/SDL2-devel-${SDL_VERSION}-VC.zip"

7z x "$ZIP_FILE"

$SDL2_ARCH = "x64"
If ($Env:TARGET -eq 'i686-pc-windows-msvc') {
  $SDL2_ARCH = "x86"
}

Copy-Item (Join-Path $PWD.Path "SDL2-${SDL_VERSION}\lib\${SDL2_ARCH}\*") (Join-Path $ENV:USERPROFILE ".multirust\toolchains\${ENV:RUST_VERSION}-${ENV:TARGET}\lib\rustlib\${ENV:TARGET}\lib")

Set-Location $SRC_DIR
