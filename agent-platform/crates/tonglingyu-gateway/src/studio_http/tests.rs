use super::*;

#[test]
fn rejects_incomplete_studio_configuration_without_network_access() {
    assert!(StudioHttpClient::new("", "key", 10).is_err());
    assert!(StudioHttpClient::new("http://studio", "", 10).is_err());
}

#[test]
fn parses_only_sse_data_fields() {
    assert_eq!(
        sse_data("id: 2\nevent: run.status\ndata: {\"sequence\":2}"),
        Some("{\"sequence\":2}".to_string())
    );
    assert_eq!(sse_data("event: ping"), None);
}

#[test]
fn identifies_studio_control_frames_separately_from_product_events() {
    assert_eq!(
        sse_event_name("event: connected\ndata: {\"ok\":true}"),
        Some("connected")
    );
    assert_eq!(sse_event_name("event: ping\ndata: {\"t\":1}"), Some("ping"));
    assert_eq!(
        sse_event_name("id: 12\nevent: artifact.updated\ndata: {}"),
        Some("artifact.updated")
    );
}
