// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! How this crate addresses a Nuclear Notes instance and a workspace inside it.
//!
//! # Why a slug and an id are two types
//!
//! [ADR-0006] D7: `me.set_current_workspace` "**rejects** `workspaceSlug`,
//! because slugs are unique per instance rather than globally. Apex callers
//! must pass `workspaceId`." A single `String` for both makes that a rule
//! somebody remembers. Two types make passing the wrong one a compile error,
//! which is the difference between a rule and a mechanism.
//!
//! [`Instance`] exists for the same clause's other half: the resolution call is
//! `workspaces.resolve_slug({slug, instance})`, so a slug is only meaningful
//! beside the instance it is unique within.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces

use core::fmt;

macro_rules! addressing_newtype {
    ($(#[$meta:meta])* $name:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Take ", $what, ".")]
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[doc = concat!("The ", $what, ", as text.")]
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

addressing_newtype!(
    /// A Nuclear Notes instance, by host.
    Instance,
    "an instance host"
);

addressing_newtype!(
    /// A workspace's immutable identifier.
    ///
    /// The only thing [`Session::attach_workspace`](super::Session::attach_workspace)
    /// accepts, per [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces) D7.
    WorkspaceId,
    "a workspace identifier"
);

addressing_newtype!(
    /// A workspace's slug, which is unique **within one instance** and not
    /// globally.
    ///
    /// Never passed to a switch. It is resolved to a [`WorkspaceId`] first.
    WorkspaceSlug,
    "a workspace slug"
);
