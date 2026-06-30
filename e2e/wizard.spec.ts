import { test, expect } from "@playwright/test";

test.describe("First-run Wizard", () => {
  test.beforeEach(async ({ page }) => {
    // Simuluj první spuštění — smaž setup-complete z localStorage
    await page.goto("/");
    await page.evaluate(() => localStorage.removeItem("weave.setup-complete"));
    await page.reload();
  });

  test("zobrazí wizard při prvním spuštění", async ({ page }) => {
    await expect(page.getByText("Weave")).toBeVisible();
    await expect(page.getByText(/Krok 1 z 4|Step 1 of 4/)).toBeVisible();
  });

  test("lze přejít na další krok", async ({ page }) => {
    const nextBtn = page.getByRole("button", { name: /Další|Next/ });
    await nextBtn.click();
    await expect(page.getByText(/Krok 2 z 4|Step 2 of 4/)).toBeVisible();
  });

  test("lze přepnout jazyk ve welcome stepu", async ({ page }) => {
    const select = page.locator("#locale-select");
    await select.selectOption("en");
    await expect(page.getByRole("button", { name: "Next" })).toBeVisible();
  });
});

test.describe("Chat UI", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.evaluate(() => {
      localStorage.setItem("weave.setup-complete", "1");
    });
    await page.reload();
  });

  test("zobrazí prázdný stav bez aktivní konverzace", async ({ page }) => {
    await expect(page.getByText("Weave")).toBeVisible();
  });
});
