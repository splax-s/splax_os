# Splax OS Community

Welcome to the Splax OS community! This document provides information on how to participate, get help, and contribute.

## Communication Channels

### Forums

The official community forum is the best place for:
- General questions and discussions
- Feature requests
- Sharing projects built on Splax OS
- Meeting other community members

**URL**: https://community.splax-os.org

### Discord

For real-time chat and quick questions:

**Invite Link**: https://discord.gg/splax-os

Channels:
- `#general` - General discussion
- `#help` - Getting help
- `#development` - Kernel and service development
- `#announcements` - Official announcements
- `#showcase` - Show off your projects

### Matrix

For those who prefer Matrix:

**Room**: `#splax-os:matrix.org`

### Mailing Lists

For in-depth technical discussions:

| List | Address | Purpose |
|------|---------|---------|
| users | users@lists.splax-os.org | User questions |
| dev | dev@lists.splax-os.org | Development discussion |
| security | security@lists.splax-os.org | Security advisories |
| announce | announce@lists.splax-os.org | Announcements |

### IRC

**Network**: irc.libera.chat
**Channel**: #splax-os

## Getting Help

### Before Asking

1. **Search existing resources**:
   - Documentation: `./docs/`
   - FAQ: https://splax-os.org/faq
   - GitHub Issues: https://github.com/splax-s/splax_os/issues
   - Forum search: https://community.splax-os.org/search

2. **Check the manual**:
   ```bash
   splax> help <topic>
   splax> man <command>
   ```

### Asking Good Questions

When asking for help, include:

1. **What you're trying to do**
2. **What you expected to happen**
3. **What actually happened**
4. **Steps to reproduce**
5. **System information**:
   ```bash
   splax> version
   splax> cpuinfo
   splax> mem
   ```
6. **Relevant logs**:
   ```bash
   splax> log show --since 5m
   ```

### Response Times

| Channel | Expected Response |
|---------|-------------------|
| Discord/Matrix | Minutes to hours |
| Forum | Hours to days |
| Mailing List | Days |
| GitHub Issues | Days (bugs) |

## Contributing

### Ways to Contribute

- **Code**: Bug fixes, features, drivers
- **Documentation**: Tutorials, guides, API docs
- **Testing**: Bug reports, test cases, fuzzing
- **Translation**: Internationalization
- **Design**: UI/UX, graphics, branding
- **Community**: Helping others, moderation

### Getting Started

1. Read `CONTRIBUTING.md`
2. Set up development environment
3. Find a "good first issue"
4. Join Discord `#development`

### Code of Conduct

We follow the [Contributor Covenant](https://www.contributor-covenant.org/). Key points:

- Be respectful and inclusive
- No harassment or discrimination
- Assume good faith
- Focus on constructive feedback

Full text: `CODE_OF_CONDUCT.md`

## Events

### Regular Meetings

| Event | When | Where |
|-------|------|-------|
| Community Call | Monthly, 1st Saturday | Discord/YouTube |
| Dev Meeting | Bi-weekly, Thursday | Discord |
| Office Hours | Weekly, Tuesday | Discord |

### Conferences

We participate in:
- FOSDEM
- Linux Plumbers Conference
- RustConf
- OSFC

### Hackathons

Periodic hackathons focused on:
- Driver development
- Service creation
- Documentation sprints
- Security auditing

## Recognition

### Contributors

All contributors are listed in:
- `CONTRIBUTORS.md`
- https://splax-os.org/contributors

### Levels

| Level | Criteria |
|-------|----------|
| Contributor | 1+ merged PR |
| Regular | 10+ contributions |
| Core | Commit access |
| Maintainer | Area ownership |

### Swag

Active contributors may receive:
- Stickers
- T-shirts
- Conference tickets

## Resources

### Official

- **Website**: https://splax-os.org
- **Documentation**: https://docs.splax-os.org
- **GitHub**: https://github.com/splax-s/splax_os
- **Blog**: https://blog.splax-os.org

### Social

- **Twitter/X**: @splax_os
- **Mastodon**: @splax_os@fosstodon.org
- **Reddit**: r/splax_os
- **YouTube**: Splax OS

### Package Repository

- **Stable**: https://repo.splax-os.org/stable
- **Nightly**: https://repo.splax-os.org/nightly

## Governance

### Decision Making

Splax OS uses a consensus-seeking model:

1. Proposal via GitHub issue or mailing list
2. Discussion period (minimum 7 days for major changes)
3. Maintainer review
4. Consensus or vote

### Roles

| Role | Responsibility |
|------|----------------|
| BDFL | Overall direction (for now) |
| Maintainers | Merge authority |
| Reviewers | Code review |
| Triagers | Issue management |

### RFC Process

Major changes require an RFC:

1. Fork RFC repo
2. Write RFC using template
3. Submit PR
4. Discussion
5. FCP (Final Comment Period)
6. Merge or close

## Sponsorship

### Supporting Splax OS

- **GitHub Sponsors**: https://github.com/sponsors/splax-s
- **Open Collective**: https://opencollective.com/splax-os

### Sponsors

Thank you to our sponsors:
- [Your company here]

### How Funds Are Used

- Infrastructure costs
- Development hardware
- Conference attendance
- Contributor rewards
- Security audits

## Legal

### License

Splax OS is dual-licensed:
- MIT License
- Apache License 2.0

See `LICENSE-MIT` and `LICENSE-APACHE`.

### Trademark

"Splax OS" and the Splax logo are trademarks. Usage guidelines at:
https://splax-os.org/trademark

---

*Community Guidelines Version: 1.0*
*Last Updated: January 2026*
