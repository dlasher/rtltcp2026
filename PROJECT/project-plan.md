# rtltcp Project Plan: Comprehensive Remediation Waves

## Executive Summary

This plan addresses all issues identified in the evaluation report, organized into sequential waves with clear priorities. Security-critical issues are prioritized first, followed by CI/CD fixes, code quality improvements, and testing enhancements.

---

## Priority Classification

- **Must-Have (Critical/High Priority)**: Essential for security, reliability, or production readiness. Must be implemented.
- **Should-Have (Medium Priority)**: Important improvements that enhance quality but are not blocking.
- **Nice-to-Have (Low Priority)**: Optional improvements that add polish or future-proofing.

---

## Wave 1: Critical Security Hardening 🔴

**Goal**: Address immediate security vulnerabilities that could allow unauthorized access or DoS attacks.

**Estimated Effort**: 2-3 days
**Dependencies**: None

### Task 1.1: Change Default Bind Address (Must-Have)
- **Current**: Defaults to `[::]` (all interfaces)
- **Target**: Change to `127.0.0.1` for local-only by default
- **Location**: `src/main.rs` line 51
- **Acceptance Criteria**: 
  - Default address is `127.0.0.1`
  - Explicit flag required to bind to `0.0.0.0` or `[::]`
  - Warning logged when binding to all interfaces

### Task 1.2: Add IP Allowlist/Access Control (Must-Have)
- **Current**: Any network client can connect and control the SDR device
- **Target**: Implement configurable IP allowlist
- **Location**: `src/main.rs` connection handling
- **Acceptance Criteria**:
  - CLI flag for allowlist file or comma-separated IPs
  - Connections from non-allowed IPs are rejected with appropriate error
  - Default behavior allows localhost connections

### Task 1.3: Add Read/Write Timeouts (Must-Have)
- **Current**: Timeouts already implemented (30s), but verify and enhance
- **Target**: Ensure timeouts are properly applied and configurable
- **Location**: `src/main.rs` lines 121-122
- **Acceptance Criteria**:
  - CLI flags for read/write timeout values
  - Timeouts are applied immediately after connection accept
  - Proper logging of timeout events

### Task 1.4: Log Client Connections (Must-Have)
- **Current**: Client address is discarded (`_addr`)
- **Target**: Log all connection attempts and rejections
- **Location**: `src/main.rs` line 119
- **Acceptance Criteria**:
  - Log client IP on connection accept
  - Log rejected connection attempts with reason
  - Include timestamp in log entries

### Task 1.5: Harden systemd Service (Should-Have)
- **Current**: Likely runs as root with no sandboxing
- **Target**: Add security directives to systemd service file
- **Location**: `rtltcp.service` or similar
- **Acceptance Criteria**:
  - `User=` and `Group=` directives set
  - `NoNewPrivileges=true`
  - `ProtectSystem=strict`
  - `ProtectHome=true`
  - `PrivateTmp=true`

---

## Wave 2: CI/CD Pipeline Fixes 🔴

**Goal**: Fix the broken cross-compilation pipeline and add missing CI/CD features.

**Estimated Effort**: 1-2 days
**Dependencies**: None

### Task 2.1: Fix Cross-Compilation Bug (Must-Have)
- **Current**: Cross targets skip build step entirely (no binaries produced)
- **Target**: Add proper cross build step using `cross` tool
- **Location**: `.github/workflows/cd.yml` lines 65-71
- **Acceptance Criteria**:
  - All 4 target platforms produce binaries
  - Cross-compiled binaries work on target architecture
  - Build artifacts are correctly named and packaged

### Task 2.2: Add MSRV Test Job (Must-Have)
- **Current**: No MSRV verification in CI
- **Target**: Add job to test with minimum supported Rust version (1.74)
- **Location**: New CI job in `.github/workflows/ci.yml`
- **Acceptance Criteria**:
  - MSRV job runs on all PRs and main
  - Tests pass with Rust 1.74
  - Fails if MSRV requirements are violated

### Task 2.3: Add Security Audit to CI (Should-Have)
- **Current**: `cargo audit` workflow exists but may not be integrated
- **Target**: Ensure audit runs on all PRs and scheduled basis
- **Location**: `.github/workflows/audit.yml`
- **Acceptance Criteria**:
  - Audit runs on schedule (weekly)
  - Audit runs on all PRs
  - Fails on known vulnerabilities

