use genpdf::{
    elements::{Break, Paragraph},
    fonts::{FontData, FontFamily},
    style::Style,
    Document, Element, SimplePageDecorator,
};
use weave_application::{
    error::{AppError, AppResult},
    ports::pdf_exporter::PdfExporter,
};
use weave_domain::{
    conversation::Conversation,
    message::{Message, Role},
};

/// Embedded font s podporou diakritiky (built-in PDF fonty ji neumí).
const DEJAVU_SANS: &[u8] = include_bytes!("../../assets/DejaVuSans.ttf");

pub struct GenpdfExporter;

impl GenpdfExporter {
    fn font_family() -> AppResult<FontFamily<FontData>> {
        let font = FontData::new(DEJAVU_SANS.to_vec(), None)
            .map_err(|e| AppError::Unexpected(anyhow::anyhow!("Font: {e}")))?;
        Ok(FontFamily {
            regular: font.clone(),
            bold: font.clone(),
            italic: font.clone(),
            bold_italic: font,
        })
    }
}

impl PdfExporter for GenpdfExporter {
    fn render(&self, conversation: &Conversation, messages: &[Message]) -> AppResult<Vec<u8>> {
        let mut doc = Document::new(Self::font_family()?);
        doc.set_title(conversation.title.as_str());

        let mut deco = SimplePageDecorator::new();
        deco.set_margins(15);
        doc.set_page_decorator(deco);

        // Nadpis
        doc.push(
            Paragraph::new(conversation.title.as_str())
                .styled(Style::new().bold().with_font_size(20)),
        );
        doc.push(
            Paragraph::new(format!(
                "Exportováno {}",
                conversation.updated_at.format("%d.%m.%Y %H:%M")
            ))
            .styled(Style::new().with_font_size(9)),
        );
        doc.push(Break::new(1.0));

        for msg in messages {
            // System zprávy (persona/kontext) do exportu nepatří
            if msg.role == Role::System {
                continue;
            }
            let label = match msg.role {
                Role::User => "Uživatel",
                Role::Assistant => "Asistent",
                Role::System => "Systém",
            };
            doc.push(Paragraph::new(label).styled(Style::new().bold().with_font_size(12)));

            // Obsah po řádcích (Paragraph sám nezalamuje na \n)
            for line in msg.content.trim().split('\n') {
                doc.push(Paragraph::new(line));
            }
            doc.push(Break::new(0.8));
        }

        let mut buffer = Vec::new();
        doc.render(&mut buffer)
            .map_err(|e| AppError::Unexpected(anyhow::anyhow!("PDF render: {e}")))?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_domain::conversation::{Conversation, ConversationTitle};

    #[test]
    fn renders_valid_pdf_with_diacritics() {
        let conv = Conversation::new(ConversationTitle::new("Příliš žluťoučký kůň").unwrap());
        let id = conv.id.clone();
        let msgs = vec![
            Message::system(id.clone(), "tajný prompt"),
            Message::user(id.clone(), "Ahoj, řekni něco česky.\nDruhý řádek."),
            Message::assistant(id, "Žluťoučký kůň úpěl ďábelské ódy.", None),
        ];

        let pdf = GenpdfExporter.render(&conv, &msgs).unwrap();

        // Validní PDF začíná "%PDF" a končí "%%EOF"
        assert!(pdf.starts_with(b"%PDF"), "chybí PDF hlavička");
        assert!(pdf.len() > 1000, "PDF je podezřele malé: {} B", pdf.len());
    }
}
