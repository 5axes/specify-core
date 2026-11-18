//! Every default Specify proposes says which rule proposed it (#196): the rule registry
//! (`src/rule_sources.rs`) and `docs/rule-sources.md` list the same ids, and every question with
//! a default or basis on the captured analyses carries a source from the registry.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::rule_sources::{self, RULES};
use specify_core::rules::{self, Question};

/// The ids in the first column of the doc's "Rule ids" table.
fn documented_ids() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/rule-sources.md");
    let text = std::fs::read_to_string(path).expect("docs/rule-sources.md");
    let section = text
        .split("\n## Rule ids\n")
        .nth(1)
        .expect("a Rule ids section")
        .split("\n## ")
        .next()
        .unwrap();
    section
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .map(|rest| rest.split('`').next().unwrap().to_string())
        .collect()
}

#[test]
fn every_rule_id_is_documented_and_every_documented_id_is_a_rule() {
    let documented = documented_ids();
    let code: BTreeSet<String> = RULES.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(code.len(), RULES.len(), "rule ids are unique");
    let undocumented: Vec<_> = code.difference(&documented).collect();
    let unknown: Vec<_> = documented.difference(&code).collect();
    assert!(
        undocumented.is_empty() && unknown.is_empty(),
        "in the code, not docs/rule-sources.md: {undocumented:?}; in the doc, not the code: {unknown:?}"
    );
}

/// Each question with a default or a basis has a source the registry holds, which the traced
/// and presented documents carry.
fn check(name: &str, qs: &[Question], seen: &mut BTreeSet<&'static str>) {
    for q in qs {
        let proposes = q.default.is_some() || !q.basis.is_empty();
        if !proposes {
            assert!(
                q.source.is_none(),
                "{name}: {} has a source but no default",
                q.id
            );
            continue;
        }
        let source = q
            .source
            .unwrap_or_else(|| panic!("{name}: {} has a default but no source", q.id));
        assert_eq!(
            rule_sources::get(source.id),
            Some(source),
            "{name}: {} names a rule not in the registry",
            q.id
        );
        assert_eq!(
            q.to_traced_dict()["source"],
            source.to_json(),
            "{name}: {}",
            q.id
        );
        assert_eq!(
            q.to_presented_dict()["source"],
            source.to_json(),
            "{name}: {}",
            q.id
        );
        assert!(
            q.to_dict().get("source").is_none(),
            "Python's document is unchanged"
        );
        seen.insert(source.id);
    }
}

#[test]
fn every_proposed_default_on_the_captured_analyses_has_a_source() {
    let mut seen = BTreeSet::new();
    let mut checked = 0;
    for case in common::cases() {
        let Some(analysis) = case.artifact("analysis").filter(|a| !common::is_error(a)) else {
            continue;
        };
        let none = Map::new();
        if let Ok(qs) = rules::questions(&analysis, &none) {
            check(&case.name, &qs, &mut seen);
            checked += 1;
        }
        if let Ok(merged) = rules::with_all_defaults(&analysis, &none)
            && let Ok(qs) = rules::questions(&analysis, &merged)
        {
            check(&case.name, &qs, &mut seen);
            checked += 1;
        }
    }
    let by_name: std::collections::HashMap<String, common::Case> = common::cases()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect();
    for scenario in common::scenarios() {
        let Some(case) = by_name.get(&scenario.case) else {
            continue;
        };
        let Some(analysis) = case.artifact("analysis").filter(|a| !common::is_error(a)) else {
            continue;
        };
        let answers = scenario.answers.as_object().cloned().unwrap_or_default();
        if let Ok(qs) = rules::questions(&analysis, &answers) {
            check(&format!("scenario {}", scenario.name), &qs, &mut seen);
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} question sets checked");
    println!("{checked} question sets; rules seen: {seen:?}");
    // The common rules are exercised; process- and mate-specific ones are covered below and in
    // the rules' own tests.
    for id in [
        "part.general_tolerance.machined",
        "part.surface_finish.machined",
        "datums.prismatic.a",
        "holes.function.clearance",
        "holes.function.tapped",
        "holes.position.tapped",
        "holes.position.floating",
    ] {
        assert!(seen.contains(id), "no captured question used {id}");
    }
}

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

#[test]
fn a_printed_part_traces_its_vendor_tolerance_and_locating() {
    let answers: Map<String, Value> =
        serde_json::from_value(json!({"part.process": "3D printed: FDM"})).unwrap();
    let qs = rules::questions(&plate(), &answers).unwrap();
    let source = |id: &str| {
        qs.iter()
            .find(|q| q.id == id)
            .and_then(|q| q.source)
            .map(|r| r.id)
    };
    assert_eq!(
        source("part.general_tolerance"),
        Some("part.general_tolerance.printed")
    );
    assert_eq!(source("part.locating"), Some("part.locating.dimensions"));
    let gt = qs
        .iter()
        .find(|q| q.id == "part.general_tolerance")
        .unwrap();
    assert_eq!(gt.to_traced_dict()["source"]["kind"], "vendor");
}

#[test]
fn review_traces_accepted_defaults_and_nothing_else() {
    let analysis = plate();
    let none = Map::new();
    // Nothing answered: nothing traced in the intent, though every question says its source.
    let review = api::review(&analysis, &none).unwrap();
    assert_eq!(review["sources"], json!({}));
    let gt = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "part.general_tolerance")
        .unwrap();
    assert_eq!(
        gt["source"],
        json!({"rule": "part.general_tolerance.machined", "kind": "convention",
               "refs": [{"standard": "ISO 2768-1"}]})
    );
    // Defaults accepted: each is traced; a changed answer is not.
    let mut accepted = rules::with_all_defaults(&analysis, &none).unwrap();
    accepted.insert("part.surface_finish".into(), json!("Ra 1.6"));
    let review = api::review(&analysis, &accepted).unwrap();
    let sources = review["sources"].as_object().unwrap();
    assert_eq!(
        sources["part.general_tolerance"]["rule"],
        "part.general_tolerance.machined"
    );
    assert_eq!(sources["datum.A"]["rule"], "datums.prismatic.a");
    assert!(!sources.contains_key("part.surface_finish"));
    // The same as the MCP characteristics tool gives beside its intent.
    assert_eq!(
        json!(api::preview_sources(&analysis, &accepted).unwrap()),
        review["sources"]
    );
}

