# rtltcp Project Plan: Final Remediation Strategy

## Executive Summary

This plan addresses the actual remaining issues in the rtltcp project based on the current codebase analysis. The original evaluation report contained many items that have already been fixed. This revised plan focuses on genuine issues that still need attention, with security hardening as the primary focus.

**Important Scope Notes:**
- This plan does NOT include multi-client support, TLS, or protocol changes - those are separate feature initiatives
- The rtl-tcp protocol is designed for single-client LAN use by design
- Many "critical" issues from the original evaluation report are already resolved

---

## Priority Classification

- **Must-Have (Critical/High Priority)**: Essential for security, reliability, or production readiness
- **Should-Have (Medium Priority)**: Important improvements that enhance quality but are not blocking
- **Nice-to-Have (Low Priority)**: Optional improvements that add polish or future-proofing

---

## Wave 1: Critical Security Hardening 🔴

**Goal**: Address immediate security vulnerabilities and improve the default security posture.

**Estimated Effort**: 1-2 days
**Dependencies**: None

### Task 1.1: Change Default Bind Address (Must-Have)
- **Current**: Defaults to `[::]` (all interfaces) - `src/main.rs:51`
- **Target**: Change to `127.0.0.1` for local-only by default
- **Acceptance Criteria**: 
  - Default address changed to `127.0.0.1`
  - Warning logged when binding to all interfaces
  - Clear documentation of breaking change and migration path
  - Migration guide: users can restore old behavior with `--address 0.0.0.0`

### Task 1.2: Add Input Validation for Protocol Payloads (Must-Have)
- **Current**: Raw u32/i32 values from network passed directly to device APIs with zero validation - `src/main.rs:159-233`
- **Target**: Validate frequency, sample rate, gain ranges before passing to device
- **Acceptance Criteria**:
  - Frequency validated against device capabilities (e.g., 0-2.2 GHz typical range)
  - Sample rate validated (positive, within device range)
  - Gain values validated (within supported range)
  - PPM values validated (reasonable range, e.g., -1000 to 1000)
  - Invalid commands logged and rejected gracefully

### Task 1.3: Add Rate Limiting for Commands (Should-Have)
- **Current**: No rate limiting for rapid command changes
- **Target**: Add rate limiting to prevent rapid hardware control abuse
- **Acceptance Criteria**:
  - Configurable rate limit (commands/second)
  - Log rate limit violations
  - Graceful degradation when limits exceeded

### Task 1.4: Make Timeouts Configurable (Should-Have)
- **Current**: Timeouts hardcoded to 30s - `src/main.rs:121-122`
- **Target**: Add CLI flags for configurable read/write timeout values
- **Acceptance Criteria**:
  - CLI flags for read/write timeout values
  - Timeouts applied immediately after connection accept
  - Proper logging of timeout events

---

## Wave 2: Code Quality & Error Handling 🟡

**Goal**: Improve code maintainability and error handling.

**Estimated Effort**: 2-3 days
**Dependencies**: None

### Task 2.1: Custom Error Type (Should-Have)
- **Current**: Using `Box<dyn std::error::Error>` throughout
- **Target**: Create custom `RtlTcpError` enum with context
- **Acceptance Criteria**:
  - All error variants covered (device, network, config, etc.)
  - Proper `Display` and `Debug` implementations
  - `From` implementations for underlying errors

### Task 2.2: Fix Racy should_exit Check (Should-Have)
- **Current**: `should_exit` checked after `read_exact` blocks indefinitely - `src/main.rs:134`
- **Target**: Make reads interruptible by signal
- **Acceptance Criteria**:
  - Signal can interrupt active read
  - Clean shutdown on Ctrl-C even during I/O
  - No race conditions in exit handling

### Task 2.3: Add Command Validation Logging (Nice-to-Have)
- **Current**: Unknown commands silently ignored with debug log - `src/main.rs:231-233`
- **Target**: Consider sending error response to client for unknown commands
- **Acceptance Criteria**:
  - Log unknown commands at warn level
  - Optionally send error response to client
  - Document behavior in README

---

## Wave 3: Testing & Documentation 🟢

**Goal**: Improve test coverage and documentation.

**Estimated Effort**: 2-3 days
**Dependencies**: None

### Task 3.1: Comprehensive Testing (Should-Have)
- **Current**: Limited test coverage
- **Target**: Add comprehensive test coverage for all protocol commands
- **Acceptance Criteria**:
  - All protocol commands tested with valid and invalid inputs
  - Edge cases tested (min/max values, invalid payloads)
  - Integration tests with mock device abstraction

### Task 3.2: Enhanced Documentation (Should-Have)
- **Current**: Basic documentation only
- **Target**: Comprehensive documentation including security considerations
- **Acceptance Criteria**:
  - Usage examples for all CLI flags
  - Security considerations documented
  - Migration guide for breaking changes
  - Updated systemd service file with hardening directives as example

---

## Breaking Changes Notice

### Default Bind Address Change

The default bind address is changing from `[::]` (all interfaces) to `127.0.0.1` (localhost only) by default. This is a **breaking change** that will require a major version bump.

**Who is affected**: Users who rely on the default behavior to accept connections from any interface.

**Migration path**: 
- Existing users should add `--address 0.0.0.0` or `--address [::]` to maintain current behavior
- Document the security implications of binding to all interfaces
- Warning logs when binding to public interfaces

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Breaking change from default bind address | High | Medium | Clear deprecation notice, maintain backward compatibility flag |
| Input validation breaking existing clients | Medium | Low | Log validation failures, add bypass flag for legacy clients |
| Rate limiting affecting legitimate use | Low | Low | Make configurable, default to reasonable values |
| rtlsdr_mt crate supply chain risk | Medium | Low | Pin versions, audit crate, have fallback plan |

---

## Success Criteria

1. **Security**: Default bind address changed to localhost, input validation implemented
2. **Code Quality**: Custom error type implemented, proper error handling
3. **Testing**: Comprehensive test coverage for all protocol commands
4. **Documentation**: Clear migration guide for breaking changes, security considerations documented

---

## Implementation Schedule

### Phase 1: Critical Security (Wave 1) - Week 1
- Days 1-2: Tasks 1.1, 1.2 (Default address change, input validation)
- Day 3: Task 1.3 (Rate limiting)
- Day 4: Task 1.4 (Configurable timeouts)

### Phase 2: Code Quality (Wave 2) - Week 2
- Days 1-2: Task 2.1 (Custom error type)
- Days 3-4: Task 2.2 (Racy should_exit fix)
- Day 5: Task 2.3 (Command validation logging)

### Phase 3: Testing & Documentation (Wave 3) - Week 3
- Days 1-2: Task 3.1 (Comprehensive testing)
- Days 3-4: Task 3.2 (Enhanced documentation)

---

## Next Steps

1. Review this plan with team stakeholders
2. Implement Wave 1 changes first (security hardening)
3. Update documentation to reflect breaking changes
4. Implement testing and code quality improvements
5. Verify all changes before release
