# Exchange Protocol

Physical proximity is verified through multiple mechanisms.

## Option A: QR Code + Audio Verification (Primary)

1. User A displays QR code containing:
   - Public key
   - One-time exchange token
   - Short audio challenge
2. User B scans QR code
3. Both devices emit/listen for ultrasonic audio handshake
4. Audio verification confirms physical proximity (prevents remote QR scanning)

## Option B: Bluetooth Low Energy (BLE)

1. Both devices advertise availability
2. RSSI (signal strength) used to verify proximity (<2m)
3. Public keys exchanged over BLE
4. Challenge-response to prevent relay attacks

## Option C: NFC (Near Field Communication)

1. Devices must be within centimeters
2. Exchange public keys and signed tokens
3. Most secure for proximity but requires NFC hardware

## Exchange Protocol Flow

```
User A                                    User B
   │                                         │
   │──── Display QR (pubkey + token) ────────│
   │                                         │
   │◄──── Scan QR, extract pubkey ───────────│
   │                                         │
   │──── Emit ultrasonic challenge ──────────│
   │                                         │
   │◄──── Respond with signed challenge ─────│
   │                                         │
   │──── X3DH Key Agreement ─────────────────│
   │                                         │
   │◄─── Exchange encrypted contact cards ───│
   │                                         │
   │──── Store contact, establish sync ──────│
   │                                         │
```

## Current Implementation

The MVP uses QR code exchange with manual paste (camera scanning planned). The X3DH key agreement is fully implemented for secure key establishment.
