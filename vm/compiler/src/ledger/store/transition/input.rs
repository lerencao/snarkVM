// Copyright (C) 2019-2022 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    atomic_write_batch,
    ledger::{
        map::{memory_map::MemoryMap, Map, MapRead},
        transition::{Input, Origin},
    },
};
use console::{
    network::prelude::*,
    program::{Ciphertext, Plaintext},
    types::Field,
};

use anyhow::Result;
use std::borrow::Cow;

/// A trait for transition input storage.
pub trait InputStorage<N: Network>: Clone + Send + Sync {
    /// The mapping of `transition ID` to `input IDs`.
    type IDMap: for<'a> Map<'a, N::TransitionID, Vec<Field<N>>>;
    /// The mapping of `input ID` to `transition ID`.
    type ReverseIDMap: for<'a> Map<'a, Field<N>, N::TransitionID>;
    /// The mapping of `plaintext hash` to `(optional) plaintext`.
    type ConstantMap: for<'a> Map<'a, Field<N>, Option<Plaintext<N>>>;
    /// The mapping of `plaintext hash` to `(optional) plaintext`.
    type PublicMap: for<'a> Map<'a, Field<N>, Option<Plaintext<N>>>;
    /// The mapping of `ciphertext hash` to `(optional) ciphertext`.
    type PrivateMap: for<'a> Map<'a, Field<N>, Option<Ciphertext<N>>>;
    /// The mapping of `serial number` to `(tag, origin)`.
    type RecordMap: for<'a> Map<'a, Field<N>, (Field<N>, Origin<N>)>;
    /// The mapping of `tag` to `serial number`.
    type RecordTagMap: for<'a> Map<'a, Field<N>, Field<N>>;
    /// The mapping of `external hash` to `()`. Note: This is **not** the record commitment.
    type ExternalRecordMap: for<'a> Map<'a, Field<N>, ()>;

    /// Initializes the transition input storage.
    fn open(dev: Option<u16>) -> Result<Self>;

    /// Returns the ID map.
    fn id_map(&self) -> &Self::IDMap;
    /// Returns the reverse ID map.
    fn reverse_id_map(&self) -> &Self::ReverseIDMap;
    /// Returns the constant map.
    fn constant_map(&self) -> &Self::ConstantMap;
    /// Returns the public map.
    fn public_map(&self) -> &Self::PublicMap;
    /// Returns the private map.
    fn private_map(&self) -> &Self::PrivateMap;
    /// Returns the record map.
    fn record_map(&self) -> &Self::RecordMap;
    /// Returns the record tag map.
    fn record_tag_map(&self) -> &Self::RecordTagMap;
    /// Returns the external record map.
    fn external_record_map(&self) -> &Self::ExternalRecordMap;

    /// Returns the optional development ID.
    fn dev(&self) -> Option<u16>;

    /// Starts an atomic batch write operation.
    fn start_atomic(&self) {
        self.id_map().start_atomic();
        self.reverse_id_map().start_atomic();
        self.constant_map().start_atomic();
        self.public_map().start_atomic();
        self.private_map().start_atomic();
        self.record_map().start_atomic();
        self.record_tag_map().start_atomic();
        self.external_record_map().start_atomic();
    }

    /// Checks if an atomic batch is in progress.
    fn is_atomic_in_progress(&self) -> bool {
        self.id_map().is_atomic_in_progress()
            || self.reverse_id_map().is_atomic_in_progress()
            || self.constant_map().is_atomic_in_progress()
            || self.public_map().is_atomic_in_progress()
            || self.private_map().is_atomic_in_progress()
            || self.record_map().is_atomic_in_progress()
            || self.record_tag_map().is_atomic_in_progress()
            || self.external_record_map().is_atomic_in_progress()
    }

    /// Aborts an atomic batch write operation.
    fn abort_atomic(&self) {
        self.id_map().abort_atomic();
        self.reverse_id_map().abort_atomic();
        self.constant_map().abort_atomic();
        self.public_map().abort_atomic();
        self.private_map().abort_atomic();
        self.record_map().abort_atomic();
        self.record_tag_map().abort_atomic();
        self.external_record_map().abort_atomic();
    }

    /// Finishes an atomic batch write operation.
    fn finish_atomic(&self) -> Result<()> {
        self.id_map().finish_atomic()?;
        self.reverse_id_map().finish_atomic()?;
        self.constant_map().finish_atomic()?;
        self.public_map().finish_atomic()?;
        self.private_map().finish_atomic()?;
        self.record_map().finish_atomic()?;
        self.record_tag_map().finish_atomic()?;
        self.external_record_map().finish_atomic()
    }

