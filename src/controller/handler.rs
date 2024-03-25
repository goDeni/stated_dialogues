use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug_span, instrument, Level};

use crate::controller::DialCtxActions;
use crate::dialogues::MessageId;

use super::{BotAdapter, CtxResult, DialInteraction};

pub type AnyResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub type HandlerResult = AnyResult<()>;

#[instrument(level = Level::DEBUG, skip(context, bot, interaction))]
pub async fn handle_interaction<T: DialCtxActions, B: BotAdapter>(
    user_id: &u64,
    bot: &Arc<B>,
    context: &RwLock<T>,
    interaction: DialInteraction,
) -> HandlerResult {
    let dial_controller = context.write().await.take_controller(user_id);

    let (controller, results) = match dial_controller {
        Some(controller) => controller.handle(interaction),
        None => {
            let (controller, results) = context.read().await.new_controller(*user_id)?;
            controller
                .handle(interaction)
                .map(|(controller, handle_results)| {
                    (
                        controller,
                        results.into_iter().chain(handle_results).collect(),
                    )
                })
        }
    }?;

    let sent_msg_ids = process_ctx_results(*user_id, results, bot).await?;
    if let Some(mut controller) = controller {
        controller.remember_sent_messages(sent_msg_ids);
        context.write().await.put_controller(*user_id, controller);
    } else {
        let (mut controller, results) = context.read().await.new_controller(*user_id)?;
        let sent_msg_ids = process_ctx_results(*user_id, results, bot).await?;
        controller.remember_sent_messages(sent_msg_ids);
        context.write().await.put_controller(*user_id, controller);
    }
    Ok(())
}

#[instrument(level = Level::DEBUG, skip(ctx_results, bot), fields(results_len=ctx_results.len()))]
pub async fn process_ctx_results<B: BotAdapter>(
    user_id: u64,
    ctx_results: Vec<CtxResult>,
    bot: &Arc<B>,
) -> AnyResult<Vec<MessageId>> {
    let mut sent_msg_ids: Vec<MessageId> = vec![];
    for ctx_result in ctx_results {
        match ctx_result {
            CtxResult::Messages(messages) => {
                let msg_span = debug_span!("messages");
                let _enter = msg_span.enter();

                for msg in messages {
                    bot.send_message(user_id, msg)
                        .await
                        .map(|msg_id| sent_msg_ids.push(msg_id))?;
                }
            }
            CtxResult::Buttons(msg, selector) => {
                let msg_span = debug_span!("keyboard");
                let _enter = msg_span.enter();

                bot.send_keyboard(user_id, msg, selector)
                    .await
                    .map(|msg_id| sent_msg_ids.push(msg_id))?;
            }
            CtxResult::RemoveMessages(messages_ids) => {
                let msg_span = debug_span!("removal");
                let _enter = msg_span.enter();

                bot.delete_messages(user_id, messages_ids).await?;
            }
        }
    }
    Ok(sent_msg_ids)
}
