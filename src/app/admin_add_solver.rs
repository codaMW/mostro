use crate::app::context::AppContext;
use crate::db::add_new_user;
use crate::util::send_dm;
use mostro_core::prelude::*;
use mostro_core::user::User;
use nostr::nips::nip59::UnwrappedGift;
use nostr_sdk::prelude::*;
use tracing::{error, info};

pub async fn admin_add_solver_action(
    ctx: &AppContext,
    msg: Message,
    event: &UnwrappedGift,
    my_keys: &Keys,
) -> Result<(), MostroError> {
    let pool = ctx.pool();
    // Get request id
    let request_id = msg.get_inner_message_kind().request_id;

    let inner_message = msg.get_inner_message_kind();
    let payload = if let Some(payload) = &inner_message.payload {
        payload
    } else {
        error!("No pubkey found!");
        return Err(MostroInternalErr(ServiceError::InvalidPayload));
    };
    let npubkey = if let Payload::TextMessage(p) = payload {
        p
    } else {
        error!("No pubkey found!");
        return Err(MostroInternalErr(ServiceError::InvalidPayload));
    };

    // Check if the pubkey is Mostro
    if event.sender.to_string() != my_keys.public_key().to_string() {
        // We create a Message
        return Err(MostroInternalErr(ServiceError::InvalidPubkey));
    }
    let trade_index = inner_message.trade_index.unwrap_or(0);
    let public_key = PublicKey::from_bech32(npubkey)
        .map_err(|_| MostroInternalErr(ServiceError::InvalidPubkey))?;
    let user = User::new(public_key.to_string(), 0, 1, 0, 0, trade_index);
    // Use CRUD to create user
    match add_new_user(pool, user).await {
        Ok(r) => info!("Solver added: {:#?}", r),
        Err(ee) => error!("Error creating solver: {:#?}", ee),
    }
    // We create a Message for admin
    let message = Message::new_dispute(None, request_id, None, Action::AdminAddSolver, None);
    let message = message
        .as_json()
        .map_err(|_| MostroInternalErr(ServiceError::MessageSerializationError))?;
    // Send the message
    send_dm(event.rumor.pubkey, my_keys, &message, None)
        .await
        .map_err(|e| MostroInternalErr(ServiceError::NostrError(e.to_string())))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::context::test_utils::{test_settings, TestContextBuilder};
    use mostro_core::prelude::*;
    use nostr_sdk::{Keys, Kind as NostrKind, Timestamp, UnsignedEvent};
    use sqlx::SqlitePool;
    use std::sync::Arc;

    fn make_event(pubkey: PublicKey) -> UnwrappedGift {
        let unsigned_event =
            UnsignedEvent::new(pubkey, Timestamp::now(), NostrKind::GiftWrap, vec![], "");
        UnwrappedGift {
            sender: pubkey,
            rumor: unsigned_event,
        }
    }

    async fn make_ctx() -> AppContext {
        let pool = Arc::new(SqlitePool::connect("sqlite::memory:").await.unwrap());
        sqlx::migrate!("./migrations")
            .run(pool.as_ref())
            .await
            .unwrap();
        TestContextBuilder::new()
            .with_pool(pool)
            .with_settings(test_settings())
            .build()
    }

    // ----------------------------------------------------------------
    // TEST 1: No payload at all
    // CURRENTLY FAILS — function returns Ok(()) and just logs the error
    // EXPECTED     — should return Err(MostroInternalErr(InvalidPayload))
    // ----------------------------------------------------------------
    #[tokio::test]
    async fn admin_add_solver_missing_payload_should_return_err_not_ok() {
        let ctx = make_ctx().await;
        let my_keys = Keys::generate();
        let event = make_event(Keys::generate().public_key());

        // Message with NO payload — simulates malformed admin request
        let msg = Message::new_order(None, Some(1), None, Action::AdminAddSolver, None);

        let result = admin_add_solver_action(&ctx, msg, &event, &my_keys).await;

        // This assertion currently FAILS — result is Ok(())
        // Proves the bug: caller has no way to know the action did nothing
        assert!(
            result.is_err(),
            "BUG REPRODUCED: admin_add_solver_action returned Ok(()) on missing payload \
            instead of Err. The admin gets no feedback and the failure is invisible \
            to the caller."
        );
    }

    // ----------------------------------------------------------------
    // TEST 2: Wrong payload type (not TextMessage)
    // CURRENTLY FAILS — falls into the else branch, returns Ok(())
    // EXPECTED     — should return Err(MostroInternalErr(InvalidPayload))
    // ----------------------------------------------------------------
    #[tokio::test]
    async fn admin_add_solver_wrong_payload_type_should_return_err_not_ok() {
        let ctx = make_ctx().await;
        let my_keys = Keys::generate();
        let event = make_event(Keys::generate().public_key());

        // Send a RatingUser payload where TextMessage is required
        let msg = Message::new_order(
            None,
            Some(1),
            None,
            Action::AdminAddSolver,
            Some(Payload::RatingUser(5)),
        );

        let result = admin_add_solver_action(&ctx, msg, &event, &my_keys).await;

        // This assertion currently FAILS — result is Ok(())
        assert!(
            result.is_err(),
            "BUG REPRODUCED: admin_add_solver_action returned Ok(()) on wrong payload type \
            instead of Err. Sending a non-TextMessage payload silently succeeds."
        );
    }
}
