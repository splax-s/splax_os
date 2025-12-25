# SPLAX OS - MASTER DEVELOPMENT BLUEPRINT (v2.0)

> This is your single source of truth. GitHub Copilot will use this context to generate correct code.

## 🎯 HOW TO USE THIS PROMPT

1. **CREATE THE FILE**: `touch SPLAX_BLUEPRINT.md` in your `/splax` directory.
2. **COPY THIS ENTIRE TEXT** into that file.
3. **OPEN YOUR EDITOR** (VS Code with Copilot) on the `/splax` folder.
4. **CREATE FILES** as listed in the "File Structure" section. For each new file, Copilot will read this blueprint and generate appropriate code.
5. **REFERENCE THIS DOCUMENT** if Copilot suggests non-compliant code: "That violates Splax Constraint #2. Use the S-CAP pattern instead."

---

## 🚀 PROJECT IDENTITY: THE SPLAX MANIFESTO

We are not building "another Linux distro" or a "research OS." We are building **Splax**: a production-grade, capability-secure, distributed-first operating system designed to power the next generation of developers, telecom infrastructure, and cloud control planes.

**The One-Liner**: *"Splax is where your laptop feels like a cloud region, nothing runs unless you ask, and security is built in, not bolted on."*

---

## 🏗️ ARCHITECTURE: THE S-PILLARS

This is our stack. **Do not deviate.**

```
┌─────────────────────────────────────────┐
│             APPLICATIONS                │  # User-facing software
│  • S-WAVE: WASM runtime (primary)       │
│  • S-NATIVE: Native sandbox (limited)   │
├─────────────────────────────────────────┤
│             SYSTEM SERVICES             │  # The "Cloud" in the OS
│  • S-ATLAS: Service registry & discovery│
│  • S-LINK: Internal capability messaging│
│  • S-GATE: External gateway (TCP/HTTP)  │
│  • S-STORAGE: Object storage            │
├─────────────────────────────────────────┤
│               S-CORE KERNEL             │  # The Foundation
│  • Deterministic scheduler              │
│  • Capability enforcement (S-CAP)       │
│  • Zero-copy IPC                        │
│  • Memory manager (no swap, no overcommit)
└─────────────────────────────────────────┘
```

---

## ⚡ NON-NEGOTIABLE CONSTRAINTS (OUR CONSTITUTION)

These rules are **absolute**. Any violation breaks the system's core promise.

1. **NO POSIX**: No `fork`, `exec`, `sudo`, `/etc`, `$HOME`, or legacy Unix APIs. We are building the future, not emulating the past.

2. **CAPABILITY-ONLY SECURITY (S-CAP)**: No users, no groups, no root. Every single resource access requires an explicit, cryptographic capability token. No token = can't even name the resource.

3. **MICROKERNEL ARCHITECTURE**: The kernel (S-CORE) manages only: CPU scheduling, memory, IPC, and capabilities. Everything else (drivers, filesystems, network stacks) runs in userspace as isolated services.

4. **DETERMINISTIC EXECUTION**: The same inputs must produce the same outputs. Predictable latency is a feature, not an optimization. No background "magic" or daemons.

5. **HEADLESS-FIRST**: No GUI code until Phase 3 (Month 7+). We serve developers, telecom, and cloud operators who live in the terminal. GUI is a service, not a privilege.

6. **CROSS-ARCH FROM DAY ONE**: The codebase must simultaneously target `x86_64-splax-none` and `aarch64-splax-none`. No "porting later."

7. **WASM-NATIVE APPS**: The default, secure application model is WebAssembly (S-WAVE). Native code (S-NATIVE) requires explicit, audited capabilities and runs in a strict sandbox.

8. **NETWORKING WITHOUT PORTS**: The concept of a "port" is deprecated. Communication uses S-LINK capability-bound channels. External compatibility is provided by S-GATE services.

9. **NO GLOBAL STATE**: No global variables, no singletons. All state is explicit and capability-scoped.

10. **MEMORY SAFETY**: 99% of the code must be safe Rust. Any `unsafe` block requires a `// SAFETY:` comment justifying its necessity and explaining its invariants.

---

## 📁 FILE STRUCTURE

```
/splax/
├── Cargo.toml                              # Workspace root
├── .cargo/
│   └── config.toml                         # Build configuration
├── splax_kernel.json                       # Custom target spec
├── bootloader/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs                         # UEFI bootloader
├── kernel/                                 # S-CORE
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                          # Kernel entry & core struct
│       ├── arch/
│       │   ├── mod.rs
│       │   ├── x86_64/                     # x86_64 specific code
│       │   │   └── mod.rs
│       │   └── aarch64/                    # ARM64 specific code
│       │       └── mod.rs
│       ├── mm/                             # Memory Manager
│       │   └── mod.rs
│       ├── sched/                          # Deterministic Scheduler
│       │   └── mod.rs
│       ├── cap/                            # S-CAP System (MOST IMPORTANT)
│       │   └── mod.rs
│       └── ipc/                            # IPC Primitives for S-LINK
│           └── mod.rs
├── services/                               # All System Services
│   ├── atlas/                              # S-ATLAS
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── link/                               # S-LINK
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── gate/                               # S-GATE
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── tcp.rs
│   │       └── http.rs
│   └── storage/                            # S-STORAGE
│       ├── Cargo.toml
│       └── src/lib.rs
├── runtime/                                # Execution Environments
│   ├── wave/                               # S-WAVE (WASM)
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── native/                             # S-NATIVE (Sandbox)
│       ├── Cargo.toml
│       └── src/lib.rs
├── tools/                                  # Developer Tools
│   ├── term/                               # S-TERM (CLI)
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── code/                               # S-CODE (Editor)
│       ├── Cargo.toml
│       └── src/main.rs
├── tests/
├── scripts/                                # Build & VM scripts
└── docs/
    └── SPLAX_BLUEPRINT.md                  # THIS FILE
```

