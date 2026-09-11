# Standalone Agent Runtime: User Guide

`agent-runtime` is a bounded, host-supervised Linux execution harness and inference client for AI agents and language models. It operates completely independently of the Kvist workflow engine, providing:

1. **Deterministic Host-Process Supervision:** Non-interactive, process-group-isolated execution with wall-clock timers, idle detectors, output bounds, and repetition breakers.
2. **Direct Local Model Transport:** Zero-dependency HTTP streaming against local inference backends (`llama-server`, `Ollama`) with slot-allocation, time-to-first-token (TTFT), and inter-token cadence watchdogs.
3. **Structured Grammar-Constrained Sampling:** Dynamic compilation of JSON Schemas and tool definitions into native GBNF (GGML BNF) grammars passed directly to `llama-server`.
4. **Multi-Tier Loop Detection Engine:** Turn-level Action Hash Ring (Tier 1), environment observation invariant tracker (Tier 2), and N-gram reasoning similarity detector (Tier 3) with progressive soft corrections and temperature jitter.
5. **Deterministic Session Trajectory Journaling & Replay:** Structured `.jsonl` event stream recording and turn-by-turn trajectory inspection (`agent-run replay`).

---

## 1. Quick Start

### 1.1 Building and Installing

`agent-runtime` is built as both a library crate and a standalone CLI binary named `agent-run`.

To build the executable in release mode:

```bash
cargo build --release -p agent-runtime
```

The resulting binary will be located at `target/release/agent-run`. You can install it into your `$PATH` via:

```bash
cargo install --path agent_runtime
```

Verify the installation:

```bash
agent-run --version
agent-run --help
```

---

### 1.2 Interactively Configuring a Provider Profile

The quickest way to configure your local or CLI providers is via the interactive setup wizard:

```bash
agent-run setup
```

The wizard will:
1. Probe local network loopbacks and standard directories for installed model backends (`llama-server`, `Ollama`, `copilot`, `gemini`).
2. Test connection and model readiness with a qualification prompt.
3. Persist a reusable profile in `~/.config/agent-runtime/config.toml`.

---

### 1.3 Running a Direct Local Model Inference

Send a prompt directly to a running `llama-server` or `Ollama` instance:

```bash
# Query Ollama with streaming output
agent-run model 
  --provider ollama 
  --endpoint http://127.0.0.1:11434 
  --model qwen2.5-coder:7b 
  --stream 
  "Write a Rust function that calculates SHA-256 digests."
```

```bash
# Query llama-server with reasoning display
agent-run model 
  --provider llama-server 
  --endpoint http://127.0.0.1:8080 
  --model deepseek-r1 
  --show-reasoning 
  --stream 
  "Explain how bubblewrap namespaces isolate process trees."
```

---

### 1.4 Executing a Command under Bounded Host Supervision

Execute any agent script or command line under supervision, ensuring it cannot hang, loop, or consume unbounded output:

```bash
# Supervise an agent run with automatic loop detection and restart policy
agent-run run 
  --command "python3 run_agent.py --prompt {prompt}" 
  --detect-loops 
  --idle-timeout 60 
  --max-retries 2 
  --allow-host-execution 
  "Analyze dependencies in src/ and generate a security audit report."
```

---

### 1.5 Replaying a Historical Execution Trajectory

Step through and inspect an agent trajectory journal (`.jsonl`) generated during execution:

```bash
# Inspect the entire trajectory
agent-run replay session_run.jsonl

# Inspect up to turn 3 in machine-readable JSON format
agent-run replay --max-turns 3 --json session_run.jsonl
```

---

## 2. Configuration & Profiles

### 2.1 Configuration File Location

`agent-run` reads profile configurations from the following location in order of precedence:
1. Explicit path supplied via the `--config <PATH>` flag.
2. `$XDG_CONFIG_HOME/agent-runtime/config.toml` (typically `~/.config/agent-runtime/config.toml`).
3. Fallback legacy path `~/.config/supervised-agent/config.toml`.

---

### 2.2 Profile Schema

Profiles define named, reusable command templates for specific models or external agent wrappers. The configuration is formatted in standard TOML:

