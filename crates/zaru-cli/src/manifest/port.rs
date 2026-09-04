// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a manifest comes from, and the writer that does not exist.
//!
//! **Nothing in this crate's product tree implements [`ManifestSource`]**,
//! exactly as nothing implements [`LayerSource`](crate::config::LayerSource),
//! [`SecretStore`](crate::credentials::SecretStore), any of `zaru-core`'s five
//! loop ports or any of its three validator ports. A check implements it; the
//! product does not.
//!
//! # Why it stops here
//!
//! Reading `./zaru.toml` needs a TOML parser, and [ADR-0003] D2's table names
//! none — its Trigger clause 7 treats that table as closed in the other
//! direction too, so declaring one is an amendment to that record rather than
//! an import. A third proposed amendment is drafted there naming `toml` for
//! exactly this file and for [ADR-0014] D1's layers 2 and 3. It is proposed and
//! **not accepted**, so the honest shape is a declared seam with no
//! implementation.
//!
//! # There is one method and it reads
//!
//! [ADR-0009] D6: "**The manifest is read, never written.** Zaru does not edit
//! `zaru.toml` on the user's behalf." So this trait has no `write`, there is no
//! `ManifestWriter`, and [`Manifest`] has no method taking a path. The
//! forbidden act has nothing to call, which is absence rather than refusal —
//! the shape [ADR-0011]'s closed built-in set uses, and the one
//! `no_product_source_writes_a_manifest` holds from the other side.
//!
//! D6's other half — "`zaru init` writes it once, on an explicit command, only
//! when absent" — is [ADR-0015] D2's command surface and is **not built**. When
//! it is, it is a writer that exists in exactly one place and that check is
//! what will make adding it a visible act.
//!
//! # One file, one reader
//!
//! `./zaru.toml` is also [ADR-0014] D1's layer 3, so a `LayerSource` for that
//! layer would be a second parse of the same bytes. The shape that avoids it:
//! this port returns a whole [`Manifest`], and layer 3's contribution is
//! derived from it by [`Manifest::contribution`]. Whatever implements layer 3
//! later is an adapter over this rather than a second reader.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::{Source, SourceFailure};
use crate::manifest::document::Manifest;

/// Somewhere a project's `zaru.toml` can be read from.
///
/// **There is no counterpart that writes one.** See the module documentation.
pub trait ManifestSource {
    /// What [ADR-0014] D3's explain block calls this file.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    fn source(&self) -> Source;

    /// The manifest, or `None` where the project has no `zaru.toml`.
    ///
    /// An absent manifest is `Ok(None)` rather than an error, because
    /// [ADR-0009] D4 makes a project without one an ordinary thing that runs
    /// the tool-call loop and is told so once. A reader that errored would make
    /// D4 unreachable.
    ///
    /// # Errors
    ///
    /// [`SourceFailure`] when the file exists and cannot be read or parsed.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    fn read(&self) -> Result<Option<Manifest>, SourceFailure>;
}
