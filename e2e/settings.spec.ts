import { test, expect } from "@playwright/test";

test.describe("Settings", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.evaluate(() => localStorage.setItem("weave.setup-complete", "1"));
    await page.reload();
  });

  test("otevře a zavře nastavení", async ({ page }) => {
    await page.getByRole("button", { name: /Nastavení|Settings/ }).click();
    await expect(page.getByRole("heading", { name: /Nastavení|Settings/ })).toBeVisible();
    // Zavřít přes Escape
    await page.keyboard.press("Escape");
    await expect(page.getByRole("heading", { name: /Nastavení|Settings/ })).not.toBeVisible();
  });

  test("přepne motiv na světlý", async ({ page }) => {
    await page.getByRole("button", { name: /Nastavení|Settings/ }).click();
    await page.getByRole("button", { name: /^Světlé$|^Light$/ }).click();
    await expect(page.locator("html")).toHaveClass(/light/);
  });

  test("přepne sekci na API klíče", async ({ page }) => {
    await page.getByRole("button", { name: /Nastavení|Settings/ }).click();
    await page.locator(".settings-nav").getByRole("button", { name: /API klíče|API Keys/ }).click();
    await expect(page.getByText("Mistral")).toBeVisible();
  });

  test("sekce Modely zobrazí formulář pro stažení", async ({ page }) => {
    await page.getByRole("button", { name: /Nastavení|Settings/ }).click();
    await page.locator(".settings-nav").getByRole("button", { name: /Modely|Models/ }).click();
    await expect(page.getByPlaceholder(/ID modelu/)).toBeVisible();
  });
});
