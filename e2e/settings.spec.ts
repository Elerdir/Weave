import { test, expect } from "@playwright/test";

// Nastavení se v aplikaci otevírá v samostatném Tauri okně
// (invoke("open_settings_window") → okno s ?view=settings). V Playwright
// (čistý prohlížeč bez Tauri backendu) se proto testuje přímo pohled
// ?view=settings — tentýž obsah, jaký renderuje samostatné okno.
test.describe("Settings", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?view=settings");
  });

  test("zobrazí nastavení", async ({ page }) => {
    await expect(page.getByRole("heading", { name: /Nastavení|Settings/ })).toBeVisible();
  });

  test("přepne motiv na světlý", async ({ page }) => {
    await page.getByRole("button", { name: /^Světlé$|^Light$/ }).click();
    await expect(page.locator("html")).toHaveClass(/light/);
  });

  test("přepne sekci na API klíče", async ({ page }) => {
    await page.locator(".settings-nav").getByRole("button", { name: /API klíče|API Keys/ }).click();
    await expect(page.getByText("Mistral")).toBeVisible();
  });

  test("sekce Modely zobrazí formulář pro stažení", async ({ page }) => {
    await page.locator(".settings-nav").getByRole("button", { name: /Modely|Models/ }).click();
    await expect(page.getByPlaceholder(/ID modelu/)).toBeVisible();
  });
});
