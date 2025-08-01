use std::path::Path;
use anyhow::{Result, Context};
use crate::models::OrderData;

pub fn read_csv_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<OrderData>> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';') // CSV uses semicolon as delimiter
        .from_path(file_path)
        .context("Failed to open CSV file")?;
    
    let mut orders = Vec::new();
    
    for result in reader.deserialize() {
        let order: OrderData = result.context("Failed to parse CSV record")?;
        orders.push(order);
    }
    
    Ok(orders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    
    #[test]
    fn test_csv_parsing() {
        let csv_content = r#"OrderID;Username;Name;Street;City;Country;Is Professional;VAT Number;Date of Purchase;Article Count;Merchandise Value;Shipment Costs;Total Value;Commission;Currency;Description;Product ID;Localized Product Name
1218804750;notsaicana;Lucas Cordeiro;Hedwig-Porschütz-Straße 28;10557 Berlin;Germany;;;2025-07-01 22:42:27;1;1,87;1,25;3,12;0,10;EUR;1x High Fae Trickster;795560;High Fae Trickster"#;
        
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(temp_file, "{}", csv_content).unwrap();
        
        let orders = read_csv_file(temp_file.path()).unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "1218804750");
        assert_eq!(orders[0].name, "Lucas Cordeiro");
    }
}
