# d2d_automations

## Overview

d2d_automations is a Rust-based desktop application for Magic: The Gathering stock checking and analysis, featuring a GUI built with egui.

## Building and Running (Development)

1. **Install Rust**
   - If you don't have Rust, install it from [rustup.rs](https://rustup.rs/).

2. **Clone the repository**
   ```sh
   git clone <your-repo-url>
   cd d2d_automations/check_stock
   ```

3. **Build and run in development mode**
   ```sh
   cargo run
   ```
   This will build and launch the application with debug info.

4. **Lint and check code**
   ```sh
   cargo clippy
   cargo fmt
   ```

## Building a Release Binary

To build an optimized release binary:

```sh
cargo build --release
```
The binary will be located at `target/release/d2d_automations` (or `d2d_automations.exe` on Windows).

## Automated Release with GitHub Actions

This project includes a GitHub Actions workflow (`.github/workflows/release.yml`) to build and upload release binaries automatically when you create a new release on GitHub.

**How to trigger a release build:**
1. Push your changes to the main branch.
2. On GitHub, go to the Releases section and create a new release (with a tag, e.g., `v1.0.0`).
3. The workflow will automatically build the project for all configured platforms and upload the binaries as release assets.

You can customize the workflow in `.github/workflows/release.yml` as needed.

## Troubleshooting

- If you encounter build errors, ensure you have the latest stable Rust toolchain: `rustup update`
- For platform-specific issues, check the workflow logs in the GitHub Actions tab.

## License

MIT License. See `LICENSE` file for details.