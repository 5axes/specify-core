//! The six AP203 repairs are kernel Add refusals, but Specify's PMI-less inputs
//! deliberately use Replace. Keep both contracts explicit, with exact repair ids.
mod common;
use haecceity::{
    p21::Document,
    pmi::{
        self,
        write::{Mode, PresentationPolicy, UpgradeRepairKind, WriteError},
    },
};
use serde_json::json;
use specify_core::{existing, part, rules, writer};

#[test]
fn six_ap203_repairs_refuse_kernel_add_but_specify_uses_replace_without_foreign_pmi() {
    use UpgradeRepairKind::{R1ApprovalAssignment, R2ConversionUnitDimensions};
    let Some(root) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "required pinned corpus"
        );
        return;
    };
    for (name, repair, ids) in [
        (
            "nist_ctc_01_asme1_rd.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ctc_02_asme1_rc.stp",
            R2ConversionUnitDimensions,
            vec![20750],
        ),
        (
            "nist_ctc_03_asme1_rc.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ctc_05_asme1_rd.stp",
            R2ConversionUnitDimensions,
            vec![6596, 6599, 6625, 6634, 6651, 6660, 6669],
        ),
        (
            "nist_ftc_06_asme1_rd.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ftc_09_asme1_rd.stp",
            R2ConversionUnitDimensions,
            vec![5778, 5781, 5805, 5814, 5827, 5836],
        ),
    ] {
        let loaded = part::load(&root.join("nist").join(name), None).unwrap();
        let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
        let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert_eq!(
            read.parts[0],
            pmi::PartPmi::default(),
            "{name}: no foreign PMI"
        );
        let model = pmi::PartPmi {
            features: vec![pmi::Feature::Items(vec![pmi::Anchor::Face(
                pmi::FaceIndex(0),
            )])],
            datums: vec![
                pmi::Datum::new(
                    pmi::DatumLabel::new("Z").unwrap(),
                    Some(pmi::FeatureId(0)),
                    vec![],
                )
                .unwrap(),
            ],
            ..Default::default()
        };
        let error = pmi::write(
            &doc,
            &defs,
            &[(pmi::PartId(0), model)],
            Mode::Add,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap_err();
        assert!(
            matches!(&error, WriteError::AddPreservation { ids: found, repairs, .. }
            if found == &ids && repairs == &ids.iter().map(|&id| (id, repair)).collect::<Vec<_>>()),
            "{name}: {error}"
        );
        // No automatic fallback: this input qualifies for Replace before writing.
        assert!(
            existing::read_existing(&loaded).unwrap().is_empty(),
            "{name}"
        );
        let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
        intent.part.insert("material".into(), json!("steel"));
        let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
        let reopened = part::load_bytes(name, bytes).unwrap();
        assert_eq!(
            existing::part_settings(&reopened).unwrap()["material"],
            "steel",
            "{name}"
        );
    }
}
