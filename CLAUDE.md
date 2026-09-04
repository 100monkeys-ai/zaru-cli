# CLAUDE.md — zaru-cli bootstrap

This repository holds code. **It does not hold knowledge.** The engineering contract for Zaru — architecture, decisions, operating principles, testing, the commit workflow, the autonomy boundary, and every lesson already learned — lives in the Zaru workspace in the cortex. Read this file once, ground against that workspace, and work from there. Where this file and the workspace disagree, the workspace wins and this file is stale.

## Where the knowledge is

- **Permalink:** https://100monkeys-ai.cortex.page/zaru/
- **Workspace UUID:** `a96c9dde-becf-4ff0-836e-ad8bef46ff42`

**Pass `workspace: "a96c9dde-becf-4ff0-836e-ad8bef46ff42"` on every cortex call — page, atom, search, tag, comment, reads and writes alike.** The MCP token has one current-workspace pointer and it is shared by every session presenting that token; any of them can move it at any moment. A call that omits `workspace` resolves against wherever the pointer happens to be, and a write that lands in another product's workspace reports success exactly like one that lands here. Prefer the UUID over the slug: a UUID cannot be re-resolved against the wrong instance.

Start at the workspace landing page `home`, which is the navigation table. An agent told to develop Zaru reads `operations/autonomous-development` next. Read a decision record itself before implementing it — never its row on the status index.

## The umbrella bootstrap

One directory up, `../CLAUDE.md` is the bootstrap for every 100monkeys repository cloned here: how to attach the cortex MCP server, which workspace governs which repository, and the rules that hold everywhere. Read it if this session has not already.

## What governs this repository's layout

**[ADR-0003: Build strategy, dependencies, and licensing](https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing)** is the authority for the Cargo workspace layout, the crate boundaries, the dependency policy, the Apache-2.0 licence, and the DCO contribution gate. When the layout and that record disagree, the record is canonical and the layout is the bug.

The harness is **pre-alpha**: no backward-compatibility shims, no legacy code paths, no "for now". If you find one, remove it.
