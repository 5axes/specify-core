//! Two revisions of an assembly: its parts paired by name, a parts list of what was added,
//! removed, changed, placed a different number of times or moved, and each changed part's own
//! comparison ([`super::diff`]); [`Assembly::picture`] draws the whole assembly coloured by it.
//!
//! A part is matched by its name in the file, not by its product id (often `SOLID` for every
//! part), and not by its geometry: a renamed part is one removed and another added. Parts of the
//! same name pair in file order.

use std::fmt::Write;

use serde::Serialize;
use serde_json::{Map, Value};

use super::{Revised, Size, edges, mark, triangles};
use crate::api::{self, Result};
use crate::part::{File, LoadedPart};
use crate::rules;

/// Whether a file is to be compared as an assembly: it has several parts, or a part placed
/// other than once.
pub fn is_assembly(file: &File) -> bool {
    let parts = file.parts();
    parts.len() > 1 || parts.iter().any(|p| p.instances != 1)
}

/// Each part's name for matching and showing, from its `(name, product id)`: its name, else
/// its product id, else `part N` (N its index + 1); the k-th part of the same name in a file is
/// `name (k)` from k = 2.
pub fn names<'a>(parts: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, (name, product_id)) in parts.into_iter().enumerate() {
        let base = [name.trim(), product_id.trim()]
            .into_iter()
            .find(|n| !n.is_empty())
            .map_or_else(|| format!("part {}", i + 1), String::from);
        let mut shown = base.clone();
        let mut k = 1;
        while out.contains(&shown) {
            k += 1;
            shown = format!("{base} ({k})");
        }
        out.push(shown);
    }
    out
}

/// The two files' parts paired by name: (name, old index, new index), in the new file's order,
/// then the removed parts in the old file's order.
pub fn matched(old: &[String], new: &[String]) -> Vec<(String, Option<usize>, Option<usize>)> {
    let mut out: Vec<_> = new
        .iter()
        .enumerate()
        .map(|(j, name)| (name.clone(), old.iter().position(|n| n == name), Some(j)))
        .collect();
    for (i, name) in old.iter().enumerate() {
        if !new.contains(name) {
            out.push((name.clone(), Some(i), None));
        }
    }
    out
}

/// Which of a file's parts, and how many times it is placed.
#[derive(Clone, Debug, Serialize)]
pub struct Placed {
    pub part: usize,
    pub instances: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartChange {
    pub name: String,
    /// `changed`, `added`, `removed`, `quantity` (the same part, placed a different number of
    /// times), `moved` (the same part and count, placed elsewhere) or `unchanged`.
    pub status: &'static str,
    pub old: Option<Placed>,
    pub new: Option<Placed>,
    /// The part's own comparison, for a part in both files.
    #[serde(rename = "report", skip_serializing_if = "Option::is_none")]
    pub revised: Option<Revised>,
    /// BOM answers carried geometrically even when a renamed part is unpaired for PMI.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub bom_answers: Map<String, Value>,
    /// The old placements no new one is at, of a part placed a different number of times or
    /// moved, for [`Assembly::picture`].
    #[serde(skip)]
    pub gone: Vec<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Assembly {
    /// The worst of the changed parts' verdicts (`problems`, then `decide`), else `ok`.
    pub verdict: String,
    pub summary: String,
    /// Every part of either file, as [`matched`] orders them.
    pub parts: Vec<PartChange>,
    /// Opaque BOM allocation/answer transport, also included by `answers()` for added parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bom_history: Option<Value>,
}

/// Whether a part's comparison finds nothing: no feature, face, answer, question, problem or
/// PMI changed, and its volume, area, box and face count are the same. Not whether the
/// alignment found a move: [`placed`] weighs that.
fn unchanged(r: &Revised) -> bool {
    let close = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 0.005,
        (a, b) => a == b,
    };
    let (o, n): (&Size, &Size) = (&r.measures.old, &r.measures.new);
    r.features.is_empty()
        && r.faces.is_empty()
        && r.gone.is_empty()
        && r.unresolved.is_empty()
        && r.check.is_empty()
        && r.questions.is_empty()
        && r.problems.added.is_empty()
        && r.pmi.items.is_empty()
        && (0..3)
            .all(|k| (o.min[k] - n.min[k]).abs() < 0.005 && (o.max[k] - n.max[k]).abs() < 0.005)
        && close(o.volume, n.volume)
        && close(o.area, n.area)
        && o.faces == n.faces
}

type Place = [[f64; 4]; 3];

/// A part in both files whose geometry did not change: `quantity` when it is placed a
/// different number of times, `moved` when an old placement has no new one at it, else
/// `unchanged`; and those old placements. *new* is where the new placements put the old part.
fn placed(old: &[Place], new: &[Place]) -> (&'static str, Vec<usize>) {
    let same = |a: &Place, b: &Place| {
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(x, y)| (x - y).abs() <= 1e-4)
    };
    let gone: Vec<usize> = (0..old.len())
        .filter(|&i| !new.iter().any(|b| same(&old[i], b)))
        .collect();
    let status = if old.len() != new.len() {
        "quantity"
    } else if gone.is_empty() {
        "unchanged"
    } else {
        "moved"
    };
    (status, gone)
}

