| Stable ID | Path | Responsibility | Provides | Requires |
|---|---|---|---|---|
| `kvist.maerg` | `maerg/` | Project artifacts, component discovery, validation, status, queues, policy, task lifecycle, context selection, evidence, and CLI dispatch | `kvist.cli/v1`, `kvist.artifacts/v1`, `kvist.host-authority/planned` | `sav.library/v1`, `galla.protocol/v1`, operating-system and VCS services |
| `sav` | `sav/` | Provider-neutral prompt acquisition, command rendering, process supervision, profiles, model transport, and reusable bounded runtime mechanisms | `sav.library/v1`, `sav.cli/v1` | Host authority interfaces and operating-system process/network services |
| `galla` | `galla/` | Strict request validation and Linux Bubblewrap enforcement for approved task grants | `galla.protocol/v1`, `galla.cli/v1` | Bubblewrap, Linux kernel isolation, and operating-system process/filesystem services |
| `skott` | `skott/` | Interactive/headless workspace agent, request budgets, native file tools and private operational journals; no task approval or canonical evidence | `skott.library/v1`, `skott.cli/v1` | `sav.library/v1`, `galla.protocol/v1`, installed Linux enforcement |
