# PowerShell script for building the RQ Library for Windows
# Stop on first error
$ErrorActionPreference = "Stop"

Write-Host "Building RQ Library for Windows (x86_64 and i686)..."

# Function to ensure a directory exists
function EnsureDirectory {
    param (
        [string]$Directory
    )
    
    if (-not (Test-Path -Path $Directory)) {
        New-Item -ItemType Directory -Path $Directory -Force | Out-Null
        Write-Host "Created directory: $Directory"
    }
}

# Build for x86_64 (64-bit Windows)
Write-Host "Building for x86_64-pc-windows-msvc..."
rustup target add x86_64-pc-windows-msvc
cargo build --target x86_64-pc-windows-msvc --release

# Create the output directory if it doesn't exist
EnsureDirectory -Directory "dist\lib\windows\x64"

# Copy the built library files for x86_64
Write-Host "Copying built files to dist\lib\windows\x64\"
Copy-Item -Path "target\x86_64-pc-windows-msvc\release\rq_library.dll" -Destination "dist\lib\windows\x64\" -Force
Copy-Item -Path "target\x86_64-pc-windows-msvc\release\rq_library.lib" -Destination "dist\lib\windows\x64\" -Force
Copy-Item -Path "target\x86_64-pc-windows-msvc\release\rq_library.dll.lib" -Destination "dist\lib\windows\x64\" -Force -ErrorAction SilentlyContinue

# Build for i686 (32-bit Windows)
Write-Host "Building for i686-pc-windows-msvc..."
rustup target add i686-pc-windows-msvc
cargo build --target i686-pc-windows-msvc --release

# Create the output directory if it doesn't exist
EnsureDirectory -Directory "dist\lib\windows\x86"

# Copy the built library files for i686
Write-Host "Copying built files to dist\lib\windows\x86\"
Copy-Item -Path "target\i686-pc-windows-msvc\release\rq_library.dll" -Destination "dist\lib\windows\x86\" -Force
Copy-Item -Path "target\i686-pc-windows-msvc\release\rq_library.lib" -Destination "dist\lib\windows\x86\" -Force
Copy-Item -Path "target\i686-pc-windows-msvc\release\rq_library.dll.lib" -Destination "dist\lib\windows\x86\" -Force -ErrorAction SilentlyContinue

Write-Host "Windows build completed. Output files in dist\lib\windows\"