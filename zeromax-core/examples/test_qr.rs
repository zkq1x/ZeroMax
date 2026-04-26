use std::time::Duration;
use zeromax_core::{ClientConfig, MaxClient};

#[tokio::main]
async fn main() {
    println!("=== QR Login Test ===\n");

    let config = ClientConfig::new("+70000000000").work_dir("/tmp/zeromax-qr-test");
    let mut client = MaxClient::new(config).await.expect("Failed to create client");

    // Connect + handshake.
    client
        .transport
        .connect(&client.user_agent.header_user_agent)
        .await
        .expect("WS connect failed");

    let ua = client.user_agent_payload();
    let resp = client
        .transport
        .request(
            zeromax_core::Opcode::SessionInit,
            serde_json::json!({
                "deviceId": client.device_id.to_string(),
                "userAgent": ua,
            }),
        )
        .await
        .expect("Handshake failed");
    println!("[OK] Handshake done\n");

    // Request QR.
    let qr = client.request_qr().await.expect("QR request failed");
    println!("QR Link: {}", qr.qr_link);
    println!("Track ID: {}", qr.track_id);
    println!("Poll interval: {}ms", qr.polling_interval_ms);
    println!("Expires at: {}", qr.expires_at_ms);
    println!("\n>>> Open this link on your phone or scan the QR <<<\n");

    // Poll until scanned.
    let interval = Duration::from_millis(qr.polling_interval_ms.max(2000));
    loop {
        tokio::time::sleep(interval).await;
        print!("Polling... ");

        match client.poll_qr_status(&qr.track_id).await {
            Ok(true) => {
                println!("SCANNED!");
                break;
            }
            Ok(false) => println!("waiting"),
            Err(e) => {
                println!("error: {e}");
                break;
            }
        }
    }

    // Complete login.
    println!("\nCompleting QR login...");
    match client.complete_qr_login(&qr.track_id).await {
        Ok(token) => {
            println!("[OK] Got token: {}...{}", &token[..10.min(token.len())], &token[token.len().saturating_sub(10)..]);

            // Set token and try sync.
            client.set_token(token).await.expect("set_token failed");
            match client.sync().await {
                Ok(_) => {
                    println!("[OK] Sync completed!");
                    if let Some(me) = &client.me {
                        println!("  Me: id={}, phone={}", me.id, me.phone);
                        if let Some(name) = me.names.first() {
                            println!("  Name: {:?}", name.name);
                        }
                    }
                    println!("  Dialogs: {}", client.dialogs.len());
                    println!("  Chats: {}", client.chats.len());
                    println!("  Channels: {}", client.channels.len());
                    println!("  Contacts: {}", client.contacts.len());
                }
                Err(e) => println!("[FAIL] Sync failed: {e}"),
            }
        }
        Err(e) => println!("[FAIL] QR login failed: {e}"),
    }

    client.transport.close().await.ok();
    println!("\n=== DONE ===");
}
