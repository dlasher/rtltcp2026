# rtltcp Project Plan: Revised Remediation Approach

## Executive Summary

This revised plan addresses the actual remaining issues in the rtltcp project, focusing on genuine problems rather than those already fixed. Based on the current codebase analysis, the original evaluation report was significantly outdated, with many "critical" issues already resolved.

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
- **Current**: Defaults to `[::]` (all interfaces)
- **Target**: Change to `127.0.0.1` for local-only by default with deprecation notice
- **Location**: `src/main.rs` line 51
- **Acceptance Criteria**: 
  - Default address changed to `127.0.0.1`
  - Warning logged when binding to all interfaces
  - Clear documentation of breaking change and migration path

### Task 1.2: Add IP Allowlist/Access Control (Should-Have)
- **Current**: Any network client can connect and control the SDR device
- **Target**: Implement configurable IP allowlist
- **Location**: `src/main.rs` connection handling
- **Acceptance Criteria**:
  - CLI flag for allowlist file or comma-separated IPs
  - Connections from non-allowed IPs are rejected with appropriate error
  - Default behavior allows localhost connections

### Task 1.3: Make Timeouts Configurable (Should-Have)
- **Current**: Timeouts are hardcoded to 30s
- **Target**: Add CLI flags for configurable read/write timeout values
- **Location**: `src/main.rs` lines 121-122
- **Acceptance Criteria**:
  - CLI flags for read/write timeout values
  - Timeouts applied immediately after connection accept
  - Proper logging of timeout events

### Task 1.4: Input Validation for Protocol Payloads (Should-Have)
- **Current**: No validation of protocol command payloads
- **Target**: Validate frequency, sample rate, gain ranges before passing to device
- **Location**: Command handlers in `src/main.rs`
- **Acceptance Criteria**:
  - Frequency validated against device capabilities
  - Sample rate validated (positive, within range)
  - Gain values validated (within supported range)
  - PPM values validated (reasonable range)
  - Invalid commands logged and rejected gracefully

---

## Wave 2: System Hardening & Testing 🟡

**Goal**: Improve system reliability, add comprehensive testing, and enhance systemd configuration.

**Estimated Effort**: 2-3 days
**Dependencies**: None

### Task 2.1: Multi-Client Handling (Should-Have)
- **Current**: No handling for second client connection
- **Target**: Gracefully reject or handle multiple connections
- **Location**: Connection handling in `src/main.rs`
- **Acceptance Criteria**:
  - Second connection is rejected with appropriate error
  - Existing connection is not affected
  - Log message for rejected connections

### Task 2.2: Harden systemd Service (Should-Have)
- **Current**: systemd service lacks hardening directives
- **Target**: Add security directives to systemd service file
- **Location**: `rtltcp.service` or similar
- **Acceptance Criteria**:
  - `User=` and `Group=` directives set
  - `NoNewPrivileges=true`
  - `ProtectSystem=strict`
  - `ProtectHome=true`
  - `PrivateTmp=true`

### Task 2.3: Comprehensive Testing (Should-Have)
- **Current**: Limited integration tests exist
- **Target**: Add comprehensive test coverage
- **Location**: `tests/` directory
- **Acceptance Criteria**:
  - All protocol commands tested with valid and invalid inputs
  - Integration tests with mock device abstraction
  - Performance tests for throughput and memory usage

---

## Wave 3: Code Quality & Documentation Improvements 🟢

**Goal**: Improve code maintainability, error handling, and documentation.

**Estimated Effort**: 2-3 days
**Dependencies**: None

### Task 3.1: Custom Error Type (Should-Have)
- **Current**: Using `Box<dyn std::error::Error>`
- **Target**: Create custom `RtlTcpError` enum with context
- **Location**: New file `src/error.rs`
- **Acceptance Criteria**:
  - All error variants covered (device, network, config, etc.)
  - Proper `Display` and `Debug` implementations
  - `From` implementations for underlying errors

