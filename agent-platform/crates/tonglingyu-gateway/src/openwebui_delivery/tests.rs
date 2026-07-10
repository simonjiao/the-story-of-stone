use serde_json::from_str;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

#[test]
fn completed_delivery_contains_only_summary_and_artifact_references() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    event.event_type = ProductRunEventType::RunCompleted;
    event.payload = json!({
        "summary": "写作任务已完成。",
        "artifacts": [{"id": "article-1", "kind": "article", "title": "晴雯"}],
        "full_text": "不应复制到 Open WebUI 的完整正文"
    });
    let delivery = delivery_for_product_event("run-1", &event);
    assert!(delivery.snapshot.contains("article-1"));
    assert!(!delivery.snapshot.contains("完整正文"));
    assert_eq!(delivery.body["type"], "replace");
}

#[test]
fn requires_action_delivery_keeps_gateway_and_action_ids_for_recovery() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    event.event_type = ProductRunEventType::RunRequiresAction;
    event.payload = json!({"action": {"id": "action-1", "title": "确认任务卡"}});
    let delivery = delivery_for_product_event("run-1", &event);
    assert!(delivery.snapshot.contains("Run ID: run-1"));
    assert!(delivery.snapshot.contains("Action ID: action-1"));
}

#[tokio::test]
async fn retries_retryable_delivery_and_reports_retry_state() {
    let base_url = delivery_server(&[500, 200]).await;
    let client =
        OpenWebuiDeliveryClient::new(&base_url, "service-key", 2, 2, 1).expect("delivery client");
    let retries = Arc::new(Mutex::new(Vec::new()));
    let observed = retries.clone();
    let delivery = OpenWebuiDelivery {
        id: "delivery-1".to_string(),
        body: json!({"type": "replace", "data": {"content": "done"}}),
        snapshot: "done".to_string(),
    };

    let attempts = client
        .deliver("chat-1", "message-1", &delivery, move |attempt, error| {
            observed
                .lock()
                .expect("retry observations")
                .push((attempt, error.code));
            Ok(())
        })
        .await
        .expect("second attempt succeeds");

    assert_eq!(attempts, 2);
    assert_eq!(
        *retries.lock().expect("retry observations"),
        vec![(1, "openwebui_delivery_http_error")]
    );
}

#[tokio::test]
async fn does_not_retry_unauthorized_delivery() {
    let base_url = delivery_server(&[401]).await;
    let client =
        OpenWebuiDeliveryClient::new(&base_url, "service-key", 2, 3, 1).expect("delivery client");
    let delivery = OpenWebuiDelivery {
        id: "delivery-2".to_string(),
        body: json!({"type": "replace"}),
        snapshot: "failed".to_string(),
    };

    let error = client
        .deliver("chat-1", "message-1", &delivery, |_, _| {
            panic!("must not retry")
        })
        .await
        .expect_err("unauthorized delivery fails");

    assert_eq!(error.code, "openwebui_delivery_unauthorized");
    assert!(!error.retryable);
}

async fn delivery_server(statuses: &[u16]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind delivery server");
    let address = listener.local_addr().expect("delivery server address");
    let statuses = statuses.to_vec();
    tokio::spawn(async move {
        for status in statuses {
            let (mut stream, _) = listener.accept().await.expect("delivery request");
            let mut request = vec![0_u8; 4096];
            let _ = stream
                .read(&mut request)
                .await
                .expect("read delivery request");
            let reason = if status == 200 {
                "OK"
            } else if status == 401 {
                "Unauthorized"
            } else {
                "Server Error"
            };
            stream
                .write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
                .await
                .expect("write delivery response");
        }
    });
    format!("http://{address}")
}
