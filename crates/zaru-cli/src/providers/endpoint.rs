// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a provider is reached, for every kind identically.
//!
//! # D5 is built as a missing field
//!
//! [ADR-0012] D5: "Ollama and OpenAI-compatible local servers configure
//! exactly like hosted ones. The sovereignty promise in [ADR-0001] D4 is not
//! credible if the local path is a second-class code path that breaks
//! quietly."
//!
//! [`ProviderEndpoint`] therefore has **one shape and no variant at all** —
//! no second case, no predicate, no flag any code could branch on. There is
//! no second-class code path because there is only one path, and that is a
//! property of the type rather than a claim about the code. It is
//! the same argument [`Contribution`](crate::config::Contribution) makes about
//! a pin: nothing has to check for the distinction because nothing can express
//! it.
//!
//! # No scheme is parsed, and no URL crate is taken
//!
//! An endpoint is text. Nothing here checks that it begins `http`, resolves a
//! host, or is a URL at all — that would be a hand-written URL vocabulary,
//! which is worse than the alternative it saves.
//!
//! **The reason has changed and the decision has not, which is worth saying
//! rather than quietly rewriting.** This paragraph used to add that "this
//! crate already treats a URL as text: `Subject::Url` carries a bare `&str`
//! and validates nothing", and that stopped being true on 2026-09-05 when
//! `web.fetch` gained [`RequestedUrl`](crate::web::RequestedUrl) — a URL that
//! parsed, carrying a scheme that surface retrieves. It also used to say a
//! parser would be a new dependency, and that stopped being true when
//! `reqwest` landed and brought `url` with it.
//!
//! So the cheap arguments are both gone and the endpoint is **still** text,
//! on the argument that survives: an endpoint is not retrieved by this crate.
//! It is a value a user configures, rendered into `zaru models`' listing and
//! into every refusal that names it, and a client composes a path onto it.
//! What is refused is derived from that surface, never from taste — see
//! below. `web.fetch` parses because it **dials** what it was handed, and
//! nothing here dials anything.
//!
//! What *is* refused is derived from a surface, never from taste, exactly as
//! [`Alias`](crate::credentials::Alias) and [`Key`](crate::config::Key) are:
//! an endpoint is rendered into `zaru models`' listing and into every refusal
//! that names it, so a shape those cannot render is refused and nothing else
//! is. **There is no character allowlist and no length cap** — a cap nobody
//! chose is a value chosen for a different caller.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use core::fmt;

/// Why an endpoint was not taken.
///
/// An endpoint is not a credential — [ADR-0014] D4 keeps bearer values out of
/// configuration and the fold refuses a credential-shaped value long before
/// one could reach here — so a refusal quotes it back: a reader has to be able
/// to see which endpoint was rejected.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointRefused {
    /// The endpoint was empty.
    Empty,
    /// The endpoint carried a control character.
    ///
    /// `zaru models` renders an endpoint into a terminal listing, where a
    /// control character can move the cursor or erase a neighbouring row — the
    /// same argument [`AliasRefused::Control`](crate::credentials::AliasRefused::Control)
    /// makes for ADR-0007 D7's listing.
    Control {
        /// The endpoint as it was offered, escaped.
        offered: String,
    },
    /// The endpoint began or ended with whitespace.
    ///
    /// Two endpoints differing only in invisible characters are one endpoint
    /// to every reader of that listing.
    SurroundingWhitespace {
        /// The endpoint as it was offered.
        offered: String,
    },
}

impl fmt::Display for EndpointRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "a provider endpoint is empty; ADR-0012 D5 has every provider configured the same \
                 way and an endpoint naming nowhere configures nothing",
            ),
            Self::Control { offered } => write!(
                f,
                "the endpoint {offered:?} carries a control character; it is rendered into a \
                 terminal listing, where one can erase or overwrite a neighbouring row",
            ),
            Self::SurroundingWhitespace { offered } => write!(
                f,
                "the endpoint {offered:?} begins or ends with whitespace; two endpoints differing \
                 only there are one endpoint to every reader of that listing",
            ),
        }
    }
}

impl std::error::Error for EndpointRefused {}

/// Where a provider is reached.
///
/// **One shape for every one of [ADR-0012] D3's four kinds.** See the module
/// documentation for why there is no local-versus-hosted distinction to be
/// found here.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderEndpoint(String);

impl ProviderEndpoint {
    /// Take an endpoint, refusing the shapes a listing cannot render.
    ///
    /// # Errors
    ///
    /// One variant of [`EndpointRefused`] per shape; the first found is
    /// returned, and the order is fixed so a given input always names the same
    /// reason.
    pub fn new(offered: &str) -> Result<Self, EndpointRefused> {
        if offered.is_empty() {
            return Err(EndpointRefused::Empty);
        }
        if offered.chars().any(char::is_control) {
            return Err(EndpointRefused::Control {
                offered: offered.escape_debug().to_string(),
            });
        }
        if offered.trim() != offered {
            return Err(EndpointRefused::SurroundingWhitespace {
                offered: offered.to_owned(),
            });
        }
        Ok(Self(offered.to_owned()))
    }

    /// The endpoint as it was configured.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