---

## 🛠️ TECHNICAL IMPLEMENTATION

### Custom Target Specification

The `splax_kernel.json` defines our platform. We are not `unknown`; we are `splax`.

To build the kernel:
```bash
cargo build -Z build-std=core,alloc --target ./splax_kernel.json --release
```

---

## 📅 DEVELOPMENT ROADMAP: PHASE 1 (0-90 DAYS)

**GOAL**: A bootable system with CLI, working capabilities, internal messaging, service discovery, and external HTTP access.

| Week | Focus | Deliverable | Success Metric |
|------|-------|-------------|----------------|
| 1-2 | Bootloader | UEFI app that loads & verifies kernel. | Boots in QEMU, passes memory map. |
| 3-4 | S-CORE Foundation | Memory manager, scheduler skeleton, interrupt handling. | Kernel panics with a message. |
| 5-6 | S-CAP System | Capability tokens, table, grant/check/revoke logic, audit logs. | Unauthorized access impossible. Unit tests pass. |
| 7-8 | S-LINK Messaging | Capability-bound channels, ring buffers, send/receive. | Two test processes can exchange messages. Latency < 5µs. |
| 9-10 | S-ATLAS Registry | Service registration, discovery, heartbeat, cleanup. | Service A can discover and request a channel to Service B. |
| 11-12 | S-GATE (TCP) & S-TERM (CLI) | TCP→S-LINK gateway. Basic CLI commands. | `curl http://localhost:8080` reaches an internal test service via CLI. |
| 13-14 | S-WAVE Runtime | WASM module loader, capability binding for WASM imports. | Can load & run a simple WASM "hello world." |
| 15-16 | Integration & Polish | Cross-arch build, comprehensive tests, performance tuning. | System meets all Phase 1 success criteria. |

### PHASE 1 SUCCESS CRITERIA

- Boots on x86_64 (QEMU) and ARM64 (Raspberry Pi 4/5 QEMU) in <200ms.
- S-CAP prevents all unauthorized access (zero bypasses in security test).
- S-LINK IPC latency < 5 microseconds.
- S-ATLAS service discovery < 50 microseconds.
- S-GATE exposes an internal HTTP service externally.
- S-TERM CLI starts in <10ms and can list services/create channels.
- S-WAVE can run a compiled WASM module.

---

## 🚫 FORBIDDEN PATTERNS

**COPILOT: NEVER SUGGEST THESE**

If you see these, the code is wrong:

- `fork()`, `exec()`, `sudo`, `chmod`, `chown`
- `listen()`, `bind()`, `accept()` (Use S-GATE)
- Global `static mut` variables
- File paths like `/etc/config.toml` (Use S-STORAGE objects)
- Environment variable configuration (`env::var`)
- `println!` in kernel (Use serial/logger service)
- Dynamic linking (`dlopen`)
- Any GUI code before Phase 3

---

## ✅ SPLAX PATTERNS

**COPILOT: ALWAYS USE THESE**

- **Access Control**: `cap_table.check(process_id, token, "operation")?`
- **Inter-Process Communication**: `s_link::Channel::create(sender, receiver, cap_token)`
- **Service Discovery**: `s_atlas::discover("auth-service", discovery_token)`
- **External Communication**: `s_gate::TcpGateway::new(port, internal_service, firewall_rules)`
- **Storage**: `s_storage::Object::create(data, capabilities)` (returns an `ObjectId`)
- **Errors**: Use explicit `thiserror`-style enums. No generic `Box<dyn Error>`.
- **Configuration**: Declarative structs passed to constructors, not global config files.

---

## 🎯 INSTRUCTIONS FOR GITHUB COPILOT

You are the primary coding assistant for the Splax OS project. Your context is this master blueprint.

**When generating code:**

1. **Check Constraints First**: Does the suggestion violate any of the 10 Non-Negotiable Constraints? If yes, reject it and suggest the Splax pattern.

2. **Follow the Architecture**: Code must belong to the correct S-pillar (S-CORE, S-CAP, S-LINK, etc.).

3. **Be Explicit with Security**: Every resource access must include a capability check. Show the token variable and the `check` call.

4. **Write Testable Code**: Prefer pure functions, explicit dependencies, and return `Result` types.

5. **Add Documentation**: Include `///` doc comments explaining the "why," especially for security-critical code.

### Example of Good Copilot Guidance

User creates `kernel/src/cap/grant.rs`

Copilot should generate: Code for a `grant` function that validates the granter's capability, creates a new child token, adds to audit log, and returns a `Result<CapabilityToken, CapError>`.

### Example of Correcting Copilot

If Copilot suggests `std::fs::read_to_string("/config.json")`

You respond: "That violates Splax Constraint #1 (NO POSIX) and #9 (Storage Model). We do not use filesystem paths. Configuration should be passed as a struct to the constructor, or stored as an object in S-STORAGE accessed via a capability token."

---

## 🏁 FINAL WORD FROM THE CTO

This document is the constitution of Splax. It exists to prevent scope creep, ensure architectural integrity, and maintain focus on our core value: **building a system that is secure by construction, fast by design, and simple by choice.**

We are not here to reinvent the Linux desktop. We are here to make Linux (and its problems) irrelevant for the next generation of systems.

**Now, build.**
