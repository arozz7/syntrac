# Phase L4d follow-up: trying the AI help against a real model server (no schema change)

The user asked to test with their real model server. The demo machine was used (its data is invented, so nothing real was sent); the server was a local LM Studio-style server on loopback, and a small model was used because such servers load a model into GPU memory the first time it is asked for one (none was loaded) and a small one keeps that light.

## What was added
- `run-demo.ps1 -LiveAi <folder> -ModelEndpoint <url> -ModelName <name>`: points the demo app's own settings at an existing server instead of the stand-in, and runs `Test-DemoAiLive` (`demo-ai-check.ps1`): opens the AI help tab, investigates the first demo finding, waits as long as the model needs, prints the timeline, saves the answer and a picture.
- `docs/images/ai-investigate.png` and an "AI help" section in the README; the user guide explains `-LiveAi`.

## Result
Worked first time. The answer arrived in 129 s the first time (the server loading the model; first words at 125 s) and in 14 s with the model loaded (first words at 8 s), 1,371 and 1,798 characters. The text explained what the program probably is, why it was flagged, what could be harmless and what is worrying, cited the parts of the evidence it relied on, said what is uncertain, gave no verdict, and showed `C:\Users\<user>\...` in the path it quoted. Streaming, the "evidence is data" instruction and the loopback-only guard all worked against a real server speaking the common chat API.

## Not done
A second kind of local server setup (a proxy that fronts llama-server) was not tried. A larger model was not tried; answer quality with a large reasoning model is the thing to judge next, and the first-words wait is longer for those (the client allows ten minutes before the first word).
