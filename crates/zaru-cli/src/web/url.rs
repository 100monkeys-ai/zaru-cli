// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a URL has to be before anything is asked about it, and which
//! destinations this surface will not reach.
//!
//! # The parse happens before the permission decision
//!
//! [`crate::process::line::CommandLine::split`] is the precedent and the
//! reason is the same one it gives: [ADR-0011] D4's transcript entry and D3's
//! prompt both show the target, and **a target that has not been extracted
//! from the arguments is not yet one**. So
//! [`Executor::execute`](crate::tools::Executor) parses here, in the
//! `web.fetch` arm, exactly where it splits in the `cmd.run` arm — and
//! [`Subject::Url`](crate::tools::Subject) carries a [`RequestedUrl`] rather
//! than a `&str`, so the line a transcript shows is a URL that parsed.
//!
//! That is what makes [ADR-0011] clause 1's "with their arguments" met
//! **exactly** for `web.fetch`, as it already is for `cmd.run` and
//! `fs.search`: the rendered subject *is* the whole argument.
//!
//! # No URL parser is written here
//!
//! [`reqwest::Url`] is the `url` crate's type, re-exported, and it has been in
//! this crate's build closure since the first provider client. Writing a
//! second parser would be the security-vocabulary authoring the autonomy
//! boundary forbids, and worse than that: a parser that disagreed with the one
//! that will actually issue the request is exactly the defect
//! `CommandLine::split` exists to prevent, where what is checked and what is
//! run are two readings of one string. **No dependency arrives.**
//!
//! # Two refusals, at two different places, and the difference is not cosmetic
//!
//! A **scheme this surface cannot retrieve** is [`UrlRefused`], raised here,
//! before the decision — it is the model having asked for something that is
//! not a call, so nothing is recorded and nothing is prompted, in the shape
//! [`NotACall`](crate::tools::NotACall) already holds.
//!
//! A **destination this surface will not reach** is [`Destinations`], applied
//! by [`crate::web::client`] during the act — because a well-formed `GET` to
//! `http://169.254.169.254/` *is* a call this surface could make and declines
//! to. [ADR-0011] D4's "mode may remove the prompt; it never removes the
//! record" is about a call, and a model reaching for a metadata endpoint is
//! precisely what a user should be able to find in their transcript
//! afterwards.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use core::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// The two schemes `web.fetch` retrieves.
///
/// [ADR-0011] D1's row is "Retrieve a URL", and retrieval over anything else
/// is a different capability wearing this one's name. Transcribed as a
/// constant so a refusal and the set it is refusing against are one list.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const RETRIEVABLE_SCHEMES: [&str; 2] = ["http", "https"];

/// Why some text is not a URL this surface can retrieve.
///
/// **No variant renders anything but the scheme and the parser's own
/// positional complaint.** A URL can carry a query string somebody pasted a
/// token into, so a refusal that echoed the whole thing would publish it into
/// the one place that reliably gets copied into a bug report — the rule
/// [`FileRefused`](crate::config::FileRefused) and
/// [`ArgumentsRefused`](crate::tools::ArgumentsRefused) already hold one and
/// two layers down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlRefused {
    /// The text did not parse as a URL at all.
    NotAUrl {
        /// The parser's own complaint. It names a syntactic position and
        /// never the input — checked by `a_refusal_never_renders_the_url`.
        because: String,
    },
    /// The URL parsed and its scheme is not one this surface retrieves.
    SchemeNotRetrievable {
        /// The scheme, escaped. Never the rest of the URL.
        scheme: String,
    },
    /// The URL parsed, carries a retrievable scheme, and names no host.
    NoHost,
}

impl fmt::Display for UrlRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAUrl { because } => write!(
                f,
                "the url given to web.fetch is not a URL: {because}. ADR-0011 D1's row for \
                 web.fetch is \"Retrieve a URL\", so there is nothing to retrieve"
            ),
            Self::SchemeNotRetrievable { scheme } => write!(
                f,
                "web.fetch retrieves {} and not {scheme:?}. A {scheme:?} URL is not a retrieval: \
                 in particular a \"file\" one would read the filesystem without ADR-0011 D4's \
                 working-directory boundary, which is fs.read without the one rule fs.read has",
                RETRIEVABLE_SCHEMES.join(" and "),
            ),
            Self::NoHost => f.write_str(
                "the url given to web.fetch names no host, so there is nothing to retrieve it \
                 from",
            ),
        }
    }
}

