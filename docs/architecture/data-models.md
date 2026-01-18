# Data Models

## Contact Card

```typescript
interface ContactCard {
  id: string;                    // Public key fingerprint
  displayName: string;           // User's chosen display name
  avatar?: EncryptedBlob;        // Optional small avatar
  fields: ContactField[];        // Contact information fields
  lastModified: timestamp;       // For sync ordering
  signature: Signature;          // Self-signed for authenticity
}
```

## Contact Field

```typescript
interface ContactField {
  id: string;                    // Unique field identifier
  type: ContactFieldType;        // phone | email | social | address | custom
  label: string;                 // User-defined label (e.g., "Work Phone")
  value: string;                 // The actual contact info
  visibility: VisibilityRule[];  // Who can see this field
}

enum ContactFieldType {
  PHONE = "phone",
  EMAIL = "email",
  SOCIAL = "social",      // Label identifies network (e.g., "twitter", "github")
  ADDRESS = "address",
  WEBSITE = "website",
  CUSTOM = "custom"
}
```

## Visibility Rule

```typescript
interface VisibilityRule {
  contactId: string;             // "*" for all, or specific contact ID
  canView: boolean;              // Permission flag
}
```

## Social Network Registry

Vauchi includes a registry of 35+ social networks with URL templates:

- Twitter, GitHub, LinkedIn, Instagram, TikTok, etc.
- Profile URLs are generated from usernames automatically
- Example: `twitter` + `@username` → `https://twitter.com/username`