    /// Stores the given `(transition ID, input)` pair into storage.
    fn insert(&self, transition_id: N::TransitionID, inputs: &[Input<N>]) -> Result<()> {
        atomic_write_batch!(self, {
            // Store the input IDs.
            self.id_map().insert(transition_id, inputs.iter().map(Input::id).copied().collect())?;

            // Store the inputs.
            for input in inputs {
                // Store the reverse input ID.
                self.reverse_id_map().insert(*input.id(), transition_id)?;
                // Store the input.
                match input.clone() {
                    Input::Constant(input_id, constant) => self.constant_map().insert(input_id, constant)?,
                    Input::Public(input_id, public) => self.public_map().insert(input_id, public)?,
                    Input::Private(input_id, private) => self.private_map().insert(input_id, private)?,
                    Input::Record(serial_number, tag, origin) => {
                        // Store the record tag.
                        self.record_tag_map().insert(tag, serial_number)?;
                        // Store the record.
                        self.record_map().insert(serial_number, (tag, origin))?
                    }
                    Input::ExternalRecord(input_id) => self.external_record_map().insert(input_id, ())?,
                }
            }

            Ok(())
        });

        Ok(())
    }

    /// Removes the input for the given `transition ID`.
    fn remove(&self, transition_id: &N::TransitionID) -> Result<()> {
        // Retrieve the input IDs.
        let input_ids: Vec<_> = match self.id_map().get(transition_id)? {
            Some(Cow::Borrowed(ids)) => ids.to_vec(),
            Some(Cow::Owned(ids)) => ids.into_iter().collect(),
            None => return Ok(()),
        };

        atomic_write_batch!(self, {
            // Remove the input IDs.
            self.id_map().remove(transition_id)?;

            // Remove the inputs.
            for input_id in input_ids {
                // Remove the reverse input ID.
                self.reverse_id_map().remove(&input_id)?;

                // If the input is a record, remove the record tag.
                if let Some(record) = self.record_map().get(&input_id)? {
                    self.record_tag_map().remove(&record.0)?;
                }

                // Remove the input.
                self.constant_map().remove(&input_id)?;
                self.public_map().remove(&input_id)?;
                self.private_map().remove(&input_id)?;
                self.record_map().remove(&input_id)?;
                self.external_record_map().remove(&input_id)?;
            }

            Ok(())
        });

        Ok(())
    }

    /// Returns the transition ID that contains the given `input ID`.
    fn find_transition_id(&self, input_id: &Field<N>) -> Result<Option<N::TransitionID>> {
        match self.reverse_id_map().get(input_id)? {
            Some(Cow::Borrowed(transition_id)) => Ok(Some(*transition_id)),
            Some(Cow::Owned(transition_id)) => Ok(Some(transition_id)),
            None => Ok(None),
        }
    }

    /// Returns the input IDs for the given `transition ID`.
    fn get_ids(&self, transition_id: &N::TransitionID) -> Result<Vec<Field<N>>> {
        // Retrieve the input IDs.
        match self.id_map().get(transition_id)? {
            Some(Cow::Borrowed(inputs)) => Ok(inputs.to_vec()),
            Some(Cow::Owned(inputs)) => Ok(inputs),
            None => Ok(vec![]),
        }
    }

