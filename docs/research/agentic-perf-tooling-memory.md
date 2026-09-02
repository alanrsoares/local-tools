# Agentic Performance Tooling for Agentic Engineering: Mem0, Cognee, Zep, and the Modern Local Stack

*Authoritative Research Report & Architecture Guide*  
*Target Environment: Dual-Agent Workflows (Claude Code & Antigravity/AGY), Headroom, Serena, RTK, and Memory Systems*

---

## 1. Executive Summary & Problem Framing

In high-velocity software engineering, developers are transitioning from ad-hoc single-agent chats to continuous, multi-agent workflows using terminal orchestrators like **Claude Code (`claude`)** and **Antigravity CLI (`agy`)**. 

As these agents perform multi-step refactorings, test runs, and cross-session task tracking, standard LLM interactions hit critical performance walls:
1. **Context Window Degradation & Token Burn:** Feeding entire files, repetitive test logs, and conversation history triggers quadratic attention cost, rapid token exhaustion, and high API bills.
2. **The "Lost in the Middle" Retrieval Collapse:** As prompt contexts stretch beyond 64k–128k tokens, recall precision degrades, causing agents to forget architectural constraints, hallucinate deleted APIs, or contradict earlier instructions.
3. **Temporal Invalidation & Codebase Drift:** Codebases evolve constantly. A static vector store remembering that *"AuthModule uses bcrypt"* will hallucinate after a commit migrates to `argon2`, because vector similarity lacks temporal validity intervals.
4. **Shell & Tool Noise Bloat:** Raw terminal outputs (`cargo test`, `git status`, `npm install`, compiler logs) consume thousands of unnecessary tokens with visual ANSI formatting, spinners, and repetitive logs.

To achieve **Agentic Performance (Agentic Perf)**, modern engineering combines three complementary layers:
- **Context & Output Optimization Layer:** **RTK** (Rust Token Killer) + **Headroom** (Context Optimization Proxy & Tool Traffic Auditor).
- **Symbolic & Semantic AST Code Engine:** **Serena** (Language Server Protocol / AST-level MCP IDE for Agents).
- **Multi-Tiered Persistent Memory Engine:** **Mem0** (Procedural / Preferences), **Cognee** (Deterministic Domain Graph), and **Zep / Graphiti** (Bi-Temporal State & Refactor Invalidation).

---

## 2. The Complete 360° Agentic Performance Stack

Below is the end-to-end architecture unifying **Claude Code**, **Antigravity CLI (`agy`)**, **Headroom**, **RTK**, **Serena**, and the **Persistent Memory Layer**:

