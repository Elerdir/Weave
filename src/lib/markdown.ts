import { convertFileSrc } from "@tauri-apps/api/core";
import DOMPurify from "dompurify";
import { Marked } from "marked";

/**
 * Markdown renderer pro bubliny zpráv. Výstup LLM je nedůvěryhodný vstup,
 * takže výsledné HTML vždy prochází DOMPurify sanitizací.
 */

const marked = new Marked({
  gfm: true, // tabulky, přeškrtnutí…
  breaks: true, // jeden \n = <br>, jak to zobrazuje ChatGPT
});

marked.use({
  renderer: {
    // Lokální cesty (vygenerované/referenční obrázky) musí jít přes asset
    // protokol, jinak je webview nezobrazí. Vzdálené URL necháváme být.
    image({ href, text }) {
      const src = /^https?:\/\//.test(href) ? href : convertFileSrc(href);
      return `<img src="${src}" alt="${text}" class="inline-image" />`;
    },
    // Odkazy se otevírají v systémovém prohlížeči (viz delegace kliků
    // v MessageBubble) — target=_blank by navigoval okno appky.
    link({ href, text }) {
      return `<a href="${href}" class="md-link">${text}</a>`;
    },
  },
});

// convertFileSrc vrací na Windows http://asset.localhost/…, na macOS
// asset://localhost/… — druhé jmenované by výchozí sanitizace zahodila.
const ALLOWED_URI = /^(?:https?|mailto|asset):|^http:\/\/asset\.localhost\//i;

export function renderMarkdown(text: string): string {
  const html = marked.parse(text, { async: false });
  return DOMPurify.sanitize(html, {
    ALLOWED_URI_REGEXP: ALLOWED_URI,
    // <a> target/rel nepotřebujeme — kliky odchytává delegace.
    FORBID_ATTR: ["style", "onerror", "onclick"],
  });
}
