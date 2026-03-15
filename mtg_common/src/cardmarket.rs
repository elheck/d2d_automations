use serde::Deserialize;

/// Cardmarket price guide entry for a single product.
///
/// Matches the JSON schema from Cardmarket's CDN price guide files.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PriceGuideEntry {
    pub id_product: u64,
    pub id_category: u64,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    #[serde(rename = "avg-foil")]
    pub avg_foil: Option<f64>,
    #[serde(rename = "low-foil")]
    pub low_foil: Option<f64>,
    #[serde(rename = "trend-foil")]
    pub trend_foil: Option<f64>,
    #[serde(rename = "avg1-foil")]
    pub avg1_foil: Option<f64>,
    #[serde(rename = "avg7-foil")]
    pub avg7_foil: Option<f64>,
    #[serde(rename = "avg30-foil")]
    pub avg30_foil: Option<f64>,
}

/// Full price guide file structure from Cardmarket's CDN.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceGuideFile {
    pub version: u32,
    pub created_at: String,
    pub price_guides: Vec<PriceGuideEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_guide_entry_deserializes_with_nulls() {
        let json = r#"{
            "idProduct": 12345,
            "idCategory": 1,
            "avg": 1.5,
            "low": 0.5,
            "trend": 1.2,
            "avg1": null,
            "avg7": null,
            "avg30": null,
            "avg-foil": null,
            "low-foil": null,
            "trend-foil": null,
            "avg1-foil": null,
            "avg7-foil": null,
            "avg30-foil": null
        }"#;

        let entry: PriceGuideEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id_product, 12345);
        assert_eq!(entry.avg, Some(1.5));
        assert_eq!(entry.avg1, None);
    }

    #[test]
    fn price_guide_file_deserializes() {
        let json = r#"{
            "version": 1,
            "createdAt": "2026-03-01T10:00:00+0100",
            "priceGuides": [{
                "idProduct": 1,
                "idCategory": 1,
                "avg": 10.0,
                "low": 8.0,
                "trend": 9.5,
                "avg1": null,
                "avg7": null,
                "avg30": null,
                "avg-foil": null,
                "low-foil": null,
                "trend-foil": null,
                "avg1-foil": null,
                "avg7-foil": null,
                "avg30-foil": null
            }]
        }"#;

        let file: PriceGuideFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.version, 1);
        assert_eq!(file.created_at, "2026-03-01T10:00:00+0100");
        assert_eq!(file.price_guides.len(), 1);
        assert_eq!(file.price_guides[0].id_product, 1);
    }
}