```
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                             AGENT RUNTIME & ORCHESTRATION                                   │
│                                                                                             │
│     ┌──────────────────────────────┐              ┌──────────────────────────────┐          │
│     │   Claude Code (`claude`)     │              │    Antigravity CLI (`agy`)   │          │
│     │   - Interactive terminal     │              │    - Autonomous subagents    │          │
│     │   - MCP tool integration     │              │    - Skills & custom rules   │          │
│     └──────────────┬───────────────┘              └──────────────┬───────────────┘          │
└────────────────────┼─────────────────────────────────────────────┼──────────────────────────┘
                     │                                             │
                     ▼                                             ▼
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                           CONTEXT & TOKEN OPTIMIZATION PROXIES                              │
├──────────────────────────────────────────────────────────────┬──────────────────────────────┤
│  HEADROOM (`headroom proxy`)                                 │  RTK (`rtk` CLI Proxy)       │
│  - Intercepts LLM inference traffic                          │  - Intercepts shell tool cmds│
│  - Audits Read-tool traffic & prunes context                 │  - Strips ANSI, deduplicates │
│  - AST search (`ast-grep`), structural diff (`difftastic`)   │  - 60–90% token reduction on │
│  - Tool failure learning (`headroom learn`)                  │    test & build outputs      │
└──────────────────────────────────────────────────────────────┴──────────────────────────────┘
                                              │
                     ┌────────────────────────┴────────────────────────┐
                     ▼                                                 ▼
┌───────────────────────────────────────────────┐     ┌───────────────────────────────────────┐
│     SEMANTIC CODE INSPECTION & REFACTOR       │     │     PERSISTENT MULTI-TIER MEMORY      │
│                 (SERENA MCP)                  │     │       (SHARED ACROSS CLAUDE & AGY)    │
├───────────────────────────────────────────────┤     ├───────────────────────────────────────┤
│  SERENA (LSP / AST-Level MCP Engine)          │     │  MEM0 (Procedural & Persona Memory)   │
│  - Symbol-level resolution (defs, references) │     │  - Stores user preferences, lint rules│
│  - Call hierarchies (callers, callees)        │     │  - Scoped to `user_id` / `agent_id`   │
│  - Safe multi-file atomic symbol renaming     │     ├───────────────────────────────────────┤
│  - 0-hallucination semantic code navigation   │     │  COGNEE (Deterministic Domain KG)     │
│                                               │     │  - Local DuckDB + LanceDB + Kùzu DB   │
├───────────────────────────────────────────────┤     │  - Ingests ADRs, specs, module graphs │
│  LOCAL-TOOLS (High-Speed Rust Binaries)       │     ├───────────────────────────────────────┤
│  - `scaffold`, `portkill`, `jwt`, `devclean`  │     │  ZEP / GRAPHITI (Bi-Temporal Graph)   │
│  - Instant offline execution, zero latency    │     │  - Ingests Git commits & PR diffs     │
│                                               │     │  - Auto-invalidates stale code facts  │
└───────────────────────────────────────────────┘     └───────────────────────────────────────┘
```

---

## 3. Deep-Dive: Memory Tooling (Mem0 vs. Cognee vs. Zep/Graphiti)

### 3.1 Mem0: Dynamic Persona, Rule & Invariant Tracking

* **Role:** Procedural memory, user coding preferences, agent roles, and invariant commandments.
* **Storage Topology:** Vector DB (Qdrant/PgVector) + KV Store (SQLite) + Entity Graph.
* **Key Mechanism:** Extracts atomic facts from interactions, deduplicating and updating user/agent-scoped rules.

```python
from mem0 import Memory

m = Memory()

# Ingest project commandments
m.add(
    messages=[
        {"role": "user", "content": "In local-tools, all crates must have zero external dependencies and use standard library only."}
    ],
    user_id="alan",
    agent_id="coding-assistant",
    metadata={"repo": "local-tools"}
)

# Semantic retrieval during task setup
rules = m.search(query="dependencies for new crate in local-tools", filters={"user_id": "alan"})
```

* **Where it fits in your stack:** Shared across `claude` and `agy`. Ensures both agents remember Alan's specific coding guidelines without re-prompting.

---

### 3.2 Cognee: Deterministic Architecture & Domain Graph Engine

* **Role:** High-level structural codebase indexing, Architecture Decision Records (ADRs), module contracts, and deterministic multi-hop reasoning.
* **Storage Topology:** **Tri-modal embedded local stack** (Relational: DuckDB, Vector: LanceDB, Graph: Kùzu DB).
* **Key Mechanism:** Modular **ECL (Extract, Cognify, Load)** pipeline. Converts markdown specs, schemas, and crate entry points into a traversable topological knowledge graph.

```python
import cognee
import asyncio

async def index_architecture():
    # Ingest repository layout & architectural contracts
    await cognee.add(data="./README.md", dataset_name="repo_spec")
    await cognee.add(data="./crates/local-common/src/paths.rs", dataset_name="repo_spec")
    
    # Cognify into deterministic entity-relationship graph
    await cognee.cognify(datasets=["repo_spec"])
    
    # Query graph completion
    arch_summary = await cognee.search(
        query="What is the shared path resolution convention across crates?",
        search_type="GRAPH_COMPLETION"
    )
    print(arch_summary)

asyncio.run(index_architecture())
```

* **Where it fits in your stack:** Provides macro-level codebase and domain reasoning that is local, fast, and completely deterministic, avoiding RAG hallucinations.

---

### 3.3 Zep & Graphiti: Bi-Temporal State & Continuous Invalidation

