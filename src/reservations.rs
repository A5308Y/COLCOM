//! Cell lookup plus an owner index: releasing a job touches only its reservations.

use crate::pawn::PawnId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::ops::Index;

#[derive(Clone, Debug)]
pub struct Reservations {
    cells: Vec<Option<PawnId>>,
    owners: BTreeMap<PawnId, Vec<usize>>,
}

impl PartialEq for Reservations {
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells
    }
}

impl Reservations {
    pub fn new(len: usize) -> Self {
        Self { cells: vec![None; len], owners: BTreeMap::new() }
    }

    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }

    pub(crate) fn set(&mut self, cell: usize, owner: Option<PawnId>) {
        if self.cells[cell] == owner {
            return;
        }
        if let Some(old) = self.cells[cell] {
            let cells = self.owners.get_mut(&old).unwrap();
            cells.retain(|&i| i != cell);
            if cells.is_empty() {
                self.owners.remove(&old);
            }
        }
        self.cells[cell] = owner;
        if let Some(id) = owner {
            self.owners.entry(id).or_default().push(cell);
        }
    }

    pub(crate) fn release(&mut self, owner: PawnId) {
        if let Some(cells) = self.owners.remove(&owner) {
            for i in cells {
                self.cells[i] = None;
            }
        }
    }
}

impl Index<usize> for Reservations {
    type Output = Option<PawnId>;
    fn index(&self, i: usize) -> &Self::Output {
        &self.cells[i]
    }
}

// Keep the existing run-length encoded save representation. The owner index
// is derived on every deserialize, including worlds nested in expeditions.
impl Serialize for Reservations {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        crate::save::rle::serialize(&self.cells, s)
    }
}
impl<'de> Deserialize<'de> for Reservations {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let cells: Vec<Option<PawnId>> = crate::save::rle::deserialize(d)?;
        let mut owners: BTreeMap<PawnId, Vec<usize>> = BTreeMap::new();
        for (i, id) in cells.iter().enumerate() {
            if let Some(id) = id {
                owners.entry(*id).or_default().push(i);
            }
        }
        Ok(Self { cells, owners })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_after_transfer_clear_and_load() {
        let (a, b) = (PawnId::new(1), PawnId::new(2));
        let mut r = Reservations::new(100);
        r.set(5, Some(a));
        r.set(6, Some(a));
        r.set(5, Some(b));
        r.set(6, None);
        r.set(7, Some(a));
        r.set(7, Some(a));
        let json = serde_json::to_string(&r).unwrap();
        let mut r: Reservations = serde_json::from_str(&json).unwrap();
        r.release(a);
        assert_eq!(r[7], None);
        assert_eq!(r[5], Some(b));
        r.release(b);
        assert!(r.cells.iter().all(Option::is_none));
        assert!(r.owners.is_empty());
    }
}
