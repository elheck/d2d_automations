# MTG Stock Checker

A desktop application for Magic: The Gathering card inventory management and stock analysis. Built with Rust and egui for a fast, native GUI experience.

## Features

### 🔍 Stock Checker
Compare your inventory against a wantslist to find matching cards:
- **Multi-language support**: Search across English, German, Spanish, French, and Italian card names
- **Flexible matching**: Optionally restrict results to preferred language only
- **Multiple output formats**:
  - **Picking List**: For warehouse order fulfillment with location codes
  - **Invoice List**: Formatted for customer invoices
  - **Stock Update CSV**: Export for bulk inventory updates
- **Discount calculation**: Apply percentage discounts to total price
- **Card selection**: Pick specific copies when multiple matches exist

### 📊 Stock Analysis
Analyze your inventory storage bins:
- View bin capacity utilization
- Find bins with available slots
- Filter by minimum free slots
- Sort by location or available space

### 🔎 Card Search
Search your inventory interactively:
- Real-time search with debouncing
- Filter by set, language, condition
- View card details and locations

### 🃏 Card Lookup
Look up any card by set code and collector number:
- **Quick input workflow**:
  1. Type card (e.g., `hou120` or just `120` with default set) → **Enter**
  2. Adjust quantity if needed (defaults to `1`) → **Enter**
  3. Card is fetched and displayed, ready for next card
- **Default fields**:
  - **Default Set**: Set once (e.g., `hou`), then only type collector numbers
  - **Default Language**: Pre-filled language code (e.g., `EN`) for future use
- **Card images**: Display card artwork fetched from Scryfall
- **Cardmarket prices**: Load comprehensive price data (~50MB) with:
  - Average, 7-day, and 30-day prices
  - Low and trend prices
  - Both regular and foil variants
- **Persistent caching**: Cards and images are cached locally for instant repeat lookups
  - **Linux**: `~/.cache/d2d_automations/`
  - **macOS**: `~/Library/Caches/d2d_automations/`
  - **Windows**: `%LOCALAPPDATA%\d2d_automations\`
- **Input format**: Last 3 digits are collector number, rest is set code
  - `hou120` → set `hou`, collector `120`
  - `mh2130` → set `mh2`, collector `130`
  - Leading zeros stripped automatically (`005` → `5`)

## Architecture

### High-Level Overview

```mermaid
flowchart TB
    subgraph GUI["GUI Layer (egui/eframe)"]
        App[StockCheckerApp]
        Welcome[WelcomeScreen]
        StockChecker[StockCheckerScreen]
        Analysis[StockAnalysisScreen]
        Search[SearchScreen]
        Listing[StockListingScreen]
    end
    
    subgraph Core["Core Business Logic"]
        IO[io.rs]
        Matching[card_matching.rs]
        Formatters[formatters.rs]
        StockAnalysis[stock_analysis.rs]
        Scryfall[scryfall.rs]
    end
    
    subgraph Data["Data Layer"]
        Models[models.rs]
        Card[Card]
        Language[Language]
        WantsEntry[WantsEntry]
    end
    
    subgraph External["External APIs"]
        ScryfallAPI[Scryfall API]
        CardmarketCDN[Cardmarket CDN]
    end
    
    App --> Welcome
    App --> StockChecker
    App --> Analysis
    App --> Search
    App --> Listing
    
    StockChecker --> IO
    StockChecker --> Matching
    StockChecker --> Formatters
    Analysis --> IO
    Analysis --> StockAnalysis
    Search --> IO
    Search --> Matching
    Listing --> Scryfall
    
    Scryfall --> ScryfallAPI
    Scryfall --> CardmarketCDN
    
    IO --> Models
    Matching --> Models
    Formatters --> Models
    StockAnalysis --> Models
```

### Module Structure

```mermaid
graph LR
    subgraph lib["lib.rs (Public API)"]
        direction TB
        card_matching
        formatters
        io
        models
        stock_analysis
        scryfall
        ui
    end
    
    subgraph ui_mod["ui/"]
        direction TB
        app.rs
        state.rs
        language.rs
        components/
        screens/
    end
    
    subgraph screens["ui/screens/"]
        welcome.rs
        stock_checker.rs
        stock_analysis.rs
        search.rs
        stock_listing.rs
    end
    
    ui --> ui_mod
    ui_mod --> screens
