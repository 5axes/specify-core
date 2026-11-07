//! For #150: separate products that are one part moved (four `M3×8 hex bolt #n` with no
//! assembly) are listed with `same_as`, each keeping its own index. The files are made here from
//! `pin.step`: each copy is its whole DATA section renumbered, renamed and translated.
use serde_json::{Value, json};
use specify_core::{api, part::File};
use std::path::Path;

const PIN: &str = include_str!("fixtures/parts/pin.step");

/// `pin.step`'s DATA section, entity by entity.
fn entities() -> Vec<String> {
    let data = PIN.split("DATA;\n").nth(1).unwrap();
    let data = data.split("ENDSEC;").next().unwrap();
    data.split(";\n")
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| e.replace('\n', " "))
        .collect()
}

/// Every `#N` in `entity` moved up by `offset`.
fn renumbered(entity: &str, offset: u64) -> String {
    let mut out = String::new();
    let mut chars = entity.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '#' {
            let mut n = String::new();
            while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n.push(*d);
                chars.next();
            }
            out.push_str(&(n.parse::<u64>().unwrap() + offset).to_string());
        }
    }
    out
}

/// `entity` with the 3D coordinates of its `kind` (a point or direction) mapped by `f`;
/// anything else unchanged.
fn mapped(entity: &str, kind: &str, f: impl Fn([f64; 3]) -> [f64; 3]) -> String {
    let head = format!("{kind}('',(");
    let Some(at) = entity.find(&head) else {
        return entity.to_string();
    };
    let start = at + head.len();
    let end = start + entity[start..].find(')').unwrap();
    let coords: Vec<f64> = entity[start..end]
        .split(',')
        .map(|v| v.trim().parse().unwrap())
        .collect();
    let [x, y, z] = coords[..] else {
        return entity.to_string();
    };
    let xyz: Vec<String> = f([x, y, z]).iter().map(|v| format!("{v:?}")).collect();
    format!("{}{}{}", &entity[..start], xyz.join(","), &entity[end..])
}

/// A 3D `CARTESIAN_POINT` moved by `shift`; anything else unchanged.
fn moved(entity: &str, shift: [f64; 3]) -> String {
    mapped(entity, "CARTESIAN_POINT", |p| {
        [p[0] + shift[0], p[1] + shift[1], p[2] + shift[2]]
    })
}

/// Points and directions turned a quarter turn about x: (x, y, z) to (x, -z, y).
fn turned(entity: String) -> String {
    let quarter = |p: [f64; 3]| [p[0], -p[2], p[1]];
    mapped(
        &mapped(&entity, "CARTESIAN_POINT", quarter),
        "DIRECTION",
        quarter,
    )
}

/// The faces of a `CLOSED_SHELL` listed in reverse order; anything else unchanged.
fn reversed_shell(entity: String) -> String {
    let head = "CLOSED_SHELL('',(";
    let Some(at) = entity.find(head) else {
        return entity;
    };
    let start = at + head.len();
    let end = start + entity[start..].find(')').unwrap();
    let faces: Vec<&str> = entity[start..end].split(',').rev().collect();
    format!("{}{}{}", &entity[..start], faces.join(","), &entity[end..])
}

/// One file of `copies` products, each `pin.step`'s part named `name #n`, moved by its shift,
/// with `edit` applied to each copy's entities.
fn file(copies: &[[f64; 3]], edit: impl Fn(usize, String) -> String) -> Vec<u8> {
    let header = PIN.split("DATA;\n").next().unwrap();
    let mut out = format!("{header}DATA;\n");
    for (n, shift) in copies.iter().enumerate() {
        for e in entities() {
            let e = renumbered(&moved(&e, *shift), 1000 * n as u64);
            let e = e.replace("'COMPOUND'", &format!("'M3x8 hex bolt #{}'", n + 1));
            out.push_str(&edit(n, e));
            out.push_str(";\n");
        }
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out.into_bytes()
}

fn listed(bytes: Vec<u8>) -> Vec<Value> {
    api::parts_of(&File::of_bytes(Path::new("copies.step"), bytes).unwrap())
}

fn same_as(parts: &[Value]) -> Vec<Value> {
    parts.iter().map(|p| p["same_as"].clone()).collect()
}

fn faces_of_first(parts: &[Value]) -> Vec<Value> {
    parts.iter().map(|p| p["faces_of_first"].clone()).collect()
}

/// `pin.step`'s face count.
const FACES: usize = 7;

#[test]
fn translated_copies_point_at_the_first_and_keep_their_indexes() {
    let parts = listed(file(
        &[[0.0; 3], [-67.0, 42.24, 0.0], [30.0, 0.0, 5.5]],
        |_, e| e,
    ));
    assert_eq!(same_as(&parts), [json!(null), json!(0), json!(0)]);
    // Moved only, so each face lands on the first part's face of the same index.
    let identity: Vec<usize> = (0..FACES).collect();
    assert_eq!(
        faces_of_first(&parts),
        [json!(null), json!(identity), json!(identity)]
    );
    let indexes: Vec<_> = parts.iter().map(|p| p["part"].clone()).collect();
    assert_eq!(indexes, [json!(0), json!(1), json!(2)]);
    // The names differ (`#1`..`#3`) and each product is placed once; neither decides it.
    assert_eq!(parts[1]["name"], json!("M3x8 hex bolt #2"));
    assert!(parts.iter().all(|p| p["instances"] == json!(1)));
}

#[test]
fn a_stated_difference_keeps_a_copy_apart() {
    // The third copy states revision B; the first two state nothing.
    let parts = listed(file(
        &[[0.0; 3], [20.0, 0.0, 0.0], [40.0, 0.0, 0.0]],
        |n, e| {
            if n == 2 && e.contains("PRODUCT_DEFINITION_FORMATION(''") {
                e.replacen(
                    "PRODUCT_DEFINITION_FORMATION(''",
                    "PRODUCT_DEFINITION_FORMATION('B'",
                    1,
                )
            } else {
                e
            }
        },
    ));
    assert_eq!(same_as(&parts), [json!(null), json!(0), json!(null)]);
}

#[test]
fn a_different_shape_is_not_a_copy() {
    // The second copy's pin is 1 mm longer at its far end: the same faces, not the same part.
    let parts = listed(file(&[[0.0; 3], [20.0, 0.0, 0.0]], |n, e| {
        if n == 1 && e.contains("CARTESIAN_POINT") && e.contains(",32.0)") {
            e.replace(",32.0)", ",33.0)")
        } else {
            e
        }
    }));
    assert_eq!(parts.len(), 2);
    assert_eq!(same_as(&parts), [json!(null), json!(null)]);
}

#[test]
fn distinct_parts_have_no_same_as() {
    let bytes = std::fs::read("tests/fixtures/parts/assembly.step").unwrap();
    let parts = listed(bytes);
    assert!(parts.len() > 1);
    assert!(parts.iter().all(|p| p["same_as"].is_null()));
}

#[test]
fn a_turned_copy_listing_its_faces_in_reverse_maps_each_face_to_the_first() {
    // The second copy is turned a quarter turn about x, moved, and its shell lists the same
    // faces last to first: its face k is the first part's face FACES - 1 - k.
    let parts = listed(file(&[[0.0; 3], [50.0, 0.0, 0.0]], |n, e| {
        if n == 1 { reversed_shell(turned(e)) } else { e }
    }));
    assert_eq!(same_as(&parts), [json!(null), json!(0)]);
    let reverse: Vec<usize> = (0..FACES).rev().collect();
    assert_eq!(faces_of_first(&parts), [json!(null), json!(reverse)]);
}
