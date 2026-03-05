# Feature Requests

This document contains feature requests for the Check Stock Application.

## Code Review Findings & Technical Debt

*Last reviewed: 2026-02-12*

### Overall Assessment: Grade A- (85/100)

**Status**: ✅ **PRODUCTION READY**

The check_stock project demonstrates professional-grade Rust development with excellent architectural decisions, comprehensive testing (189 tests, all passing), and strong security practices. Zero clippy warnings, no unsafe code.

### High Priority Improvements

#### 0. Use a GUI framework that is multiplatform ✅ Done — eframe/egui targets Linux, macOS, Windows; CI builds all three

#### 1. Implement Rate Limiting for Scryfall API
**Issue**: Semaphore limits concurrency but not requests/second (Scryfall limit: 10 req/s)
- **Location**: [src/ui/screens/picking.rs:105](src/ui/screens/picking.rs#L105)
- **Fix**: Add proper rate limiter (e.g., `governor` crate)
- **Effort**: 1 day
- **Priority**: MEDIUM

### Medium Priority Improvements

### Low Priority

#### 4. Add Module Documentation
- 34 doc comments exist, but 63 public functions
- Missing detailed docs in io.rs, formatters.rs
- Add rustdoc examples for public APIs

#### 5. Optimize String Allocations
- Profile first before optimizing
- Format strings in card matching hot paths
- Likely not a bottleneck with typical inventory sizes

### Security Audit: ✅ EXCELLENT

- **SQL Injection**: N/A (no SQL, CSV-based)
- **Input Validation**: Comprehensive CSV and wantslist parsing
- **No Secrets in Code**: ✅ Clean
- **No Unsafe Code**: ✅ Zero unsafe blocks
- **API Security**: User-Agent headers set, proper error handling


### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Perfect architecture adherence to CLAUDE.md
- Comprehensive performance tests
- Zero clippy warnings