    /// Returns the input for the given `transition ID`.
    fn get(&self, transition_id: &N::TransitionID) -> Result<Vec<Input<N>>> {
        // Constructs the input given the input ID and input value.
        macro_rules! into_input {
            (Input::Record($input_id:ident, $input:expr)) => {
                match $input {
                    Cow::Borrowed((tag, origin)) => Input::Record($input_id, *tag, *origin),
                    Cow::Owned((tag, origin)) => Input::Record($input_id, tag, origin),
                }
            };
            (Input::$Variant:ident($input_id:ident, $input:expr)) => {
                match $input {
                    Cow::Borrowed(input) => Input::$Variant($input_id, input.clone()),
                    Cow::Owned(input) => Input::$Variant($input_id, input),
                }
            };
        }

        // A helper function to construct the input given the input ID.
        let construct_input = |input_id| {
            let constant = self.constant_map().get(&input_id)?;
            let public = self.public_map().get(&input_id)?;
            let private = self.private_map().get(&input_id)?;
            let record = self.record_map().get(&input_id)?;
            let external_record = self.external_record_map().get(&input_id)?;

            // Retrieve the input.
            let input = match (constant, public, private, record, external_record) {
                (Some(constant), None, None, None, None) => into_input!(Input::Constant(input_id, constant)),
                (None, Some(public), None, None, None) => into_input!(Input::Public(input_id, public)),
                (None, None, Some(private), None, None) => into_input!(Input::Private(input_id, private)),
                (None, None, None, Some(record), None) => into_input!(Input::Record(input_id, record)),
                (None, None, None, None, Some(_)) => Input::ExternalRecord(input_id),
                (None, None, None, None, None) => bail!("Missing input '{input_id}' in transition '{transition_id}'"),
                _ => bail!("Found multiple inputs for the input ID '{input_id}' in transition '{transition_id}'"),
            };

            Ok(input)
        };

        // Retrieve the input IDs.
        match self.id_map().get(transition_id)? {
            Some(Cow::Borrowed(ids)) => ids.iter().map(|input_id| construct_input(*input_id)).collect(),
            Some(Cow::Owned(ids)) => ids.iter().map(|input_id| construct_input(*input_id)).collect(),
            None => Ok(vec![]),
        }
    }
}

/// An in-memory transition input storage.
#[derive(Clone)]
pub struct InputMemory<N: Network> {
    /// The mapping of `transition ID` to `input IDs`.
    id_map: MemoryMap<N::TransitionID, Vec<Field<N>>>,
    /// The mapping of `input ID` to `transition ID`.
    reverse_id_map: MemoryMap<Field<N>, N::TransitionID>,
    /// The mapping of `plaintext hash` to `(optional) plaintext`.
    constant: MemoryMap<Field<N>, Option<Plaintext<N>>>,
    /// The mapping of `plaintext hash` to `(optional) plaintext`.
    public: MemoryMap<Field<N>, Option<Plaintext<N>>>,
    /// The mapping of `ciphertext hash` to `(optional) ciphertext`.
    private: MemoryMap<Field<N>, Option<Ciphertext<N>>>,
    /// The mapping of `serial number` to `(tag, origin)`.
    record: MemoryMap<Field<N>, (Field<N>, Origin<N>)>,
    /// The mapping of `record tag` to `serial number`.
    record_tag: MemoryMap<Field<N>, Field<N>>,
    /// The mapping of `external hash` to `()`. Note: This is **not** the record commitment.
    external_record: MemoryMap<Field<N>, ()>,
    /// The optional development ID.
    dev: Option<u16>,
}

#[rustfmt::skip]
impl<N: Network> InputStorage<N> for InputMemory<N> {
    type IDMap = MemoryMap<N::TransitionID, Vec<Field<N>>>;
    type ReverseIDMap = MemoryMap<Field<N>, N::TransitionID>;
    type ConstantMap = MemoryMap<Field<N>, Option<Plaintext<N>>>;
    type PublicMap = MemoryMap<Field<N>, Option<Plaintext<N>>>;
    type PrivateMap = MemoryMap<Field<N>, Option<Ciphertext<N>>>;
    type RecordMap = MemoryMap<Field<N>, (Field<N>, Origin<N>)>;
    type RecordTagMap = MemoryMap<Field<N>, Field<N>>;
    type ExternalRecordMap = MemoryMap<Field<N>, ()>;

    /// Initializes the transition input storage.
    fn open(dev: Option<u16>) -> Result<Self> {
        Ok(Self {
            id_map: MemoryMap::default(),
            reverse_id_map: MemoryMap::default(),
            constant: MemoryMap::default(),
            public: MemoryMap::default(),
            private: MemoryMap::default(),
            record: MemoryMap::default(),
            record_tag: MemoryMap::default(),
            external_record: MemoryMap::default(),
            dev,
        })
    }

    /// Returns the ID map.
    fn id_map(&self) -> &Self::IDMap {
        &self.id_map
    }

    /// Returns the reverse ID map.
    fn reverse_id_map(&self) -> &Self::ReverseIDMap {
        &self.reverse_id_map
    }

    /// Returns the constant map.
    fn constant_map(&self) -> &Self::ConstantMap {
        &self.constant
    }

    /// Returns the public map.
    fn public_map(&self) -> &Self::PublicMap {
        &self.public
    }

    /// Returns the private map.
    fn private_map(&self) -> &Self::PrivateMap {
        &self.private
    }

