//! Which rule proposed a default, and what that rule rests on (#196). See
//! `docs/rule-sources.md`, whose "Rule ids" table lists every rule here (a test holds the two
//! together).
//!
//! Every question with a default carries one of these beside its free-text `basis`. Each rule
//! is defined once, here; the rules refer to the statics, never to an id written out. A rule
//! id is stable: front ends may key on it. An edition or clause is given only where the code or
//! its docs record it.

use serde_json::{Map, Value, json};

/// How firm a rule's source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Read from a published standard's table or formula.
    Standard,
    /// Specify's own choice: common practice, not required by any standard.
    Convention,
    /// A figure published by manufacturing services, where no standard exists.
    Vendor,
    /// A value no source gives, stated as assumed.
    Assumption,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Standard => "standard",
            Kind::Convention => "convention",
            Kind::Vendor => "vendor",
            Kind::Assumption => "assumption",
        }
    }
}

/// A standard a rule cites, with its edition and clause where recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref {
    pub standard: &'static str,
    pub edition: Option<&'static str>,
    pub clause: Option<&'static str>,
}

/// A rule that proposes a default.
#[derive(Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: &'static str,
    pub kind: Kind,
    pub refs: &'static [Ref],
}

impl Rule {
    /// `{"rule", "kind", "refs": [{"standard", "edition"?, "clause"?}]}`.
    pub fn to_json(&self) -> Value {
        let refs: Vec<Value> = self
            .refs
            .iter()
            .map(|r| {
                let mut out = Map::new();
                out.insert("standard".into(), json!(r.standard));
                if let Some(edition) = r.edition {
                    out.insert("edition".into(), json!(edition));
                }
                if let Some(clause) = r.clause {
                    out.insert("clause".into(), json!(clause));
                }
                Value::Object(out)
            })
            .collect();
        json!({"rule": self.id, "kind": self.kind.as_str(), "refs": refs})
    }
}

const fn cite(standard: &'static str) -> Ref {
    Ref {
        standard,
        edition: None,
        clause: None,
    }
}

const ISO_2768_1: Ref = cite("ISO 2768-1");
const ISO_273: Ref = cite("ISO 273");
const ISO_273_MEDIUM: Ref = Ref {
    clause: Some("medium series"),
    ..ISO_273
};
// The edition is recalled, not checked against a copy (docs/next-session.md), so none is cited.
const ISO_261: Ref = cite("ISO 261");
const ISO_261_COARSE: Ref = Ref {
    clause: Some("coarse pitch"),
    ..ISO_261
};
const ISO_286_1: Ref = Ref {
    standard: "ISO 286-1",
    edition: Some("2010"),
    clause: None,
};

const fn rule(id: &'static str, kind: Kind, refs: &'static [Ref]) -> Rule {
    Rule { id, kind, refs }
}

use Kind::{Assumption, Convention, Standard, Vendor};

// Part settings.
pub static GENERAL_TOLERANCE_MACHINED: Rule =
    rule("part.general_tolerance.machined", Convention, &[ISO_2768_1]);
pub static GENERAL_TOLERANCE_PRINTED: Rule = rule("part.general_tolerance.printed", Vendor, &[]);
pub static SURFACE_FINISH_MACHINED: Rule = rule("part.surface_finish.machined", Convention, &[]);
pub static EDGES_MACHINED: Rule = rule("part.edges.machined", Convention, &[]);
pub static EDGES_SHEET_METAL: Rule = rule("part.edges.sheet_metal", Convention, &[]);
pub static COATING_NONE: Rule = rule("part.coating.none", Assumption, &[]);
pub static HEAT_TREATMENT_NONE: Rule = rule("part.heat_treatment.none", Assumption, &[]);
pub static NOTES_NONE: Rule = rule("part.notes.none", Assumption, &[]);
pub static LOCATING_POSITION: Rule = rule("part.locating.position", Convention, &[]);
pub static LOCATING_DIMENSIONS: Rule = rule("part.locating.dimensions", Convention, &[]);
pub static PROCESS_MATERIAL_CLASS: Rule = rule("process.material_class", Convention, &[]);
pub static PROCESS_ACCEPTED: Rule = rule("process.accepted", Convention, &[]);

// Datums.
pub static DATUM_A_PRISMATIC: Rule = rule("datums.prismatic.a", Convention, &[]);
pub static DATUM_A_CONTACT: Rule = rule("datums.prismatic.a.contact", Convention, &[]);
pub static DATUM_B_PRISMATIC: Rule = rule("datums.prismatic.b", Convention, &[]);
pub static DATUM_C_PRISMATIC: Rule = rule("datums.prismatic.c", Convention, &[]);
pub static DATUM_A_TURNED: Rule = rule("datums.turned.a", Convention, &[]);
pub static DATUM_B_TURNED: Rule = rule("datums.turned.b", Convention, &[]);
pub static DATUM_NONE: Rule = rule("datums.none", Convention, &[]);
pub static DATUM_CONTROL_NONE: Rule = rule("datums.control.none", Convention, &[]);

