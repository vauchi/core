import { test, expect, Page } from '@playwright/test';

// Test data
export const TEST_USER = {
  displayName: 'Test User',
  initialFields: [
    { type: 'email', label: 'Email', value: 'test@example.com' },
    { type: 'phone', label: 'Phone', value: '+1234567890' },
  ],
};

export const TEST_CONTACT = {
  displayName: 'Contact Test',
  fields: [
    { type: 'email', label: 'Email', value: 'contact@example.com' },
  ],
};

// Helper functions
export async function setupTestUser(page: Page): Promise<void> {
  // Navigate to setup page if no identity exists
  await page.goto('/');
  
  // Check if we're on setup page
  if (await page.locator('[data-testid="setup-page"]').isVisible()) {
    await page.fill('[data-testid="display-name-input"]', TEST_USER.displayName);
    await page.click('[data-testid="create-identity-btn"]');
    
    // Wait for main app to load
    await page.waitForSelector('[data-testid="main-app"]');
  }
}

export async function addTestFields(page: Page): Promise<void> {
  for (const field of TEST_USER.initialFields) {
    await page.click('[data-testid="add-field-btn"]');
    await page.selectOption('[data-testid="field-type-select"]', field.type);
    await page.fill('[data-testid="field-label-input"]', field.label);
    await page.fill('[data-testid="field-value-input"]', field.value);
    await page.click('[data-testid="save-field-btn"]');
    await page.waitForTimeout(500); // Brief wait for field to be added
  }
}

export async function generateQRCode(page: Page): Promise<string> {
  await page.click('[data-testid="exchange-tab"]');
  await page.click('[data-testid="generate-qr-btn"]');
  
  // Wait for QR code to appear
  await page.waitForSelector('[data-testid="qr-code"]');
  
  // Get QR code data
  const qrData = await page.locator('[data-testid="qr-data"]').textContent();
  expect(qrData).toBeTruthy();
  
  return qrData || '';
}

export async function completeExchange(page: Page, qrData: string): Promise<void> {
  await page.click('[data-testid="exchange-tab"]');
  await page.click('[data-testid="scan-qr-btn"]');
  
  // Mock QR scan by directly entering data
  await page.fill('[data-testid="qr-input"]', qrData);
  await page.click('[data-testid="complete-exchange-btn"]');
  
  // Wait for success message
  await page.waitForSelector('[data-testid="exchange-success"]');
}

export async function verifyContactExists(page: Page, displayName: string): Promise<void> {
  await page.click('[data-testid="contacts-tab"]');
  
  const contactCard = page.locator(`[data-testid="contact-card"]`).filter({ hasText: displayName });
  await expect(contactCard).toBeVisible({ timeout: 10000 });
}

export async function checkAppState(page: Page): Promise<{
  hasIdentity: boolean;
  contactCount: number;
  isOnline: boolean;
}> {
  const contactCards = page.locator('[data-testid="contact-card"]');
  const contactCount = await contactCards.count();
  
  const hasIdentity = await page.locator('[data-testid="main-app"]').isVisible();
  const isOnline = await page.locator('[data-testid="connection-status"]').textContent();
  
  return {
    hasIdentity,
    contactCount,
    isOnline: isOnline?.includes('Online') || false,
  };
}

export async function waitForSync(page: Page): Promise<void> {
  await page.click('[data-testid="sync-btn"]');
  await page.waitForSelector('[data-testid="sync-complete"]', { timeout: 30000 });
}

export async function checkFieldVisibility(
  page: Page,
  contactName: string,
  fieldLabel: string,
  shouldBeVisible: boolean
): Promise<void> {
  await page.click('[data-testid="contacts-tab"]');
  await page.locator(`[data-testid="contact-card"]`).filter({ hasText: contactName }).click();
  
  await page.waitForSelector('[data-testid="contact-detail"]');
  
  const field = page.locator(`[data-testid="field"]`).filter({ hasText: fieldLabel });
  
  if (shouldBeVisible) {
    await expect(field).toBeVisible();
  } else {
    await expect(field).not.toBeVisible();
  }
}

export async function createBackup(page: Page, password: string): Promise<string> {
  await page.click('[data-testid="settings-tab"]');
  await page.click('[data-testid="backup-export-btn"]');
  await page.fill('[data-testid="backup-password-input"]', password);
  await page.click('[data-testid="create-backup-btn"]');
  
  // Wait for backup data
  await page.waitForSelector('[data-testid="backup-data"]');
  const backupData = await page.locator('[data-testid="backup-data"]').textContent();
  
  expect(backupData).toBeTruthy();
  return backupData || '';
}

export async function restoreBackup(page: Page, backupData: string, password: string): Promise<void> {
  await page.click('[data-testid="settings-tab"]');
  await page.click('[data-testid="backup-import-btn"]');
  await page.fill('[data-testid="backup-data-input"]', backupData);
  await page.fill('[data-testid="backup-password-input"]', password);
  await page.click('[data-testid="restore-backup-btn"]');
  
  // Wait for restore completion
  await page.waitForSelector('[data-testid="restore-success"]');
}

// Error handling helpers
export async function handleErrorToast(page: Page): Promise<string | null> {
  const errorToast = page.locator('[data-testid="error-toast"]');
  if (await errorToast.isVisible({ timeout: 2000 })) {
    return await errorToast.textContent();
  }
  return null;
}

export async function checkAccessibility(page: Page): Promise<void> {
  // Basic accessibility checks
  await expect(page.locator('h1, h2, h3')).toHaveCount({ min: 1 });
  await expect(page.locator('button')).toHaveCount({ min: 1 });
  await expect(page.locator('[aria-label]')).toHaveCount({ min: 1 });
}

// Network simulation
export async function simulateOffline(page: Page): Promise<void> {
  await page.context().setOffline(true);
}

export async function simulateOnline(page: Page): Promise<void> {
  await page.context().setOffline(false);
}