### Task 3.2: Fix Racy should_exit Check (Should-Have)
- **Current**: `should_exit` checked after `read_exact` blocks indefinitely
- **Target**: Make reads interruptible by signal
- **Location**: `src/main.rs` line 134
- **Acceptance Criteria**:
  - Signal can interrupt active read
  - Clean shutdown on Ctrl-C even during I/O
  - No race conditions in exit handling

### Task 3.3: Enhanced Documentation (Nice-to-Have)
- **Current**: Basic documentation only
- **Target**: Comprehensive documentation for all public interfaces
- **Location**: `README.md` and inline documentation
- **Acceptance Criteria**:
  - Usage examples for all CLI flags
  - Security considerations documented
  - Migration guide for breaking changes
  - Configuration examples

---

## Wave 4: CI/CD & Supply Chain Security 🔵

**Goal**: Ensure robust CI/CD pipeline and supply chain security.

**Estimated Effort**: 1-2 days
**Dependencies**: None

### Task 4.1: Update Security Audit Workflow (Should-Have)
- **Current**: `rustsec/audit-check@v2` is deprecated
- **Target**: Update to maintained security action
- **Location**: `.github/workflows/audit.yml`
- **Acceptance Criteria**:
  - Replace deprecated GitHub Action
  - Ensure audit runs on all PRs and scheduled basis
  - Fix any security vulnerabilities found

### Task 4.2: Verify Cross-Platform Builds (Should-Have)
- **Current**: Cross-compilation already works correctly
- **Target**: Ensure all target platforms produce verified artifacts
- **Location**: `.github/workflows/cd.yml`
- **Acceptance Criteria**:
  - All 4 target platforms produce working binaries
  - Cross-compiled binaries work on target architecture
  - Build artifacts are correctly named and packaged

---

## Implementation Schedule

### Phase 1: Critical Security (Wave 1) - Week 1
- Days 1-2: Tasks 1.1, 1.3 (Default address change, input validation)
- Days 2-3: Task 1.2, 1.4 (IP allowlist, timeout configuration)

### Phase 2: System Hardening (Wave 2) - Week 2
- Days 1-2: Tasks 2.1, 2.2 (Multi-client handling, systemd hardening)
- Days 3-5: Task 2.3 (Comprehensive testing)

### Phase 3: Code Quality (Wave 3) - Week 3
- Days 1-2: Task 3.1 (Custom error type)
- Days 2-3: Task 3.2 (Racy should_exit fix)
- Days 3-5: Task 3.3 (Enhanced documentation)

### Phase 4: CI/CD Improvements (Wave 4) - Week 4
- Days 1-2: Task 4.1, 4.2 (Security audit update, cross-platform verification)

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Breaking change from default bind address | High | Medium | Clear deprecation notice, maintain backward compatibility flag |
| IP allowlist complexity for simple SDR server | Medium | Medium | Consider if allowlist is necessary or if localhost default suffices |
| Supply chain risk from rtlsdr_mt crate | Medium | Low | Pin versions, audit crate, have fallback plan |
| TLS complexity if added later | High | Low | Separate port approach rather than protocol negotiation |

---

## Success Criteria

1. **Security**: All critical security findings resolved (default bind address, authentication, timeouts)
2. **Code Quality**: Custom error type implemented, proper error handling
3. **Testing**: Comprehensive test coverage for all protocol commands
4. **CI/CD**: Updated security audit, verified cross-platform builds
5. **Documentation**: Clear migration guide for breaking changes
6. **Systemd**: Proper hardening directives in service configuration

---

## Breaking Changes Notice

The following changes constitute breaking changes that will require a major version bump:

1. **Default Bind Address Change**: Changing from `[::]` (all interfaces) to `127.0.0.1` (localhost only) by default is a breaking change for users who rely on the default behavior. Users who need to bind to all interfaces will need to explicitly specify `--address 0.0.0.0` or `--address [::]`.

Migration path:
- Existing users should add `--address 0.0.0.0` to maintain current behavior
- Document the security implications of binding to all interfaces
- Add warning logs when binding to public interfaces

---

## Next Steps

1. Review this revised plan with team stakeholders
2. Implement Wave 1 changes first (security hardening)
3. Update documentation to reflect breaking changes
4. Implement testing and hardening improvements
5. Verify CI/CD pipeline updates