// Holes.
pub static HOLE_CLEARANCE: Rule = rule("holes.function.clearance", Standard, &[ISO_273]);
pub static HOLE_TAPPED: Rule = rule("holes.function.tapped", Convention, &[ISO_261_COARSE]);
pub static HOLE_GENERAL: Rule = rule("holes.function.general", Convention, &[]);
pub static HOLE_PRINTED: Rule = rule("holes.function.printed", Convention, &[ISO_273]);
pub static HOLE_FRAME: Rule = rule("holes.frame.primary", Convention, &[]);
pub static POSITION_FITTED: Rule = rule("holes.position.fitted", Convention, &[]);
pub static POSITION_TAPPED: Rule = rule("holes.position.tapped", Convention, &[ISO_273_MEDIUM]);
pub static POSITION_FLOATING: Rule = rule("holes.position.floating", Convention, &[ISO_273]);
pub static POSITION_NO_BOLT: Rule = rule("holes.position.no_bolt", Convention, &[]);
pub static LOCATION_GENERAL: Rule = rule("holes.location.general", Convention, &[]);
pub static FULL_THREAD: Rule = rule("holes.full_thread", Convention, &[ISO_261_COARSE]);
pub static FULL_THREAD_GIVEN: Rule = rule("holes.full_thread.given", Convention, &[]);

// Turned diameters.
pub static TURNED_GENERAL: Rule = rule("turned.fit.general", Convention, &[]);
pub static RUNOUT_NONE: Rule = rule("turned.runout.none", Convention, &[]);

// Assembly mates.
pub static MATE_TAPPED: Rule = rule("mates.tapped", Convention, &[ISO_273]);
pub static MATE_CLEARANCE: Rule = rule("mates.clearance", Convention, &[ISO_273]);
pub static MATE_FIT: Rule = rule("mates.fit", Assumption, &[ISO_286_1]);
pub static MATE_THREAD: Rule = rule("mates.thread", Convention, &[ISO_261]);

// BOM suggestions.
pub static MAKE_BUY_GUESS: Rule = rule("bom.make_buy.guess", Convention, &[]);
pub static MAKE_BUY_RECORDED: Rule = rule("bom.make_buy.recorded", Convention, &[]);

/// Every rule, in the order `docs/rule-sources.md` lists them.
pub static RULES: [&Rule; 40] = [
    &GENERAL_TOLERANCE_MACHINED,
    &GENERAL_TOLERANCE_PRINTED,
    &SURFACE_FINISH_MACHINED,
    &EDGES_MACHINED,
    &EDGES_SHEET_METAL,
    &COATING_NONE,
    &HEAT_TREATMENT_NONE,
    &NOTES_NONE,
    &LOCATING_POSITION,
    &LOCATING_DIMENSIONS,
    &PROCESS_MATERIAL_CLASS,
    &PROCESS_ACCEPTED,
    &DATUM_A_PRISMATIC,
    &DATUM_A_CONTACT,
    &DATUM_B_PRISMATIC,
    &DATUM_C_PRISMATIC,
    &DATUM_A_TURNED,
    &DATUM_B_TURNED,
    &DATUM_NONE,
    &DATUM_CONTROL_NONE,
    &HOLE_CLEARANCE,
    &HOLE_TAPPED,
    &HOLE_GENERAL,
    &HOLE_PRINTED,
    &HOLE_FRAME,
    &POSITION_FITTED,
    &POSITION_TAPPED,
    &POSITION_FLOATING,
    &POSITION_NO_BOLT,
    &LOCATION_GENERAL,
    &FULL_THREAD,
    &FULL_THREAD_GIVEN,
    &TURNED_GENERAL,
    &RUNOUT_NONE,
    &MATE_TAPPED,
    &MATE_CLEARANCE,
    &MATE_FIT,
    &MATE_THREAD,
    &MAKE_BUY_GUESS,
    &MAKE_BUY_RECORDED,
];

/// The rule with `id`, if there is one.
pub fn get(id: &str) -> Option<&'static Rule> {
    RULES.iter().copied().find(|r| r.id == id)
}

/// The rule behind an assembly mate's suggested `value` (`mates::suggest`): a thread, a tapped
/// hole, a clearance hole, or a fit (`fit:H7` in a bore, `h6` on a shaft).
pub fn of_mate(value: &str) -> &'static Rule {
    if value.starts_with("tapped:") {
        &MATE_TAPPED
    } else if value.starts_with("clearance") {
        &MATE_CLEARANCE
    } else if value.starts_with("thread:") {
        &MATE_THREAD
    } else {
        &MATE_FIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_are_unique_and_found() {
        for (i, rule) in RULES.iter().enumerate() {
            assert!(
                RULES[..i].iter().all(|r| r.id != rule.id),
                "{} is listed twice",
                rule.id
            );
            assert_eq!(get(rule.id), Some(*rule));
        }
    }

    #[test]
    fn a_source_states_its_rule_kind_and_refs() {
        assert_eq!(
            POSITION_TAPPED.to_json(),
            json!({"rule": "holes.position.tapped", "kind": "convention",
                   "refs": [{"standard": "ISO 273", "clause": "medium series"}]})
        );
        assert_eq!(
            MATE_FIT.to_json()["refs"],
            json!([{"standard": "ISO 286-1", "edition": "2010"}])
        );
        assert_eq!(of_mate("h6"), &MATE_FIT);
        assert_eq!(of_mate("clearance:M5"), &MATE_CLEARANCE);
    }
}
