# check_stock_core

Logic and data layer of the [MTG Stock Checker](../README.md): inventory
database, repricing rules, bulk lots, precon EV, mispricing, restock, bin
consolidation, wantslist parsing, CSV import/export, caches and API clients.
No UI, so both the egui desktop app (`../src/`) and the planned web server can
use it.

## File locations are passed in

Core never chooses where files live and does not use `dirs`. The app passes
the locations in:

| What | API |
|---|---|
| Inventory database | `InventoryDb::in_dir(data_dir)` / `InventoryDb::at(file)`; backups go next to the file |
| Image, card-type, MTGJSON, sales caches | `ImageCache::in_root(cache_root)`, `CardTypeCache::in_root`, `DeckCache::in_root`, `SalesCache::in_root` |
| Scryfall card cache | `CardCache::load_in(cache_root)` |
| Repricing ruleset | `repricing::RULESET_FILE_NAME` inside the app's config folder |

The folder and file names (`DIR_NAME`, `FILE_NAME`, `RULESET_FILE_NAME`) are
pinned by tests: changing one would make existing data disappear for users.

## Tests

```bash
cd ..            # check_stock/, the workspace root
cargo test       # runs core's and the desktop app's tests
```

Test helpers for other crates' tests (e.g. `Card::test_default`) are behind the
`test-support` feature. Performance tests for the card search are described in
[PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md).