/// The new part's placements, each after the part's own move between the revisions when it
/// moved in its own frame: where they put the old part.
fn placing_old(new: &[Place], r: &Revised) -> Vec<Place> {
    let a = match &r.correspondence {
        Some(c) if !r.still => &c.alignment,
        _ => return new.to_vec(),
    };
    new.iter()
        .map(|p| {
            std::array::from_fn(|i| {
                let row = |k: usize| (0..3).map(|j| p[i][j] * a.rotation[j][k]).sum::<f64>();
                let shift = (0..3).map(|j| p[i][j] * a.translation[j]).sum::<f64>() + p[i][3];
                [row(0), row(1), row(2), shift]
            })
        })
        .collect()
}

fn names_of(parts: &[LoadedPart]) -> Vec<String> {
    names(
        parts
            .iter()
            .map(|p| (p.name.as_str(), p.definition.product_id.as_str())),
    )
}

/// Compares every part of two revisions of an assembly. *given* is the old parts' answers by
/// name, `{part name: {question id: value}}`, or, when the old file is a single part, a part's
/// `{question id: value}`; a part with none carries its own stored answers.
pub fn diff(
    old: &[LoadedPart],
    new: &[LoadedPart],
    given: &Map<String, Value>,
) -> Result<Assembly> {
    diff_with_options(old, new, given, super::DiffOptions::default())
}

