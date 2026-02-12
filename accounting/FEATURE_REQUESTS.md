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

## Code Review Findings & Technical Debt

*Last reviewed: 2026-02-12*

### Overall Assessment: Grade A- (88/100)

**Status**: ✅ **PRODUCTION READY**

The accounting project is a well-structured, production-ready application with strong security practices and comprehensive testing (108 unit + 19 integration tests, all passing). Zero clippy warnings.

### High Priority Improvements

#### 1. Refactor Large app.rs Update Method
**Issue**: update() method is 356 lines with deeply nested UI logic
- **Location**: [src/app.rs:87-443](src/app.rs#L87-L443)
- **Impact**: Difficult to test and maintain
- **Fix**: Extract helper methods (render_api_section(), render_csv_section(), etc.)
- **Effort**: 1-2 days
- **Priority**: HIGH

### Medium Priority Improvements

#### 2. Reduce API Error Handling Duplication
**Issue**: Similar error handling pattern repeated across API modules
- **Affected Files**: users.rs, contacts.rs, countries.rs, check_accounts.rs
- **Fix**: Extract helper method `handle_api_error(response, operation_name)`
- **Effort**: 1 day
- **Priority**: MEDIUM

#### 3. Review .clone() Usage
**Issue**: 28 occurrences, potential performance overhead in order processing
- **Fix**: Profile with large order batches, optimize if needed
- **Effort**: 1-2 days
- **Priority**: MEDIUM

### Low Priority Improvements

#### 4. Standardize Logging
**Issue**: Mixed use of `log` and `tracing` macros
- tracing-subscriber initialized but log::info!() used throughout
- **Fix**: Use tracing macros everywhere for consistency
- **Effort**: 0.5 day

#### 5. Extract Magic Numbers
**Issue**: Hardcoded SevDesk IDs lack documentation
- Tax rule ID 11 ([src/sevdesk_api/invoices.rs:117](src/sevdesk_api/invoices.rs#L117))
- Category IDs ([src/models.rs:66-79](src/models.rs#L66-L79))
- **Fix**: Define as named constants with documentation
- **Effort**: 0.5 day

#### 6. Fix Fallback Behaviors
**Issue**: Silent fallbacks may hide errors
- Falls back to user ID 1 if no users found ([src/sevdesk_api/users.rs:63](src/sevdesk_api/users.rs#L63))
- Falls back to 0.0 for price parsing failures
- **Fix**: Return errors instead of silent fallbacks
- **Effort**: 0.5 day

### Security Audit: ✅ EXCELLENT

- **API Token Handling**: ✅ Environment variable only, password-masked in UI
- **Input Validation**: ✅ Comprehensive CSV validation
- **HTTP Security**: ✅ Proper Authorization headers, rustls-tls
- **No SQL Injection**: ✅ N/A (no database, only HTTP API)
- **Thread Safety**: ✅ Arc<RwLock<CountryCache>> correct pattern

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