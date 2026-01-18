# WebBook User Guide

Welcome to WebBook, a privacy-focused contact card exchange app. This guide will help you get started and make the most of WebBook's features.

---

## Table of Contents

1. [Getting Started](#getting-started)
2. [Your Contact Card](#your-contact-card)
3. [Exchanging Contacts](#exchanging-contacts)
4. [Managing Visibility](#managing-visibility)
5. [Multi-Device Sync](#multi-device-sync)
6. [Backup & Recovery](#backup--recovery)
7. [Troubleshooting](#troubleshooting)

---

## Getting Started

### Creating Your Identity

When you first open WebBook, you'll be asked to create your identity:

1. **Enter your display name** - This is how contacts will see you
2. **Tap "Create Identity"** - WebBook generates your unique cryptographic identity

Your identity includes:
- A unique public ID (like a fingerprint for your identity)
- Cryptographic keys for secure communication
- Your display name

**Important:** Your identity keys are stored only on your device. There's no account to log into - your device IS your account.

### Understanding the Home Screen

After creating your identity, you'll see:

- **Your Card** - The contact information you've added
- **Fields** - Individual pieces of contact info (email, phone, etc.)
- **Navigation** - Access to Contacts, Exchange, and Settings

---

## Your Contact Card

### Adding Fields

Your contact card contains fields - pieces of information you want to share with contacts.

**To add a field:**
1. Tap the **+** button on the home screen
2. Select a field type:
   - **Email** - Email addresses
   - **Phone** - Phone numbers
   - **Website** - URLs and websites
   - **Address** - Physical addresses
   - **Social** - Social media profiles
   - **Custom** - Any other information
3. Enter a **label** (e.g., "Work", "Personal")
4. Enter the **value** (e.g., "john@example.com")
5. Tap **Add**

### Editing Fields

To edit a field's value:
1. Tap the **edit** button next to the field
2. Modify the value
3. Tap **Save**

### Removing Fields

To remove a field:
1. Tap the **delete** button (×) next to the field
2. Confirm the deletion

**Note:** Removing a field removes it from your card. Contacts who previously received this field will no longer see updates to it.

---

## Exchanging Contacts

### How Exchange Works

WebBook uses QR codes for secure, in-person contact exchange. Both parties must be present and scan each other's codes.

### Starting an Exchange

1. Navigate to **Exchange** from the home screen
2. Your QR code appears on screen
3. Have your contact scan your code
4. Scan their code in return

### Scanning a Contact's Code

1. Tap **Scan QR Code**
2. Point your camera at their QR code
3. The exchange completes automatically

### After Exchange

Once exchanged:
- You appear in each other's contact list
- You can see fields they've made visible to you
- Changes to visible fields sync automatically
- You control what they see via visibility settings

### Verifying Contacts

For maximum security, verify your contact's identity in person:

1. Open the contact's detail page
2. Tap **Verify Identity**
3. Compare the displayed fingerprints verbally
4. If they match, tap **Mark as Verified**

Verified contacts show a verification badge, confirming the person you exchanged with is who they claim to be.

---

## Managing Visibility

### Per-Contact Visibility

You control exactly what each contact can see. By default, new fields are visible to everyone.

**To manage visibility for a contact:**
1. Go to **Contacts**
2. Select a contact
3. Scroll to "What They Can See"
4. Toggle fields on/off

### Visibility Options

For each field, you can set:
- **Visible** - The contact can see this field
- **Hidden** - The contact cannot see this field

### Bulk Visibility Changes

On the home screen, tap the **visibility** button next to any field to:
- **Show to all** - Make visible to all contacts
- **Hide from all** - Hide from all contacts
- Toggle individual contacts

---

## Multi-Device Sync

### Linking a New Device

Use the same WebBook identity across multiple devices:

1. On your existing device, go to **Settings > Devices**
2. Tap **Link New Device**
3. A link code appears (valid for 10 minutes)
4. On your new device, install WebBook
5. Choose **Join Existing Identity**
6. Enter or scan the link code

### Managing Devices

View and manage linked devices:

1. Go to **Settings > Devices**
2. See all linked devices
3. Your current device is marked

### Revoking a Device

If a device is lost or stolen:

1. Go to **Settings > Devices** on another device
2. Find the device to revoke
3. Tap **Revoke**
4. Confirm the action

**Important:** You cannot revoke your current device. Use another linked device to revoke a lost one.

### How Sync Works

- Changes sync automatically when online
- Sync uses end-to-end encryption
- The relay server cannot read your data
- Offline changes sync when connectivity returns

---

## Backup & Recovery

### Creating a Backup

Protect your identity with an encrypted backup:

1. Go to **Settings > Backup**
2. Tap **Export Backup**
3. Enter a strong password (must pass strength check)
4. Confirm the password
5. Copy or save the backup code

**Important:**
- Store your backup code securely (password manager, printed copy)
- Remember your backup password - it cannot be recovered
- The backup code + password = your entire identity

### Restoring from Backup

To restore on a new device:

1. Install WebBook
2. Choose **Restore from Backup**
3. Paste your backup code
4. Enter your backup password
5. Your identity is restored

### Social Recovery

If you lose access to all devices and don't have a backup:

1. Install WebBook on a new device
2. Create a new identity
3. Go to **Settings > Recovery**
4. Tap **Recover Old Identity**
5. Enter your old public ID
6. A recovery claim is generated
7. Share this claim with trusted contacts
8. They create vouchers to verify it's really you
9. Once enough vouchers are collected, recovery completes

**Requirements:**
- You need vouchers from multiple contacts
- Each contact must have previously exchanged with your old identity
- This proves your social network recognizes the recovery request

### Helping Others Recover

If a contact asks you to vouch for their recovery:

1. Go to **Settings > Recovery**
2. Tap **Help Someone Recover**
3. Paste their recovery claim
4. Verify their identity (call them, meet in person)
5. Create a voucher
6. Share the voucher with them

**Warning:** Only vouch if you're certain of their identity. This prevents identity theft.

---

## Troubleshooting

### Sync Not Working

**Check your connection:**
- Ensure you have internet connectivity
- Check if the relay server is reachable
- Try manual sync from Settings

**Check device status:**
- Verify the device is still linked (Settings > Devices)
- Ensure the device hasn't been revoked

### Contact Not Updating

- Changes may take a few moments to sync
- Both devices must be online
- Check visibility settings - they may have hidden the field

### QR Code Won't Scan

- Ensure adequate lighting
- Hold the camera steady
- Clean your camera lens
- Try adjusting distance to the QR code
- Ensure the full QR code is visible

### Lost Device

1. From another linked device, revoke the lost device
2. If no other devices exist, use social recovery
3. Consider creating a new backup after recovery

### Forgot Backup Password

Unfortunately, backup passwords cannot be recovered. The encryption is designed so that only you can decrypt your backup. Options:

1. Use social recovery if available
2. Create a new identity and re-exchange with contacts
3. Check if you have another linked device still accessible

### App Crashes or Freezes

1. Force close and restart the app
2. Check for app updates
3. Restart your device
4. If issues persist, export a backup and reinstall

---

## Tips & Best Practices

### Security Tips

1. **Verify important contacts** in person
2. **Create a backup** as soon as you set up
3. **Use a strong backup password** (passphrase recommended)
4. **Revoke lost devices** immediately
5. **Don't share backup codes** via insecure channels

### Privacy Tips

1. **Review visibility settings** when adding new fields
2. **Remove contacts** you no longer communicate with
3. **Use specific labels** to control field visibility precisely
4. **Check what contacts can see** periodically

### Organization Tips

1. **Use descriptive labels** (e.g., "Work Email", "Personal Cell")
2. **Group related fields** by type
3. **Update outdated information** promptly

---

## Keyboard Shortcuts (Desktop/TUI)

| Action | Shortcut |
|--------|----------|
| Navigate down | `j` or `↓` |
| Navigate up | `k` or `↑` |
| Go back | `Esc` |
| Add field | `a` |
| Delete selected | `x` or `Delete` |
| Open contacts | `c` |
| Open exchange | `e` |
| Open settings | `s` |
| Help | `?` |
| Quit | `q` |

---

## Getting Help

- **GitHub Issues:** Report bugs or request features
- **FAQ:** See frequently asked questions in docs/faq.md
- **Source Code:** WebBook is open source - inspect how it works

---

*Thank you for using WebBook. Your privacy matters.*
