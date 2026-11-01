//! Exercise real agent turns and persistence with a deterministic local provider.
//! This fixture never opens a network connection or reads provider credentials.
use anyhow::{Result, ensure};
use futures::StreamExt;
use jcode::agent::Agent;
use jcode::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use jcode::provider::{EventStream, Provider};
use jcode::session::Session;
use jcode::tool::Registry;
use std::{sync::Arc, time::Duration};

struct FixtureProvider;

#[async_trait::async_trait]
impl Provider for FixtureProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let pending = messages.iter().rev().find(|message| message.role == Role::User)
            .is_some_and(|message| message.content.iter().any(|block| matches!(block, ContentBlock::Text { text, .. } if text.contains("fixture-cancel"))));
        if pending {
            return Ok(Box::pin(
                futures::stream::iter([Ok(StreamEvent::TextDelta("pending fixture".into()))])
                    .chain(futures::stream::pending::<Result<StreamEvent>>()),
            ));
        }
        Ok(Box::pin(futures::stream::iter([
            Ok(StreamEvent::TextDelta("fixture response".into())),
            Ok(StreamEvent::MessageEnd {
                stop_reason: Some("end_turn".into()),
            }),
        ])))
    }

    fn name(&self) -> &str {
        "fusion-fixture"
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

pub async fn verify() -> Result<()> {
    ensure!(
        std::env::var_os("JCODE_HOME").is_some(),
        "The fusion harness requires an isolated JCODE_HOME"
    );
    let provider: Arc<dyn Provider> = Arc::new(FixtureProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider.clone(), registry);
    let response = agent.run_once_capture("fixture-message").await?;
    ensure!(
        response.contains("fixture response"),
        "Agent did not consume the provider stream"
    );
    println!("{}", serde_json::json!({"stage":"agent_message_passed"}));
    let id = agent.session_id().to_string();
    let shutdown = agent.graceful_shutdown_signal();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        shutdown.fire();
    });
    // Either partial text or a cancellation error is acceptable; timing out is not.
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let _cancelled = tokio::time::timeout(
        Duration::from_secs(10),
        agent.run_once_streaming_mpsc("fixture-cancel", Vec::new(), None, event_tx),
    )
    .await?;
    agent.mark_closed();
    drop(agent);
    let saved = Session::load(&id)?;
    ensure!(saved.messages.iter().any(|message| message.content.iter().any(|block| matches!(block, ContentBlock::Text { text, .. } if text.contains("fixture-message")))), "Session transcript was not persisted");
    let registry = Registry::new(provider.clone()).await;
    let mut restored = Agent::new_with_session(provider, registry, saved, None);
    let response = restored.run_once_capture("fixture-resume").await?;
    ensure!(
        response.contains("fixture response"),
        "Restored agent could not continue"
    );
    ensure!(
        restored.session_id() == id,
        "Resume changed the session identity"
    );
    restored.mark_closed();
    println!(
        "{}",
        serde_json::json!({"stage":"agent_send_cancel_resume_passed","session_id":id})
    );
    Ok(())
}