```toml
schema_version = 1

[[profiles]]
name = "local-llama"
provider = "llama-server"
command = "curl -s http://127.0.0.1:8080/v1/chat/completions -d '{"model":"default","messages":[{"role":"user","content":"{prompt_json}"}]}'"

[[profiles]]
name = "qwen-coder"
provider = "ollama"
command = "ollama run qwen2.5-coder:7b '{prompt}'"

[[profiles]]
name = "copilot-cli"
provider = "copilot"
command = "gh copilot suggest -t shell '{prompt}'"

[[profiles]]
name = "gemini-cli"
provider = "gemini"
command = "gemini -p '{prompt}'"
```

---

### 2.3 Template Placeholders

When defining command templates, `agent-runtime` substitutes values safely without invoking an intermediate shell:

| Placeholder | Description |
| :--- | :--- |
| `{prompt}` | The raw, unescaped prompt text supplied by the user or file. |
| `{prompt_json}` | The prompt safely encoded as a JSON string literal (including quotes and escapes), suitable for embedding into JSON payloads. |
| `{context}` | One or more context file paths supplied via repeated `--context <PATH>` arguments. |
| `{target_directory}` | The target working directory where the agent should operate (defaults to `.`). |
| `{reasoning_effort}` | Reasoning effort level (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). |

---

### 2.4 Supervision Policy Settings

When running supervised processes (`agent-run run`), the harness applies strict bounds:

* **Idle Timeout (`--idle-timeout <SECONDS>`):** Maximum duration (default: 900s) allowed to elapse without bytes emitted on standard output or standard error. Prevents hanging processes waiting on unmonitored stdin.
* **Attempt Timeout:** Optional maximum wall-clock duration permitted for a single attempt.
* **Output Bound (`--max-output-bytes <BYTES>`):** Cumulative stdout and stderr bytes allowed before terminating the attempt (default: 1 MiB).
* **Retry Limit (`--max-retries <COUNT>`):** Number of supervised restarts (default: 3) permitted when an attempt encounters an idle timeout or repetition loop.
* **Loop Detection (`--detect-loops`):** Enables sliding stream cycle analysis and repetition detection.
* **Host Execution Acknowledgement (`--allow-host-execution`):** Required security flag acknowledging that commands are executed directly on the host rather than inside a container or sandbox namespace.

---

## 3. Core Capabilities & Usage Examples

### 3.1 Use Case: Direct Local Inference with Hardware Watchdogs

Local inference engines running on consumer GPUs or CPUs frequently suffer from driver freezes, context allocation stalls, or prefill deadlocks. `agent-runtime` integrates three hardware watchdogs directly into its transport layer:

```
[Prompt Sent] ---> [Slot Allocation Timeout (15s)]
                         | (Slot allocated & HTTP 200)
                         v
                   [Time-to-First-Token TTFT (45s)]
                         | (First token chunk arrives)
                         v
                   [Inter-Token Cadence Watchdog (Configurable)]
                         | (Stream continues to completion)
                         v
                   [Terminal Turn Emitted]
```

#### Example: CLI Invocation
```bash
agent-run model 
  --provider llama-server 
  --endpoint http://127.0.0.1:8080 
  --model default 
  --timeout 120 
  --stream 
  "Refactor the error handling module in src/error.rs."
```

If the server stalls while allocating context slots, `agent-run` detects the freeze, drops the socket connection immediately (releasing server resources), and reports:
```
error: slot allocation timed out after 15s
```

---

### 3.2 Use Case: Grammar-Constrained Sampling via GBNF

To guarantee that a local model never generates invalid syntax, unclosed quotes, or missing JSON fields, `agent-runtime` compiles JSON Schema constraints into native GBNF (GGML BNF) grammar rules:

#### Step 1: Create a JSON Schema (`schema.json`)
```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["read", "write", "patch", "delete"]
    },
    "path": {
      "type": "string"
    },
    "line_number": {
      "type": "integer"
    }
  },
  "required": ["action", "path"]
}
```

#### Step 2: Query with Sampler-Level Enforcement
```bash
agent-run model 
  --provider llama-server 
  --endpoint http://127.0.0.1:8080 
  --model default 
  --output-schema "$(< schema.json)" 
  "Identify the entrypoint file in this repository."
```

