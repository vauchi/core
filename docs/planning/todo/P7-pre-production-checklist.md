# P7: Pre-Production Checklist

**Status**: Planning
**Priority**: P0 (Blocking production launch)
**Created**: 2026-01-18

## Overview

Comprehensive checklist of items to address before WebBook can be considered production-ready. Based on deep audit of codebase, user research, and community standards.

---

## 1. FAQ Gaps (User-Requested Information)

Based on research, users of privacy-focused apps most commonly ask about:

### 1.1 App Performance Questions
| Question | Current Status | Action |
|----------|---------------|--------|
| How much storage does the app need? | Not documented | Add to FAQ: ~50MB app, data scales with contacts |
| Does the app drain battery? | Not documented | Add to FAQ: Minimal, no background polling |
| Does the app use mobile data in background? | Not documented | Add to FAQ: Only during active sync |
| What permissions does the app need and why? | Not documented | Add to FAQ: Camera (QR), no contacts access needed |

### 1.2 Privacy Deep-Dive Questions
| Question | Current Status | Action |
|----------|---------------|--------|
| What metadata do you collect? | Partially covered | Expand: IP for rate limiting only, deleted in 24h |
| Where are your servers located? | Not documented | Add to FAQ: Document relay server locations |
| Is encryption on by default? | Implied | Make explicit: Yes, always, cannot be disabled |
| Has the code been audited? | Not documented | Add: Audit status, link when complete |

### 1.3 Accessibility Questions
| Question | Current Status | Action |
|----------|---------------|--------|
| Does the app work with VoiceOver/TalkBack? | Not documented | Test and document accessibility status |
| Are there accessibility settings? | Unknown | Audit and document accessibility features |

### 1.4 Offline & Sync Questions
| Question | Current Status | Action |
|----------|---------------|--------|
| What happens if I make changes offline? | Covered | Already in FAQ |
| How long until contacts see my updates? | Not specific | Add: Real-time when online, queued when offline |

**Files to modify**: `docs/faq.md`

---

## 2. Relay Server Security

### 2.1 TLS Warning on Startup
**Current**: Relay starts without warning if no TLS configured
**Risk**: Operators may accidentally run unencrypted relay
**Action**: Add startup warning when not behind TLS proxy

```rust
// webbook-relay/src/main.rs - Add at startup
if !is_behind_tls_proxy() {
    warn!("⚠️  WARNING: Relay is not configured for TLS!");
    warn!("⚠️  Deploy behind a TLS-terminating reverse proxy (nginx, caddy)");
    warn!("⚠️  See docs/deployment/relay-production.md for setup guide");
}
```

**Detection options**:
1. Check for `X-Forwarded-Proto: https` header presence
2. Environment variable `RELAY_TLS_VERIFIED=true`
3. Check if bound to localhost only (safe for reverse proxy)

**Files to modify**: `webbook-relay/src/main.rs`, `webbook-relay/src/config.rs`

### 2.2 Third-Party Relay Accessibility
**Current status**: Excellent - Docker, systemd, Helm all available
**Action**: Create "Run Your Own Relay" guide for community operators

**Content for `docs/deployment/community-relay.md`**:
- Why run a relay (contribute to network, data sovereignty)
- Minimum requirements (VPS with 1GB RAM, 10GB storage)
- One-command setup with Docker
- How to register with network (future: relay discovery)
- Monitoring and maintenance tips

---

## 3. Missing Governance Documents

### 3.1 CODE_OF_CONDUCT.md
**Status**: Missing
**Action**: Create based on Contributor Covenant

```markdown
# Contributor Covenant Code of Conduct

## Our Pledge

We as members, contributors, and leaders pledge to make participation
in our community a harassment-free experience for everyone...
```

**Location**: Repository root

### 3.2 CONTRIBUTING.md
**Status**: Only inline in README
**Action**: Create dedicated file with full guidelines

