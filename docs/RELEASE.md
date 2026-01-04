# Splax OS Release Management

This document describes the release process, versioning scheme, and long-term support (LTS) policy for Splax OS.

## Versioning

Splax OS follows [Semantic Versioning](https://semver.org/):

```
MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]
```

- **MAJOR**: Breaking changes to APIs or system behavior
- **MINOR**: New features, backward-compatible
- **PATCH**: Bug fixes, security updates
- **PRERELEASE**: alpha, beta, rc.1, rc.2, etc.

### Examples

| Version | Type | Description |
|---------|------|-------------|
| 0.1.0-alpha | Alpha | Early development, unstable |
| 0.2.0-beta | Beta | Feature complete, testing |
| 0.3.0-rc.1 | Release Candidate | Final testing |
| 1.0.0 | Stable | First stable release |
| 1.0.1 | Patch | Bug fix release |
| 1.1.0 | Minor | New features |
| 2.0.0 | Major | Breaking changes |

## Release Channels

### Nightly

- Built from `main` branch every night
- Cutting edge features
- May contain bugs
- Not recommended for production
- Version format: `0.x.0-nightly.YYYYMMDD`

### Alpha

- Early development releases
- Major features being developed
- APIs may change
- Version format: `0.x.0-alpha.N`

### Beta

- Feature freeze
- Focus on testing and stabilization
- APIs mostly stable
- Version format: `0.x.0-beta.N`

### Release Candidate

- Final testing before stable
- Only critical bug fixes
- APIs frozen
- Version format: `0.x.0-rc.N`

### Stable

- Production ready
- Full support and maintenance
- Regular patch releases
- Version format: `X.Y.Z`

### LTS (Long Term Support)

- Extended support period
- Security updates for 3 years
- Bug fixes for 2 years
- Version format: `X.Y.Z-lts`

## Release Schedule

### Regular Releases

| Milestone | Timeline |
|-----------|----------|
| Feature freeze | 4 weeks before release |
| Beta | 3 weeks before release |
| RC.1 | 2 weeks before release |
| RC.2 (if needed) | 1 week before release |
| Stable | Release day |

### Release Cadence

- **Minor releases**: Every 3 months
- **Patch releases**: As needed (security), at least monthly
- **Major releases**: When necessary (breaking changes)

### Current Schedule

| Version | Type | Expected Date | Status |
|---------|------|---------------|--------|
| 0.1.0 | Alpha | Q1 2025 | In Progress |
| 0.2.0 | Alpha | Q2 2025 | Planned |
| 0.3.0 | Beta | Q3 2025 | Planned |
| 0.4.0 | Beta | Q4 2025 | Planned |
| 1.0.0 | Stable | Q1 2026 | Planned |
| 1.0.0-lts | LTS | Q1 2026 | Planned |

## LTS Policy

### Support Tiers

| Tier | Duration | Includes |
|------|----------|----------|
| Active | 1 year | Features, bug fixes, security |
| Maintenance | 1 year | Bug fixes, security |
| Security | 1 year | Security only |
| EOL | - | No support |

### LTS Versions

We designate one major release per year as LTS:

| LTS Version | Release | Active Until | Maint Until | EOL |
|-------------|---------|--------------|-------------|-----|
| 1.0-lts | Q1 2026 | Q1 2027 | Q1 2028 | Q1 2029 |
| 2.0-lts | Q1 2027 | Q1 2028 | Q1 2029 | Q1 2030 |

### What Gets Backported

**Always backported to LTS:**
- Security vulnerabilities
- Data corruption bugs
- Critical system stability issues

**May be backported:**
- Important bug fixes
- Performance improvements (low risk)
- Driver updates for new hardware

**Never backported:**
- New features
- API changes
- Breaking changes

## Release Process

### 1. Preparation

```bash
# Create release branch
git checkout -b release/v0.1.0

# Update version
./scripts/splax version set 0.1.0

# Update CHANGELOG
$EDITOR CHANGELOG.md
```

### 2. Testing

```bash
# Run full test suite
./scripts/splax test all

# Run benchmarks
./scripts/splax bench all

# Build for all architectures
./scripts/splax build --target x86_64
./scripts/splax build --target aarch64
./scripts/splax build --target riscv64
```

### 3. Release Artifacts

```bash
# Create ISOs
./scripts/splax iso --release

# Create checksums
sha256sum target/iso/*.iso > checksums.txt

# Sign releases
gpg --armor --detach-sign target/iso/splax-0.1.0-x86_64.iso
```

### 4. Documentation

- Update README with new version
- Publish API documentation
- Update website
- Write release notes

### 5. Publishing

```bash
# Tag the release
git tag -s v0.1.0 -m "Release v0.1.0"
git push origin v0.1.0

# Push release branch
git push origin release/v0.1.0

# Create GitHub release
gh release create v0.1.0 \
  --title "Splax OS v0.1.0" \
  --notes-file RELEASE_NOTES.md \
  target/iso/*.iso \
  checksums.txt \
  *.sig
```

### 6. Post-Release

- Announce on blog/social media
- Update package repositories
- Merge release branch to main
- Increment version on main

## Artifact Checksums

All release artifacts include SHA256 checksums:

```
splax-0.1.0-x86_64.iso         sha256:abc123...
splax-0.1.0-aarch64.iso        sha256:def456...
splax-0.1.0-riscv64.iso        sha256:789ghi...
```

## Signing

Releases are signed with GPG:

- **Key ID**: `0xABCD1234`
- **Fingerprint**: `XXXX XXXX XXXX XXXX XXXX  XXXX XXXX XXXX XXXX XXXX`
- **Download**: https://splax-os.org/keys/release.asc

Verify signatures:
```bash
gpg --verify splax-0.1.0-x86_64.iso.sig splax-0.1.0-x86_64.iso
```

## Package Repository

### Repository Structure

```
https://repo.splax-os.org/
├── stable/
│   ├── x86_64/
│   ├── aarch64/
│   └── riscv64/
├── beta/
│   └── ...
├── nightly/
│   └── ...
└── lts/
    └── ...
```

### Adding the Repository

```bash
# Stable
splax> pkg source add https://repo.splax-os.org/stable

# LTS
splax> pkg source add https://repo.splax-os.org/lts

# Nightly (not recommended for production)
splax> pkg source add https://repo.splax-os.org/nightly
```

## Security Updates

### Severity Levels

| Level | Response Time | Examples |
|-------|--------------|----------|
| Critical | 24 hours | Remote code execution |
| High | 1 week | Privilege escalation |
| Medium | 2 weeks | Information disclosure |
| Low | Next release | Minor issues |

### Security Advisories

Security advisories are published at:
- https://splax-os.org/security/
- https://github.com/splax-s/splax_os/security/advisories

### Reporting Vulnerabilities

Email: security@splax-os.org
PGP Key: https://splax-os.org/keys/security.asc

## Deprecation Policy

### Timeline

1. **Deprecated**: Feature marked as deprecated, still functional
2. **Warning**: Usage generates warnings
3. **Removed**: Feature removed in next major version

### Minimum Notice

| Item | Notice |
|------|--------|
| Public API | 2 minor releases |
| CLI commands | 1 minor release |
| Configuration | 1 minor release |
| Internal API | Next minor release |

### Migration Guides

For each deprecated feature, we provide:
- Reason for deprecation
- Recommended alternative
- Migration guide
- Timeline

## Upgrade Guide

### Minor Versions

Generally safe to upgrade:
```bash
splax> pkg upgrade
```

### Major Versions

Review breaking changes first:
```bash
# Read release notes
cat /docs/UPGRADE-2.0.md

# Backup data
splax> backup create

# Upgrade
splax> pkg upgrade --major
```

---

*Release Management Version: 1.0*
*Last Updated: January 2026*
