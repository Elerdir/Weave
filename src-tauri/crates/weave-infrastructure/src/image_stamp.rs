//! Označení AI-generovaných obrázků: PNG tEXt metadata (strojově čitelná
//! deklarace vč. IPTC digital source type) + neviditelný vodoznak v LSB
//! bitech pixelů. Vodoznak přežije bezeztrátové úpravy/kopírování PNG;
//! ztrátová konverze (JPEG) ho smaže, metadata zpravidla také — jde o
//! poctivé označení původu, ne o DRM.

use std::io::{BufReader, BufWriter};
use std::path::Path;

use anyhow::Context;

/// Značka vkládaná do LSB bitů prvních pixelů obrázku.
pub const AI_MARKER: &[u8] = b"WEAVE-AI";

/// IPTC Digital Source Type pro plně AI-generovaná média.
const DIGITAL_SOURCE_TYPE: &str =
    "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia";

/// tEXt klíč s pozitivním promptem generování.
const PROMPT_KEY: &str = "prompt";
/// tEXt klíč s negativním promptem generování.
const NEGATIVE_PROMPT_KEY: &str = "negative_prompt";

/// Přepíše PNG na místě: doplní metadata o AI původu (vč. promptu, pokud je
/// zadán) a vloží neviditelný vodoznak. Pixely se změní nejvýš o 1 v LSB —
/// okem nerozlišitelné.
pub fn stamp_ai_image(
    path: &Path,
    prompt: Option<&str>,
    negative_prompt: Option<&str>,
) -> anyhow::Result<()> {
    let file = std::fs::File::open(path).context("otevření PNG")?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().context("čtení PNG hlavičky")?;
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("velikost PNG bufferu přetekla")?
    ];
    let frame = reader.next_frame(&mut buf).context("dekódování PNG")?;
    let data = &mut buf[..frame.buffer_size()];

    // Neviditelný vodoznak — jen u 8bit kanálů (ComfyUI generuje 8bit RGB/A)
    if frame.bit_depth == png::BitDepth::Eight {
        embed_lsb_marker(data, AI_MARKER);
    }

    let out = std::fs::File::create(path).context("zápis PNG")?;
    let mut encoder = png::Encoder::new(BufWriter::new(out), frame.width, frame.height);
    encoder.set_color(frame.color_type);
    encoder.set_depth(frame.bit_depth);
    encoder
        .add_text_chunk("AI-Generated".into(), "true".into())
        .context("tEXt AI-Generated")?;
    encoder
        .add_text_chunk("Software".into(), "Weave (AI generated image)".into())
        .context("tEXt Software")?;
    encoder
        .add_text_chunk("digitalsourcetype".into(), DIGITAL_SOURCE_TYPE.into())
        .context("tEXt digitalsourcetype")?;

    // Prompt(y) — jen když jsou. tEXt je latin1; ne-latin1 znaky (diakritika
    // z LoRA trigger words) by zápis shodily, proto best-effort: neúspěch
    // jen zalogujeme, označení AI původu tím netrpí.
    for (key, value) in [(PROMPT_KEY, prompt), (NEGATIVE_PROMPT_KEY, negative_prompt)] {
        if let Some(text) = value.filter(|t| !t.is_empty()) {
            if let Err(e) = encoder.add_text_chunk(key.into(), text.into()) {
                tracing::warn!("tEXt {key} se nepodařilo zapsat: {e}");
            }
        }
    }

    let mut writer = encoder.write_header().context("PNG hlavička")?;
    writer.write_image_data(data).context("PNG data")?;
    writer.finish().context("dokončení PNG")?;
    Ok(())
}

/// Ověří AI označení: LSB vodoznak v pixelech NEBO tEXt metadata.
pub fn is_ai_stamped(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let decoder = png::Decoder::new(BufReader::new(file));
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };

    let has_text_stamp = reader
        .info()
        .uncompressed_latin1_text
        .iter()
        .any(|chunk| chunk.keyword == "AI-Generated" && chunk.text == "true");

    let Some(size) = reader.output_buffer_size() else {
        return has_text_stamp;
    };
    let mut buf = vec![0u8; size];
    let Ok(frame) = reader.next_frame(&mut buf) else {
        return has_text_stamp;
    };
    has_text_stamp || extract_lsb_marker(&buf[..frame.buffer_size()], AI_MARKER.len()) == AI_MARKER
}

