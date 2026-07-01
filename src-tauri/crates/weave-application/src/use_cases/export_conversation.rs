use std::sync::Arc;

use serde::{Deserialize, Serialize};
use weave_domain::{
    conversation::{Conversation, ConversationId},
    message::{Message, Role},
};

use crate::{
    error::{AppError, AppResult},
    ports::conversation_repository::{ConversationRepository, MessageRepository},
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Html,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Html => "html",
        }
    }
}

pub struct ExportConversationUseCase {
    conv_repo: Arc<dyn ConversationRepository>,
    msg_repo: Arc<dyn MessageRepository>,
}

impl ExportConversationUseCase {
    pub fn new(
        conv_repo: Arc<dyn ConversationRepository>,
        msg_repo: Arc<dyn MessageRepository>,
    ) -> Self {
        Self {
            conv_repo,
            msg_repo,
        }
    }

    /// Vyrenderuje konverzaci do zvoleného formátu.
    pub async fn render(
        &self,
        conversation_id: &ConversationId,
        format: ExportFormat,
    ) -> AppResult<String> {
        let conversation = self
            .conv_repo
            .find_by_id(conversation_id)
            .await?
            .ok_or_else(|| AppError::Repository("Konverzace neexistuje".into()))?;
        let messages = self.msg_repo.list_by_conversation(conversation_id).await?;

        Ok(match format {
            ExportFormat::Markdown => render_markdown(&conversation, &messages),
            ExportFormat::Html => render_html(&conversation, &messages),
        })
    }

    /// Navrhne název souboru (bezpečný, s příponou).
    pub async fn suggested_filename(
        &self,
        conversation_id: &ConversationId,
        format: ExportFormat,
    ) -> AppResult<String> {
        let conversation = self
            .conv_repo
            .find_by_id(conversation_id)
            .await?
            .ok_or_else(|| AppError::Repository("Konverzace neexistuje".into()))?;
        Ok(format!(
            "{}.{}",
            sanitize_filename(conversation.title.as_str()),
            format.extension()
        ))
    }
}

fn role_label(role: &Role) -> &'static str {
    match role {
        Role::User => "Uživatel",
        Role::Assistant => "Asistent",
        Role::System => "Systém",
    }
}

fn role_icon(role: &Role) -> &'static str {
    match role {
        Role::User => "👤",
        Role::Assistant => "🤖",
        Role::System => "⚙️",
    }
}

pub(crate) fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "konverzace".to_string()
    } else {
        trimmed.replace(' ', "_")
    }
}

pub(crate) fn render_markdown(conversation: &Conversation, messages: &[Message]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", conversation.title.as_str()));
    out.push_str(&format!(
        "_Exportováno {}_\n\n",
        conversation.updated_at.format("%d.%m.%Y %H:%M")
    ));

    for msg in messages {
        // System zprávy (persona/kontext) do exportu nepatří
        if msg.role == Role::System {
            continue;
        }
        out.push_str(&format!(
            "## {} {}\n\n{}\n\n",
            role_icon(&msg.role),
            role_label(&msg.role),
            msg.content.trim()
        ));
        if let Some(stats) = &msg.stats {
            out.push_str(&format!(
                "> _{:.1} tok/s · {}_\n\n",
                stats.tokens_per_second, stats.model_id
            ));
        }
    }
    out
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn render_html(conversation: &Conversation, messages: &[Message]) -> String {
    let mut body = String::new();
    for msg in messages {
        if msg.role == Role::System {
            continue;
        }
        let role_class = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        };
        body.push_str(&format!(
            r#"<div class="msg {}"><div class="role">{} {}</div><div class="content">{}</div></div>"#,
            role_class,
            role_icon(&msg.role),
            role_label(&msg.role),
            escape_html(msg.content.trim()).replace('\n', "<br>")
        ));
    }

    format!(
        r#"<!doctype html>
<html lang="cs">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  body {{ font-family: -apple-system, "Segoe UI", sans-serif; max-width: 760px; margin: 2rem auto; padding: 0 1rem; color: #1a1a2e; background: #f4f4f8; }}
  h1 {{ color: #5a48e8; }}
  .meta {{ color: #6060a0; font-size: 0.85rem; margin-bottom: 2rem; }}
  .msg {{ border-radius: 12px; padding: 0.75rem 1rem; margin-bottom: 1rem; }}
  .msg.user {{ background: #e8e8ff; }}
  .msg.assistant {{ background: #fff; border: 1px solid #d0d0e0; }}
  .role {{ font-weight: 600; font-size: 0.8rem; margin-bottom: 0.4rem; color: #6060a0; }}
  .content {{ line-height: 1.7; white-space: pre-wrap; }}
</style>
</head>
<body>
<h1>{title}</h1>
<div class="meta">Exportováno {date}</div>
{body}
</body>
</html>"#,
        title = escape_html(conversation.title.as_str()),
        date = conversation.updated_at.format("%d.%m.%Y %H:%M"),
        body = body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_domain::conversation::ConversationTitle;
    use weave_domain::message::{GenerationStats, ModelBackend};

    fn sample() -> (Conversation, Vec<Message>) {
        let conv = Conversation::new(ConversationTitle::new("Moje konverzace").unwrap());
        let id = conv.id.clone();
        let msgs = vec![
            Message::system(id.clone(), "TAJNÝ persona prompt"),
            Message::user(id.clone(), "Ahoj"),
            Message::assistant(
                id.clone(),
                "Zdravím!",
                Some(GenerationStats {
                    tokens_per_second: 42.5,
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    model_id: "mistral-small".into(),
                    backend: ModelBackend::MistralApi,
                }),
            ),
        ];
        (conv, msgs)
    }

    #[test]
    fn markdown_has_title_and_roles_but_not_system() {
        let (c, m) = sample();
        let md = render_markdown(&c, &m);
        assert!(md.contains("# Moje konverzace"));
        assert!(md.contains("Uživatel"));
        assert!(md.contains("Asistent"));
        assert!(md.contains("42.5 tok/s"));
        // System prompt se nesmí objevit
        assert!(!md.contains("TAJNÝ persona prompt"));
    }

    #[test]
    fn html_escapes_and_omits_system() {
        let conv = Conversation::new(ConversationTitle::new("Test <b>").unwrap());
        let id = conv.id.clone();
        let msgs = vec![
            Message::system(id.clone(), "secret"),
            Message::user(id.clone(), "a < b && c > d"),
        ];
        let html = render_html(&conv, &msgs);
        assert!(html.contains("Test &lt;b&gt;"));
        assert!(html.contains("a &lt; b &amp;&amp; c &gt; d"));
        assert!(!html.contains("secret"));
        assert!(html.contains("<!doctype html>"));
    }

    #[test]
    fn sanitize_filename_strips_unsafe_chars() {
        assert_eq!(sanitize_filename("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize_filename("Můj chat 1"), "Můj_chat_1");
        assert_eq!(sanitize_filename("   "), "konverzace");
    }

    #[test]
    fn format_extension() {
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Html.extension(), "html");
    }
}
