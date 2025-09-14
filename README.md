# d2d_automations

[![Rust CI](https://github.com/elheck/d2d_automations/workflows/Rust%20CI/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/rust.yml)
[![Release](https://github.com/elheck/d2d_automations/workflows/Release/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/release.yml)

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

5. **Run tests**
   ```sh
   cargo test
   ```

6. **Run performance tests**
   ```sh
   cargo test test_search_performance -- --nocapture
   ```

## Testing

The project includes comprehensive test suites:

- **Unit tests**: Basic functionality testing for I/O operations and utilities
- **Integration tests**: Testing the interaction between components
- **Performance tests**: Comprehensive performance testing for the search function

### Performance Testing

The search function performance is tested with various scenarios:
- Different inventory sizes (10 to 10,000+ cards)
- Various language preferences and filtering modes
- Edge cases and concurrent safety
- Memory usage estimation

For detailed information about performance testing, see [`check_stock/PERFORMANCE_TESTING.md`](check_stock/PERFORMANCE_TESTING.md).

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