#[test]
fn the_command_line_and_serve_give_sources_when_asked_and_python_s_document_otherwise() {
    let analysis = plate();
    let dir = std::env::temp_dir().join(format!("rule-sources-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("analysis.json");
    std::fs::write(&a, serde_json::to_vec(&analysis).unwrap()).unwrap();
    let ask = |extra: &[&str]| -> Value {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("questions")
            .arg(&a)
            .args(extra)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let qs = rules::questions(&analysis, &Map::new()).unwrap();
    let plain: Vec<Value> = qs.iter().map(Question::to_dict).collect();
    let traced: Vec<Value> = qs.iter().map(Question::to_traced_dict).collect();
    assert_eq!(ask(&[]), json!(plain));
    assert_eq!(ask(&["--sources"]), json!(traced));
    assert!(traced.iter().any(|q| q.get("source").is_some()));

    let mut server = specify_core::serve::Server::default();
    let reply = |sources: Option<bool>, server: &mut specify_core::serve::Server| {
        let mut request = json!({"id": 1, "op": "questions", "analysis": analysis, "answers": {}});
        if let Some(s) = sources {
            request["sources"] = json!(s);
        }
        server.reply(&request)["result"]["questions"].clone()
    };
    assert_eq!(reply(None, &mut server), json!(plain));
    assert_eq!(reply(Some(true), &mut server), json!(traced));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_answers_export_traces_its_accepted_defaults() {
    let step = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions/plate_split.old.step.gz");
    let loaded = specify_core::part::load(&step, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given: Map<String, Value> = serde_json::from_value(
        json!({"part.material": "6061-T6", "part.surface_finish": "Ra 1.6"}),
    )
    .unwrap();
    let exported = api::export_answers(&loaded, &analysis, &given).unwrap();
    let sources = exported["_specify"]["sources"].as_object().unwrap();
    assert_eq!(
        sources["part.general_tolerance"]["rule"],
        "part.general_tolerance.machined"
    );
    assert!(sources.contains_key("datum.A"));
    // An answer that differs from the default, and one with no default, are not traced.
    assert!(!sources.contains_key("part.surface_finish"));
    assert!(!sources.contains_key("part.material"));
    // Every traced answer is in the export, and the export reads back as the same answers.
    for id in sources.keys() {
        assert!(exported.contains_key(id), "{id}");
    }
    let again = api::export_answers(&loaded, &analysis, &exported).unwrap();
    assert_eq!(again, exported, "re-exporting is idempotent");
}
