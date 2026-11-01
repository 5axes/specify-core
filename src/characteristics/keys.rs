//! Hand-written storage vocabulary. These spellings are a compatibility contract;
//! see docs/characteristic-numbers.md. Never use Debug or derived enum serialization.
use haecceity::pmi::ThreadSide;
use quiddity::dimensioning::{Kind, PatternRole, Required};

pub(super) fn thread_side(side: ThreadSide) -> &'static str {
    // Retain schema 1's short, public type names, now independent of Rust variant names.
    match side {
        ThreadSide::Internal => "Internal",
        ThreadSide::External => "External",
    }
}

pub(super) fn planner(required: &Required) -> String {
    let measurement = match &required.kind {
        // The role is the planner's stated measurement name, e.g. bore.depth. Faces
        // already identify the feature; arena IDs, nominal values and tolerance do not.
        Kind::Size { role, .. } => format!("measure={role}"),
        Kind::Location { axis, .. } => format!(
            "axis={}",
            match axis {
                0 => "x",
                1 => "y",
                2 => "z",
                _ => "unknown",
            }
        ),
    };
    let pattern = match required.pattern {
        None => "",
        Some(PatternRole::FirstMember) => ";pattern=first",
        Some(PatternRole::Centre) => ";pattern=centre",
        Some(PatternRole::Pitch { .. }) => ";pattern=pitch",
    };
    format!("{measurement}{pattern}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characteristics::items;
    use haecceity::pmi;
    use serde_json::json;

    #[test]
    fn exact_pattern_keys_through_the_writer() {
        let loaded = crate::part::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/parts/plate.step"),
            None,
        )
        .unwrap();
        let analysis = crate::api::analyse(&loaded).unwrap();
        let given = json!({"part.material":"steel"});
        let intent =
            crate::rules::apply_with_defaults(&analysis, given.as_object().unwrap(), true).unwrap();
        let (bytes, _) = crate::writer::write_bytes(&loaded, &intent, None).unwrap();
        let doc = haecceity::p21::Document::parse(bytes).unwrap();
        let model = pmi::read(&doc, &[loaded.definition]).unwrap();
        let keys: Vec<_> = items(&model.parts[0], None, None)
            .into_iter()
            .filter(|i| i.key.role.contains(";pattern="))
            .map(|i| serde_json::to_string(&i.key).unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                r#"{"role":"location|planner:axis=x;pattern=pitch","kind":"dimension","type":"Location_LinearDistance","faces":[11],"reference":[10]}"#,
                r#"{"role":"location|planner:axis=y;pattern=pitch","kind":"dimension","type":"Location_LinearDistance","faces":[10],"reference":[6]}"#,
                r#"{"role":"location|planner:axis=x;pattern=centre","kind":"dimension","type":"Location_LinearDistance","faces":[6,7,8,10,11,13],"reference":[0]}"#,
            ]
        );
    }

    #[test]
    fn exact_keys_for_fits_thread_class_general_tolerance_knurl_and_modified_gdt() {
        let loaded = crate::part::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/characteristic-numbers-016.step.gz"),
            None,
        )
        .unwrap();
        let doc = haecceity::p21::Document::parse(loaded.bytes.to_vec()).unwrap();
        let mut model = pmi::read(&doc, &[loaded.definition])
            .unwrap()
            .parts
            .remove(0);
        for (dimension, notation) in model.dimensions.iter().zip(["h6", "H7"]) {
            let pmi::DimTolerance::Fit { class, .. } = dimension.tolerance else {
                panic!("captured fit")
            };
            assert_eq!(class.to_string(), notation);
        }
        assert_eq!(model.threads[0].fit_class, "6g");
        assert!(model.general.iter().any(|g| matches!(g,
            pmi::GeneralTolerance::Class { text, .. } if text == "ISO 2768-m")));
        let keys = |p: &pmi::PartPmi| {
            items(p, None, None)
                .into_iter()
                .map(|i| serde_json::to_string(&i.key).unwrap())
                .collect::<Vec<_>>()
        };
        // h6 shaft fit, H7 hole fit, 6g thread class, ISO 2768-m and diamond knurl
        // are real 0.1.6 PMI. None of their value/notation text enters these keys.
        assert_eq!(
            keys(&model),
            [
                r#"{"role":"datum","kind":"datum","type":"A","faces":[6],"reference":[]}"#,
                r#"{"role":"size","kind":"dimension","type":"Size_Diameter","faces":[0],"reference":[]}"#,
                r#"{"role":"size","kind":"dimension","type":"Size_Diameter","faces":[3],"reference":[]}"#,
                r#"{"role":"feature","kind":"geometric_tolerance","type":"Flatness","faces":[6],"reference":[]}"#,
                r#"{"role":"feature","kind":"thread","type":"External","faces":[4],"reference":[]}"#,
                r#"{"role":"feature","kind":"knurl","type":"knurl","faces":[0],"reference":[]}"#,
            ]
        );
        let before = keys(&model);
        model.dimensions[0].qualifier = Some(pmi::Qualifier::Maximum);
        model.dimensions[0].tolerance = pmi::DimTolerance::Basic;
        model.threads[0].fit_class = "6H".into();
        model.knurls[0].pattern = pmi::KnurlPattern::Straight;
        model.general.clear();
        model.tolerances[0].modifiers = vec![pmi::ToleranceModifier::MaximumMaterialRequirement];
        assert_eq!(keys(&model), before);
        model.tolerances[0].kind = pmi::ToleranceKind::Position;
        let position = r#"{"role":"feature","kind":"geometric_tolerance","type":"Position","faces":[6],"reference":[]}"#;
        assert_eq!(keys(&model)[3], position);
        model.tolerances[0].modifiers = vec![pmi::ToleranceModifier::LeastMaterialRequirement];
        assert_eq!(keys(&model)[3], position);
        model.threads[0].side = ThreadSide::Internal;
        assert_eq!(
            keys(&model)[4],
            r#"{"role":"feature","kind":"thread","type":"Internal","faces":[4],"reference":[]}"#
        );
    }

    #[test]
    fn planner_key_vocabulary_is_explicit_and_independent_of_values_and_ids() {
        let mut r: Required = serde_json::from_value(json!({
            "id":"stored:dim:hole:9/bore.diameter:10:Iso286Class { deviation: H }",
            "kind":{"size":{"feature":"hole:9","role":"bore.diameter","kind":"diameter"}},
            "nominal":10.,"count":6,"pattern":null,"tolerance":"general",
            "reference":null,"covers":["hole:9"]
        }))
        .unwrap();
        assert_eq!(planner(&r), "measure=bore.diameter");
        r.id = "a renamed upstream identifier".into();
        r.nominal = 20.;
        r.count = 8;
        r.tolerance = quiddity::dimensioning::Tolerance::Basic;
        assert_eq!(planner(&r), "measure=bore.diameter");
        for (pattern, text) in [
            (
                PatternRole::FirstMember,
                "measure=bore.diameter;pattern=first",
            ),
            (PatternRole::Centre, "measure=bore.diameter;pattern=centre"),
            (
                PatternRole::Pitch { intervals: 6 },
                "measure=bore.diameter;pattern=pitch",
            ),
            (
                PatternRole::Pitch { intervals: 8 },
                "measure=bore.diameter;pattern=pitch",
            ),
        ] {
            r.pattern = Some(pattern);
            assert_eq!(planner(&r), text);
        }
        let support = quiddity::dimensioning::Support {
            feature: "plane:4".into(),
            faces: vec![4],
            body: None,
            at: [0.; 3],
            plane: true,
        };
        r.pattern = None;
        for (axis, text) in [(0, "axis=x"), (1, "axis=y"), (2, "axis=z")] {
            r.kind = Kind::Location {
                from: support.clone(),
                to: support.clone(),
                axis,
            };
            assert_eq!(planner(&r), text);
        }
        assert_eq!(thread_side(ThreadSide::Internal), "Internal");
        assert_eq!(thread_side(ThreadSide::External), "External");
    }
}