`agent-runtime` compiles the schema into GBNF and injects it into the `/v1/chat/completions` payload:
```bnf
root_action_1 ::= (""read"" ws | ""write"" ws | ""patch"" ws | ""delete"" ws)
root_line_number_2 ::= integer
root_path_3 ::= string
root_4 ::= "{" ws ""action"" ws ":" ws root_action_1 "," ws ""line_number"" ws ":" ws root_line_number_2 "," ws ""path"" ws ":" ws root_path_3 "}" ws
root ::= root_4
```
The model's logits are constrained during sampling; generating responses that violate the schema is mathematically impossible.

---

### 3.3 Use Case: Bounded Process Supervision & Repetition Breakers

When running external coding agents (such as Aider, Cline, Goose, or custom Python agents), unsupervised processes can enter infinite retry loops, emit endless error cascades, or hang waiting for terminal input.

`agent-run run` supervises the process:
* Spawns the process in its own detached process group (`setsid`).
* Monopolizes child process reaping: on timeout or termination, sends `SIGTERM`, waits a drain period, and enforces cleanup with `SIGKILL` across the entire process group (`kill -9 -<pgid>`).
* Emits supervisor retry context warning the next attempt about prior side effects.

```bash
agent-run run 
  --command "aider --message {prompt}" 
  --idle-timeout 30 
  --max-retries 2 
  --detect-loops 
  --allow-host-execution 
  "Fix failing unit tests in tests/unit.rs"
```

If the agent emits identical repetitive error lines (e.g. repeating `error: cannot find value in scope`), the supervisor terminates the run, increments the attempt counter, and restarts with an advisory prompt:
```
[supervisor] retrying after a repetition loop (attempt 2/3)
[Supervisor retry context]
This is attempt 2 after the prior attempt ended because of a repetition loop.
The prior attempt may have changed files or external systems.
Inspect and reconcile current state before repeating any non-idempotent action.
```

---

### 3.4 Use Case: Trajectory Journaling and Step-Through Replay

The runtime records every turn, action, observation, and metric to a structured `.jsonl` journal:

```jsonl
{"event":"session_start","session_id":"run_482","task_id":"refactor-auth","timestamp":1772899200}
{"event":"turn_start","turn":1,"timestamp":1772899201}
{"event":"prompt_eval","turn":1,"cached_tokens":2048,"new_tokens":120,"eval_duration_ms":110}
{"event":"model_reasoning","turn":1,"reasoning":"Checking database connection settings."}
{"event":"tool_dispatch","turn":1,"call_id":"c1","tool":"fs_read","args":{"path":"config.json"},"action_hash":"sha256:7f83b165..."}
{"event":"tool_result","turn":1,"call_id":"c1","tool":"fs_read","stdout":"{"port": 5432}","stderr":"","exit_code":0,"bytes":16,"state_mutated":false}
{"event":"turn_finish","turn":1,"output_tokens":32,"finish_reason":"tool_calls"}
{"event":"session_finish","session_id":"run_482","task_id":"refactor-auth","total_turns":1,"total_tokens":2200,"success":true}
```

#### Replaying the Session
You can replay and inspect historical trajectories for regression testing or offline debugging:

```bash
# Human-readable step-through
agent-run replay run_482.trajectory.jsonl

# Output:
# Replaying session `run_482` for task `refactor-auth`:
# Total events: 8, Replayed turns: 1, Tool calls: 1
# ------------------------------------------------------------
# [SessionStart] id=run_482 task=refactor-auth time=1772899200
#
# --- Turn 1 (time=1772899201) ---
# [Turn 1 PromptEval] new_tokens=Some(120) cached_tokens=Some(2048)
# [Turn 1 Reasoning]: Checking database connection settings.
# [Turn 1 ToolDispatch] fs_read (hash: sha256:7f83b165...)
#   args: {"path":"config.json"}
# [Turn 1 ToolResult] fs_read exit_code=0 bytes=16 mutated=false
# [Turn 1 Finish] reason=tool_calls output_tokens=Some(32)
#
# [SessionFinish] turns=1 tokens=2200 success=true
```

---

## 4. Rust Library API Guide

If you are embedding `agent-runtime` directly into your own Rust applications, add the dependency to your `Cargo.toml`:

```toml
[dependencies]
agent-runtime = { path = "agent_runtime" }
serde_json = "1.0"
```

### 4.1 Multi-Tier Loop Detection with `ActionHashRing`

