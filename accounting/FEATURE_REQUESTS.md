# Feature Requests

This document contains feature requests for the SevDesk Invoicing Application.

## High Priority

### 1. Invoice Completion Workflow
**Description**: Add functionality to finish/finalize invoices after creation
- Mark invoices as completed/finalized in SevDesk
- Validate invoice data before finalization
- Handle invoice status transitions (draft → finalized)
- Add confirmation dialog before finalizing
- Support batch finalization of multiple invoices

**Priority**: High
**Status**: Requested

### 2. Invoice Download Functionality
**Description**: Implement ability to download generated invoices
- Download invoices as PDF files
- Support downloading individual invoices
- Support batch download of multiple invoices
- Configurable download location
- Automatic file naming (e.g., `Invoice_{number}_{date}.pdf`)
- Progress indicator for downloads

**Priority**: High
**Status**: Requested

### 3. Invoice Booking Against Accounts
**Description**: Add functionality to book finalized invoices against accounting accounts
- Integration with SevDesk accounting module
- Select target account for booking
- Automatic posting to general ledger
- Support for different account types (revenue, VAT, etc.)
- Validation of accounting rules
- Audit trail for all bookings

**Priority**: High
**Status**: Requested

### 4. Complete Address Information
**Description**: Extend invoice address fields to include complete customer address
- Add street address field
- Add city field
- Add postal/ZIP code field
- Add state/province field (optional)
- Maintain backward compatibility with existing name/country fields
- Support for international address formats
- Address validation (optional)

**Priority**: High
**Status**: Requested

## Medium Priority


### 6. Multi-Currency Support
**Description**: Handle invoices in different currencies
- Currency conversion
- Exchange rate management
- Multi-currency reporting

**Priority**: Medium
**Status**: Future consideration

## Low Priority

### 7. Invoice Preview
**Description**: Preview invoices before creation
- Real-time preview as data is entered
- Print preview functionality

**Priority**: Low
**Status**: Future consideration


---

## How to Contribute

If you have additional feature requests:

1. Create an issue in the repository
2. Use the "feature request" label
3. Provide detailed description and use case
4. Include mockups or examples if applicable

## Implementation Notes

- All features should maintain compatibility with SevDesk API
- Consider error handling and validation for each feature
- Ensure proper logging and audit trails
- Follow existing code patterns and architecture