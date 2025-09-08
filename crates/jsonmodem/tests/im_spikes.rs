#![cfg(test)]
#![allow(missing_docs)]
//! Spikes exercising `EcoString` and `rpds` persistent containers to understand
//! mutation behaviour.

use ecow::EcoString;
use rpds::{RedBlackTreeMapSync, VectorSync};

#[test]
fn eco_string_clone_is_copy_on_write() {
    let mut original = EcoString::from("hello");
    let mut cloned = original.clone();

    cloned.push_str(" world");
    assert_eq!(
        original.as_str(),
        "hello",
        "mutating the clone must not alter the original"
    );
    assert_eq!(cloned.as_str(), "hello world");

    original.push('!');
    assert_eq!(
        original.as_str(),
        "hello!",
        "original remains mutable and independent"
    );
    assert_eq!(cloned.as_str(), "hello world");
}

#[test]
fn eco_string_inline_capacity_handles_fragment_appends() {
    let mut s = EcoString::from("");
    for _ in 0..4 {
        s.push_str("abcd");
    }
    assert_eq!(s.len(), 16, "appends should extend the EcoString length");
    assert_eq!(s.as_str(), "abcdabcdabcdabcd");
}

#[test]
fn vector_sync_structural_sharing() {
    let base = VectorSync::new_sync();
    let extended = base.push_back(1).push_back(2);

    assert_eq!(extended.len(), 2);
    assert_eq!(
        base.len(),
        0,
        "push_back returns a new vector without mutating the original"
    );

    let mut mutable = extended.clone();
    mutable.push_back_mut(3);
    assert_eq!(mutable.len(), 3);
    assert_eq!(mutable.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);

    assert_eq!(
        extended.iter().collect::<Vec<_>>(),
        vec![&1, &2],
        "mutating the clone must not affect the original vector"
    );
}

#[test]
fn vector_sync_set_mut_updates_in_place() {
    let mut values = VectorSync::new_sync();
    values.push_back_mut(10);
    values.push_back_mut(20);

    {
        let slot = values.get_mut(1).expect("second element present");
        *slot = 99;
    }

    assert_eq!(values.iter().collect::<Vec<_>>(), vec![&10, &99]);
}

#[test]
fn red_black_tree_map_sync_insert_mut_preserves_existing_entries() {
    let mut map = RedBlackTreeMapSync::new_sync();
    map.insert_mut("a", 1);
    map.insert_mut("b", 2);

    {
        let entry = map.get_mut("a").expect("entry exists");
        *entry = 42;
    }

    assert_eq!(map.get("a"), Some(&42));
    assert_eq!(map.get("b"), Some(&2));

    let snapshot_keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(snapshot_keys, vec!["a", "b"]);
}
