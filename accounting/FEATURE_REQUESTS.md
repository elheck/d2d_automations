# Feature Requests

This document contains feature requests for the SevDesk Invoicing Application.

## Medium Priority

### 1. Multi-Currency Support
**Description**: Handle invoices in different currencies
- Currency conversion
- Exchange rate management
- Multi-currency reporting

**Priority**: Medium
**Status**: Future consideration

## Low Priority

### 2. Invoice Preview
**Description**: Preview invoices before creation
- Real-time preview as data is entered
- Print preview functionality

**Priority**: Low
**Status**: Future consideration


---


### Test Coverage

| Component | Coverage | Tests |
|-----------|----------|-------|
| CSV Processor | ~95% | 35 tests |
| SevDesk API Utils | ~80% | 16 tests |
| Workflow Operations | ~80% | 18 tests |
| Contacts & Invoices | ~80% | 22 tests |
| UI/App Logic | <5% | 0 tests |
| **Total** | **~146 tests** | **All passing** |

### Strengths to Maintain

- Security-first approach (no credentials in code)
- Comprehensive test fixtures (8 real-world CSV samples)
- Excellent error messages (descriptive, actionable, with context)
- Clean module boundaries (minimal coupling)
- Dry-run mode (excellent for testing without side effects)
- Proper async pattern (Tokio runtime + block_on())

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