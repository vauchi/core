import { test, expect } from '@playwright/test';
import { 
  setupTestUser, 
  addTestFields, 
  generateQRCode, 
  completeExchange,
  verifyContactExists,
  checkAppState,
  createBackup,
  restoreBackup,
  TEST_USER
} from '../fixtures/test-helpers';

test.describe('Identity Management', () => {
  test('should create new identity', async ({ page }) => {
    await page.goto('/');
    
    // Should show setup page for new user
    await expect(page.locator('[data-testid="setup-page"]')).toBeVisible();
    await expect(page.locator('[data-testid="display-name-input"]')).toBeVisible();
    
    // Create identity
    await page.fill('[data-testid="display-name-input"]', TEST_USER.displayName);
    await page.click('[data-testid="create-identity-btn"]');
    
    // Should redirect to main app
    await expect(page.locator('[data-testid="main-app"]')).toBeVisible();
    
    const state = await checkAppState(page);
    expect(state.hasIdentity).toBe(true);
    expect(state.contactCount).toBe(0);
  });

  test('should display user identity information', async ({ page }) => {
    await setupTestUser(page);
    
    // Check user info is displayed
    await expect(page.locator('[data-testid="user-display-name"]')).toContainText(TEST_USER.displayName);
    await expect(page.locator('[data-testid="user-public-id"]')).toBeVisible();
  });

  test('should add fields to identity', async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
    
    // Verify fields were added
    for (const field of TEST_USER.initialFields) {
      const fieldElement = page.locator('[data-testid="field"]').filter({ hasText: field.label });
      await expect(fieldElement).toBeVisible();
      await expect(fieldElement.locator('[data-testid="field-value"]')).toContainText(field.value);
    }
  });

  test('should update existing field', async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
    
    // Update the email field
    await page.locator('[data-testid="field"]').filter({ hasText: 'Email' }).click();
    await page.click('[data-testid="edit-field-btn"]');
    await page.fill('[data-testid="field-value-input"]', 'updated@example.com');
    await page.click('[data-testid="save-field-btn"]');
    
    // Verify update
    await expect(page.locator('[data-testid="field"]')).toContainText('updated@example.com');
  });

  test('should remove field from identity', async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
    
    const fieldCountBefore = await page.locator('[data-testid="field"]').count();
    
    // Remove the phone field
    await page.locator('[data-testid="field"]').filter({ hasText: 'Phone' }).click();
    await page.click('[data-testid="remove-field-btn"]');
    await page.click('[data-testid="confirm-remove-btn"]');
    
    const fieldCountAfter = await page.locator('[data-testid="field"]').count();
    expect(fieldCountAfter).toBe(fieldCountBefore - 1);
  });
});

test.describe('Contact Exchange', () => {
  test.beforeEach(async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
  });

  test('should generate QR code for exchange', async ({ page }) => {
    const qrData = await generateQRCode(page);
    
    expect(qrData).toBeTruthy();
    expect(qrData.length).toBeGreaterThan(0);
    
    // Verify QR code is displayed
    await expect(page.locator('[data-testid="qr-code"]')).toBeVisible();
  });

  test('should complete contact exchange', async ({ page }) => {
    // Generate QR from first instance
    const qrData = await generateQRCode(page);
    
    // Complete exchange in same instance (simulating second device)
    await completeExchange(page, qrData);
    
    // Verify contact was added
    await verifyContactExists(page, TEST_USER.displayName);
  });

  test('should show exchange progress', async ({ page }) => {
    await page.click('[data-testid="exchange-tab"]');
    await page.click('[data-testid="generate-qr-btn"]');
    
    // Check progress indicators
    await expect(page.locator('[data-testid="exchange-progress"]')).toBeVisible();
    await expect(page.locator('[data-testid="qr-code"]')).toBeVisible();
  });
});

