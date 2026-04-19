//! Interned string tags. A `Tag` is a cheap `Copy` handle to a
//! globally-unique string; two tags produced from the same string are
//! pointer-equal, so equality and hashing stay O(1) on a stable 64-bit
//! `&'static str`.
//!
//! No central enum. Any module (archetype, substance, interaction rule,
//! future plugin) can call `Tag::new("whatever")` and get back a handle.
//! This is the basis for the decentralized body/substance/interaction
//! framework — archetypes declare their properties as tags, rules are
//! keyed on tag pairs, and no one has to edit a central registry file
//! to add a new kind of thing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Interned string handle. Comparing two Tags is a pointer compare; the
/// underlying `&'static str` lives for the rest of the process.
#[derive(Clone, Copy, Debug)]
pub struct Tag(&'static str);

impl Tag {
    /// Look up (or create) the interned handle for `s`.
    pub fn new(s: &str) -> Self {
        let table = intern_table();
        let mut t = table.lock().unwrap();
        if let Some(&existing) = t.get(s) {
            return Tag(existing);
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        t.insert(leaked.to_string(), leaked);
        Tag(leaked)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        // Interning guarantees one canonical `&'static str` per unique
        // string, so pointer equality is sufficient and faster than
        // strcmp. Not all &'static strs are interned by us, though — if
        // someone constructs a `Tag` via an unintended path, this could
        // break. `Tag::new` is the only constructor so we're fine.
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr()) && self.0.len() == other.0.len()
    }
}
impl Eq for Tag {}

impl std::hash::Hash for Tag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // Hash on the pointer, matching the PartialEq definition.
        (self.0.as_ptr() as usize).hash(h);
        self.0.len().hash(h);
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Lexical-sorted tag pair used as a key into the interaction book. The
/// ordering is stable regardless of caller order so a rule for
/// `(flammable, blood)` matches whether the `flammable` thing or the
/// `blood` thing initiated the contact.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TagPair(pub Tag, pub Tag);

impl TagPair {
    pub fn new(a: Tag, b: Tag) -> Self {
        if a.as_str() <= b.as_str() {
            TagPair(a, b)
        } else {
            TagPair(b, a)
        }
    }
}

/// Set of tags carried by a substance / body / entity. Kept as a small
/// `Vec` because typical counts are <8 — a SmallVec would save a heap
/// allocation but isn't worth the dep yet.
#[derive(Default, Clone, Debug)]
pub struct TagSet {
    pub tags: Vec<Tag>,
}

impl TagSet {
    pub fn from_iter<I: IntoIterator<Item = Tag>>(iter: I) -> Self {
        Self { tags: iter.into_iter().collect() }
    }
    pub fn from_strs(strs: &[&str]) -> Self {
        Self { tags: strs.iter().map(|s| Tag::new(s)).collect() }
    }
    pub fn has(&self, t: Tag) -> bool {
        self.tags.iter().any(|&x| x == t)
    }
    pub fn add(&mut self, t: Tag) {
        if !self.has(t) {
            self.tags.push(t);
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = Tag> + '_ {
        self.tags.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.tags.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

fn intern_table() -> &'static Mutex<HashMap<String, &'static str>> {
    static TABLE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_returns_same_handle() {
        let a = Tag::new("flammable");
        let b = Tag::new("flammable");
        assert_eq!(a, b);
        assert_eq!(a.as_str().as_ptr(), b.as_str().as_ptr());
    }

    #[test]
    fn pair_is_order_insensitive() {
        let a = Tag::new("blood");
        let b = Tag::new("on_fire");
        assert_eq!(TagPair::new(a, b), TagPair::new(b, a));
    }
}