```

### Data Flow - Stock Checking

```mermaid
sequenceDiagram
    participant User
    participant GUI as StockCheckerScreen
    participant IO as io.rs
    participant Match as card_matching.rs
    participant Format as formatters.rs
    
    User->>GUI: Select inventory CSV
    User->>GUI: Select wantslist
    User->>GUI: Click "Check Stock"
    
    GUI->>IO: read_csv(inventory_path)
    IO-->>GUI: Vec<Card>
    
    GUI->>IO: read_wantslist(wantslist_path)
    IO-->>GUI: Vec<WantsEntry>
    
    loop For each wanted card
        GUI->>Match: find_matching_cards()
        Match-->>GUI: Vec<MatchedCard>
    end
    
    GUI->>Format: format_picking_list() / format_invoice_list()
    Format-->>GUI: Formatted String
    
    GUI->>User: Display results
```

### Data Flow - Card Lookup

```mermaid
sequenceDiagram
    participant User
    participant GUI as StockListingScreen
    participant Cache as CardCache/ImageCache
    participant Scryfall as scryfall.rs
    participant API as Scryfall API
    participant CDN as Cardmarket CDN
    
    User->>GUI: Type "mh2130" + Enter
    GUI->>GUI: Parse → set: mh2, num: 130
    GUI->>GUI: Focus moves to Qty field
    
    User->>GUI: Enter (confirm qty=1)
    
    GUI->>Cache: Check card cache
    alt Cache hit
        Cache-->>GUI: ScryfallCard
    else Cache miss
        GUI->>Scryfall: fetch_card_cached()
        Scryfall->>API: GET /cards/mh2/130
        API-->>Scryfall: Card JSON
        Scryfall->>Cache: Store card
        Scryfall-->>GUI: ScryfallCard
    end
    
    GUI->>Cache: Check image cache
    alt Cache hit
        Cache-->>GUI: Image bytes
    else Cache miss
        GUI->>Scryfall: fetch_image_cached()
        Scryfall->>API: GET image URL
        API-->>Scryfall: Image bytes
        Scryfall->>Cache: Store image file
        Scryfall-->>GUI: Image bytes
    end
    
    GUI->>GUI: Update default_set to "mh2"
    GUI->>GUI: Clear inputs, focus back to Card
    GUI->>User: Display card image + prices
    
    Note over User,GUI: Next card: just type "131" + Enter + Enter
    
    opt Load prices
        User->>GUI: Click "Load Cardmarket Prices"
        GUI->>Scryfall: PriceGuide::fetch()
        Scryfall->>CDN: GET price_guide_1.json
        CDN-->>Scryfall: ~50MB price data
        Scryfall-->>GUI: PriceGuide
    end
    
    GUI->>User: Display card image + prices
```

### Core Components

#### `models.rs`
Domain models representing the core data structures:

| Type | Purpose |
|------|---------|
| `Card` | Represents a card in inventory with all attributes (name, set, condition, price, location, etc.) |
| `Language` | Enum for supported languages with parsing and conversion helpers |
| `WantsEntry` | A single entry from a wantslist (quantity + card name) |

#### `io.rs`
File I/O operations:
- `read_csv()`: Parse Cardmarket-format inventory CSV exports
- `read_wantslist()`: Parse simple "quantity name" format wantslists

#### `card_matching.rs`
Card matching and search logic:
- Multi-language name matching
- Language preference sorting
- Price-based sorting for optimal matches
- Quantity aggregation across sets

#### `formatters.rs`
Output formatting for different use cases:
- `format_regular_output()`: Human-readable match results
- `format_picking_list()`: Warehouse picking with locations
- `format_invoice_list()`: Customer invoice format
- `format_update_stock_csv()`: CSV export for stock updates

#### `stock_analysis.rs`
Inventory bin analysis:
- Bin capacity tracking (60 cards per bin)
- Free slot calculation
- Location-based sorting

#### `scryfall.rs`
External API integration and caching:

| Type | Purpose |
|------|---------|
| `ScryfallCard` | Card data from Scryfall API (name, set, images, prices) |
| `CardCache` | Persistent JSON cache for card lookups |
| `ImageCache` | File-based cache for card images |
| `PriceGuide` | Cardmarket price data lookup by product ID |
| `PriceGuideEntry` | Individual price entry with avg, trend, low prices |

Key functions:
- `fetch_card()` / `fetch_card_cached()`: Get card data from Scryfall
- `fetch_image()` / `fetch_image_cached()`: Get card images
- `PriceGuide::fetch()`: Download Cardmarket price guide (~50MB)

### UI Architecture

```mermaid
stateDiagram-v2
    [*] --> Welcome
    Welcome --> StockChecker: "Stock Checker" button
    Welcome --> StockAnalysis: "Stock Analysis" button
    Welcome --> Search: "Search Cards" button
    Welcome --> StockListing: "Card Lookup" button
    
    StockChecker --> Welcome: "← Back"
    StockAnalysis --> Welcome: "← Back"
    Search --> Welcome: "← Back"
    StockListing --> Welcome: "← Back"