/// Apply the report options to every matched part.
pub fn diff_with_options(
    old: &[LoadedPart],
    new: &[LoadedPart],
    given: &Map<String, Value>,
    options: super::DiffOptions,
) -> Result<Assembly> {
    let (old_names, new_names) = (names_of(old), names_of(new));
    let wrapped;
    let given = match old_names.as_slice() {
        [only] if given.values().any(|a| !a.is_object()) => {
            wrapped = Map::from_iter([(only.clone(), Value::Object(given.clone()))]);
            &wrapped
        }
        _ => given,
    };
    let invalid = |m: String| api::Error::Rules(rules::Error::Invalid(m));
    for (name, answers) in given {
        if !answers.is_object() {
            return Err(invalid(
                "assembly requirements use {part name: {question id: value}}".into(),
            ));
        }
        crate::input::answers_shape(answers.as_object().expect("checked above"))
            .map_err(|e| invalid(format!("part {name:?}: {e}")))?;
        if !old_names.contains(name) {
            let path = old.first().map(|p| p.path.display().to_string());
            return Err(invalid(format!(
                "{} has no part named {name:?}",
                path.unwrap_or_default()
            )));
        }
    }
    let count = |parts: &[LoadedPart], k: Option<usize>| {
        k.map(|k| Placed {
            part: k,
            instances: parts[k].placements.len(),
        })
    };
    let bom_answers = crate::bom::assembly_answers(old, new).map_err(api::Error::Analysis)?;
    let mut rows = Vec::new();
    for (name, o, n) in matched(&old_names, &new_names) {
        let (status, gone, revised) = match (o, n) {
            (Some(i), Some(j)) => {
                let answers = given
                    .get(&name)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let r =
                    super::diff_with_options(&old[i], &new[j], &answers, options).map_err(|e| {
                        match e {
                            api::Error::Rules(rules::Error::Invalid(message)) => {
                                invalid(format!("part {name:?}: {message}"))
                            }
                            other => other,
                        }
                    })?;
                let (status, gone) = if unchanged(&r) {
                    placed(&old[i].placements, &placing_old(&new[j].placements, &r))
                } else {
                    ("changed", Vec::new())
                };
                (status, gone, Some(r))
            }
            (Some(_), None) => ("removed", Vec::new(), None),
            _ => ("added", Vec::new(), None),
        };
        rows.push(PartChange {
            name,
            status,
            old: count(old, o),
            new: count(new, n),
            revised,
            bom_answers: n.map(|i| bom_answers[i].clone()).unwrap_or_default(),
            gone,
        });
    }
    let verdicts: Vec<&str> = rows
        .iter()
        .filter(|r| r.status == "changed")
        .filter_map(|r| r.revised.as_ref().map(|r| r.verdict.as_str()))
        .collect();
    let verdict = ["problems", "decide"]
        .into_iter()
        .find(|v| verdicts.contains(v))
        .unwrap_or("ok");
    let mut transport = Map::new();
    if let Some(first) = old.first() {
        crate::bom::carry(first, &mut transport).map_err(api::Error::Analysis)?;
    }
    Ok(Assembly {
        bom_history: transport.remove(crate::bom::CARRY),
        verdict: verdict.into(),
        summary: summary(&rows),
        parts: rows,
    })
}

/// `2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10 unchanged.`: each kind of change
/// counted, its parts named when there are 5 or fewer.
fn summary(rows: &[PartChange]) -> String {
    let of = |status: &str| -> Vec<&str> {
        rows.iter()
            .filter(|r| r.status == status)
            .map(|r| r.name.as_str())
            .collect()
    };
    let unchanged = of("unchanged").len();
    if unchanged == rows.len() {
        return format!("No part changed ({unchanged} unchanged).");
    }
    let mut groups = Vec::new();
    for (status, words) in [
        ("changed", "changed"),
        ("added", "added"),
        ("removed", "removed"),
        ("quantity", "quantity changed"),
        ("moved", "moved"),
    ] {
        let names = of(status);
        if names.is_empty() {
            continue;
        }
        let mut group = if groups.is_empty() {
            let parts = if names.len() == 1 { "part" } else { "parts" };
            format!("{} {parts} {words}", names.len())
        } else {
            format!("{} {words}", names.len())
        };
        if names.len() <= 5 {
            let _ = write!(group, " ({})", names.join(", "));
        }
        groups.push(group);
    }
    if unchanged > 0 {
        groups.push(format!("{unchanged} unchanged"));
    }
    groups.join(", ") + "."
}

