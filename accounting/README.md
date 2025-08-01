# German Invoice Generator / Deutsche Rechnungserstellung

Eine Rust-Anwendung mit Slint GUI zur automatischen Generierung von PDF-Rechnungen aus CSV-Daten, speziell für deutsche Kleinunternehmen nach § 19 UStG (Kleinunternehmerregelung).

## Features

- **CSV Import**: Liest Bestelldaten aus semicolon-separated CSV-Dateien
- **PDF-Rechnungsgenerierung**: Erstellt professionelle PDF-Rechnungen
- **Kleinunternehmerregelung**: Automatische Berücksichtigung der deutschen Steuergesetzgebung (§ 19 UStG)
- **Intelligente Rechnungsnummern**: Format JJJJMMNNNN (Jahr-Monat-fortlaufende Nummer)
- **Anpassbare Firmendaten**: Konfigurierbare Firmeninformationen
- **Benutzerfreundliche GUI**: Moderne Slint-basierte Benutzeroberfläche

## Aufbau und Installation

### Voraussetzungen

- Rust (neueste stabile Version)
- Cargo

### Installation

1. **Repository klonen:**
   ```bash
   git clone <your-repo-url>
   cd d2d_automations/accounting
   ```

2. **Dependencies installieren und kompilieren:**
   ```bash
   cargo build --release
   ```

3. **Anwendung starten:**
   ```bash
   cargo run
   ```

## Verwendung

### 1. Firmendaten konfigurieren
- Firmenname
- Adresse
- Telefonnummer
- E-Mail-Adresse

### 2. CSV-Datei laden
- Pfad zur CSV-Datei eingeben oder durchsuchen
- CSV laden (unterstützt semicolon-separierte Dateien)

### 3. Rechnungsnummer festlegen
- Startrechnungsnummer im Format JJJJMMNNNN eingeben
- Beispiel: `2025080001` für Jahr 2025, Monat 08, erste Rechnung

### 4. Rechnungen generieren
- Alle geladenen Bestellungen werden in separate PDF-Rechnungen umgewandelt
- PDFs werden im Ordner `invoices/` gespeichert

## CSV-Format

Die Anwendung erwartet CSV-Dateien mit folgenden Spalten (semicolon-getrennt):

```
OrderID;Username;Name;Street;City;Country;Is Professional;VAT Number;Date of Purchase;Article Count;Merchandise Value;Shipment Costs;Total Value;Commission;Currency;Description;Product ID;Localized Product Name
```

## Rechtliche Hinweise

- Die generierten Rechnungen entsprechen der deutschen Kleinunternehmerregelung (§ 19 UStG)
- Keine Umsatzsteuer wird ausgewiesen
- Automatische Berechnung der Zahlungsfristen (14 Tage)
- Alle erforderlichen Pflichtangaben werden eingeschlossen

## Technische Details

### Architektur

- **Frontend**: Slint (moderne GUI-Bibliothek)
- **Backend**: Rust mit folgenden Hauptkomponenten:
  - `csv_reader.rs`: CSV-Parsing und Datenvalidierung
  - `invoice_generator.rs`: PDF-Generierung mit printpdf
  - `models.rs`: Datenstrukturen und Geschäftslogik

### Dependencies

- `slint`: GUI-Framework
- `csv`: CSV-Parsing
- `printpdf`: PDF-Generierung
- `chrono`: Datum/Zeit-Handling
- `serde`: Serialisierung/Deserialisierung
- `anyhow`: Fehlerbehandlung

### Ordnerstruktur

```
accounting/
├── src/
│   ├── main.rs              # Hauptanwendung und GUI-Logik
│   ├── models.rs            # Datenstrukturen
│   ├── csv_reader.rs        # CSV-Import-Funktionalität
│   └── invoice_generator.rs # PDF-Rechnungsgenerierung
├── ui/
│   └── main.slint          # GUI-Definition
├── Cargo.toml              # Dependencies und Projektkonfiguration
└── build.rs                # Build-Skript für Slint
```

## Entwicklung

### Tests ausführen
```bash
cargo test
```

### Code formatieren
```bash
cargo fmt
```

### Linting
```bash
cargo clippy
```

### Release-Build
```bash
cargo build --release
```

## Anpassungen

### Rechnungslayout ändern
Das PDF-Layout kann in `src/invoice_generator.rs` angepasst werden.

### CSV-Format erweitern
Neue Felder können in `src/models.rs` hinzugefügt und in `src/csv_reader.rs` verarbeitet werden.

### GUI-Anpassungen
Das Slint-Interface kann in `ui/main.slint` modifiziert werden.

## Troubleshooting

### Häufige Probleme

1. **CSV-Parsing-Fehler**
   - Prüfen Sie das Dateiformat (semicolon-getrennt)
   - Stellen Sie sicher, dass alle erforderlichen Spalten vorhanden sind

2. **PDF-Generierung schlägt fehl**
   - Prüfen Sie die Schreibberechtigung für den Ausgabeordner
   - Stellen Sie sicher, dass genügend Speicherplatz vorhanden ist

3. **GUI startet nicht**
   - Aktualisieren Sie Rust: `rustup update`
   - Prüfen Sie die Systemanforderungen für Slint

## Lizenz

MIT License. Siehe `LICENSE` Datei für Details.
