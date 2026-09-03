//! The code catalogue shipped to SDK and documentation readers is the one the crate
//! declares. A drift here means a producer and a consumer disagree on a string.

#[test]
fn the_published_catalogue_matches_the_crate() {
    let published: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/session/v1/codes.json"))
            .expect("codes.json parses");
    assert_eq!(
        published,
        brain_protocol::codes::catalogue(),
        "contracts/session/v1/codes.json is out of step with brain_protocol::codes"
    );
}