    /// Returns the record map.
    fn record_map(&self) -> &Self::RecordMap {
        &self.record
    }

    /// Returns the record tag map.
    fn record_tag_map(&self) -> &Self::RecordTagMap {
        &self.record_tag
    }

    /// Returns the external record map.
    fn external_record_map(&self) -> &Self::ExternalRecordMap {
        &self.external_record
    }

    /// Returns the optional development ID.
    fn dev(&self) -> Option<u16> {
        self.dev
    }
}

/// The transition input store.
#[derive(Clone)]
pub struct InputStore<N: Network, I: InputStorage<N>> {
    /// The map of constant inputs.
    constant: I::ConstantMap,
    /// The map of public inputs.
    public: I::PublicMap,
    /// The map of private inputs.
    private: I::PrivateMap,
    /// The map of record inputs.
    record: I::RecordMap,
    /// The map of record tags.
    record_tag: I::RecordTagMap,
    /// The map of external record inputs.
    external_record: I::ExternalRecordMap,
    /// The input storage.
    storage: I,
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Initializes the transition input store.
    pub fn open(dev: Option<u16>) -> Result<Self> {
        // Initialize a new transition input storage.
        let storage = I::open(dev)?;
        // Return the transition input store.
        Ok(Self {
            constant: storage.constant_map().clone(),
            public: storage.public_map().clone(),
            private: storage.private_map().clone(),
            record: storage.record_map().clone(),
            record_tag: storage.record_tag_map().clone(),
            external_record: storage.external_record_map().clone(),
            storage,
        })
    }

    /// Initializes a transition input store from storage.
    pub fn from(storage: I) -> Self {
        Self {
            constant: storage.constant_map().clone(),
            public: storage.public_map().clone(),
            private: storage.private_map().clone(),
            record: storage.record_map().clone(),
            record_tag: storage.record_tag_map().clone(),
            external_record: storage.external_record_map().clone(),
            storage,
        }
    }

    /// Stores the given `(transition ID, input)` pair into storage.
    pub fn insert(&self, transition_id: N::TransitionID, inputs: &[Input<N>]) -> Result<()> {
        self.storage.insert(transition_id, inputs)
    }

    /// Removes the input for the given `transition ID`.
    pub fn remove(&self, transition_id: &N::TransitionID) -> Result<()> {
        self.storage.remove(transition_id)
    }

    /// Starts an atomic batch write operation.
    pub fn start_atomic(&self) {
        self.storage.start_atomic();
    }

    /// Checks if an atomic batch is in progress.
    pub fn is_atomic_in_progress(&self) -> bool {
        self.storage.is_atomic_in_progress()
    }

    /// Aborts an atomic batch write operation.
    pub fn abort_atomic(&self) {
        self.storage.abort_atomic();
    }

    /// Finishes an atomic batch write operation.
    pub fn finish_atomic(&self) -> Result<()> {
        self.storage.finish_atomic()
    }

    /// Returns the optional development ID.
    pub fn dev(&self) -> Option<u16> {
        self.storage.dev()
    }
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Returns the input IDs for the given `transition ID`.
    pub fn get_input_ids(&self, transition_id: &N::TransitionID) -> Result<Vec<Field<N>>> {
        self.storage.get_ids(transition_id)
    }

    /// Returns the inputs for the given `transition ID`.
    pub fn get_inputs(&self, transition_id: &N::TransitionID) -> Result<Vec<Input<N>>> {
        self.storage.get(transition_id)
    }
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Returns the transition ID that contains the given `input ID`.
    pub fn find_transition_id(&self, input_id: &Field<N>) -> Result<Option<N::TransitionID>> {
        self.storage.find_transition_id(input_id)
    }
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Returns `true` if the given input ID exists.
    pub fn contains_input_id(&self, input_id: &Field<N>) -> Result<bool> {
        self.storage.reverse_id_map().contains_key(input_id)
    }

    /// Returns `true` if the given serial number exists.
    pub fn contains_serial_number(&self, serial_number: &Field<N>) -> Result<bool> {
        self.record.contains_key(serial_number)
    }

    /// Returns `true` if the given tag exists.
    pub fn contains_tag(&self, tag: &Field<N>) -> Result<bool> {
        self.record_tag.contains_key(tag)
    }
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Returns an iterator over the input IDs, for all transition inputs.
    pub fn input_ids(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.storage.reverse_id_map().keys()
    }

    /// Returns an iterator over the constant input IDs, for all transition inputs that are constant.
    pub fn constant_input_ids(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.constant.keys()
    }