---

## Wave 3: Code Quality Improvements 🟡

**Goal**: Address code duplication, error handling, and maintainability issues.

**Estimated Effort**: 3-4 days
**Dependencies**: None (can be parallel with Wave 2)

### Task 3.1: Extract Mutex Helper (Must-Have)
- **Current**: ~125 lines of repeated mutex lock pattern
- **Target**: Extract to reusable helper function
- **Location**: `src/main.rs` lines 158-234
- **Acceptance Criteria**:
  - Reduce duplicated mutex handling code
  - Single helper function with proper error logging
  - All command handlers use the new helper

### Task 3.2: Improve Error Logging (Must-Have)
- **Current**: Error values discarded with `Err(_)`
- **Target**: Log actual error details for debugging
- **Location**: Multiple locations in `src/main.rs`
- **Acceptance Criteria**:
  - All device operation failures log actual error
  - Error context preserved where possible
  - No silent failures in production code

### Task 3.3: Custom Error Type (Should-Have)
- **Current**: Using `Box<dyn std::error::Error>`
- **Target**: Create custom `RtlTcpError` enum with context
- **Location**: New file `src/error.rs`
- **Acceptance Criteria**:
  - All error variants covered (device, network, config, etc.)
  - Proper `Display` and `Debug` implementations
  - `From` implementations for underlying errors

### Task 3.4: Add Input Validation (Must-Have)
- **Current**: No validation of protocol command payloads
- **Target**: Validate frequency, sample rate, gain ranges before passing to device
- **Location**: Command handlers in `src/main.rs`
- **Acceptance Criteria**:
  - Frequency validated against device capabilities
  - Sample rate validated (positive, within range)
  - Gain values validated (within supported range)
  - PPM values validated (reasonable range)
  - Invalid commands logged and rejected gracefully

### Task 3.5: Add Named Constants (Should-Have)
- **Current**: Some constants like buffer size hardcoded
- **Target**: Define all magic numbers as named constants
- **Location**: Top of `src/main.rs`
- **Acceptance Criteria**:
  - All magic numbers defined as constants
  - Constants are documented with units and valid ranges
  - No remaining hardcoded values

### Task 3.6: Fix Racy should_exit Check (Should-Have)
- **Current**: `should_exit` checked after `read_exact` blocks indefinitely
- **Target**: Make reads interruptible by signal
- **Location**: `src/main.rs` line 134
- **Acceptance Criteria**:
  - Signal can interrupt active read
  - Clean shutdown on Ctrl-C even during I/O
  - No race conditions in exit handling

---

## Wave 4: Testing & Verification 🟢

**Goal**: Add comprehensive testing to ensure correctness and prevent regressions.

**Estimated Effort**: 3-4 days
**Dependencies**: Waves 1-3 (for testable code structure)

### Task 4.1: Protocol Parsing Tests (Must-Have)
- **Current**: Only trivial constant tests exist
- **Target**: Add tests for protocol parsing and byte order
- **Location**: `tests/protocol.rs` or `src/main.rs` tests module
- **Acceptance Criteria**:
  - Tests for each command type (frequency, sample rate, gain, etc.)
  - Byte order validation tests
  - Edge cases (min/max values, invalid payloads)
  - Unknown command handling tests

### Task 4.2: Integration Tests (Must-Have)
- **Current**: Binary exists/help tests only
- **Target**: Add connection and protocol integration tests
- **Location**: `tests/integration.rs`
- **Acceptance Criteria**:
  - Mock device tests for full command lifecycle
  - Connection establishment tests
  - Multiple command sequence tests
  - Error handling tests

### Task 4.3: Multi-Client Handling (Should-Have)
- **Current**: No handling for second client connection
- **Target**: Gracefully reject or handle multiple connections
- **Location**: Connection handling in `src/main.rs`
- **Acceptance Criteria**:
  - Second connection is rejected with appropriate error
  - Existing connection is not affected
  - Log message for rejected connections