Use `ActionHashRing` to prevent models from getting trapped in repetitive action cycles:

```rust
use agent_runtime::{
    ActionHashRing, LoopDecision, compute_action_hash, compute_observation_hash,
};
use serde_json::json;

fn main() {
    let mut ring = ActionHashRing::new();

    // 1. Proposed action from the model
    let tool_name = "read_file";
    let arguments = json!({ "path": "src/main.rs" });
    let action_hash = compute_action_hash(tool_name, &arguments);

    // 2. Check if action triggers an identical cycle
    match ring.check_proposed_action(&action_hash) {
        LoopDecision::Proceed => {
            // Action is safe to execute
            ring.record_action(action_hash, tool_name.to_string());

            // 3. Execute tool and record observation hash
            let stdout = b"fn main() {}";
            let stderr = b"";
            let obs_hash = compute_observation_hash(stdout, stderr);
            
            if let LoopDecision::SoftCorrection { rejection_message, .. } = ring.record_observation(obs_hash) {
                // Tier 2: Output invariant detected! Inject soft correction into next prompt.
                println!("{rejection_message}");
            }
        }
        LoopDecision::SoftCorrection { rejection_message, .. } => {
            // Tier 1: Soft-reject repetitive action without executing it
            println!("{rejection_message}");
        }
        LoopDecision::TemperatureJitter { jittered_temperature, rejection_message, .. } => {
            // Tier 2: Repeated stall; elevate temperature dynamically
            println!("Elevating temperature to {jittered_temperature}");
            println!("{rejection_message}");
        }
        LoopDecision::CircuitBreaker { reason, .. } => {
            // Tier 3: Hard abort after 4 consecutive stalled turns
            eprintln!("Aborting run: {reason}");
        }
    }
}
```

---

### 4.2 Compiling Tool Definitions to GBNF

```rust
use agent_runtime::{ToolDefinition, compile_tools_schema};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tools = vec![
        ToolDefinition {
            name: "fs_read".to_owned(),
            description: "Read file contents".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "shell_exec".to_owned(),
            description: "Execute bash command".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        },
    ];

    let gbnf_grammar = compile_tools_schema(&tools)?;
    println!("Compiled GBNF Grammar:
{gbnf_grammar}");
    Ok(())
}
```

---

### 4.3 Direct HTTP Transport with TTFT and Cadence Watchdogs

```rust
use std::time::Duration;
use agent_runtime::{
    CancellationToken, DirectModelTransport, LocalModelProvider, ModelMessage, ModelRequest, ToolChoice,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        "http://127.0.0.1:8080",
        Duration::from_secs(60),
        1024 * 1024,
    )?
    .with_slot_timeout(Duration::from_secs(10))
    .with_ttft_timeout(Duration::from_secs(30))
    .with_cadence_timeout(Duration::from_secs(5));

    let request = ModelRequest {
        model: "default".to_owned(),
        messages: vec![
            ModelMessage::System("You are a helpful coding assistant.".to_owned()),
            ModelMessage::User("Print numbers 1 to 10 in Rust.".to_owned()),
        ],
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        reasoning_effort: None,
        output_schema: None,
    };

    let cancellation = CancellationToken::new();

    // Stream tokens with real-time watchdogs
    transport.stream(&request, &cancellation, &mut |event| {
        match event {
            agent_runtime::ModelStreamEvent::TextDelta(text) => print!("{text}"),
            agent_runtime::ModelStreamEvent::ReasoningDelta(r) => eprint!("{r}"),
            _ => {}
        }
        Ok(())
    })?;

    println!();
    Ok(())
}
```

---

## 5. Security & Isolation Considerations

* **Host Process Authority:** `agent-run` operates on the host machine. While it enforces strict process group reaping (`setsid` + `SIGKILL`), CPU quotas, and output size caps, it does not create container or filesystem namespaces on its own. For root filesystem isolation or network containment, use Kvist's `sandbox_runner` boundary or bubblewrap (`bwrap`).
* **Environment Redaction:** Secret environment variables and sensitive API tokens should not be embedded directly into command lines; use credential files or stdin streaming where feasible.
* **Bounded Staging:** Always configure `--max-output-bytes` and `--idle-timeout` on long-running commands to avoid filling disk partitions with runaway log generation.
