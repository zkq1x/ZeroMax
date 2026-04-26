use zeromax_core::{ClientConfig, MaxClient};

#[tokio::main]
async fn main() {
    // ── Test 1: WEB handshake + QR ───────────────────────────
    println!("\n=== TEST 1: WEB + QR ===");

    let config = ClientConfig::new("+70000000000").work_dir("/tmp/zeromax-test");
    let mut client = MaxClient::new(config).await.expect("Failed to create client");

    client
        .transport
        .connect(&client.user_agent.header_user_agent)
        .await
        .expect("WS connect failed");

    println!("[OK] WebSocket connected");

    // Handshake
    let ua = client.user_agent_payload();
    let handshake_payload = serde_json::json!({
        "deviceId": client.device_id.to_string(),
        "userAgent": ua,
    });

    let resp = client
        .transport
        .request(zeromax_core::Opcode::SessionInit, handshake_payload)
        .await
        .expect("Handshake failed");

    println!(
        "[OK] Handshake response: {}",
        serde_json::to_string_pretty(&resp.payload).unwrap()
    );

    // Try QR
    println!("\n--- Requesting QR ---");
    let qr_resp = client
        .transport
        .request(zeromax_core::Opcode::GetQr, serde_json::json!({}))
        .await;

    match qr_resp {
        Ok(frame) => println!(
            "[OK] QR response: {}",
            serde_json::to_string_pretty(&frame.payload).unwrap()
        ),
        Err(e) => println!("[FAIL] QR request failed: {e}"),
    }

    client.transport.close().await.ok();

    // ── Test 2: DESKTOP handshake + phone auth ──────────────
    println!("\n=== TEST 2: DESKTOP + Phone Auth ===");

    let config2 = ClientConfig::new("+70000000000")
        .work_dir("/tmp/zeromax-test2")
        .device_type("DESKTOP");
    let mut client2 = MaxClient::new(config2).await.expect("Failed to create client2");

    client2
        .transport
        .connect(&client2.user_agent.header_user_agent)
        .await
        .expect("WS connect failed");

    println!("[OK] WebSocket connected (DESKTOP)");

    let ua2 = client2.user_agent_payload();
    let hs2 = serde_json::json!({
        "deviceId": client2.device_id.to_string(),
        "userAgent": ua2,
    });

    let resp2 = client2
        .transport
        .request(zeromax_core::Opcode::SessionInit, hs2)
        .await
        .expect("Handshake 2 failed");

    println!(
        "[OK] Handshake response: {}",
        serde_json::to_string_pretty(&resp2.payload).unwrap()
    );

    // Try auth request
    println!("\n--- Requesting auth code (DESKTOP over WS) ---");
    let auth_resp = client2
        .transport
        .request(
            zeromax_core::Opcode::AuthRequest,
            serde_json::json!({
                "phone": "+70000000000",
                "type": "START_AUTH",
                "language": "ru"
            }),
        )
        .await;

    match auth_resp {
        Ok(frame) => println!(
            "[OK] Auth response: {}",
            serde_json::to_string_pretty(&frame.payload).unwrap()
        ),
        Err(e) => println!("[FAIL] Auth request failed: {e}"),
    }

    client2.transport.close().await.ok();
    println!("\n=== DONE ===");
}