### Task 4.4: SIGTERM Verification (Should-Have)
- **Current**: SIGTERM handling unverified
- **Target**: Add test to verify SIGTERM triggers clean shutdown
- **Location**: Integration tests
- **Acceptance Criteria**:
  - SIGTERM triggers graceful shutdown
  - Device is properly released
  - No resource leaks

### Task 4.5: Performance Tests (Nice-to-Have)
- **Current**: No performance benchmarking
- **Target**: Add basic performance tests
- **Location**: `tests/performance.rs`
- **Acceptance Criteria**:
  - Throughput measurement tests
  - Memory usage tests
  - Connection latency tests

---

## Wave 5: Advanced Hardening (Future) 🟢

**Goal**: Implement advanced security features for production deployments.

**Estimated Effort**: 5-7 days
**Dependencies**: All previous waves

### Task 5.1: TLS Support (Nice-to-Have)
- **Current**: No encryption for network traffic
- **Target**: Optional TLS for internet-facing deployments
- **Acceptance Criteria**:
  - TLS optional via feature flag
  - Certificate configuration via CLI
  - Backward compatible with non-TLS clients

### Task 5.2: Rate Limiting (Should-Have)
- **Current**: No connection rate limiting
- **Target**: Add rate limiting for connections
- **Acceptance Criteria**:
  - Configurable rate limit (connections/minute)
  - Exponential backoff for repeated violations
  - Log rate limit violations

### Task 5.3: Audit Logging (Nice-to-Have)
- **Current**: Basic logging only
- **Target**: Structured audit logging for production
- **Acceptance Criteria**:
  - All security-relevant events logged
  - Log format suitable for SIEM integration
  - Configurable log levels and outputs

### Task 5.4: Seccomp Filtering (Nice-to-Have)
- **Current**: No syscall filtering
- **Target**: Add seccomp filtering to limit syscalls
- **Acceptance Criteria**:
  - Only necessary syscalls allowed
  - Graceful fallback if seccomp not available
  - Document required syscalls

---

## Implementation Schedule

### Week 1: Critical Security (Wave 1)
- Days 1-2: Tasks 1.1, 1.4 (Default address, connection logging)
- Days 2-3: Tasks 1.2, 1.3 (IP allowlist, timeouts)
- Day 4-5: Task 1.5 (systemd hardening) + review

### Week 2: CI/CD & Code Quality (Waves 2-3)
- Days 1-2: Tasks 2.1, 2.2 (Cross-compilation fix, MSRV job)
- Days 2-3: Tasks 2.3, 3.1, 3.2 (Audit, mutex helper, error logging)
- Days 3-5: Tasks 3.3, 3.4, 3.5 (Custom error, input validation, constants)

### Week 3: Testing & Polish (Waves 3-4)
- Days 1-2: Tasks 3.6, 4.1, 4.2 (Racy fix, protocol tests, integration tests)
- Days 2-3: Tasks 4.3, 4.4 (Multi-client, SIGTERM)
- Days 3-5: Task 4.5 (Performance tests) + review

### Week 4: Advanced Features (Wave 5)
- Days 1-3: Tasks 5.1, 5.2 (TLS, rate limiting)
- Days 3-5: Tasks 5.3, 5.4 (Audit logging, seccomp) + final review

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Breaking changes from security hardening | High | Medium | Maintain backward compatibility where possible, document breaking changes |
| Cross-compilation complexity | Medium | Medium | Test on actual hardware where possible |
| Protocol validation breaking clients | Medium | Low | Log validation failures, add bypass flag for legacy clients |
| Test complexity with hardware | High | High | Use mock device abstraction for tests |
| TLS complexity | Medium | Medium | Make optional, start with basic implementation |

---

## Success Criteria

1. **Security**: All critical and high security findings from evaluation report resolved
2. **CI/CD**: All 4 target platforms produce working binaries in release
3. **Code Quality**: No clippy warnings, all errors logged with context
4. **Testing**: >80% code coverage, all protocol commands tested
5. **Documentation**: All breaking changes documented with migration guide
6. **Performance**: No regression in throughput or latency

---

## Next Steps

1. Review this plan with team stakeholders
2. Assign tasks to developers based on expertise
3. Set up tracking board (GitHub Projects, etc.)
4. Begin Wave 1 implementation
5. Review progress weekly and adjust plan as needed
