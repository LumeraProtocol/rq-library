# Build

## Native Library (C compatible)

```bash
cargo build --release
```

This will build the native library in release mode for DEFAULT target.<br/> 
The library will be located in:
* `target/release/lib<name>.so` on Linux
* `target/release/lib<name>.dylib` on macOS
* `target/release/<name>.dll` on Windows.

To verify default target
```bash
rustc -vV | grep host
```

## Browser WASM (Emscripten)

```bash
./build_emscripten.sh
```

## Non-native targets

Following is a list of supported targets on different platforms:
* macOS
  * iOS
  * macOS Intel
  * macOS Apple Silicon
  * Windows with `x86_64-w64-mingw32-gcc` (`brew install mingw-w64`)
  * Linux static
* Linux
  * Android
  * Linux
  * Linux static
* Windows
  * Windows

### Building targets
> drop the `--release` flag for debug builds

#### macOS - Intel
```bash
cargo build --target x86_64-apple-darwin --release
```

#### macOS - Apple Silicon
```bash
cargo build --target aarch64-apple-darwin --release
```

#### iOS
```bash
cargo build --target aarch64-apple-ios --release
```

#### Linux - dynamic linking
```bash
cargo build --target x86_64-unknown-linux-gnu --release
```

#### Linux - static linking
```bash
cargo build --target x86_64-unknown-linux-musl --release
```

#### Android
```bash
cargo build --target aarch64-linux-android --release
```

#### Windows
```bash
cargo build --target x86_64-pc-windows-msvc --release
```


### WIP!!! - Cross Compilation with Zig

Install `Zig`

#### MacOS
```bash
brew install zig
```
#### Windows
```bash
winget install -e --id zig.zig
```
#### Linux
```bash
sudo apt install zig
```
#### Check installation
```bash
zig version
```