test.describe('Contact Management', () => {
  test.beforeEach(async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
  });

  test('should display contact list', async ({ page }) => {
    // Add a contact first
    const qrData = await generateQRCode(page);
    await completeExchange(page, qrData);
    
    await page.click('[data-testid="contacts-tab"]');
    
    const contactCards = page.locator('[data-testid="contact-card"]');
    await expect(contactCards).toHaveCount(1);
    await expect(contactCards.first()).toContainText(TEST_USER.displayName);
  });

  test('should search contacts', async ({ page }) => {
    // Add multiple contacts
    for (let i = 0; i < 3; i++) {
      const qrData = await generateQRCode(page);
      await completeExchange(page, qrData);
    }
    
    await page.click('[data-testid="contacts-tab"]');
    
    // Search functionality
    await page.fill('[data-testid="contact-search"]', 'Test');
    const searchResults = page.locator('[data-testid="contact-card"]');
    await expect(searchResults).toHaveCount({ min: 1 });
    
    // Clear search
    await page.fill('[data-testid="contact-search"]', '');
    await expect(searchResults).toHaveCount(3);
  });

  test('should remove contact', async ({ page }) => {
    // Add a contact
    const qrData = await generateQRCode(page);
    await completeExchange(page, qrData);
    
    await page.click('[data-testid="contacts-tab"]');
    await page.locator('[data-testid="contact-card"]').first().click();
    await page.click('[data-testid="remove-contact-btn"]');
    await page.click('[data-testid="confirm-remove-btn"]');
    
    // Verify contact is removed
    await expect(page.locator('[data-testid="contact-card"]')).toHaveCount(0);
  });
});

test.describe('Sync Functionality', () => {
  test.beforeEach(async ({ page }) => {
    await setupTestUser(page);
  });

  test('should sync with relay server', async ({ page }) => {
    await page.click('[data-testid="sync-btn"]');
    
    // Should show syncing state
    await expect(page.locator('[data-testid="sync-status"]')).toContainText('Syncing');
    
    // Should complete (mock or real depending on test environment)
    await page.waitForSelector('[data-testid="sync-complete"]', { timeout: 30000 });
    
    // Should show last sync time
    await expect(page.locator('[data-testid="last-sync-time"]')).toBeVisible();
  });

  test('should handle sync failure gracefully', async ({ page }) => {
    // Simulate offline
    await page.context().setOffline(true);
    
    await page.click('[data-testid="sync-btn"]');
    
    // Should show error state
    await expect(page.locator('[data-testid="sync-error"]')).toBeVisible();
    await expect(page.locator('[data-testid="sync-error"]')).toContainText('No internet connection');
    
    // Restore connection
    await page.context().setOffline(false);
  });
});

test.describe('Backup and Recovery', () => {
  test.beforeEach(async ({ page }) => {
    await setupTestUser(page);
    await addTestFields(page);
  });

  test('should create backup', async ({ page }) => {
    const password = 'test-password-123';
    const backupData = await createBackup(page, password);
    
    expect(backupData).toBeTruthy();
    expect(backupData.length).toBeGreaterThan(0);
    expect(backupData).toMatch(/^[A-Za-z0-9+/=]+$/); // Base64 format
  });

  test('should restore from backup', async ({ page }) => {
    const password = 'test-password-123';
    const backupData = await createBackup(page, password);
    
    // Create new identity to overwrite current
    await page.goto('/');
    await page.fill('[data-testid="display-name-input"]', 'Another User');
    await page.click('[data-testid="create-identity-btn"]');
    
    // Restore backup
    await restoreBackup(page, backupData, password);
    
    // Verify restoration
    await expect(page.locator('[data-testid="user-display-name"]')).toContainText(TEST_USER.displayName);
    
    for (const field of TEST_USER.initialFields) {
      await expect(page.locator('[data-testid="field"]')).toContainText(field.value);
    }
  });

  test('should validate backup password strength', async ({ page }) => {
    await page.click('[data-testid="settings-tab"]');
    await page.click('[data-testid="backup-export-btn"]');
    
    // Test weak password
    await page.fill('[data-testid="backup-password-input"]', '123');
    await expect(page.locator('[data-testid="password-strength"]')).toContainText('Weak');
    await expect(page.locator('[data-testid="create-backup-btn"]')).toBeDisabled();
    
    // Test strong password
    await page.fill('[data-testid="backup-password-input"]', 'Str0ng-P@ssw0rd!');
    await expect(page.locator('[data-testid="password-strength"]')).toContainText('Strong');
    await expect(page.locator('[data-testid="create-backup-btn"]')).toBeEnabled();
  });
});