impl std::error::Error for UrlRefused {}

/// A URL that parsed and carries a scheme this surface retrieves.
///
/// There is no other constructor, so a value of this type has been through
/// [`Self::parse`] — which is what lets
/// [`Subject::Url`](crate::tools::Subject) mean "a URL" rather than "some text
/// nobody has looked at".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedUrl {
    inner: reqwest::Url,
}

impl RequestedUrl {
    /// Read text as a URL this surface could retrieve.
    ///
    /// # Errors
    ///
    /// [`UrlRefused`] when the text is not a URL, carries a scheme other than
    /// `http` or `https`, or names no host.
    pub fn parse(text: &str) -> Result<Self, UrlRefused> {
        let inner = reqwest::Url::parse(text).map_err(|because| UrlRefused::NotAUrl {
            because: because.to_string(),
        })?;
        if !RETRIEVABLE_SCHEMES.contains(&inner.scheme()) {
            return Err(UrlRefused::SchemeNotRetrievable {
                scheme: inner.scheme().escape_debug().to_string(),
            });
        }
        if inner.host_str().is_none() {
            return Err(UrlRefused::NoHost);
        }
        Ok(Self { inner })
    }

    /// The URL as the request will be made to it.
    ///
    /// This is the URL's own serialisation rather than the text that arrived,
    /// so that what the transcript shows and what is dialled are one string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    /// The parsed URL, for the client that will dial it.
    pub(crate) const fn inner(&self) -> &reqwest::Url {
        &self.inner
    }
}

impl fmt::Display for RequestedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a destination was not reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusedDestination {
    /// The host is this machine.
    ThisMachine {
        /// The host as the URL spells it.
        host: String,
    },
    /// The host is in a link-local range.
    LinkLocal {
        /// The host as the URL spells it.
        host: String,
    },
}

impl fmt::Display for RefusedDestination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThisMachine { host } => write!(
                f,
                "web.fetch does not retrieve from {host}, which is this machine. A service \
                 listening on the loopback interface is one the user started for themselves and \
                 did not publish, and web.fetch is the one built-in that acts on a model's own \
                 choice with no prompt at the default permission mode"
            ),
            Self::LinkLocal { host } => write!(
                f,
                "web.fetch does not retrieve from {host}, which is link-local. 169.254.169.254 \
                 is the instance-metadata address on every major cloud, served over plain HTTP \
                 with no authentication, so it is a credential source rather than a document \
                 source"
            ),
        }
    }
}

impl std::error::Error for RefusedDestination {}

