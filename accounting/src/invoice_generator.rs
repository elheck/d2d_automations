use printpdf::*;
use anyhow::Result;
use std::path::Path;
use crate::models::{InvoiceData, OrderData};

#[derive(Clone)]
pub struct CompanyInfo {
    pub name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
}

impl Default for CompanyInfo {
    fn default() -> Self {
        Self {
            name: "Ihr Firmenname".to_string(),
            address: "Ihre Adresse\n12345 Ihre Stadt".to_string(),
            phone: "+49 123 456789".to_string(),
            email: "info@ihrefirma.de".to_string(),
        }
    }
}

pub struct InvoiceGenerator {
    company_info: CompanyInfo,
}

impl InvoiceGenerator {
    pub fn new(company_info: CompanyInfo) -> Self {
        Self { company_info }
    }

    pub fn generate_invoice<P: AsRef<Path>>(&self, invoice_data: &InvoiceData, output_path: P) -> Result<()> {
        let (doc, page1, layer1) = PdfDocument::new("Rechnung", Mm(210.0), Mm(297.0), "Layer 1");
        let current_layer = doc.get_page(page1).get_layer(layer1);
        
        // Load a built-in font
        let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
        let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

        let mut y_position = 280.0f32;
        let layer = current_layer;

        // Company header
        layer.use_text(&self.company_info.name, 14.0, Mm(20.0), Mm(y_position), &font_bold);
        y_position -= 15.0;

        for line in self.company_info.address.lines() {
            layer.use_text(line, 10.0, Mm(20.0), Mm(y_position), &font);
            y_position -= 12.0;
        }

        layer.use_text(&format!("Tel: {}", self.company_info.phone), 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 12.0;
        layer.use_text(&format!("E-Mail: {}", self.company_info.email), 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 30.0;

        // Customer address
        layer.use_text(&invoice_data.customer_name, 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 12.0;
        layer.use_text(&invoice_data.customer_address, 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 30.0;

        // Invoice header
        layer.use_text("RECHNUNG", 20.0, Mm(20.0), Mm(y_position), &font_bold);
        y_position -= 20.0;

        layer.use_text(&format!("Rechnungsnummer: {}", invoice_data.invoice_number), 12.0, Mm(20.0), Mm(y_position), &font_bold);
        y_position -= 15.0;

        layer.use_text(
            &format!("Rechnungsdatum: {}", invoice_data.invoice_date),
            10.0, Mm(20.0), Mm(y_position), &font
        );
        y_position -= 15.0;

        layer.use_text(
            &format!("Leistungsdatum: {}", invoice_data.service_date),
            10.0, Mm(20.0), Mm(y_position), &font
        );
        y_position -= 20.0;

        // Items section
        layer.use_text("Sehr geehrte Damen und Herren,".to_string(), 12.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 15.0;

        layer.use_text("hiermit stellen wir Ihnen folgende Positionen in Rechnung:".to_string(), 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 20.0;

        // Table header
        layer.use_text("Pos.", 10.0, Mm(20.0), Mm(y_position), &font_bold);
        layer.use_text("Beschreibung", 10.0, Mm(35.0), Mm(y_position), &font_bold);
        layer.use_text("Menge", 10.0, Mm(130.0), Mm(y_position), &font_bold);
        layer.use_text("Einzelpreis", 10.0, Mm(150.0), Mm(y_position), &font_bold);
        layer.use_text("Gesamtpreis", 10.0, Mm(175.0), Mm(y_position), &font_bold);
        y_position -= 5.0;

        // Add line under header
        let points = vec![(Point::new(Mm(20.0), Mm(y_position)), false), (Point::new(Mm(190.0), Mm(y_position)), false)];
        let line = Line {
            points,
            is_closed: false,
        };
        layer.add_line(line);
        y_position -= 10.0;

        // Add items
        layer.use_text("1", 10.0, Mm(20.0), Mm(y_position), &font);
        layer.use_text(&invoice_data.description, 10.0, Mm(35.0), Mm(y_position), &font);
        layer.use_text("1", 10.0, Mm(130.0), Mm(y_position), &font);
        layer.use_text(&format!("{:.2} EUR", invoice_data.net_amount), 10.0, Mm(150.0), Mm(y_position), &font);
        layer.use_text(&format!("{:.2} EUR", invoice_data.net_amount), 10.0, Mm(175.0), Mm(y_position), &font);
        y_position -= 15.0;

        // Add shipping if present
        if invoice_data.shipping_cost > 0.0 {
            layer.use_text("2", 10.0, Mm(20.0), Mm(y_position), &font);
            layer.use_text("Versandkosten", 10.0, Mm(35.0), Mm(y_position), &font);
            layer.use_text("1", 10.0, Mm(130.0), Mm(y_position), &font);
            layer.use_text(&format!("{:.2} EUR", invoice_data.shipping_cost), 10.0, Mm(150.0), Mm(y_position), &font);
            layer.use_text(&format!("{:.2} EUR", invoice_data.shipping_cost), 10.0, Mm(175.0), Mm(y_position), &font);
            y_position -= 15.0;
        }

        // Totals section
        y_position -= 10.0;
        let line_points = vec![(Point::new(Mm(130.0), Mm(y_position)), false), (Point::new(Mm(190.0), Mm(y_position)), false)];
        let total_line = Line {
            points: line_points,
            is_closed: false,
        };
        layer.add_line(total_line);
        y_position -= 10.0;

        layer.use_text("Nettobetrag:", 10.0, Mm(130.0), Mm(y_position), &font);
        layer.use_text(&format!("{:.2} EUR", invoice_data.total_amount), 10.0, Mm(175.0), Mm(y_position), &font);
        y_position -= 12.0;

        // VAT exempt note
        layer.use_text("MwSt. (0%):", 10.0, Mm(130.0), Mm(y_position), &font);
        layer.use_text("0,00 EUR", 10.0, Mm(175.0), Mm(y_position), &font);
        y_position -= 15.0;

        // Double line before total
        let line1_points = vec![(Point::new(Mm(130.0), Mm(y_position)), false), (Point::new(Mm(190.0), Mm(y_position)), false)];
        let final_line1 = Line {
            points: line1_points,
            is_closed: false,
        };
        layer.add_line(final_line1);

        y_position -= 2.0;
        let line2_points = vec![(Point::new(Mm(130.0), Mm(y_position)), false), (Point::new(Mm(190.0), Mm(y_position)), false)];
        let final_line2 = Line {
            points: line2_points,
            is_closed: false,
        };
        layer.add_line(final_line2);
        y_position -= 10.0;

        layer.use_text("Rechnungsbetrag:", 12.0, Mm(130.0), Mm(y_position), &font_bold);
        layer.use_text(&format!("{:.2} EUR", invoice_data.total_amount), 12.0, Mm(175.0), Mm(y_position), &font_bold);
        y_position -= 30.0;

        // Legal text for small business regulation
        let legal_text = "Kleinunternehmerregelung nach §19 UStG: Es wird keine Umsatzsteuer berechnet.";
        layer.use_text(legal_text, 9.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 20.0;

        // Payment information
        layer.use_text("Vielen Dank für Ihr Vertrauen!", 10.0, Mm(20.0), Mm(y_position), &font);
        y_position -= 12.0;

        let payment_text = "Bitte überweisen Sie den Rechnungsbetrag innerhalb von 14 Tagen.";
        layer.use_text(payment_text, 9.0, Mm(20.0), Mm(y_position), &font);

        layer.use_text(&format!("Mit freundlichen Grüßen\n{}", self.company_info.name), 9.0, Mm(20.0), Mm(y_position - 24.0), &font);

        // Save the PDF
        doc.save(&mut std::io::BufWriter::new(std::fs::File::create(output_path)?))?;
        Ok(())
    }

    pub fn generate_invoice_from_order(&self, order: &OrderData, invoice_number: &str, output_dir: &str) -> Result<String> {
        let invoice_data = InvoiceData {
            invoice_number: invoice_number.to_string(),
            invoice_date: chrono::Local::now().format("%d.%m.%Y").to_string(),
            service_date: order.date_of_purchase.clone(),
            customer_name: order.name.clone(),
            customer_address: format!("{}\n{}\n{}", order.street, order.city, order.country),
            description: order.description.clone(),
            net_amount: order.merchandise_value.replace(',', ".").parse().unwrap_or(0.0),
            shipping_cost: order.shipment_costs.replace(',', ".").parse().unwrap_or(0.0),
            total_amount: order.total_value.replace(',', ".").parse().unwrap_or(0.0),
            currency: order.currency.clone(),
        };

        let filename = format!("Rechnung_{}.pdf", invoice_number);
        let output_path = format!("{}/{}", output_dir, filename);
        
        self.generate_invoice(&invoice_data, &output_path)?;
        Ok(output_path)
    }
}
