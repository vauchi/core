# Future Features (P3)

This directory contains feature specifications for planned but not-yet-implemented functionality. These are advanced privacy features scheduled for post-launch development.

## Features in This Directory

| Feature | Description | Scenarios |
|---------|-------------|-----------|
| `duress_password.feature` | Decoy profile under coercion, silent alerts | 45 |
| `hidden_contacts.feature` | Secret gesture/PIN to reveal contacts, plausible deniability | 36 |
| `tor_mode.feature` | Route traffic through Tor, circuit management, bridge support | 29 |

**Total**: 110 scenarios (unimplemented)

## Why P3 Priority?

These features are classified as P3 (post-launch) because:

1. **Not required for MVP** - Core functionality works without them
2. **Complex implementation** - Require significant additional infrastructure
3. **Niche use cases** - Target users with specific threat models
4. **Opt-in features** - Won't affect users who don't enable them

## Implementation Notes

When implementing these features:

1. Create a planning document in `docs/planning/todo/`
2. Follow TDD: Write failing tests first
3. Consider security implications carefully (see `docs/THREAT_ANALYSIS.md`)
4. These features may require platform-specific implementations

## Related Documentation

- `docs/THREAT_ANALYSIS.md` - Security threat model
- `docs/planning/` - Implementation planning documents
- Parent `features/README.md` - Full feature status overview
