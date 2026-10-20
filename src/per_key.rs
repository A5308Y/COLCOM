//! A value for every variant of a small enum (skills per `Skill`, priorities
//! per `WorkType`), indexed by the enum itself. Kept in a fixed array, but
//! saved as a map from variant name to value, so adding or reordering
//! variants can't shift what an old save holds: missing kinds get their
//! default.

use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

/// An enum with a fixed list of variants, `ALL[k.index()] == k`.
pub trait Key: Copy + Eq + std::fmt::Debug + Serialize + DeserializeOwned + 'static {
    const ALL: &'static [Self];
    fn index(self) -> usize;
}

/// One `V` per variant of `K`; `N` is `K::ALL.len()`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerKey<K: Key, V, const N: usize> {
    values: [V; N],
    keys: PhantomData<K>,
}

impl<K: Key, V, const N: usize> PerKey<K, V, N> {
    pub fn from_fn(mut f: impl FnMut(K) -> V) -> Self {
        PerKey { values: std::array::from_fn(|i| f(K::ALL[i])), keys: PhantomData }
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> {
        K::ALL.iter().copied().zip(&self.values)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.values.iter_mut()
    }
}

impl<K: Key, V: Copy, const N: usize> PerKey<K, V, N> {
    pub fn filled(v: V) -> Self {
        PerKey { values: [v; N], keys: PhantomData }
    }
}

impl<K: Key, V, const N: usize> std::ops::Index<K> for PerKey<K, V, N> {
    type Output = V;
    fn index(&self, k: K) -> &V {
        &self.values[k.index()]
    }
}

impl<K: Key, V, const N: usize> std::ops::IndexMut<K> for PerKey<K, V, N> {
    fn index_mut(&mut self, k: K) -> &mut V {
        &mut self.values[k.index()]
    }
}

impl<K: Key, V: Serialize, const N: usize> Serialize for PerKey<K, V, N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(N))?;
        for (k, v) in self.iter() {
            map.serialize_entry(&k, v)?;
        }
        map.end()
    }
}

impl<'de, K: Key, V: Deserialize<'de> + Default, const N: usize> Deserialize<'de> for PerKey<K, V, N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Entries<K, V, const N: usize>(PhantomData<(K, V)>);
        impl<'de, K: Key, V: Deserialize<'de> + Default, const N: usize> Visitor<'de> for Entries<K, V, N> {
            type Value = PerKey<K, V, N>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map from kind to value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
                let mut out = PerKey::from_fn(|_| V::default());
                while let Some((k, v)) = entries.next_entry::<K, V>()? {
                    out[k] = v;
                }
                Ok(out)
            }
        }
        d.deserialize_map(Entries(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use crate::pawn::{PerSkill, PerWork, Skill, WorkPriority, WorkType};

    #[test]
    fn all_lists_follow_the_enum_order() {
        for (i, s) in Skill::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, i, "{s:?}");
        }
        for (i, w) in WorkType::ALL.into_iter().enumerate() {
            assert_eq!(w as usize, i, "{w:?}");
        }
    }

    #[test]
    fn saved_by_name_and_tolerant_of_new_kinds() {
        let mut skills = PerSkill::filled(1u8);
        skills[Skill::Mining] = 7;
        let json = serde_json::to_string(&skills).unwrap();
        assert!(json.contains("\"Mining\":7"), "{json}");
        let back: PerSkill<u8> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, skills);
        // An older save that didn't know about a kind of work: it gets the default.
        let old: PerWork<WorkPriority> = serde_json::from_str(r#"{"Mining":1,"Doctor":0}"#).unwrap();
        assert_eq!(old[WorkType::Mining], WorkPriority::First);
        assert_eq!(old[WorkType::Doctor], WorkPriority::Never);
        assert_eq!(old[WorkType::Hauling], WorkPriority::Third);
    }
}
