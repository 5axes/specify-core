//! What `write --accept-defaults` stores, found without specify-core's own planning of which
//! answers write PMI (`api::stored_with_defaults`): the part is written with every default
//! taken and only the given answers stored, then reopened; the defaults the file states come
//! back as `resume.present`. The answers stored are the given ones and every other default
//! taken, but the process (a suggestion even when taken).

use std::path::Path;

use serde_json::{Map, Value};
use specify_core::{api, part, rules};

/// `given` and the defaults taken for part `k` of `input` (analysed as `analysis`) that the
/// file written with them all does not state.
pub fn stored(
    input: &Path,
    k: usize,
    analysis: &Value,
    given: &Map<String, Value>,
) -> Map<String, Value> {
    let all = rules::with_all_defaults(analysis, given).unwrap();
    let intent = api::apply(analysis, given, true).unwrap();
    let file = part::File::read(input).unwrap();
    let bytes = api::write_bytes(
        file.bytes().as_ref().clone(),
        &[(intent, Some(given.clone()))],
    )
    .unwrap()
    .0;
    let written = part::File::of_bytes(input, bytes).unwrap();
    let reopened = api::analyse_reporting_in(&written, &written.load(Some(k)).unwrap())
        .unwrap()
        .0;
    let present = reopened["resume"]["present"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut out = given.clone();
    for (id, value) in all {
        if !given.contains_key(&id)
            && !present.contains_key(&id)
            && !id.starts_with("bom.")
            && id != "part.process"
        {
            out.insert(id, value);
        }
    }
    out
}
