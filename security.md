# Security Policy

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to security@splax.org.

You should receive a response within 48 hours. If for some reason you do not, please follow up via email to ensure we received your original message.

## Supported Versions

Currently, Splax is in active development and does not have versioned releases. All security reports should be made against the latest main branch.

## Security Architecture

Splax uses a capability-based security model (S-CAP) that differs significantly from traditional operating systems:

### Key Security Features:
- **No root user**: All access requires explicit capability tokens
- **Microkernel isolation**: Drivers and services run in userspace
- **WASM sandboxing**: Applications run in WebAssembly sandboxes
- **Zero-trust networking**: Services communicate via capability-bound channels

### Security Considerations:
- The kernel (S-CORE) is written in Rust but contains some `unsafe` code
- The capability system is cryptographic but not yet formally verified
- The project is pre-1.0 and should not be used in production

## Security Updates

When security vulnerabilities are reported:
1. We will acknowledge receipt within 48 hours
2. We will investigate and confirm the vulnerability
3. We will develop a fix and test it
4. We will release the fix and credit the reporter (unless they wish to remain anonymous)

## Security Research

We welcome responsible security research. If you wish to conduct security research on Splax:

1. **Do** test against your own local installations
2. **Do** report any vulnerabilities you find
3. **Do not** attempt to access data that isn't yours
4. **Do not** attempt to disrupt services
5. **Do not** attempt social engineering or physical attacks

## Security Audits

We plan to conduct formal security audits before the 1.0 release. If you are a security researcher or audit firm interested in reviewing Splax, please contact security@splax.org.