test.describe('Settings and Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await setupTestUser(page);
  });

  test('should update relay server settings', async ({ page }) => {
    await page.click('[data-testid="settings-tab"]');
    await page.click('[data-testid="relay-settings-btn"]');
    
    const customRelay = 'wss://custom.relay.example.com';
    await page.fill('[data-testid="relay-url-input"]', customRelay);
    await page.click('[data-testid="save-relay-btn"]');
    
    // Should show success message
    await expect(page.locator('[data-testid="settings-success"]')).toBeVisible();
    
    // Verify setting is saved
    await expect(page.locator('[data-testid="relay-url-input"]')).toHaveValue(customRelay);
  });

  test('should display application info', async ({ page }) => {
    await page.click('[data-testid="settings-tab"]');
    await page.click('[data-testid="about-btn"]');
    
    await expect(page.locator('[data-testid="app-version"]')).toBeVisible();
    await expect(page.locator('[data-testid="build-info"]')).toBeVisible();
  });
});

test.describe('Accessibility', () => {
  test('should meet basic accessibility standards', async ({ page }) => {
    await setupTestUser(page);
    
    // Check for proper heading structure
    const headings = page.locator('h1, h2, h3');
    await expect(headings).toHaveCount({ min: 1 });
    
    // Check for ARIA labels on interactive elements
    const buttons = page.locator('button');
    const buttonCount = await buttons.count();
    expect(buttonCount).toBeGreaterThan(0);
    
    // Check for keyboard navigation
    await page.keyboard.press('Tab');
    const focusedElement = page.locator(':focus');
    await expect(focusedElement).toBeVisible();
  });

  test('should support screen readers', async ({ page }) => {
    await setupTestUser(page);
    
    // Check for proper ARIA roles
    await expect(page.locator('[role="main"]')).toBeVisible();
    await expect(page.locator('[role="navigation"]')).toBeVisible();
    
    // Check for alt text on images
    const images = page.locator('img');
    const imageCount = await images.count();
    if (imageCount > 0) {
      await expect(images.first()).toHaveAttribute('alt');
    }
  });
});

test.describe('Responsive Design', () => {
  ['Mobile Chrome', 'Desktop Chrome'].forEach(deviceName => {
    test(`should work correctly on ${deviceName}`, async ({ page }, testInfo) => {
      testInfo.snapshotSuffix = deviceName;
      
      await setupTestUser(page);
      
      // Check core functionality is accessible
      await expect(page.locator('[data-testid="main-app"]')).toBeVisible();
      await expect(page.locator('[data-testid="user-display-name"]')).toBeVisible();
      
      // Mobile-specific checks
      if (deviceName.includes('Mobile')) {
        await expect(page.locator('[data-testid="mobile-menu"]')).toBeVisible();
      }
    });
  });
});

test.describe('Error Handling', () => {
  test('should handle network errors gracefully', async ({ page }) => {
    await setupTestUser(page);
    
    // Simulate network failure
    await page.context().setOffline(true);
    
    // Try to sync
    await page.click('[data-testid="sync-btn"]');
    
    // Should show error message
    await expect(page.locator('[data-testid="error-toast"]')).toBeVisible();
    await expect(page.locator('[data-testid="error-toast"]')).toContainText('connection');
  });

  test('should handle invalid QR codes', async ({ page }) => {
    await setupTestUser(page);
    
    await page.click('[data-testid="exchange-tab"]');
    await page.click('[data-testid="scan-qr-btn"]');
    await page.fill('[data-testid="qr-input"]', 'invalid-qr-data');
    await page.click('[data-testid="complete-exchange-btn"]');
    
    // Should show error
    await expect(page.locator('[data-testid="qr-error"]')).toBeVisible();
    await expect(page.locator('[data-testid="qr-error"]')).toContainText('Invalid QR code');
  });
});