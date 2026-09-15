// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A home and a project a command check owns. Compiled only under
//! `cfg(test)`.
//!
//! [Testing]'s rule: "Each test owns its own state... its own configuration
//! directory, its own session store, and its own working directory, writing
//! to the paths the product actually writes to inside that root." So these
//! are real directories with real files in them, laid out exactly as
//! [ADR-0015] D3 spells the two built locations, and the loader under test
//! reads them through `std::fs` rather than through a seam that could answer
//! more simply than a filesystem does ([Verification lessons] §24).
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::commands::document::COMMANDS_DIRECTORY;
use crate::tools::fixtures::nonce;
use std::path::{Path, PathBuf};

/// A `~/.zaru`-equivalent home and a project root, side by side.
pub(crate) struct Scratch {
    base: PathBuf,
}

impl Scratch {
    /// Build it. Panics rather than returning: a check whose staging failed
    /// must refuse rather than skip ([Verification lessons] §4).
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    pub(crate) fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("cmd-scratch"));
        std::fs::create_dir_all(base.join("home")).expect("staging: the home");
        std::fs::create_dir_all(base.join("project")).expect("staging: the project");
        Self { base }
    }

    /// The `~/.zaru`-equivalent root.
    pub(crate) fn home(&self) -> PathBuf {
        self.base.join("home")
    }

    /// The working directory a project's commands are read under.
    pub(crate) fn project(&self) -> PathBuf {
        self.base.join("project")
    }

    /// A second working directory, so that "per project" can be measured
    /// rather than asserted.
    pub(crate) fn elsewhere(&self) -> PathBuf {
        let path = self.base.join("elsewhere");
        std::fs::create_dir_all(&path).expect("staging: a second project");
        path
    }

    /// Write `<name>.md` into the user location.
    pub(crate) fn user_command(&self, name: &str, text: &str) -> PathBuf {
        write_into(&self.home().join(COMMANDS_DIRECTORY), name, text)
    }

    /// Write `<name>.md` into the project location.
    pub(crate) fn project_command(&self, name: &str, text: &str) -> PathBuf {
        write_into(
            &self.project().join(".zaru").join(COMMANDS_DIRECTORY),
            name,
            text,
        )
    }

    /// Write `<name>.md` into a second project's location.
    pub(crate) fn command_in(&self, root: &Path, name: &str, text: &str) -> PathBuf {
        write_into(&root.join(".zaru").join(COMMANDS_DIRECTORY), name, text)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A whole command file: the fences, the head, and the body.
pub(crate) fn file(head: &str, body: &str) -> String {
    format!("+++\n{head}+++\n{body}")
}

fn write_into(directory: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(directory).expect("staging: a commands directory");
    let path = directory.join(format!("{name}.md"));
    std::fs::write(&path, text.as_bytes()).expect("staging: a command file");
    path
}
