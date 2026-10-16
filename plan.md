# Plan: Yozora module system

## Header

```yaml header
design: docs/waves/design/snapshot.html
bootstrap: cargo fetch
gates: {format: "cargo fmt --check", lint: "cargo clippy --workspace --all-targets -- -D warnings", typecheck: "cargo check --workspace --all-targets", unit: "cargo test --workspace"}
max_wave_width: 8
```

## Feature index

| Feature | Spec |
|---|---|
| F1 Module schema | docs/waves/specs/F1.md |
| F2 Component kit | docs/waves/specs/F2.md |
| F3 Module runtime | docs/waves/specs/F3.md |
| F4 Bar item and popover renderer | docs/waves/specs/F4.md |
| F5 yozora module CLI | docs/waves/specs/F5.md |
| F6 Guidelines doc | docs/waves/specs/F6.md |
| F7 Claude sessions module | docs/waves/specs/F7.md |
| F8 CachyOS updates module | docs/waves/specs/F8.md |

## Phases

```yaml phase
id: P1a
spec: F1
steps: [F1.1, F1.2]
owns: [crates/yozora-module/**, Cargo.toml]
depends_on: []
model: sonnet
score: {scope: 1, ambiguity: 1, risk: 0, reasoning: 1}
rationale: "Strict serde schema, several judgment calls on field design"
```

```yaml phase
id: P2a
spec: F2
steps: [F2.1, F2.2]
owns: [crates/yozora-ui/src/kit/display.rs, crates/yozora-ui/src/kit/controls.rs, crates/yozora-ui/tests/**]
depends_on: [P1a]
model: sonnet
score: {scope: 1, ambiguity: 1, risk: 0, reasoning: 1}
rationale: "GPUI components from a frozen design"
```

```yaml phase
id: P3a
spec: F3
steps: [F3.1, F3.2]
owns: [crates/yozora-bar/src/services/modules.rs, crates/yozora-bar/tests/module_runtime.rs, crates/yozora-bar/tests/module_restart.rs, crates/yozora-bar/tests/module_action.rs]
depends_on: [P1a]
model: sonnet
score: {scope: 1, ambiguity: 1, risk: 0, reasoning: 2}
rationale: "Process supervision and backoff need care"
```

```yaml phase
id: P5a
spec: F5
steps: [F5.1]
owns: [crates/yozora-cli/src/module.rs, crates/yozora-cli/tests/**]
depends_on: [P1a]
model: sonnet
score: {scope: 1, ambiguity: 1, risk: 0, reasoning: 1}
rationale: "Validation errors with line numbers"
```

```yaml phase
id: P6a
spec: F6
steps: [F6.1]
owns: [docs/modules.md, crates/yozora-module/tests/guidelines.rs]
depends_on: [P1a]
model: haiku
score: {scope: 0, ambiguity: 1, risk: 0, reasoning: 0}
rationale: "Mostly prose plus two small tests"
```

```yaml phase
id: P4a
spec: F4
steps: [F4.1]
owns: [crates/yozora-bar/src/modules/module.rs, crates/yozora-bar/tests/module_error_state.rs]
depends_on: [P2a, P3a]
model: sonnet
score: {scope: 2, ambiguity: 1, risk: 0, reasoning: 1}
rationale: "Wires kit and runtime into the bar"
```

```yaml phase
id: P7a
spec: F7
steps: [F7.1]
owns: [packaging/modules/claude-sessions/**]
depends_on: [P4a, P5a, P6a]
model: haiku
score: {scope: 1, ambiguity: 0, risk: 0, reasoning: 0}
rationale: "Manifest plus a thin script over an existing tool"
```

```yaml phase
id: P8a
spec: F8
steps: [F8.1]
owns: [packaging/modules/cachy-updates/**]
depends_on: [P4a, P5a, P6a]
model: haiku
score: {scope: 1, ambiguity: 1, risk: 0, reasoning: 0}
rationale: "Manifest plus a script that parses checkupdates and paru output"
```

## Four graphs

### Dependency graph

| Phase | Depends on |
|---|---|
| P1a | none |
| P2a | P1a |
| P3a | P1a |
| P5a | P1a |
| P6a | P1a |
| P4a | P2a, P3a |
| P7a | P4a, P5a, P6a |
| P8a | P4a, P5a, P6a |

### Ownership graph

