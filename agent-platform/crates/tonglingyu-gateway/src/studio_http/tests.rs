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
