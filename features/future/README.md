# Future Features (P2/P3)

This directory contains feature specifications for planned but not-yet-implemented functionality. These are advanced features scheduled for post-launch development.

## Features in This Directory

| Feature | Description | Scenarios | Priority |
|---------|-------------|-----------|----------|
| `contact_recovery.feature` | Recover contacts via social vouching after device loss | 47 | P2 |
| `duress_password.feature` | Decoy profile under coercion, silent alerts | 45 | P3 |
| `hidden_contacts.feature` | Secret gesture/PIN to reveal contacts, plausible deniability | 36 | P3 |
| `tor_mode.feature` | Route traffic through Tor, circuit management, bridge support | 29 | P3 |

**Total**: 157 scenarios (unimplemented)

## Priority Definitions

### P2: Contact Recovery
`contact_recovery.feature` is P2 because:
- **Important for user confidence** - Users need assurance they can recover
- **No external dependencies** - Uses existing relay infrastructure
- **Clear implementation path** - Well-defined protocol

### P3: Advanced Privacy
Other features are P3 (post-launch) because:
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
