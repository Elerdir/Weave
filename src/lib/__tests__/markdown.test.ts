import { describe, it, expect } from "vitest";
import { renderMarkdown } from "$lib/markdown";

describe("renderMarkdown", () => {
  it("renderuje nadpisy, seznamy a tučné písmo", () => {
    const html = renderMarkdown("# Nadpis\n\n- první\n- druhá\n\n**tučně**");
    expect(html).toContain("<h1>Nadpis</h1>");
    expect(html).toContain("<li>první</li>");
    expect(html).toContain("<strong>tučně</strong>");
  });

  it("renderuje GFM tabulku", () => {
    const html = renderMarkdown("| A | B |\n|---|---|\n| 1 | 2 |");
    expect(html).toContain("<table>");
    expect(html).toContain("<th>A</th>");
    expect(html).toContain("<td>2</td>");
  });

  it("renderuje code blok s jazykem beze změny obsahu", () => {
    const html = renderMarkdown('```rust\nfn main() { println!("<ahoj>"); }\n```');
    expect(html).toContain("<pre>");
    expect(html).toContain("language-rust");
    // Obsah kódu musí zůstat escapovaný, ne interpretovaný jako HTML
    expect(html).toContain("&lt;ahoj&gt;");
    expect(html).not.toContain("<ahoj>");
  });

  it("jeden \\n uvnitř odstavce dělá <br> (chování jako ChatGPT)", () => {
    const html = renderMarkdown("první řádek\ndruhý řádek");
    expect(html).toContain("<br>");
  });

  it("lokální cesta obrázku jde přes convertFileSrc, vzdálená URL zůstává", () => {
    const local = renderMarkdown("![obrázek](C:/data/weave/gallery/img.png)");
    expect(local).toContain('src="asset://localhost/C:/data/weave/gallery/img.png"');

    const remote = renderMarkdown("![foto](https://example.com/foto.png)");
    expect(remote).toContain('src="https://example.com/foto.png"');
  });

  it("odkaz dostane třídu md-link a href zůstane", () => {
    const html = renderMarkdown("[web](https://example.com)");
    expect(html).toContain('href="https://example.com"');
    expect(html).toContain('class="md-link"');
  });

  it("sanitizace: <script> a event handlery se zahodí", () => {
    const html = renderMarkdown('ahoj <script>alert(1)</script> <img src="x" onerror="alert(1)">');
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("onerror");
  });

  it("sanitizace: javascript: odkaz se zahodí", () => {
    const html = renderMarkdown("[klikni](javascript:alert(1))");
    expect(html).not.toContain("javascript:");
  });
});
