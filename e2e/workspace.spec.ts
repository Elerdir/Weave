import { test, expect } from "@playwright/test";

test.describe("Workspace panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.evaluate(() => localStorage.setItem("weave.setup-complete", "1"));
    await page.reload();
  });

  test("tlačítko workspace toggleuje panel", async ({ page }) => {
    const wsBtn = page.locator("button[title='Workspace']");
    await wsBtn.click();
    await expect(page.getByText("Workspace", { exact: true })).toBeVisible();
    await wsBtn.click();
    await expect(page.getByText("Otevřít složku")).not.toBeVisible();
  });

  test("prázdný workspace zobrazí výzvu k otevření složky", async ({ page }) => {
    const wsBtn = page.locator("button[title='Workspace']");
    await wsBtn.click();
    await expect(page.getByText("Otevřít složku")).toBeVisible();
  });
});