/// Which destinations a client may reach.
///
/// # Why there is a rule here at all, when D2 says the harness is not a sandbox
///
/// It does **not** make the harness a sandbox and nothing here claims it does:
/// [ADR-0011] D2 says `bare` is advisory, [ADR-0001] D1 gives `bare` no
/// membrane, and `cmd.run` can reach every destination this type refuses. What
/// the rule buys is narrower, and it is derived from the record's own
/// asymmetry rather than asserted:
///
/// **`web.fetch` is the only one of the seven built-ins that acts on a model's
/// choice with no prompt at the default mode.**
/// [`Effect::prompts_in_ask`](crate::tools::Effect::prompts_in_ask) returns
/// `false` for [`Effect::Retrieve`](crate::tools::Effect::Retrieve), because
/// D3's `ask` row is "prompts before any write or command" and a retrieval is
/// neither; `cmd.run` is [`Effect::Command`](crate::tools::Effect::Command)
/// and prompts. So this is the one built-in where a structural rule is the
/// only thing between a model-chosen destination and a credential source,
/// because there is no user in the loop to be the other thing. Whether D3's
/// row should gain a third term instead is an open question on the record and
/// is **not** answered here.
///
/// # The set is closed, small, and derived rather than chosen
///
/// Loopback, the unspecified address, and link-local. Those are IANA
/// special-purpose ranges plus one name RFC 6761 reserves to mean exactly
/// "this host" — determined by the address architecture, not by a judgement
/// about which hosts are dangerous. That is the test
/// [`Shapes`](crate::tools::Shapes) applies to ADR-0011 D6's four categories:
/// `rm -r` is determined by the record's own words and "disk operations" is a
/// list somebody would have to write, so one is transcribed and the other
/// matches nothing.
///
/// **RFC 1918 is deliberately not refused.** `10/8`, `172.16/12` and
/// `192.168/16` are a judgement about a user's own network — refusing them
/// would stop a fetch from a machine down the hall — and that is the list
/// nobody chose.
///
/// # What it costs, and the mechanism that would give it back
///
/// `web.fetch` cannot reach a local development server, which is a plausible
/// thing to want from a developer's harness. The mechanism that would admit
/// one is a user-layer grant in
/// [`tools.allowlist`](crate::tools::Allowed)'s shape — **named here and
/// built by nobody**, exactly as [ADR-0015] D4's per-project admission is
/// named and not built for that allowlist.
///
/// # The limit this does not close
///
/// The rule is held on the host **as the URL spells it**. A hostname that
/// resolves to a refused address defeats it, and `reqwest` offers no hook to
/// reject a resolved address before connecting without a resolver feature this
/// workspace does not take. **No containment is claimed against a name chosen
/// to resolve inward** — the same sentence [`files`](crate::tools::files)
/// already carries for D4's check-at-a-moment, and at `contained` [ADR-0004]'s
/// membrane is the answer that does not depend on winning a race.
///
/// A delegated coordinator ruling of 2026-09-05 under Jeshua's directive of
/// that day, open to his veto, recorded on ADR-0011 D2 and D4.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destinations(Reach);

/// The reaches, private so that [`Destinations`] has exactly the constructors
/// below and no literal anywhere else can widen one.
///
/// **In a product build this enum has exactly one variant.** The second is
/// `#[cfg(test)]`, so the permissive reach is not an unreachable value in the
/// shipped library — it is not a value at all, and there is nothing for a
/// future `match` arm, a `Default`, or a deserialiser to reach. That is the
/// strongest form of the claim and it is what the compiler was already saying
/// when the variant was merely unconstructed: `variant "Loopback" is never
/// constructed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Everything except this machine and the link-local range.
    Public,
    /// Also this machine. Exists only under `cfg(test)` — see
    /// [`Destinations::including_this_machine`].
    #[cfg(test)]
    Loopback,
}

impl Destinations {
    /// The reach every caller in this workspace has.
    ///
    /// The only public constructor, so a [`Destinations`] obtained anywhere
    /// outside this module's own checks refuses loopback.
    #[must_use]
    pub const fn public() -> Self {
        Self(Reach::Public)
    }