**Content**:
- Development setup (all platforms)
- TDD workflow requirement
- PR process and review guidelines
- Code style and formatting
- Testing requirements (90%+ coverage for core)
- Documentation requirements
- Security vulnerability reporting

**Location**: Repository root

### 3.3 SECURITY.md
**Status**: Missing
**Action**: Create vulnerability disclosure policy

**Content**:
- How to report security issues (private email)
- What to include in reports
- Response timeline commitment
- Safe harbor statement
- Scope (what's in/out of security scope)

**Location**: Repository root

### 3.4 LICENSE
**Status**: Referenced as MIT but file missing
**Action**: Add LICENSE file with MIT text

**Location**: Repository root

---

## 4. GitHub Best Practices

### 4.1 Issue Templates
**Status**: Missing
**Action**: Create `.github/ISSUE_TEMPLATE/`

Templates needed:
- `bug_report.md` - Bug reports with reproduction steps
- `feature_request.md` - Feature proposals
- `security_vulnerability.md` - Private security reports

### 4.2 PR Template
**Status**: Missing
**Action**: Create `.github/PULL_REQUEST_TEMPLATE.md`

**Content**:
- Description of changes
- Related issue link
- Test coverage confirmation
- Documentation update confirmation
- TDD confirmation checkbox

### 4.3 GitHub Actions
**Status**: Not present
**Action**: Create CI/CD workflows

Workflows needed:
- `ci.yml` - Run tests on PR
- `release.yml` - Build and publish releases
- `security.yml` - Security scanning (cargo audit)

### 4.4 FUNDING.yml
**Status**: Missing
**Action**: Create `.github/FUNDING.yml`

```yaml
github: anthropics
open_collective: webbook
custom: ["https://webbook.app/donate"]
```

---

## 5. In-App Help & Links

### 5.1 Help/Support Links
**Current**: No in-app help links
**Action**: Add help links to Settings screens

**All platforms should link to**:
- User Guide: `https://webbook.app/user-guide`
- FAQ: `https://webbook.app/faq`
- Report Issue: `https://github.com/anthropics/webbook/issues`
- Privacy Policy: `https://webbook.app/privacy`

**Files to modify**:
- `webbook-android/.../SettingsScreen.kt`
- `webbook-ios/.../SettingsView.swift`
- `webbook-desktop/ui/src/pages/Settings.tsx`
- `webbook-tui/src/ui/settings.rs`

### 5.2 Contribute/Donate Links
**Current**: None in app
**Action**: Add to Settings or About screen

**Links**:
- Contribute: `https://webbook.app/contribute`
- Donate: `https://webbook.app/donate`
- Source Code: `https://github.com/anthropics/webbook`

### 5.3 Version and Build Info
**Current**: Not displayed
**Action**: Add version number to Settings/About

**Display**: "Version 1.0.0 (build 123)"

---

## 6. Website Content

### 6.1 Workflow Descriptions
**Current**: Basic "how it works" section
**Action**: Add detailed workflow pages with screenshots

**Pages needed**:
- `/workflows/exchange` - Step-by-step contact exchange
- `/workflows/visibility` - Managing field visibility
- `/workflows/recovery` - Social recovery process
- `/workflows/multi-device` - Setting up multiple devices

### 6.2 User Guide Web Version
**Current**: Only in docs/
**Action**: Publish user-guide.md to website as HTML

**Location**: `webbook-website/pages/user-guide.html`

### 6.3 FAQ Web Version
**Current**: Only in docs/
**Action**: Publish faq.md to website as HTML

**Location**: `webbook-website/pages/faq.html`

---

## 7. Documentation Improvements

### 7.1 API Documentation
**Current**: Architecture docs exist but no API reference
**Action**: Generate rustdoc and publish

**Command**: `cargo doc --no-deps --document-private-items`

### 7.2 Mobile SDK Documentation
**Current**: UniFFI bindings exist but undocumented
**Action**: Document mobile API for third-party apps

### 7.3 Relay Protocol Documentation
**Current**: Basic in relay README
**Action**: Expand with message format specs, rate limits, error codes

---

## 8. Repository Structure

### 8.1 Multi-Repo Setup
**Status**: Script created, not yet executed
**Action**: Run `scripts/reorganize-monorepo.sh` after review

**Result structure**:
```
WebBook/
├── webbook-code/     # Main code (public)
├── webbook-website/  # GitHub Pages (public)
└── webbook-market/   # Marketing (private)
```

### 8.2 GitHub Repository Creation
**Action**: Create GitHub repositories

1. `webbook` → rename to `webbook-code` or keep as `webbook`
2. `webbook-website` → new public repo, enable GitHub Pages
3. `webbook-market` → new private repo

---

## 9. Pre-Launch Testing

### 9.1 Cross-Platform E2E
**Action**: Full end-to-end test across all platform combinations

| From | To | Exchange | Sync | Recovery |
|------|-----|----------|------|----------|
| Android | iOS | ☐ | ☐ | ☐ |
| Android | Desktop | ☐ | ☐ | ☐ |
| iOS | Desktop | ☐ | ☐ | ☐ |
| CLI | TUI | ☐ | ☐ | ☐ |

### 9.2 Accessibility Audit
**Action**: Test with screen readers on each platform

- [ ] Android TalkBack
- [ ] iOS VoiceOver
- [ ] Desktop screen readers (NVDA, VoiceOver)

### 9.3 Load Testing
**Action**: Verify relay handles expected load

- [ ] 1000 concurrent WebSocket connections
- [ ] 10,000 message/minute throughput
- [ ] 7-day message retention under load

---

## 10. Infrastructure

### 10.1 Production Relay
**Action**: Deploy relay with TLS

- [ ] Provision VPS (Hetzner/DigitalOcean)
- [ ] Configure nginx with Let's Encrypt
- [ ] Deploy relay behind nginx
- [ ] Set up monitoring (Prometheus + Grafana)
- [ ] Configure alerting

### 10.2 Website Deployment
**Action**: Deploy webbook-website to GitHub Pages

- [ ] Push webbook-website to GitHub
- [ ] Enable GitHub Pages
- [ ] Configure custom domain (webbook.app)
- [ ] Verify SSL

### 10.3 App Store Preparation
**Action**: Prepare for app store submissions

**Android**:
- [ ] Generate signed APK/AAB
- [ ] Prepare store listing (screenshots, description)
- [ ] Privacy policy URL
- [ ] Submit to Google Play

**iOS**:
- [ ] Archive for App Store
- [ ] Prepare App Store Connect listing
- [ ] Submit for review

**Desktop**:
- [ ] Code signing (macOS notarization)
- [ ] Windows code signing
- [ ] Create installers (DMG, MSI, AppImage)

---

## Priority Order

1. **P0 - Blocking**:
   - LICENSE file
   - Relay TLS warning
   - FAQ performance questions
   - In-app help links

2. **P1 - Important**:
   - CODE_OF_CONDUCT.md
   - CONTRIBUTING.md
   - SECURITY.md
   - GitHub templates

3. **P2 - Nice to Have**:
   - Workflow documentation
   - API documentation
   - Accessibility audit

4. **P3 - Future**:
   - Community relay guide
   - Third-party SDK docs

---

## Verification

- [ ] All P0 items complete
- [ ] All tests passing
- [ ] Security checklist verified
- [ ] Cross-platform E2E tested
- [ ] Relay deployed and monitored
- [ ] App stores ready for submission

---

## Notes

- FAQ additions based on research showing 63% uninstall for battery drain, 50% for storage issues
- TLS warning critical because relay currently starts with zero security warnings
- In-app links important for user trust and support reduction
- GitHub best practices improve contributor experience significantly
