# SevDesk API Module

This module provides functionality for interacting with the SevDesk API to create invoices, manage contacts, and handle country lookups with caching.

## Architecture Overview

```mermaid
graph TB
    subgraph "Public Interface"
        API[SevDeskApi]
    end
    
    subgraph "Core Operations"
        INV[invoices]
        SIM[simulation]
    end
    
    subgraph "Entity Management"
        CON[contacts]
        USR[users]
        CTR[countries]
    end
    
    subgraph "Infrastructure"
        CLI[client]
        UTL[utils]
    end
    
    subgraph "External"
        SD[SevDesk API<br>my.sevdesk.de/api/v1]
    end
    
    API -->|"create_invoice()"| INV
    API -->|"simulate_invoice_creation()"| SIM
    API -->|"test_connection()"| CLI
    
    INV -->|uses| CON
    INV -->|uses| USR
    INV -->|uses| CTR
    INV -->|uses| UTL
    
    SIM -->|uses| CTR
    SIM -->|uses| UTL
    
    CON -->|uses| CTR
    
    CLI --> SD
    CON --> SD
    USR --> SD
    CTR --> SD
    INV --> SD
```

## Module Structure

```
sevdesk_api/
├── mod.rs           # SevDeskApi struct definition & re-exports
├── client.rs        # HTTP client wrapper, connection testing
├── contacts.rs      # Customer/contact CRUD operations
├── countries.rs     # Country ID resolution with caching
├── invoices.rs      # Invoice creation and line items
├── simulation.rs    # Dry-run validation without API calls
├── users.rs         # User information retrieval
├── utils.rs         # Price parsing utilities
└── tests/           # Unit tests
    ├── mod.rs
    ├── construction_tests.rs
    ├── countries_tests.rs
    ├── models_tests.rs
    └── utils_tests.rs
```

## Components

### SevDeskApi (mod.rs)

The main API client that provides access to all SevDesk operations.

```rust
pub struct SevDeskApi {
    client: Client,
    api_token: String,
    base_url: String,
    country_cache: Arc<RwLock<CountryCache>>,
}

impl SevDeskApi {
    pub fn new(api_token: String) -> Self;
    pub async fn create_invoice(&self, order: &OrderRecord) -> Result<InvoiceCreationResult>;
    pub async fn simulate_invoice_creation(&self, order: &OrderRecord) -> Result<InvoiceCreationResult>;
    pub async fn test_connection(&self) -> Result<bool>;
}
```

**Responsibilities:**
- Hold HTTP client and authentication
- Provide thread-safe country caching
- Expose high-level API operations

### client.rs

HTTP client wrapper and connection testing.

```rust
impl SevDeskApi {
    pub async fn test_connection(&self) -> Result<bool>;
}
```

**Responsibilities:**
- Test API connectivity
- Verify authentication token validity

### contacts.rs

Customer/contact management via SevDesk Contact API.

```rust
impl SevDeskApi {
    async fn get_or_create_contact(&self, order: &OrderRecord) -> Result<u32>;
}
```

**Contact Flow:**
```mermaid
flowchart TD
    A[OrderRecord] --> B{Search by name}
    B -->|Found| C[Return existing ID]
    B -->|Not found| D[Create new contact]
    D --> E[Set address with country]
    E --> F[Return new contact ID]
```

### countries.rs

Country ID resolution with in-memory caching.

```rust
struct CountryCache {
    name_to_id: HashMap<String, u32>,
    loaded: bool,
}

impl SevDeskApi {
    async fn fetch_countries(&self) -> Result<()>;
    async fn get_country_id(&self, country_name: &str) -> Result<u32>;
}
```

**Caching Strategy:**
```mermaid
sequenceDiagram
    participant Caller
    participant API as SevDeskApi
    participant Cache as CountryCache
    participant SD as SevDesk API
    
    Caller->>API: get_country_id("Germany")
    API->>Cache: Check if loaded
    
    alt Not loaded
        API->>SD: GET /StaticCountry
        SD-->>API: Country list
        API->>Cache: Populate name_to_id map
        Note over Cache: Store both local & English names
    end
    
    API->>Cache: Lookup "germany" (lowercase)
    Cache-->>API: ID 1
    API-->>Caller: 1
```

**Cache Features:**
- Case-insensitive lookups
- Stores both local and English country names
- Common aliases (e.g., "UK" → "United Kingdom")
- Partial matching fallback
- Defaults to Germany (ID: 1) if unknown

### invoices.rs

Invoice creation and line item management.

```rust
impl SevDeskApi {
    pub async fn create_invoice(&self, order: &OrderRecord) -> Result<InvoiceCreationResult>;
    async fn create_invoice_internal(&self, order: &OrderRecord) -> Result<(String, String)>;
    async fn add_invoice_position(&self, invoice_id: &str, ...) -> Result<()>;
}
```

