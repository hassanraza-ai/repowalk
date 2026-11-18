# Research: improving repowalk

Date: 2026-11-18. Base: `hassanraza-ai/repowalk` at `8a0dcde`.
This note is the research behind the changes on this branch, and the
measurements taken before and after the indexing work.

repowalk is a local, offline tool. You open a folder and it shows structure,
call and import relationships, a reading order, and a health report. It does
not call a model and it does not use the network. The improvements below stay
inside that job. They do not add summaries from a language model, git history,
or a web port. Those are called out as non-goals in the project README.

## How the tool is built

As of `8a0dcde` the workspace is three crates:

- `crates/core` walks the tree, parses Rust, Python, TypeScript, and
  JavaScript with tree-sitter, resolves call and import edges, and writes
  `.repowalk/index.json` plus `.repowalk/analysis.json`.
- `crates/gui` is an eframe application. Opening a folder calls `learn`.
- `crates/cli` is a thin clap front end over the same core, so the terminal
  and the window cannot disagree about a revision.

`learn` runs walk, parse, resolve, analyze, then cache. A per-file stamp
`(mtime in nanoseconds, size)` and a producer revision decide what to
re-analyze. `parallel::map_files` already existed and was tested for
deterministic merges, but the parse and analyze loops did not call it.
Each file also built a new tree-sitter `Parser` and compiled its queries
again.

Tests are `cargo test` (unit tests, golden fixtures, CLI end-to-end).
`cargo clippy --workspace --all-targets -- -D warnings` is the lint gate.
`scripts/bench.sh` times a warm `repowalk map`. It does not time a cold learn.

## What we compared it with