/// *text* for a URL's query, as `jq`'s `@uri` writes it: `A-Za-z0-9-_.~` kept, every other
/// byte `%XX`.
fn uri(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

impl Assembly {
    /// The answers carried to the new revision, by part: `{part name: answers}` for each part
    /// that has some.
    pub fn answers(&self) -> Map<String, Value> {
        self.parts
            .iter()
            .filter(|p| p.new.is_some())
            .filter_map(|p| {
                let mut a = p.bom_answers.clone();
                if let Some(r) = &p.revised {
                    a.extend(r.answers.clone());
                }
                if let Some(history) = &self.bom_history {
                    a.insert(crate::bom::CARRY.into(), history.clone());
                }
                (!a.is_empty()).then(|| (p.name.clone(), Value::Object(a)))
            })
            .collect()
    }

    /// The report as Markdown, for a pull request comment: the summary, *image* (the picture's
    /// link), the parts that differ, and a section for each changed part. *viewer*, the page
    /// that shows these revisions in 3D, links each changed part there with `&part=NAME`.
    pub fn markdown(&self, title: &str, image: Option<&str>, viewer: Option<&str>) -> String {
        let mut s = format!(
            "### {title}: Specify\n\n{} {}\n\n",
            mark(&self.verdict),
            self.summary
        );
        if let Some(image) = image {
            let _ = writeln!(
                s,
                "![The assembly: changed, re-counted and moved parts in orange, added in green, \
                 removed in red]({image})\n"
            );
        }
        let differ: Vec<&PartChange> = self
            .parts
            .iter()
            .filter(|p| p.status != "unchanged")
            .collect();
        if !differ.is_empty() {
            s += "| Part | Quantity | Change |\n|---|---|---|\n";
            for p in &differ {
                let count = |x: &Option<Placed>| x.as_ref().map(|x| x.instances);
                let quantity = match (count(&p.old), count(&p.new)) {
                    (Some(a), Some(b)) if a != b => format!("{a} → {b}"),
                    (_, Some(n)) | (Some(n), None) => n.to_string(),
                    (None, None) => String::new(),
                };
                let change = match (p.status, &p.revised) {
                    ("changed", Some(r)) => format!("{} changed", r.mark()),
                    ("quantity", _) => "quantity changed".into(),
                    (status, _) => status.into(),
                };
                let _ = writeln!(s, "| {} | {quantity} | {change} |", p.name);
            }
            s += "\n";
            let rest = self.parts.len() - differ.len();
            if rest == 1 {
                s += "1 other part is unchanged.\n\n";
            } else if rest > 1 {
                let _ = writeln!(s, "{rest} other parts are unchanged.\n");
            }
        }
        for p in &differ {
            let (Some(r), "changed") = (&p.revised, p.status) else {
                continue;
            };
            let link =
                viewer.map(|v| format!("[Turn {} round in 3D]({v}&part={})", p.name, uri(&p.name)));
            s += &r.markdown_headed(&format!("#### {}", p.name), None, link.as_deref());
        }
        s
    }

    /// The new assembly's picture, as [`super::picture`] draws a part: every part where it is
    /// placed, changed, re-counted and moved parts orange, added green, the rest grey, and in
    /// translucent red, where they are not under a part now, the removed parts and the
    /// placements that went; a PNG *width* pixels wide.
    pub fn picture(&self, old: &[LoadedPart], new: &[LoadedPart], width: u32) -> Vec<u8> {
        let (mut solid, mut ghost, mut lines) = (Vec::new(), Vec::new(), Vec::new());
        let mut graphics = Vec::new();
        let red = [0.84, 0.16, 0.16];
        let place = |at: &Place| {
            let at = *at;
            move |q: [f64; 3]| {
                [0, 1, 2].map(|i| at[i][0] * q[0] + at[i][1] * q[1] + at[i][2] * q[2] + at[i][3])
            }
        };
        for p in &self.parts {
            if let Some(revised) = &p.revised {
                for graphic in &revised.pmi_graphics {
                    let placed = if graphic.old {
                        p.old.as_ref().map(|o| &old[o.part])
                    } else {
                        p.new.as_ref().map(|n| &new[n.part])
                    };
                    if let Some(part) = placed {
                        for at in &part.placements {
                            graphics.push(graphic.placed(&place(at)));
                        }
                    }
                }
            }
            if let (false, Some(o)) = (p.gone.is_empty(), &p.old) {
                let mesh = crate::mesh::mesh(&old[o.part]);
                for &i in &p.gone {
                    ghost.extend(triangles(
                        &mesh,
                        &|_| Some(red),
                        &place(&old[o.part].placements[i]),
                    ));
                }
            }
            let (parts, k, colour) = match (p.status, &p.old, &p.new) {
                ("removed", Some(o), _) => (old, o.part, red),
                (_, _, Some(n)) => (
                    new,
                    n.part,
                    match p.status {
                        "unchanged" => [0.80, 0.81, 0.83],
                        "changed"
                            if p.revised.as_ref().is_some_and(|r| {
                                r.features.is_empty() && r.faces.is_empty() && r.gone.is_empty()
                            }) =>
                        {
                            [0.80, 0.81, 0.83]
                        }
                        "added" => [0.04, 0.56, 0.24],
                        _ => [0.88, 0.48, 0.0],
                    },
                ),
                _ => continue,
            };
            let mesh = crate::mesh::mesh(&parts[k]);
            for at in &parts[k].placements {
                let place = place(at);
                let tris = triangles(&mesh, &|_| Some(colour), &place);
                if p.status == "removed" {
                    ghost.extend(tris);
                } else {
                    solid.extend(tris);
                    lines.extend(edges(&mesh, &place));
                }
            }
        }
        super::render_with_pmi(&solid, &ghost, &lines, &graphics, true, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn parts_pair_by_name_in_the_new_file_s_order_then_the_removed() {
        let old = strings(&["shank", "shaft", "plate"]);
        let new = strings(&["shaft", "shank", "washer"]);
        assert_eq!(
            matched(&old, &new),
            vec![
                ("shaft".into(), Some(1), Some(0)),
                ("shank".into(), Some(0), Some(1)),
                ("washer".into(), None, Some(2)),
                ("plate".into(), Some(2), None),
            ]
        );
    }

    #[test]
    fn a_renamed_part_is_one_removed_and_another_added() {
        let got = matched(
            &strings(&["drive plate"]),
            &strings(&["GRM-04 drive plate"]),
        );
        assert_eq!(
            got,
            vec![
                ("GRM-04 drive plate".into(), None, Some(0)),
                ("drive plate".into(), Some(0), None),
            ]
        );
    }

    #[test]
    fn duplicate_names_pair_in_file_order_and_unnamed_parts_fall_back() {
        let shown = names([
            ("SOLID", ""),
            ("", "SOLID"),
            ("", ""),
            ("bolt", "X"),
            (" ", ""),
        ]);
        assert_eq!(
            shown,
            strings(&["SOLID", "SOLID (2)", "part 3", "bolt", "part 5"])
        );
        let old = names([("SOLID", ""), ("SOLID", "")]);
        let new = names([("SOLID", ""), ("SOLID", ""), ("SOLID", "")]);
        assert_eq!(
            matched(&old, &new),
            vec![
                ("SOLID".into(), Some(0), Some(0)),
                ("SOLID (2)".into(), Some(1), Some(1)),
                ("SOLID (3)".into(), None, Some(2)),
            ]
        );
    }

    #[test]
    fn an_unchanged_part_is_counted_moved_or_unchanged_by_its_placements() {
        let at = |x: f64| {
            [
                [1.0, 0.0, 0.0, x],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ]
        };
        assert_eq!(
            placed(&[at(0.0), at(5.0)], &[at(0.0), at(5.0), at(10.0)]),
            ("quantity", vec![])
        );
        assert_eq!(
            placed(&[at(0.0), at(5.0), at(10.0)], &[at(10.0), at(0.0)]),
            ("quantity", vec![1])
        );
        assert_eq!(placed(&[at(0.0)], &[at(1.0)]), ("moved", vec![0]));
        assert_eq!(
            placed(&[at(0.0), at(5.0)], &[at(5.0), at(1e-9)]),
            ("unchanged", vec![])
        );
    }

    #[test]
    fn a_part_name_is_written_as_jq_s_uri_writes_it() {
        assert_eq!(uri("GRM-04 drive plate"), "GRM-04%20drive%20plate");
        assert_eq!(uri("Ø8 a/b&c~_."), "%C3%988%20a%2Fb%26c~_.");
    }
}
