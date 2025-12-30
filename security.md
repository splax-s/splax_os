# Security Policy

## 🔒 Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

### Private Disclosure

Report vulnerabilities through one of these channels:

1. **GitHub Security Advisories** (Preferred)
   - Go to the [Security tab](https://github.com/splax-s/splax_os/security/advisories)
   - Click "New draft security advisory"
   - Fill in the details and submit

2. **Email**
   - Send to: `security@splax.org`
   - Use our PGP key (see below) for sensitive information

### What to Include

Please provide as much information as possible:

- **Description**: Clear explanation of the vulnerability
- **Impact**: What can an attacker achieve?
- **Reproduction steps**: How to trigger the issue
- **Affected components**: Which files/modules are involved
- **Suggested fix**: If you have one
- **Your contact**: For follow-up questions

### Response Timeline

| Stage | Timeframe |
|-------|-----------|
| Initial acknowledgment | 24-48 hours |
| Preliminary assessment | 72 hours |
| Fix development | 1-2 weeks (severity dependent) |
| Public disclosure | After fix is released |

### Bug Bounty

We don't currently have a formal bug bounty program, but we do:
- Credit all security researchers in our release notes
- Provide swag for significant findings
- Consider bounties for critical vulnerabilities

---

## Supported Versions

| Version | Supported |
|---------|-----------|
| main (development) | ✅ |
| v0.x.x (pre-release) | ✅ |
| < 0.1.0 | ❌ |

Currently, Splax is in active development. All security reports should be made against the latest `main` branch.

---

## Security Architecture

Splax uses a **capability-based security model** (S-CAP) that differs fundamentally from traditional operating systems:

### Defense in Depth

```
┌─────────────────────────────────────────────────────────────┐
│                     WASM Sandbox                            │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                Application Code                        │  │
│  └───────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                  Capability Checks (S-CAP)                  │
│     • Unforgeable tokens • Cryptographic validation        │
├─────────────────────────────────────────────────────────────┤
│                  Userspace Services                         │
│   • Process isolation • Memory protection • IPC channels   │
├─────────────────────────────────────────────────────────────┤
│                  Microkernel (S-CORE)                       │
│         • Minimal TCB • Rust memory safety                 │
├─────────────────────────────────────────────────────────────┤
│                     Hardware                                │
│     • MMU/IOMMU • NX bit • SMEP/SMAP (x86_64)             │
└─────────────────────────────────────────────────────────────┘
```

### Key Security Features

| Feature | Description |
|---------|-------------|
| **No Root User** | All access requires explicit capability tokens. There is no superuser. |
| **Microkernel Isolation** | Drivers and services run in userspace with minimal privileges. |
| **WASM Sandboxing** | Applications run in WebAssembly sandboxes with capability-gated host functions. |
| **Zero-Copy IPC** | Shared memory with capability-based access control prevents confused deputy attacks. |
| **Cryptographic Capabilities** | Tokens are cryptographically signed and cannot be forged. |
| **Memory Safety** | Kernel written in Rust; `unsafe` blocks are minimized and audited. |

### Security Boundaries

1. **Kernel ↔ Userspace**: Hardware-enforced via MMU, ring levels
2. **Service ↔ Service**: Capability-based IPC, no shared state
3. **App ↔ Runtime**: WASM sandbox with explicit imports
4. **Node ↔ Node**: TLS with mutual authentication (planned)

---

## Known Security Considerations

### Current Limitations

| Area | Status | Notes |
|------|--------|-------|
| `unsafe` code in kernel | ⚠️ Audited | Required for hardware access; minimized |
| Capability system | ⚠️ Not formally verified | Planned for 1.0 |
| Crypto implementations | ⚠️ Not constant-time | Using standard algorithms |
| Network stack | ⚠️ New implementation | Needs fuzzing |
| Production readiness | ❌ Not ready | Pre-1.0 software |

### Hardening Roadmap

- [ ] Formal verification of capability model
- [ ] Stack canaries and CFI
- [ ] ASLR for userspace services
- [ ] Memory tagging (aarch64 MTE)
- [ ] Kernel address space isolation
- [ ] Constant-time crypto operations
- [ ] Fuzzing infrastructure

---

## Security Best Practices for Contributors

### Code Review Checklist

When reviewing security-sensitive code:

- [ ] No unbounded allocations from untrusted input
- [ ] All `unsafe` blocks have safety comments
- [ ] Capability checks before privileged operations
- [ ] Integer overflow checks on size calculations
- [ ] Proper error handling (no panics on invalid input)
- [ ] Input validation at trust boundaries

### High-Risk Areas

These areas require extra scrutiny:

1. **Memory Management** (`kernel/src/mm/`)
   - Page table manipulation
   - Physical memory allocation
   - DMA buffer handling

2. **Capability System** (`kernel/src/cap/`, `services/cap/`)
   - Token generation and validation
   - Permission checks
   - Capability delegation

3. **IPC** (`kernel/src/ipc/`)
   - Message validation
   - Shared memory mapping
   - Channel access control

4. **Drivers** (`kernel/src/block/`, `kernel/src/net/`, etc.)
   - Hardware register access
   - DMA operations
   - Interrupt handling

5. **Crypto** (`kernel/src/crypto/`)
   - Key handling
   - Random number generation
   - Algorithm implementations

---

## Security Research

We welcome responsible security research!

### In Scope

- Kernel vulnerabilities (memory corruption, privilege escalation)
- Capability system bypasses
- WASM sandbox escapes
- IPC vulnerabilities
- Driver security issues
- Cryptographic weaknesses

### Out of Scope

- Social engineering
- Physical attacks
- Denial of service (unless particularly novel)
- Issues in dependencies (report upstream)

### Safe Harbor

We will not pursue legal action against security researchers who:

1. Make good faith efforts to avoid privacy violations
2. Avoid data destruction or service degradation
3. Report vulnerabilities promptly
4. Allow reasonable time for fixes before disclosure

---

## Security Audits

We plan to conduct formal security audits before the 1.0 release.

### Audit Status

| Component | Status | Auditor |
|-----------|--------|---------|
| Capability system | Planned | TBD |
| Memory manager | Planned | TBD |
| IPC subsystem | Planned | TBD |
| WASM runtime | Planned | TBD |

If you are a security researcher or audit firm interested in reviewing Splax, please contact `security@splax.org`.

---

## PGP Key

For encrypted communication:

```
Coming soon - key will be published at keys.openpgp.org
Fingerprint: TBD
```

---

## Acknowledgments

We thank the following security researchers for their responsible disclosures:

*No disclosures yet - you could be the first!*

---

*Last updated: December 2024*