People trying to understand an unfamiliar repository ask a small set of
questions: where is the thing named like this, who calls it, what does it
call, and what will a change touch. Sillito, Murphy, and De Volder catalogued
44 of those questions from newcomers and from industrial programmers
([Asking and Answering Questions during a Programming Change Task](https://www.cs.ubc.ca/~murphy/papers/other/asking-answering-fse06.pdf)).
repowalk's tour, search, explain card, flow trace, and reference list already
line up with the first three groups. The fourth group, impact across a whole
subgraph, is only partly covered (flow from an entrypoint, fan-in, cycles).

Shneiderman's information-seeking mantra is overview first, zoom and filter,
then details on demand
([The Eyes Have It](https://www.mat.ucsb.edu/g.legrady/academic/courses/11w259/schneiderman.pdf)).
The map and the graph are the overview. Search and the confidence filter are
the filter. The explain card and the source pane are the details. The graph
already dims a node's neighborhood. What is still thin is a durable "you are
here" trail when the graph is large, and a drawing that stays readable once
call edges get dense. Hierarchical edge bundles were designed for that clutter
([Holten, 2006](https://pubmed.ncbi.nlm.nih.gov/17080795/)). Force-directed
layouts and Sugiyama-style layered layouts answer different questions: the
layered view shows direction, the force view shows clusters, and neither is a
substitute for the other. repowalk already offers both. Bundling is a later
drawing project, not a bug.

Sourcetrail is the closest product ancestor: an offline source explorer with
a graph and a source view
([CoatiSoftware/Sourcetrail](https://github.com/CoatiSoftware/Sourcetrail)).
It was archived in 2021. CodeSee was absorbed into another product. The tools
that are still maintained and that match a slice of repowalk are narrower:
[dependency-cruiser](https://github.com/sverweij/dependency-cruiser) for
import graphs and cycles, and
[rust-code-analysis](https://github.com/mozilla/rust-code-analysis) for
tree-sitter metrics. Code-city visualizations (packages as districts, types
as buildings) are a different picture. A 3D city would fight the "one
selection, every pane" model this app already has, and it would not answer
"who calls this function" any more honestly than the graph does.

A 2025 review of software visualization for program comprehension finds that
the tools that get used cluster on three topics: code metrics, dependencies,
and change history
([the review's abstract](https://doi.org/10.1007/978-3-032-15538-2_41)).
repowalk deliberately skips change history. Metrics and dependencies are the
product. The useful move is to make those two sharper, not to add a third
topic the project has rejected.

## Metrics

Cyclomatic complexity (McCabe, 1976) counts independent paths. It is a good
hint for how many tests a function needs. It is a weak hint for how hard the
function is to read: a flat `match` and a nest of loops can share a number.
Sonar's cognitive complexity charges a break in straight-line flow, adds the
nesting level when that break sits inside another, counts a run of the same
boolean operator once, and counts a direct recursive call
([Cognitive Complexity, Campbell, v1.7](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)).
repowalk already stored cyclomatic complexity and maximum nesting. It did not
store a readability score. The two should stay side by side. Replacing
cyclomatic complexity would throw away the testability signal.

At module scale, Robert Martin's package metrics are the usual vocabulary.
Afferent coupling Ca is how many other modules depend on this one. Efferent
coupling Ce is how many this one depends on. Instability is
`I = Ce / (Ca + Ce)`. A module with high Ca and a high complexity score is
the one a reader should treat carefully: many callers, hard internals.
[Wikipedia's summary of the package metrics](https://en.wikipedia.org/wiki/Software_package_metrics)
matches the formulas used in the report added here. Submodule edges are
containment, not a dependency, so they are not counted as coupling.

## Performance

tree-sitter's own notes say a fresh parse is the expensive case and that
updating an existing tree is cheap. A batch index does not have an old tree
per file, but it does repeat two costs that are pure overhead: constructing
a `Parser` and compiling the same query string for every file.
ast-grep's write-up of tree-sitter cost puts query compilation and parser
setup in the part worth amortizing
([Optimizing Tree-sitter for end-to-end performance](https://ast-grep.github.io/blog/tree-sitter-end-to-end)).
Sonda, a Rust static-analysis tool, parses with tree-sitter, walks files in
parallel, and keeps query compilation per language rather than per file
([Building a static analysis platform in Rust](https://www.sonda.sh/blog/building-a-static-analysis-platform-in-rust)).

On this branch, before any indexing change, a release build of `learn` on a
generated corpus (80 Rust files with 12 functions each, 40 Python files with
8 functions each, 120 files) was measured with
`cargo run --release -p repowalk-core --example bench_learn` on a 4-core
machine, rustc 1.100.0:

| Run | Cold learn | Files re-analyzed | Warm learn |
|---|---:|---:|---:|
| 1 | 1461 ms | 120 | 32 ms |
| 2 | 1604 ms | 120 | 42 ms |
| 3 | 1420 ms | 120 | 34 ms |
| 4 | 1450 ms | 120 | 35 ms |

Median cold: 1455 ms. Median warm: 34 ms. Warm is a stamp hit (0 files
re-analyzed) and was not the target.

After reusing parsers and queries and running parse and analyze through
`map_files`, the same example on the same corpus:

| Run | Cold learn | Files re-analyzed | Warm learn |
|---|---:|---:|---:|
| 1 | 116 ms | 120 | 33 ms |
| 2 | 114 ms | 120 | 32 ms |
| 3 | 113 ms | 120 | 31 ms |

Median cold: 114 ms, 12.8 times faster than the 1455 ms median. Median
warm: 32 ms, unchanged in any way that matters. The output is still
path-ordered. A unit test checks that one worker and four workers extract
the same symbols, calls, and imports. Rust files still re-sort the
accumulated analysis after they are merged, which is what the single-threaded
loop did, so mixed-language order does not start depending on the scheduler.

The cold path still parses a Rust or Python file twice: once for symbols and
calls, once for variables and control flow. TypeScript is analyzed twice for
the same reason. Sharing one syntax tree between those passes is the next
performance step. It was left alone here because the two passes do not see
the same symbol table, and folding them together is a behavior change, not
an amortization.

## Graphics and UX

Nielsen's heuristics that matter here are visibility of system status, match
with the real world, and recognition rather than recall
([10 usability heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/)).
The learn progress line is the status. Honest edge confidence (dashed when
the resolver is not sure) is the match with the real world. The keyboard map
in the README is recognition.

WCAG 2.2 asks for at least 4.5:1 contrast for body text and 3:1 for
non-text UI
([contrast minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)).
The light and dark text tokens already have contrast tests. The module
colors on the graph did not. They were a red/green-heavy set. Okabe and Ito's
palette is the usual categorical set that survives the common color-vision
deficiencies ([Color Universal Design](https://jfly.uni-koeln.de/color/)).
The graph now uses that set. The black swatch is replaced with a light
neutral, because black disappears on the dark background. A test checks that
the eight colors are distinct and that each clears 3:1 against the dark
background.

The health list and the control-flow diagram now show cognitive complexity
next to cyclomatic complexity, so the readability number is not trapped in
the CLI.

## Reliability

`is_entrypoint` used to treat any file containing the substrings `fn main` or
`__main__` as an entrypoint. A markdown note that mentions `fn main`, a
function named `main_helper`, and a Python comment were all entrypoints.
Tour ranking uses that flag. The check is now the real Rust signature on a
`.rs` file, and a non-comment `__main__` on a `.py` file. `main.rs`,
`lib.rs`, and `__main__.py` are unchanged.

## Plan, and what this branch carries out

1. Record this research. Done, this file.
2. Measure cold `learn` before touching the indexer. Done, table above.
3. Reuse parsers and queries, and run the existing deterministic parallel
   map on parse and analyze. Done. Re-measured.
4. Add cognitive complexity as a second number, not a replacement. Done, with
   tests taken from the Sonar examples (a `match` is 1, the nested
   `sumOfPrimes` shape is 7, boolean runs, `else if`, direct recursion, and
   the Python try/except nest of 9).
5. Add Ca, Ce, and instability to the architecture report. Done, unit-tested.
   The hotspot ranking is still by cyclomatic complexity, so existing order
   does not move.
6. Fix entrypoint false positives and switch the module palette. Done.
7. Correct `docs/architecture.md`, which still said the GUI called
   `build_index` and that `analysis.json` was invalidated by version alone.

Left for a later change, on purpose:

- One shared syntax tree for the symbol pass and the analysis pass.
- Indirect recursion, and Sonar's special case for Python decorators.
  Nested functions are scored on their own row instead of being folded into
  the parent, so a hotspot list does not charge a parent for a child that
  has its own line.
- Hierarchical edge bundling and a focus camera for the layered graph.
  Both are drawing projects and need a display to judge. The neighborhood
  highlight is already there.
- A city view, git churn, and model-written explanations. Those would make
  repowalk a different tool.

## How to re-measure

```bash
cargo run --release -p repowalk-core --example bench_learn
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

The example rebuilds the same generated corpus in a temp directory, so the
number does not move just because the repository gained a file.
