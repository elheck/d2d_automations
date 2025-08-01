mod models;
mod csv_reader;
mod invoice_generator;

use slint::{ModelRc, VecModel, Model};
use std::rc::Rc;
use anyhow::Result;
use models::{OrderData as ModelsOrderData};
use csv_reader::read_csv_file;
use invoice_generator::{InvoiceGenerator, CompanyInfo};

slint::include_modules!();

fn main() -> Result<()> {
    let ui = Main::new()?;
    
    let ui_handle = ui.as_weak();
    
    // Handle CSV file browsing
    ui.on_browse_csv_file({
        let ui_handle = ui_handle.clone();
        move || {
            if let Some(ui) = ui_handle.upgrade() {
                // In a real application, you would use a file dialog here
                // For now, we'll just set a placeholder message
                ui.set_status_message("Verwenden Sie den Pfad-Eingabefeld um eine CSV-Datei zu laden".into());
            }
        }
    });
    
    // Handle CSV loading
    ui.on_load_csv_file({
        let ui_handle = ui_handle.clone();
        move || {
            if let Some(ui) = ui_handle.upgrade() {
                let csv_path = ui.get_csv_file_path();
                
                if csv_path.is_empty() {
                    ui.set_status_message("Bitte geben Sie einen CSV-Dateipfad ein".into());
                    return;
                }
                
                ui.set_processing(true);
                ui.set_status_message("Lade CSV-Datei...".into());
                
                match load_csv_orders(&csv_path) {
                    Ok(orders) => {
                        let slint_orders: Vec<slint_generatedMain::OrderData> = orders.into_iter().map(convert_order_to_slint).collect();
                        let model = Rc::new(VecModel::from(slint_orders.clone()));
                        ui.set_orders(ModelRc::from(model));
                        ui.set_status_message(format!("{} Bestellungen erfolgreich geladen", slint_orders.len()).into());
                    }
                    Err(e) => {
                        ui.set_status_message(format!("Fehler beim Laden der CSV: {}", e).into());
                    }
                }
                
                ui.set_processing(false);
            }
        }
    });
    
    // Handle invoice generation
    ui.on_generate_invoices({
        let ui_handle = ui_handle.clone();
        move || {
            if let Some(ui) = ui_handle.upgrade() {
                let orders_model = ui.get_orders();
                let starting_number = ui.get_starting_invoice_number();
                
                if starting_number.is_empty() {
                    ui.set_status_message("Bitte geben Sie eine Startrechnungsnummer ein".into());
                    return;
                }
                
                if orders_model.row_count() == 0 {
                    ui.set_status_message("Keine Bestellungen zum Verarbeiten vorhanden".into());
                    return;
                }
                
                ui.set_processing(true);
                ui.set_status_message("Generiere Rechnungen...".into());
                
                let company_info = CompanyInfo {
                    name: ui.get_company_name().to_string(),
                    address: ui.get_company_address().to_string(),
                    phone: ui.get_company_phone().to_string(),
                    email: ui.get_company_email().to_string(),
                };
                
                match generate_all_invoices(&orders_model, &starting_number, company_info) {
                    Ok(count) => {
                        ui.set_status_message(format!("{} Rechnungen erfolgreich generiert", count).into());
                    }
                    Err(e) => {
                        ui.set_status_message(format!("Fehler beim Generieren der Rechnungen: {}", e).into());
                    }
                }
                
                ui.set_processing(false);
            }
        }
    });
    
    ui.run()?;
    Ok(())
}

fn load_csv_orders(file_path: &str) -> Result<Vec<ModelsOrderData>> {
    read_csv_file(file_path)
}

fn convert_order_to_slint(order: ModelsOrderData) -> slint_generatedMain::OrderData {
    slint_generatedMain::OrderData {
        order_id: order.order_id.into(),
        username: order.username.into(),
        name: order.name.into(),
        street: order.street.into(),
        city: order.city.into(),
        country: order.country.into(),
        is_professional: order.is_professional.into(),
        vat_number: order.vat_number.into(),
        date_of_purchase: order.date_of_purchase.into(),
        article_count: order.article_count,
        merchandise_value: order.merchandise_value.into(),
        shipment_costs: order.shipment_costs.into(),
        total_value: order.total_value.into(),
        commission: order.commission.into(),
        currency: order.currency.into(),
        description: order.description.into(),
        product_id: order.product_id.into(),
        localized_product_name: order.localized_product_name.into(),
    }
}

fn convert_slint_to_order(slint_order: &slint_generatedMain::OrderData) -> ModelsOrderData {
    ModelsOrderData {
        order_id: slint_order.order_id.to_string(),
        username: slint_order.username.to_string(),
        name: slint_order.name.to_string(),
        street: slint_order.street.to_string(),
        city: slint_order.city.to_string(),
        country: slint_order.country.to_string(),
        is_professional: slint_order.is_professional.to_string(),
        vat_number: slint_order.vat_number.to_string(),
        date_of_purchase: slint_order.date_of_purchase.to_string(),
        article_count: slint_order.article_count,
        merchandise_value: slint_order.merchandise_value.to_string(),
        shipment_costs: slint_order.shipment_costs.to_string(),
        total_value: slint_order.total_value.to_string(),
        commission: slint_order.commission.to_string(),
        currency: slint_order.currency.to_string(),
        description: slint_order.description.to_string(),
        product_id: slint_order.product_id.to_string(),
        localized_product_name: slint_order.localized_product_name.to_string(),
    }
}

fn generate_all_invoices(
    orders_model: &ModelRc<slint_generatedMain::OrderData>,
    starting_number: &str,
    company_info: CompanyInfo,
) -> Result<usize> {
    let generator = InvoiceGenerator::new(company_info);
    
    // Create output directory
    let output_dir = std::path::Path::new("invoices");
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    
    let mut generated_count = 0;
    
    for i in 0..orders_model.row_count() {
        if let Some(slint_order) = orders_model.row_data(i) {
            let order = convert_slint_to_order(&slint_order);
            let invoice_number = generate_invoice_number(starting_number, i as u32)?;
            
            let output_path = format!("invoices/Rechnung_{}.pdf", invoice_number);
            generator.generate_invoice_from_order(&order, &invoice_number, "invoices")?;
            generated_count += 1;
        }
    }
    
    Ok(generated_count)
}

fn generate_invoice_number(starting_number: &str, increment: u32) -> Result<String> {
    // Parse the starting number (format: YYYYMMNNNN)
    if starting_number.len() != 10 {
        return Err(anyhow::anyhow!("Invoice number must be 10 digits (YYYYMMNNNN format)"));
    }
    
    let base_number: u64 = starting_number.parse()?;
    let new_number = base_number + increment as u64;
    
    Ok(format!("{:010}", new_number))
}