| Phase | Owns |
|---|---|
| P1a | crates/yozora-module/**, Cargo.toml |
| P2a | crates/yozora-ui/src/kit/display.rs, crates/yozora-ui/src/kit/controls.rs, crates/yozora-ui/tests/** |
| P3a | crates/yozora-bar/src/services/modules.rs, crates/yozora-bar/tests/module_runtime.rs, crates/yozora-bar/tests/module_restart.rs, crates/yozora-bar/tests/module_action.rs |
| P5a | crates/yozora-cli/src/module.rs, crates/yozora-cli/tests/** |
| P6a | docs/modules.md, crates/yozora-module/tests/guidelines.rs |
| P4a | crates/yozora-bar/src/modules/module.rs, crates/yozora-bar/tests/module_error_state.rs |
| P7a | packaging/modules/claude-sessions/** |
| P8a | packaging/modules/cachy-updates/** |

### Context graph

| Phase | Reads |
|---|---|
| P1a | crates/yozora-config/src/bar.rs |
| P2a | crates/yozora-ui/src/controls.rs, docs/brand/**, docs/waves/design/snapshot.html#screen-component-sheet |
| P3a | crates/yozora-bar/src/services/custom.rs |
| P5a | crates/yozora-module/src/lib.rs |
| P6a | docs/brand/**, docs/configuration.md |
| P4a | crates/yozora-bar/src/modules/custom.rs, crates/yozora-bar/src/popover.rs |
| P7a | docs/modules.md, docs/waves/design/snapshot.html#screen-claude-popover |
| P8a | docs/modules.md, docs/waves/design/snapshot.html#screen-updates-popover |

### Validation graph

| Phase | Shallow gate | Wave gate criteria |
|---|---|---|
| P1a | format, lint, typecheck, unit | F1.AC1, F1.AC2, F1.AC3, F1.AC4 |
| P2a | format, lint, typecheck, unit | F2.AC1, F2.AC2, F2.AC3, F3.AC1, F3.AC2, F3.AC3, F5.AC1, F5.AC2, F6.AC1, F6.AC2 |
| P3a | format, lint, typecheck, unit | F2.AC1, F2.AC2, F2.AC3, F3.AC1, F3.AC2, F3.AC3, F5.AC1, F5.AC2, F6.AC1, F6.AC2 |
| P5a | format, lint, typecheck, unit | F2.AC1, F2.AC2, F2.AC3, F3.AC1, F3.AC2, F3.AC3, F5.AC1, F5.AC2, F6.AC1, F6.AC2 |
| P6a | format, lint, typecheck, unit | F2.AC1, F2.AC2, F2.AC3, F3.AC1, F3.AC2, F3.AC3, F5.AC1, F5.AC2, F6.AC1, F6.AC2 |
| P4a | format, lint, typecheck, unit | F4.AC1, F4.AC2 |
| P7a | format, lint, typecheck, unit | F7.AC1, F7.AC2, F8.AC1, F8.AC2 |
| P8a | format, lint, typecheck, unit | F7.AC1, F7.AC2, F8.AC1, F8.AC2 |

## Waves

```yaml wave
wave: 1
phases: [P1a]
integration_owned: [Cargo.lock]
criteria: [F1.AC1, F1.AC2, F1.AC3, F1.AC4]
integration_checks: []
```

```yaml wave
wave: 2
phases: [P2a, P3a, P5a, P6a]
integration_owned: [Cargo.lock, crates/yozora-ui/Cargo.toml, crates/yozora-ui/src/lib.rs, crates/yozora-ui/src/kit/mod.rs, crates/yozora-bar/Cargo.toml, crates/yozora-bar/src/services/mod.rs, crates/yozora-cli/Cargo.toml, crates/yozora-cli/src/main.rs]
criteria: [F2.AC1, F2.AC2, F2.AC3, F3.AC1, F3.AC2, F3.AC3, F5.AC1, F5.AC2, F6.AC1, F6.AC2]
integration_checks: []
```

```yaml wave
wave: 3
phases: [P4a]
integration_owned: [Cargo.lock, crates/yozora-bar/src/modules/mod.rs, crates/yozora-bar/src/bar.rs]
criteria: [F4.AC1, F4.AC2]
integration_checks: []
```

```yaml wave
wave: 4
phases: [P7a, P8a]
integration_owned: [Cargo.lock]
criteria: [F7.AC1, F7.AC2, F8.AC1, F8.AC2]
integration_checks: []
```

## Execution protocol

Executed by `/wave-planning:wave-execute`. See that skill for the worktree, dispatch, gate and merge procedure.

## Status

```yaml status
base_branch: feat/modules
integration_branch: wave/integration
integration_head: 8e2c29cc214a204f9de14a8842c312308dd83540
W1: done
W2: pending
W3: pending
W4: pending
P1a: done
P2a: pending
P3a: pending
P5a: pending
P6a: pending
P4a: pending
P7a: pending
P8a: pending
```
