# Phase 4: AI help (optional, local)

Status: **approved 2026-11-21**. Everything here is **optional and off by default**. SYNTRAC works exactly as before until you turn it on.

## What it is
An **Investigate** button on a finding or a program. SYNTRAC gathers the evidence it already holds, asks a language model to explain it in plain
words, and shows the answer: what it is, why it was flagged, what could be harmless, and what to check next. The answer is text only. It
never decides anything, never changes a setting, and never marks anything expected. Detection (collection, rules, baselines, findings) never calls
the model, so a stopped or slow model cannot blind anything.

## Two ways to get an answer (both optional)
1. **A model server on this machine.** Any server that speaks the OpenAI chat API on **loopback** works: LM Studio, `llama-server`, and
   others. SYNTRAC refuses an endpoint that is not this machine (`localhost`, `127.x.x.x` or `[::1]`, plain `http`, no user name or password in
   the address). It talks to the server only when you press Investigate or Start.
2. **Copy a prompt for an outside model.** SYNTRAC builds the same question with the strongest redaction, shows you **exactly** what will be
   copied (in a box you can edit), and puts it on your clipboard when you press *Copy*. Pasting it into a hosted assistant is your own action and
   shares it with that service: SYNTRAC itself sends nothing anywhere.

## Settings (a small "AI help" section; stored in your own per-user settings file)
| Setting | Default | Meaning |
|---|---|---|
| Enable AI help | off | Nothing AI-related is shown or run until this is on |
| Endpoint | empty | The server's address, for example `http://127.0.0.1:1234/v1` (loopback only) |
| Model name | empty | Sent verbatim. Some servers switch models when the name changes, so SYNTRAC never picks one for you |
| Launch command (optional) | empty | Program, arguments and working directory used by the *Start the server* button. Empty means SYNTRAC does not manage the server: it only checks that it answers |
| Detail level | summary | `summary` or `full` for the local server (see below) |
| Start the server when I press Investigate | off | Otherwise the button says the server is not running |
| Stop the server when SYNTRAC quits | off | Applies only to a server SYNTRAC started itself |

## Launching a server (only if you set a launch command)
- **Only by your action.** The *Start the server* button shows the exact command first. Nothing starts at app start.
- The command is run **directly, with no shell**, from an **absolute path** that exists, in the working directory you set. A program in a temporary
  or downloads folder is refused. Arguments are a list, never one string to be parsed.
- Readiness comes from the server itself: `/health` when it has one, otherwise `/v1/models`. The panel says *Starting (a large model can take
  minutes to load)*, then *Ready*, or why it failed. SYNTRAC only stops a server **it started** (it keeps the process it made).
- Residual: anything running as you can edit the settings file and so change the command you are shown; that is the same power it already has, and you
  see the command before it runs.

## What is sent, at three detail levels
| | `summary` (local, default) | `full` (local, deliberate) | `external` (copy for an outside model) |
|---|---|---|---|
| Your user name in paths | replaced by `<user>` | replaced by `<user>` | replaced by `<user>` |
| Program: file name, folder, product, description, version | yes | yes | **no**: only the kind of place it runs from (for example `user_profile`), the signature state and the publisher |
| Destinations | the network owner and the name asked for | plus addresses (ports are not stored per program) | counts only (how many, how many on the Internet), the bytes, no owner, name or address |
| Finding text (title, reason, evidence lines) | yes, addresses removed | yes | **no**: only the kind of finding, its tier and how often it was found |
| File hashes | no | yes | no |
| Account a program runs as | the kind (SYSTEM, a service, a user) | plus the name | the kind only |
| Counts and hourly numbers | yes | yes | yes |
| Bounded size | yes | yes | yes, smaller |

SYNTRAC does not store command lines, so none can be sent. The evidence comes from the same read-only repositories the screens use (no raw SQL), through
a fixed set of bounded readers.

## Safety
- **Prompt injection.** A program's name or path is attacker-controlled text. It is passed as quoted data inside a structured block, the instruction
  says to treat it as data, the model has no tools and no way to change anything, and the answer is shown as **plain text** (no HTML, no links followed).
- **Honesty.** The prompt asks for uncertainty to be stated and for no verdicts; the panel lists the evidence that was sent so you can check the answer
  against it. An answer is never a decision.
- **Conversation logs.** If the server reports that it stores prompts in plain text, the panel warns you. SYNTRAC does not change the server's settings.
- **Failure.** A server that is off, slow, or answers badly produces a plain message and nothing else. Cancel stops a running answer.

## Slices (each: tests first, `verify.ps1`, a change log, a commit)
- **L4a: the client.** Settings (in the per-user settings file), the loopback guard, readiness check, and a streaming chat client, tested against a
  fake server on loopback. New dependency: a small blocking HTTP client (`ureq`, TLS off: it only talks to loopback).
- **L4b: the evidence.** The bounded readers, the three detail levels and the prompt builder, pure and tested (including against the demo data).
- **L4c: the launcher.** Start, stop and state of the server process behind a trait with a fake; the status UI.
- **L4d: Investigate.** The button on findings and programs, the streaming panel, cancel, the error states, the *copy a prompt* box with its preview,
  the settings section, the user guide.
- **L4e (later, optional).** A free-form question box with read-only tools.

## Out of scope
Any cloud call made by SYNTRAC itself; any model or key management; any change to detection; automatic decisions.