```

The UI follows a simple screen-based navigation pattern:
- **AppState**: Shared state for the stock checker screen
- **StockAnalysisState**: Isolated state for bin analysis
- **SearchState**: Isolated state for card search
- **StockListingState**: Isolated state for card lookup with:
  - `default_set`: Remembered set code for quick entry
  - `default_language`: Pre-filled language code
  - `card_input` / `quantity_input`: Current entry fields
  - `focus_request`: One-shot focus management for workflow
  - `card_cache` / `image_cache`: Persistent caches

## Usage

### Building

```bash
cd check_stock
cargo build --release
```

The optimized binary will be at `target/release/d2d_automations`.

### Running

```bash
# With default debug logging
cargo run --release

# With different log levels
RUST_LOG=info cargo run --release
RUST_LOG=warn cargo run --release
RUST_LOG=d2d_automations=trace cargo run --release
```

### Input File Formats

#### Inventory CSV (Cardmarket Export)
Standard Cardmarket stock export with columns:
- `cardmarketId`, `quantity`, `name`, `set`, `setCode`, `cn`
- `condition`, `language`, `isFoil`, `isPlayset`, `isSigned`
- `price`, `comment`, `location`
- `nameDE`, `nameES`, `nameFR`, `nameIT` (localized names)
- `rarity`, `listedAt`

#### Wantslist
Simple text file, one entry per line:
```
4 Lightning Bolt
2 Counterspell
1 Black Lotus
```

Lines starting with "Deck" are ignored (supports MTG deck export formats).

### Output Formats

#### Picking List
Optimized for warehouse picking with:
- Cards grouped by location prefix
- Sorted by bin location for efficient walking
- Quantity indicators

#### Invoice List
Customer-facing format with:
- Card names and quantities
- Prices with optional discount
- Total calculations

#### Stock Update CSV
For bulk updates:
- Cardmarket-compatible format
- Reduced quantities for matched cards

## Development

### Project Structure

```
check_stock/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs              # Entry point, logger init
│   ├── lib.rs               # Public API exports
│   ├── models.rs            # Domain models
│   ├── io.rs                # File I/O
│   ├── card_matching.rs     # Matching logic
│   ├── formatters.rs        # Output formatters
│   ├── stock_analysis.rs    # Bin analysis
│   ├── scryfall.rs          # Scryfall API + caching
│   └── ui/
│       ├── mod.rs
│       ├── app.rs           # Main app, screen routing
│       ├── state.rs         # Application state
│       ├── language.rs      # UI language enum
│       ├── components/      # Reusable UI components
│       └── screens/         # Screen implementations
│           ├── mod.rs
│           ├── welcome.rs
│           ├── stock_checker.rs
│           ├── stock_analysis.rs
│           ├── search.rs
│           └── stock_listing.rs
└── tests/
    ├── io_tests.rs
    ├── performance_tests.rs
    └── fixtures/            # Test data files
```

### Running Tests

```bash
cargo test
```

### Code Quality

```bash
./run_quality_checks.sh
# Or manually:
cargo clippy -- -D warnings
cargo fmt --check
```

### Dependencies

| Crate | Purpose |
|-------|---------|
| `eframe` | Native GUI framework (egui backend) |
| `egui_extras` | Additional egui widgets and image support |
| `csv` | CSV parsing |
| `serde` + `serde_json` | Serialization/deserialization |
| `chrono` | Date/time handling |
| `rfd` | Native file dialogs |
| `regex` | Pattern matching |
| `log` + `env_logger` | Logging infrastructure |
| `reqwest` | HTTP client for API requests |
| `image` | Image decoding for card artwork |
| `dirs` | Cross-platform cache directory detection |

## External APIs

### Scryfall API
- **Endpoint**: `GET https://api.scryfall.com/cards/{set}/{number}`
- **Purpose**: Fetch card data and image URLs
- **Rate limiting**: Respect Scryfall's fair use guidelines

### Cardmarket Price Guide
- **URL**: `https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json`
- **Purpose**: Comprehensive pricing data for all cards
- **Size**: ~50MB (contains all price points for all products)

## License

Part of the d2d_automations project.