    /// Also this machine, for a check that serves its own response.
    ///
    /// # This is absent from the compiled library, and that is the point
    ///
    /// The corpus needs a real socket and the CI runner has no network, so a
    /// check serves one canned response from a `std::net::TcpListener` on
    /// `127.0.0.1` — which [`Self::public`] refuses. Rather than give the
    /// product a value that grants it, this constructor is compiled **only
    /// under `cfg(test)`**, so it is not in the `zaru-cli` rlib that the
    /// binary links, and it is not in the rlib that an integration test under
    /// `tests/` links either. [`Reach`] is private, so there is no literal a
    /// caller could write instead.
    ///
    /// The forbidden reach therefore has **nothing to call** rather than
    /// something that refuses — the form
    /// [`ToolName`](crate::tools::ToolName)'s closed set already takes for
    /// "there is no eighth built-in".
    ///
    /// What it costs is stated rather than hidden: the successful-retrieval
    /// path can only be driven from an in-crate check, on the precedent of
    /// `crate::process`'s checks, which drive **real child processes** the
    /// same way. Every refusal is driven from outside the crate, over the real
    /// client, in `tests/fetch_from_outside.rs`, because a refusal needs no
    /// server.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn including_this_machine() -> Self {
        Self(Reach::Loopback)
    }

    /// Whether this reach includes this machine.
    ///
    /// The one place [`Reach`] is matched, so that the `cfg` lives here and
    /// not in the rule below. **Link-local is not on this axis at all**: no
    /// reach admits it, which is why it is refused before this is asked.
    const fn reaches_this_machine(self) -> bool {
        match self.0 {
            Reach::Public => false,
            #[cfg(test)]
            Reach::Loopback => true,
        }
    }

    /// Why this URL's destination is not reached, if it is not.
    ///
    /// Called on the URL that was asked for **and on every redirect hop**,
    /// which is what stops `http://a.example/` redirecting to
    /// `http://169.254.169.254/`. One predicate, one place — the shape
    /// [`ToolName::addresses_a_path`](crate::tools::ToolName::addresses_a_path)
    /// takes for the same reason: two matches over one set can disagree, and
    /// this one decides where a security boundary applies.
    #[must_use]
    pub fn refuses(self, url: &reqwest::Url) -> Option<RefusedDestination> {
        let host = url.host_str()?;
        match classify(host) {
            // No reach admits link-local, so the check's own client cannot
            // reach a metadata endpoint either. A rule the checks are exempt
            // from is a rule nothing measures.
            Some(HostClass::LinkLocal) => Some(RefusedDestination::LinkLocal {
                host: host.escape_debug().to_string(),
            }),
            Some(HostClass::ThisMachine) if !self.reaches_this_machine() => {
                Some(RefusedDestination::ThisMachine {
                    host: host.escape_debug().to_string(),
                })
            }
            Some(HostClass::ThisMachine) | None => None,
        }
    }
}

/// What a host is, for the two classes this module refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostClass {
    /// Loopback, or the unspecified address, which connects to loopback.
    ThisMachine,
    /// `169.254.0.0/16` or `fe80::/10`.
    LinkLocal,
}

/// Classify a host as the URL spells it.
///
/// The host arrives already normalised by the URL parser: a decimal or octal
/// IPv4 spelling is serialised as dotted-quad and an IPv6 literal is
/// bracketed, both of which are pinned by checks rather than assumed, because
/// this function is where a hostile spelling would slip through.
fn classify(host: &str) -> Option<HostClass> {
    // An IPv6 literal is bracketed in a URL's host serialisation.
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);

    if let Ok(address) = bare.parse::<Ipv4Addr>() {
        return classify_v4(address);
    }
    if let Ok(address) = bare.parse::<Ipv6Addr>() {
        // `::ffff:127.0.0.1` is loopback wearing an IPv6 spelling, and the
        // connect goes to the same place. Mapping first is what stops the
        // spelling being the bypass.
        if let Some(mapped) = address.to_ipv4_mapped() {
            return classify_v4(mapped);
        }
        if address.is_loopback() || address.is_unspecified() {
            return Some(HostClass::ThisMachine);
        }
        // `fe80::/10`. `Ipv6Addr::is_unicast_link_local` is unstable, so the
        // prefix is tested here rather than waiting for it.
        if address.segments()[0] & 0xffc0 == 0xfe80 {
            return Some(HostClass::LinkLocal);
        }
        return None;
    }

    // A name. RFC 6761 reserves `localhost` and everything under it to mean
    // this host, so both spellings are one rule rather than two.
    let name = bare.trim_end_matches('.').to_ascii_lowercase();
    if name == "localhost" || name.ends_with(".localhost") {
        return Some(HostClass::ThisMachine);
    }
    None
}

/// Classify an IPv4 address, for both the literal and the mapped spelling.
const fn classify_v4(address: Ipv4Addr) -> Option<HostClass> {
    if address.is_loopback() || address.is_unspecified() {
        // `0.0.0.0` is here rather than in a class of its own because a
        // connect to it reaches this machine, which is the property the class
        // is named for rather than the name it goes by.
        return Some(HostClass::ThisMachine);
    }
    if address.is_link_local() {
        return Some(HostClass::LinkLocal);
    }
    None
}
