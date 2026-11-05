# Build, Toolchain & Installation Guide (`BUILD_AND_INSTALL.md`)

This document records the exact commands required to set up the development environment, build, test, and benchmark **TradinationalTUI (`ttui`)** across **Linux**, **macOS**, and **Windows**.

---

## 1. Prerequisites & Toolchain Installation

### 1.1 Linux

#### Fedora / RHEL (Verified on Host: Fedora Linux 44 `x86_64`)
```bash
# 1. Ensure C toolchain, make, pkg-config, and git are present
sudo dnf install -y gcc gcc-c++ make pkg-config git openssl-devel libsecret-devel

# 2. Install Rust toolchain via official rustup (non-interactive, stable profile)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default
source "$HOME/.cargo/env"

# 3. Verify rustfmt and clippy components
rustup component add rustfmt clippy rust-src

# 4. Install recommended CLI developer & testing utilities
cargo install cargo-nextest --locked
```

#### Ubuntu / Debian
```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config git libssl-dev libsecret-1-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustup component add rustfmt clippy rust-src
```

#### Arch Linux
```bash
sudo pacman -S --needed base-devel pkgconf git openssl libsecret rustup
rustup default stable
rustup component add rustfmt clippy rust-src
```

---

### 1.2 macOS (Apple Silicon `aarch64` & Intel `x86_64`)

```bash
# 1. Install Xcode Command Line Tools
xcode-select --install

# 2. Install Rust via rustup (or Homebrew)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustup component add rustfmt clippy rust-src

# 3. Optional fast test runner & CLI search tools via Homebrew
brew install cargo-nextest ripgrep fd fzf just
```

---

### 1.3 Windows (Windows 10 / 11 with Windows Terminal)

```powershell
# 1. Install Visual Studio C++ Build Tools & Git via winget
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Git.Git -e

# 2. Install Rustup (MSVC ABI)
winget install --id Rustlang.Rustup -e

# 3. In a new PowerShell window, configure components
rustup default stable-x86_64-pc-windows-msvc
rustup component add rustfmt clippy rust-src

# 4. Optional developer utilities
winget install --id BurntSushi.ripgrep.MSVC -e
winget install --id sharkdp.fd -e
```

---

## 2. Building, Running & Verifying `ttui`

```bash
# Clone and enter the repository
cd TradinationalTUI

# 1. Check compilation & types
cargo check --all-targets

# 2. Run code formatting & static analysis lints
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# 3. Run unit, property, and headless UI snapshot tests
cargo test --all-targets

# 4. Run smoke test / version check in headless mode
cargo run -- --version
cargo run -- --check-health

# 5. Build optimized release binary
cargo build --release

# 6. Run Criterion performance benchmarks
cargo bench
```

---

## 3. Release Profile Optimization (`Cargo.toml`)

The release profile is tuned for maximum runtime speed, small binary footprint, and fast startup:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "unwind" # Required so panic hook can always restore terminal state cleanly!
```

> **Important Note on `panic = "unwind"`**: Unlike CLI batch tools that use `panic = "abort"`, a TUI IDE must use `panic = "unwind"` so the custom panic hook and RAII `Drop` guards can execute `disable_raw_mode()` and `LeaveAlternateScreen` before process termination.