**Invoice Creation Flow:**
```mermaid
sequenceDiagram
    participant User
    participant API as SevDeskApi
    participant SD as SevDesk API
    
    User->>API: create_invoice(order)
    
    API->>API: get_or_create_contact(order)
    API->>API: get_current_user()
    API->>API: parse_price(merchandise_value)
    API->>API: get_country_id(country)
    
    API->>SD: POST /Invoice
    SD-->>API: invoice_id, invoice_number
    
    loop Each order item
        API->>SD: POST /InvoicePos
    end
    
    opt Has shipping costs
        API->>SD: POST /InvoicePos (shipping)
    end
    
    API-->>User: InvoiceCreationResult
```

**Kleingewerbe Tax Handling:**
- Tax rate: 0% (no VAT)
- Tax rule ID: 11 (Kleinunternehmerregelung §19 UStG)
- Price net = Price gross

### simulation.rs

Dry-run invoice validation without API calls.

```rust
impl SevDeskApi {
    pub async fn simulate_invoice_creation(&self, order: &OrderRecord) -> Result<InvoiceCreationResult>;
    async fn simulate_invoice_validation(&self, order: &OrderRecord) -> Result<String>;
}
```

**Validation Checks:**
| Check | Description |
|-------|-------------|
| Country mapping | Validates country can be resolved to ID |
| Price parsing | Validates all price fields are parseable |
| Items validation | Logs what invoice positions would be created |

**Note:** Simulation still calls the country API (for cache population) but does NOT create any invoices or contacts.

### users.rs

User information retrieval for invoice contact person.

```rust
impl SevDeskApi {
    async fn get_current_user(&self) -> Result<u32>;
}
```

### utils.rs

Pure utility functions for data transformation.

```rust
impl SevDeskApi {
    fn parse_price(&self, price_str: &str) -> Result<f64>;
}
```

| Function | Input | Output | Description |
|----------|-------|--------|-------------|
| `parse_price` | `"5,00"` or `"5.00"` | `5.0` | Handles comma/dot decimals |

## Data Flow

```mermaid
sequenceDiagram
    participant App
    participant API as SevDeskApi
    participant Contacts as contacts.rs
    participant Countries as countries.rs
    participant Invoices as invoices.rs
    participant SD as SevDesk API
    
    App->>API: create_invoice(order)
    
    API->>Contacts: get_or_create_contact(order)
    Contacts->>Countries: get_country_id(country)
    Countries->>SD: GET /StaticCountry (if not cached)
    Countries-->>Contacts: country_id
    Contacts->>SD: GET /Contact?name=...
    
    alt Contact exists
        SD-->>Contacts: contact_id
    else Not found
        Contacts->>SD: POST /Contact
        SD-->>Contacts: new contact_id
    end
    
    Contacts-->>API: contact_id
    
    API->>Invoices: create_invoice_internal(order)
    Invoices->>SD: POST /Invoice
    SD-->>Invoices: invoice_id
    
    loop Each item
        Invoices->>SD: POST /InvoicePos
    end
    
    Invoices-->>API: (invoice_id, invoice_number)
    API-->>App: InvoiceCreationResult
```

## Usage Example

```rust
use sevdesk_invoicing::sevdesk_api::SevDeskApi;
use sevdesk_invoicing::models::OrderRecord;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api = SevDeskApi::new("your_api_token".to_string());
    
    // Test connection
    if api.test_connection().await? {
        println!("✓ Connected to SevDesk API");
    } else {
        eprintln!("✗ Connection failed");
        return Ok(());
    }
    
    // Simulate invoice creation (dry run)
    let order: OrderRecord = /* ... */;
    let simulation = api.simulate_invoice_creation(&order).await?;
    
    if simulation.error.is_none() {
        println!("✓ Simulation passed: {}", simulation.invoice_number.unwrap());
        
        // Create real invoice
        let result = api.create_invoice(&order).await?;
        
        match result.error {
            None => println!("✓ Invoice created: {}", result.invoice_number.unwrap()),
            Some(err) => eprintln!("✗ Failed: {}", err),
        }
    }
    
    Ok(())
}
```

## API Endpoints Used

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/Tools/bookkeepingSystemVersion` | GET | Connection test |
| `/StaticCountry` | GET | Fetch country list |
| `/Contact` | GET | Search contacts by name |
| `/Contact` | POST | Create new contact |
| `/SevUser` | GET | Get current user |
| `/Invoice` | POST | Create invoice |
| `/InvoicePos` | POST | Add invoice line item |

## Testing

Unit tests are in the `tests/` subdirectory:

- **construction_tests.rs** - API client construction
- **countries_tests.rs** - Country cache functionality
- **models_tests.rs** - Model types (OrderRecord, InvoiceCreationResult)
- **utils_tests.rs** - Price parsing utilities

Run tests:
```bash
cargo test sevdesk_api
```

**Note:** All tests are offline-only and do not contact the real SevDesk API.