    /// Returns an iterator over the public input IDs, for all transition inputs that are public.
    pub fn public_input_ids(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.public.keys()
    }

    /// Returns an iterator over the private input IDs, for all transition inputs that are private.
    pub fn private_input_ids(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.private.keys()
    }

    /// Returns an iterator over the serial numbers, for all transition inputs that are records.
    pub fn serial_numbers(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.record.keys()
    }

    /// Returns an iterator over the external record input IDs, for all transition inputs that are external records.
    pub fn external_input_ids(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.external_record.keys()
    }
}

impl<N: Network, I: InputStorage<N>> InputStore<N, I> {
    /// Returns an iterator over the constant inputs, for all transitions.
    pub fn constant_inputs(&self) -> impl '_ + Iterator<Item = Cow<'_, Plaintext<N>>> {
        self.constant.values().flat_map(|input| match input {
            Cow::Borrowed(Some(input)) => Some(Cow::Borrowed(input)),
            Cow::Owned(Some(input)) => Some(Cow::Owned(input)),
            _ => None,
        })
    }

    /// Returns an iterator over the constant inputs, for all transitions.
    pub fn public_inputs(&self) -> impl '_ + Iterator<Item = Cow<'_, Plaintext<N>>> {
        self.public.values().flat_map(|input| match input {
            Cow::Borrowed(Some(input)) => Some(Cow::Borrowed(input)),
            Cow::Owned(Some(input)) => Some(Cow::Owned(input)),
            _ => None,
        })
    }

    /// Returns an iterator over the private inputs, for all transitions.
    pub fn private_inputs(&self) -> impl '_ + Iterator<Item = Cow<'_, Ciphertext<N>>> {
        self.private.values().flat_map(|input| match input {
            Cow::Borrowed(Some(input)) => Some(Cow::Borrowed(input)),
            Cow::Owned(Some(input)) => Some(Cow::Owned(input)),
            _ => None,
        })
    }

    /// Returns an iterator over the tags, for all transition inputs that are records.
    pub fn tags(&self) -> impl '_ + Iterator<Item = Cow<'_, Field<N>>> {
        self.record_tag.keys()
    }

    /// Returns an iterator over the origins, for all transition inputs that are records.
    pub fn origins(&self) -> impl '_ + Iterator<Item = Cow<'_, Origin<N>>> {
        self.record.values().map(|input| match input {
            Cow::Borrowed((_, origin)) => Cow::Borrowed(origin),
            Cow::Owned((_, origin)) => Cow::Owned(origin),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_get_remove() {
        // Sample the transition inputs.
        for (transition_id, input) in crate::ledger::transition::input::test_helpers::sample_inputs() {
            // Initialize a new input store.
            let input_store = InputMemory::open(None).unwrap();

            // Ensure the transition input does not exist.
            let candidate = input_store.get(&transition_id).unwrap();
            assert!(candidate.is_empty());

            // Insert the transition input.
            input_store.insert(transition_id, &[input.clone()]).unwrap();

            // Retrieve the transition input.
            let candidate = input_store.get(&transition_id).unwrap();
            assert_eq!(vec![input.clone()], candidate);

            // Remove the transition input.
            input_store.remove(&transition_id).unwrap();

            // Retrieve the transition input.
            let candidate = input_store.get(&transition_id).unwrap();
            assert!(candidate.is_empty());
        }
    }

    #[test]
    fn test_find_transition_id() {
        // Sample the transition inputs.
        for (transition_id, input) in crate::ledger::transition::input::test_helpers::sample_inputs() {
            // Initialize a new input store.
            let input_store = InputMemory::open(None).unwrap();

            // Ensure the transition input does not exist.
            let candidate = input_store.get(&transition_id).unwrap();
            assert!(candidate.is_empty());

            // Ensure the transition ID is not found.
            let candidate = input_store.find_transition_id(input.id()).unwrap();
            assert!(candidate.is_none());

            // Insert the transition input.
            input_store.insert(transition_id, &[input.clone()]).unwrap();

            // Find the transition ID.
            let candidate = input_store.find_transition_id(input.id()).unwrap();
            assert_eq!(Some(transition_id), candidate);

            // Remove the transition input.
            input_store.remove(&transition_id).unwrap();

            // Ensure the transition ID is not found.
            let candidate = input_store.find_transition_id(input.id()).unwrap();
            assert!(candidate.is_none());
        }
    }
}
