# SevDesk Invoice Creator

A Rust application with egui GUI for creating invoices in SevDesk from CSV data.

## Features

- Load order data from CSV files (tab-separated format)
- Parse multiple items per order correctly (no duplicate invoices)
- Validate CSV data before processing
- Test SevDesk API connection
- Automatically create or find customers in SevDesk
- Create invoices with line items for products and shipping
- Support for Kleingewerbe (tax rule 11, 0% VAT)
- German address format parsing (postal code + city)
- Real-time progress tracking during invoice creation
- Error handling and reporting
- Configurable logging levels

## Setup

1. **Get your SevDesk API Token:**
   - Log into your SevDesk account
   - Go to Settings → API
   - Generate a new API token
   - Set it as an environment variable: `export SEVDESK_API="your_token_here"`

2. **Install Rust:**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **Install system dependencies (Linux):**
   ```bash
   sudo apt-get update
   sudo apt-get install -y \
     build-essential \
     pkg-config \
     libssl-dev \
     libfontconfig1-dev \
     libfreetype6-dev \
     libxcb1-dev \
     libxcb-render0-dev \
     libxcb-shape0-dev \
     libxcb-xfixes0-dev \
     libxkbcommon-dev
   ```

4. **Build and run:**
   ```bash
   cd accounting
   cargo run
   ```

5. **Adjust logging level (optional):**
   ```bash
   # For debug output
   RUST_LOG=sevdesk_invoicing=debug cargo run
   
   # Default is info level
   cargo run
   ```

## Development

### Testing
```bash
cargo test
```

### Code formatting
```bash
cargo fmt
```

### Linting
```bash
cargo clippy
```

### Building for release
```bash
cargo build --release
```

## CI/CD

This project uses GitHub Actions for continuous integration and deployment:

### Continuous Integration (rust.yml)
- **Triggers:** Push to main branch, pull requests
- **Jobs:**
  - Code formatting check (`cargo fmt`)
  - Linting with Clippy (`cargo clippy`)
  - Build verification (`cargo build`)
  - Test execution (`cargo test`)
  - Release build (`cargo build --release`)

### Release Builds (release.yml)
- **Triggers:** Git tags starting with 'v' (e.g., `v1.0.0`)
- **Artifacts:** Cross-platform binaries for Linux and Windows
- **Assets:** Compressed archives attached to GitHub releases

### Running CI locally
```bash
# Format check
cargo fmt --all -- --check

# Clippy linting
cargo clippy --all-targets --all-features -- -D warnings

# Tests
cargo test --verbose

# Release build
cargo build --release --verbose
```

## CSV Format

The application expects tab-separated CSV files with the following columns:

- OrderID
- Username
- Name (Customer name)
- Street
- City
- Country
- Is Professional (optional)
- VAT Number (optional)
- Date of Purchase
- Article Count
- Merchandise Value
- Shipment Costs
- Total Value
- Commission
- Currency
- Description
- Product ID
- Localized Product Name

Example:
```
OrderID	Username	Name	Street	City	Country	Is Professional	VAT Number	Date of Purchase	Article Count	Merchandise Value	Shipment Costs	Total Value	Commission	Currency	Description	Product ID	Localized Product Name
1218804750	notsaicana	Lucas Cordeiro	Hedwig-Porschütz-Straße 28	10557 Berlin	Germany			2025-07-01 22:42:27	1	1,87	1,25	3,12	0,10	EUR	1x High Fae Trickster (Magic: The Gathering Foundations) - 40 - Rare - NM - English - 1,87 EUR	795560	High Fae Trickster
```

## Usage

1. **Set API Token:** Enter your SevDesk API token in the application (or set the `SEVDESK_API` environment variable)
2. **Test Connection:** Click "Test Connection" to verify your API token works
3. **Load CSV:** Click "Select CSV File" to load your order data
4. **Create Invoices:** Click "Create Invoices" to process all orders

The application will:
- Create customers in SevDesk if they don't exist
- Generate invoices with appropriate line items
- Handle tax calculations (19% VAT for Germany)
- Display progress and results in real-time

## Error Handling

The application validates CSV data and handles various error conditions:
- Missing required fields
- Invalid price formats
- API connection issues
- Duplicate processing protection

All errors are displayed in the GUI with detailed messages.

## Technical Details

- **Framework:** Iced GUI framework for cross-platform desktop apps
- **HTTP Client:** Reqwest for SevDesk API communication
- **CSV Processing:** CSV crate for parsing tab-separated files
- **Async Runtime:** Tokio for handling API requests
- **Serialization:** Serde for JSON handling

## Configuration

The application uses these default settings:
- Tax rate: 19% (German VAT)
- Invoice status: Draft (100)
- Currency: Taken from CSV data
- Contact category: Customer (3)
- Address category: Billing (47)
- Unity: Piece (1)

These can be modified in the source code if needed for your specific requirements.
