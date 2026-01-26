# Security Policy

## Reporting a Vulnerability

The Vauchi team takes security seriously. We appreciate your efforts to responsibly disclose your findings.

### How to Report

**Do NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to:

**security@vauchi.app**

Include the following information:

- Type of vulnerability (e.g., buffer overflow, cryptographic weakness, data exposure)
- Full paths of source file(s) related to the vulnerability
- Location of the affected source code (tag/branch/commit or direct URL)
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact assessment and potential attack scenarios

### What to Expect

| Timeline | Action                                         |
| -------- | ---------------------------------------------- |
| 24 hours | Acknowledgment of your report                  |
| 72 hours | Initial assessment and severity classification |
| 7 days   | Detailed response with remediation plan        |
| 90 days  | Public disclosure (coordinated with reporter)  |

We will keep you informed of our progress throughout the process.

### Scope

**In Scope:**

- `vauchi-core` - Cryptographic implementation, key management, data storage
- `vauchi-mobile` - UniFFI bindings, mobile-specific security

**Out of Scope:**

- Third-party dependencies (report directly to maintainers, but let us know)
- Social engineering attacks
- Denial of service attacks that don't reveal design flaws
- Issues in development/test environments only

### Safe Harbor

We consider security research conducted in accordance with this policy to be:

- Authorized under the Computer Fraud and Abuse Act (CFAA)
- Exempt from DMCA restrictions on circumvention
- Lawful and helpful to the overall security of the project

We will not pursue legal action against researchers who:

- Act in good faith
- Avoid privacy violations, data destruction, and service disruption
- Report findings promptly and allow reasonable time for remediation
- Do not exploit vulnerabilities beyond proof-of-concept

### Recognition

We recognize security researchers who help improve Vauchi:

- Acknowledgment in release notes (with permission)
- Listing in our Security Hall of Fame (optional)
- Reference letters for researchers (upon request)

We are a nonprofit and cannot offer monetary bounties at this time.

## Security Design

For details on Vauchi's security architecture, see:

- [Threat Analysis](docs/THREAT_ANALYSIS.md) - Threat model and mitigations
- [Security Audit Checklist](docs/SECURITY_AUDIT.md) - Audit guide for reviewers
- [Architecture Docs](docs/architecture/) - System design

### Key Security Properties

| Property              | Implementation                               |
| --------------------- | -------------------------------------------- |
| End-to-end encryption | AES-256-GCM with per-contact keys            |
| Forward secrecy       | Double Ratchet protocol                      |
| Key derivation        | HKDF-SHA256 with domain separation           |
| Password protection   | PBKDF2 (100k iterations) + zxcvbn validation |
| Key zeroing           | `zeroize` crate for memory cleanup           |
| Cryptographic library | `ring` (audited, no custom crypto)           |

### Supported Versions

| Version | Supported        |
| ------- | ---------------- |
| 1.x.x   | Yes              |
| < 1.0   | No (pre-release) |

## Contact

- Security issues: security@vauchi.app
- General questions: hello@vauchi.app
- Project: https://github.com/vauchi
