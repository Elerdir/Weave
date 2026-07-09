import { test, expect } from "@playwright/test";

test.describe("First-run Wizard", () => {
  test.beforeEach(async ({ page }) => {
    // Simuluj první spuštění — smaž setup-complete z localStorage
    await page.goto("/");
    await page.evaluate(() => localStorage.removeItem("weave.setup-complete"));
    await page.reload();
  });

  // Počet kroků se s vývojem mění (naposledy přibyl OpenVINO krok) —
  // testy kotví jen na číslo aktuálního kroku, ne na celkový počet.
  test("zobrazí wizard při prvním spuštění", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Weave" })).toBeVisible();
    await expect(page.getByText(/Krok 1 z \d+|Step 1 of \d+/)).toBeVisible();
  });

  test("lze přejít na další krok", async ({ page }) => {
    const nextBtn = page.getByRole("button", { name: /Další|Next/ });
    await nextBtn.click();
    await expect(page.getByText(/Krok 2 z \d+|Step 2 of \d+/)).toBeVisible();
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
    // Logo Weave je v sidebaru i v prázdném stavu — stačí že je aspoň jedno vidět
    await expect(page.getByText("Weave").first()).toBeVisible();
  });
});
