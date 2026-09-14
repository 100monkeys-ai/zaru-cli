// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D2: the order validators run in, derived from `after` and from
//! nothing else.
//!
//! D2: "`after` names prerequisites… **File-order dependence is a trap:**
//! reordering for readability silently changes behaviour, and the resulting
//! bug is invisible in review."
//!
//! # The order is decided once, at construction, and refusals happen there
//!
//! A [`Plan`] cannot be built out of declarations whose dependencies do not
//! resolve. A cycle, a prerequisite nothing declares and two validators
//! sharing one name are each refused **by name**, before anything runs, which
//! is [Operating principles]' "validate at boundaries" applied to the one
//! place a manifest becomes an execution order.
//!
//! Refusing rather than dropping matters for the same reason [ADR-0014] D5
//! refuses an unknown key: a prerequisite silently ignored is a validator that
//! runs when it should not have, and the manifest still reads as though the
//! ordering holds.
//!
//! # File order is a tie-break and never a dependency
//!
//! Among validators whose prerequisites are all already placed, the one
//! declared earliest goes first. That is deterministic — an order that moved
//! between runs would make the event stream unreadable — and it is **not** the
//! same as file-order dependence: a validator declared before its prerequisite
//! still runs after it. `dependency_order_is_not_file_order` is the check that
//! separates the two, and it declares the dependent first on purpose, because
//! a fixture declaring them in dependency order cannot tell a correct
//! implementation from `sort by declaration index`
//! ([Verification lessons] §9).
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Operating principles]: https://100monkeys-ai.cortex.page/project-management/p/process/operating-principles
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::iteration::validator::declaration::Declared;
use crate::iteration::validator::name::Name;
use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

/// Why a set of declarations is not a runnable order.
///
/// Every variant names the validators involved, because [ADR-0009] D1 puts
/// them in a file the user wrote and a refusal they cannot locate is
/// [ADR-0016] D2's "stack trace with better grammar".
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanRefused {
    /// Two validators were declared with one name.
    ///
    /// Refused rather than resolved by taking one, because `after` names a
    /// prerequisite by name and there is no answer to which of the two it
    /// meant.
    DuplicateName {
        /// The name declared twice.
        name: Name,
    },
    /// A validator's `after` names something nothing declares.
    UnknownPrerequisite {
        /// The validator that declared it.
        validator: Name,
        /// The prerequisite that is not declared.
        missing: Name,
    },
    /// The prerequisites form a cycle.
    ///
    /// **Carries every member**, not the first one found: a cycle a reader has
    /// to trace by hand is a refusal that has told them there is a problem and
    /// not where it is ([Verification lessons] §36). A validator naming itself
    /// is a cycle of one and arrives here.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2
    Cycle {
        /// Every validator caught in it, in declaration order.
        members: Vec<Name>,
    },
}

impl fmt::Display for PlanRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName { name } => write!(
                f,
                "two validators are declared with the name {name:?}; `after` names \
                 a prerequisite by name, and there is no answer to which of the two it means",
            ),
            Self::UnknownPrerequisite { validator, missing } => write!(
                f,
                "the validator {validator:?} declares `after = [… {missing:?} …]` and no \
                 validator is declared with that name; a prerequisite that is silently ignored \
                 is a validator that runs when it should not",
            ),
            Self::Cycle { members } => {
                let names: Vec<&str> = members.iter().map(Name::as_str).collect();
                write!(
                    f,
                    "these {} validators declare prerequisites that form a cycle, so none of \
                     them can run first: {:?}",
                    members.len(),
                    names,
                )
            }
        }
    }
}

impl std::error::Error for PlanRefused {}

/// The declared validators in the order [ADR-0009] D2 puts them.
///
/// Constructed only through [`Plan::from_declared`], so a plan that exists has
/// already had its dependencies resolved.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    ordered: Vec<Declared>,
}

impl Plan {
    /// Resolve declarations into a runnable order.
    ///
    /// A repeated prerequisite — `after = ["build", "build"]` — is one edge
    /// rather than two. It says the same thing twice and refusing it would be
    /// this module inventing a rule the record does not state.
    ///
    /// # Errors
    ///
    /// [`PlanRefused`], naming the validators involved. Duplicate names are
    /// found first, then unknown prerequisites, then cycles, so a given input
    /// always produces the same reason.
    pub fn from_declared(declared: Vec<Declared>) -> Result<Self, PlanRefused> {
        let mut position: BTreeMap<&Name, usize> = BTreeMap::new();
        for (index, one) in declared.iter().enumerate() {
            if position.insert(&one.name, index).is_some() {
                return Err(PlanRefused::DuplicateName {
                    name: one.name.clone(),
                });
            }
        }

        // Distinct prerequisites per validator, by index. Resolved before any
        // ordering happens, so an unknown prerequisite is reported as itself
        // rather than as a cycle nobody can find.
        let mut prerequisites: Vec<BTreeSet<usize>> = Vec::with_capacity(declared.len());
        for one in &declared {
            let mut edges = BTreeSet::new();
            for prerequisite in &one.after {
                let Some(index) = position.get(prerequisite) else {
                    return Err(PlanRefused::UnknownPrerequisite {
                        validator: one.name.clone(),
                        missing: prerequisite.clone(),
                    });
                };
                edges.insert(*index);
            }
            prerequisites.push(edges);
        }

        // Repeatedly take the earliest-declared validator whose prerequisites
        // are all placed. Quadratic, over a list a person typed by hand; the
        // readable version is the right one at this size, and the scan is what
        // makes the tie-break "earliest declared" rather than "whatever the
        // queue happened to hold".
        let mut placed: Vec<bool> = vec![false; declared.len()];
        let mut order: Vec<usize> = Vec::with_capacity(declared.len());
        loop {
            let next = (0..declared.len()).find(|index| {
                !placed[*index]
                    && prerequisites[*index]
                        .iter()
                        .all(|prerequisite| placed[*prerequisite])
            });
            let Some(index) = next else { break };
            placed[index] = true;
            order.push(index);
        }

        if order.len() != declared.len() {
            return Err(PlanRefused::Cycle {
                members: declared
                    .iter()
                    .zip(&placed)
                    .filter(|(_, placed)| !**placed)
                    .map(|(one, _)| one.name.clone())
                    .collect(),
            });
        }

        let mut taken: Vec<Option<Declared>> = declared.into_iter().map(Some).collect();
        let ordered = order
            .into_iter()
            .map(|index| {
                taken[index]
                    .take()
                    .expect("each index is placed exactly once")
            })
            .collect();
        Ok(Self { ordered })
    }

    /// The validators, in the order they run.
    #[must_use]
    pub fn ordered(&self) -> &[Declared] {
        &self.ordered
    }

    /// How many validators are declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ordered.len()
    }

    /// Whether nothing is declared.
    ///
    /// A manifest with no `[[validator]]` block is a real thing — [ADR-0009]
    /// D4's project runs the tool-call loop only — so an empty plan is a plan
    /// rather than a refusal.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ordered.is_empty()
    }

    /// The declared names, in the order they run.
    ///
    /// A reader for checks and for consumers that want the order without the
    /// declarations, so neither has to walk [`Plan::ordered`] and rebuild it.
    pub fn names(&self) -> impl Iterator<Item = &Name> {
        self.ordered.iter().map(|one| &one.name)
    }
}