* **Role:** Tracking code evolution over time, commit diff histories, PR triage notes, and solving the **Temporal Contradiction Problem**.
* **Storage Topology:** FalkorDB / Neo4j + Vector Index.
* **Key Mechanism:** **Bi-temporal edge modeling** (`valid_at`, `invalidated_at`). When a refactor occurs, Graphiti updates the historical validity interval rather than blindly overwriting facts.

```python
from datetime import datetime
from graphiti_core import Graphiti
from graphiti_core.models import EpisodeType

async def track_git_evolution(graphiti: Graphiti):
    # Record commit episode
    await graphiti.add_episode(
        name="commit_7f8a12",
        episode_body="Refactored portkill from lsof CLI parsing to Darwin libproc sysctl calls.",
        source=EpisodeType.text,
        source_description="Git Commit",
        reference_time=datetime.now()
    )

    # Hybrid query: automatically filters out invalidated lsof facts
    active_state = await graphiti.search(query="How does portkill find socket owners?")
```

* **Where it fits in your stack:** Exists as an MCP server. Both `claude` and `agy` query Graphiti to understand recent code changes and active state transitions.

---

## 4. Complementary Perf Engines: Headroom, Serena, and RTK

To understand why memory systems alone are insufficient without context & symbolic tooling, consider the role of each tool in the local toolchain:

### 4.1 RTK (Rust Token Killer)
* **What it does:** High-speed, zero-dependency Rust CLI proxy that intercepts terminal commands (`git`, `cargo`, `bun`, `pytest`, `docker`).
* **Perf Impact:** Compresses terminal command output by **60–90%** via ANSI stripping, spinner removal, deduplication counters (`35 tests passed...`), and error-focused truncation before output enters LLM context.
* **Integration:** Used by agent shell hooks in both Claude Code and AGY.

### 4.2 Headroom
* **What it does:** Context Optimization Layer and intelligent proxy for LLM applications.
* **Perf Impact:**
  - `headroom proxy`: Intercepts and optimizes active prompt payloads and caching headers.
  - `headroom audit-reads`: Audits file read traffic to identify and compress bloated context reads.
  - `headroom learn`: Learns from past tool call failures to prevent repetitive error loops.
  - Bundles structural utilities: `ast-grep` (`headroom sg`), `difftastic` (`headroom diff`), `scc` (`headroom loc`).

### 4.3 Serena (LSP / AST MCP Server)
* **What it does:** Provides IDE-grade Language Server Protocol (LSP) capabilities to agents over MCP.
* **Perf Impact:** Replaces dumb `grep` and full-file dumps with exact symbol navigation (definitions, callers, type hierarchies, safe atomic renames) across 40+ languages.
* **Difference from Cognee:** Serena is **in-memory, live, and workspace-synchronous** (instant AST / LSP compiler state); Cognee is **persisted, macro-architectural, and cross-session knowledge-graph driven**.

---

## 5. Architectural Synergy Matrix

| Tool | Primary Layer | Operational Focus | Token / Latency Impact |
| :--- | :--- | :--- | :--- |
| **RTK** | Terminal / CLI Execution | Compresses command output (`cargo`, `git`, `test`) | **60–90% output token reduction** (<10ms overhead) |
| **Headroom** | Proxy & Context Optimization | Read-traffic pruning, tool failure learning, proxy cache | **30–50% prompt token reduction**, reduced error loops |
| **Serena** | Symbolic Code Engine (LSP) | Exact symbol lookup, caller graphs, atomic refactoring | **Zero-hallucination edits**, eliminates blind grep scans |
| **Mem0** | Procedural / Persona Memory | Developer preferences, project rules, lint constraints | **90% context compression** for rules (<30ms retrieval) |
| **Cognee** | Semantic Domain Graph | Local embedded knowledge graph (DuckDB+LanceDB+Kùzu) | **Deterministic multi-hop queries** across architecture |
| **Zep / Graphiti** | Bi-Temporal State Graph | Commit evolution, state transitions, temporal invalidation | **Sub-second hybrid search**, eliminates stale-code bugs |

