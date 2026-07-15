//! Tests for product_catalog.

use super::*;

/// Create a test product entry with default values
pub fn make_test_product(id_product: u64, name: &str) -> ProductEntry {
    ProductEntry {
        id_product,
        name: name.to_string(),
        id_category: 1,
        category_name: "Magic Single".to_string(),
        id_expansion: 1,
        id_metacard: id_product,
        date_added: "2007-01-01 00:00:00".to_string(),
    }
}

#[test]
fn product_catalog_from_entries() {
    let entries = vec![
        make_test_product(1, "Black Lotus"),
        make_test_product(2, "Mox Pearl"),
    ];
    let catalog = ProductCatalog::from_entries(entries);

    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.singles_count(), 2);
    assert_eq!(catalog.non_singles_count(), 0);
    assert_eq!(catalog.get(1).unwrap().name, "Black Lotus");
    assert_eq!(catalog.get(2).unwrap().name, "Mox Pearl");
    assert!(catalog.get(999).is_none());
}

#[test]
fn product_entry_deserializes() {
    let json = r#"{
        "idProduct": 12345,
        "name": "Black Lotus",
        "idCategory": 1,
        "categoryName": "Magic Single",
        "idExpansion": 1,
        "idMetacard": 567,
        "dateAdded": "2007-01-01 00:00:00"
    }"#;

    let entry: ProductEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.id_product, 12345);
    assert_eq!(entry.name, "Black Lotus");
    assert_eq!(entry.category_name, "Magic Single");
}