/// Přečte prompt a negativní prompt z PNG tEXt metadat (zapsaných při
/// [`stamp_ai_image`]). Chybějící chunk = `None`.
pub fn read_prompt_metadata(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(file) = std::fs::File::open(path) else {
        return (None, None);
    };
    let decoder = png::Decoder::new(BufReader::new(file));
    let Ok(reader) = decoder.read_info() else {
        return (None, None);
    };
    let find = |key: &str| {
        reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .find(|c| c.keyword == key)
            .map(|c| c.text.clone())
    };
    (find(PROMPT_KEY), find(NEGATIVE_PROMPT_KEY))
}

/// Vloží bity značky do LSB po sobě jdoucích bajtů obrazových dat.
fn embed_lsb_marker(data: &mut [u8], marker: &[u8]) {
    let bits_needed = marker.len() * 8;
    if data.len() < bits_needed {
        return; // miniaturní obrázek — vodoznak vynecháme, metadata zůstávají
    }
    for (i, byte) in data.iter_mut().take(bits_needed).enumerate() {
        let bit = (marker[i / 8] >> (7 - (i % 8))) & 1;
        *byte = (*byte & !1) | bit;
    }
}

/// Přečte značku z LSB bitů (inverz k [`embed_lsb_marker`]).
fn extract_lsb_marker(data: &[u8], marker_len: usize) -> Vec<u8> {
    let bits_needed = marker_len * 8;
    if data.len() < bits_needed {
        return Vec::new();
    }
    let mut out = vec![0u8; marker_len];
    for (i, byte) in data.iter().take(bits_needed).enumerate() {
        out[i / 8] |= (byte & 1) << (7 - (i % 8));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vytvoří testovací RGB PNG s gradientem.
    fn write_test_png(path: &Path, width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                data.push((x * 7 % 256) as u8);
                data.push((y * 13 % 256) as u8);
                data.push(((x + y) * 3 % 256) as u8);
            }
        }
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        data
    }

    fn temp_png(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("weave_stamp_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn stamp_adds_metadata_and_invisible_watermark() {
        let path = temp_png("obrazek.png");
        let original = write_test_png(&path, 64, 64);

        assert!(!is_ai_stamped(&path), "čerstvý PNG nesmí být označený");
        stamp_ai_image(&path, None, None).unwrap();
        assert!(is_ai_stamped(&path), "po označení musí být detekovatelný");

        // Pixely se smí lišit nejvýš o 1 (LSB) — okem neviditelné
        let file = std::fs::File::open(&path).unwrap();
        let mut reader = png::Decoder::new(BufReader::new(file)).read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buf).unwrap();
        let stamped = &buf[..frame.buffer_size()];
        assert_eq!(stamped.len(), original.len());
        let max_diff = original
            .iter()
            .zip(stamped)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max_diff <= 1, "vodoznak změnil pixel o víc než LSB");
    }

    #[test]
    fn stamp_writes_text_chunks() {
        let path = temp_png("meta.png");
        write_test_png(&path, 16, 16);
        stamp_ai_image(&path, None, None).unwrap();

        let file = std::fs::File::open(&path).unwrap();
        let reader = png::Decoder::new(BufReader::new(file)).read_info().unwrap();
        let texts = &reader.info().uncompressed_latin1_text;
        let get = |k: &str| {
            texts
                .iter()
                .find(|c| c.keyword == k)
                .map(|c| c.text.clone())
        };
        assert_eq!(get("AI-Generated").as_deref(), Some("true"));
        assert!(get("Software").unwrap().contains("Weave"));
        assert!(get("digitalsourcetype")
            .unwrap()
            .contains("trainedAlgorithmicMedia"));
    }

    #[test]
    fn tiny_image_still_gets_metadata() {
        let path = temp_png("mini.png");
        write_test_png(&path, 2, 2); // 12 bajtů dat < 64 bitů značky
        stamp_ai_image(&path, None, None).unwrap();
        assert!(is_ai_stamped(&path), "metadata musí stačit i bez vodoznaku");
    }

    #[test]
    fn stamp_roundtrips_prompt_metadata() {
        let path = temp_png("prompt.png");
        write_test_png(&path, 16, 16);
        stamp_ai_image(
            &path,
            Some("a knight in shining armor, detailed"),
            Some("blurry, bad eyes"),
        )
        .unwrap();

        let (prompt, negative) = read_prompt_metadata(&path);
        assert_eq!(
            prompt.as_deref(),
            Some("a knight in shining armor, detailed")
        );
        assert_eq!(negative.as_deref(), Some("blurry, bad eyes"));
    }

    #[test]
    fn read_prompt_metadata_none_when_absent() {
        let path = temp_png("noprompt.png");
        write_test_png(&path, 16, 16);
        stamp_ai_image(&path, None, None).unwrap();

        let (prompt, negative) = read_prompt_metadata(&path);
        assert!(prompt.is_none() && negative.is_none());
    }
}