---

## 6. End-to-End Workflow: The Shared Dual-Agent Setup

Here is how a day-to-day workflow operates across **Claude Code** and **Antigravity CLI (`agy`)** on this machine:

```
                  ┌────────────────────────────────────────────────────────┐
                  │ 1. Engineer starts task in `agy` or `claude`:          │
                  │    "Add JWT claim humanizer to `crates/jwt`"           │
                  └───────────────────────────┬────────────────────────────┘
                                              │
                                              ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ 2. PRE-FLIGHT MEMORY RETRIEVAL (Shared MCP Layer)                                        │
│    - Mem0 returns: "Rule: Zero external crates; use std only; Rust 2021"                 │
│    - Cognee returns: "Contract: local-common::paths for config resolution"               │
│    - Zep returns: "Recent commit: jwt crate scaffolded in PR #12"                        │
│    => Injected prompt budget: < 1,500 tokens (instead of 40,000 tokens of raw repo)     │
└──────────────────────────────────────────────────────────────────────────────────────────┘
                                              │
                                              ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ 3. CODE INSPECTION & EDITING (Serena + Local Tools)                                      │
│    - Agent queries Serena MCP: `get_symbol_definition("JwtHeader")`                      │
│    - Serena returns exact 12-line struct slice with type definitions                     │
│    - Agent performs atomic edit                                                          │
└──────────────────────────────────────────────────────────────────────────────────────────┘
                                              │
                                              ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ 4. VERIFICATION & OUTPUT COMPRESSION (RTK + Headroom)                                    │
│    - Agent runs `cargo test -p jwt`                                                      │
│    - RTK intercepts and compresses 200 lines of build/test logs into 4 lines of summary  │
│    - Headroom records zero tool failure regressions                                      │
└──────────────────────────────────────────────────────────────────────────────────────────┘
                                              │
                                              ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ 5. POST-COMMIT STATE SYNCHRONIZATION                                                     │
│    - Git hook triggers `graphiti.add_episode(commit_diff)`                               │
│    - Now, when opening `claude` or `agy` tomorrow, both agents immediately know the new   │
│      claim humanizer signature without re-reading the crate!                             │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. Concrete Configuration: Unifying MCP for Claude & AGY

To share the same memory and perf sidecars between Claude Code and Antigravity, configure the shared MCP registration:

### Claude Code (`~/.claude.json` or project config) & AGY (`~/.gemini/antigravity-cli/mcp_config.json`):

```json
{
  "mcpServers": {
    "serena": {
      "command": "serena-mcp",
      "args": ["--workspace", "/Users/alanrsoares/dev/local-tools"]
    },
    "headroom": {
      "command": "headroom",
      "args": ["mcp"]
    },
    "graphiti-memory": {
      "command": "graphiti-mcp",
      "args": ["--uri", "bolt://localhost:7687", "--user", "neo4j", "--password", "password"]
    },
    "mem0-preferences": {
      "command": "uv",
      "args": ["run", "/Users/alanrsoares/.local/bin/mem0-mcp-server.py"]
    }
  }
}
```

---

## 8. Summary & Strategic Recommendations

1. **Activate RTK & Headroom as the Outer Shield:** Let RTK compress all tool command outputs and Headroom optimize read-tool traffic and proxy caching. This immediately cuts active token consumption by 60–80%.
2. **Use Serena for Workspace-Local Precision:** Route all symbol definitions, references, and AST refactorings through Serena MCP to avoid hallucinated search-and-replace errors.
3. **Use Mem0 for Shared Cross-Agent Commandments:** Keep user preferences, style guide rules, and workspace invariants in Mem0 so both Claude and AGY stay aligned.
4. **Use Cognee for Local Embedded Repo Ontologies:** Run Cognee's embedded ECL engine (`DuckDB + LanceDB + Kùzu`) over markdown documentation and ADRs for zero-cloud, deterministic domain reasoning.
5. **Use Zep / Graphiti for Temporal Evolution:** Maintain a bi-temporal knowledge graph of git commits and PRs to eliminate stale-state contradictions across multi-day coding tasks.
