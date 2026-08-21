For the extended demo, I would **not demonstrate EKOS as a collection of CLI commands**. The demo should tell a story:

> **"I inherited an unfamiliar GitHub repository. EKOS helps me understand it, document it, expose its knowledge to an AI agent, and generate a technical walkthrough."**

That gives you one coherent narrative connecting your existing features.

# EKOS Extended Demo — “From Codebase to Engineering Knowledge”

**Target length:** 5–7 minutes
**Audience:** developers, software architects, engineering managers, AI-agent developers
**Goal:** demonstrate the complete EKOS workflow rather than individual features.

---

## 0. Prepare the demo

Use **one real, recognizable open-source GitHub repository**.

I would choose a project that:

* has enough complexity to demonstrate architecture;
* has multiple modules;
* has dependencies;
* has documentation gaps;
* is not enormous;
* can be processed locally.

Good candidates:

* a small-to-medium Rust project;
* a Python backend;
* a Node.js application;
* an open-source MCP server.

Avoid using EKOS itself for the first demo. It creates a circular story and makes it harder for viewers to understand the value.

### Prepare this beforehand

Have:

```text
demo/
├── source-repository/
├── ekos-output/
├── generated-documentation/
├── architecture-diagram/
└── demo-video/
```

And ideally have the repository already indexed so that the demo doesn't spend 3 minutes waiting for compilation.

---

# 1. Opening — The Problem

### Screen

Show a GitHub repository.

Do **not** start with the EKOS logo.

Start with the problem.

### Narration

> "Imagine you're joining a software project you've never seen before.
>
> You have a GitHub repository with thousands of lines of code, several modules, external dependencies, and very little documentation.
>
> Before making your first change, you need to understand how the system works."

Then show the repository.

> "Normally, you start opening files and trying to reconstruct the architecture manually."

Pause.

> "EKOS takes a different approach."

---

# 2. Introduce EKOS

Now show EKOS.

### Narration

> "EKOS compiles engineering knowledge from the repository into a structured knowledge model.
>
> Instead of treating the repository as a collection of files, EKOS identifies objects, relationships, dependencies, and evidence."

Show:

```text
Repository
     ↓
EKOS
     ↓
Engineering Knowledge
```

Then:

```text
Objects
Relationships
Dependencies
Evidence
```

Keep this section under **30 seconds**.

---

# 3. Analyze the Repository

Now actually run the workflow.

For example:

```bash
ekos compile ./demo-repository
```

Or whatever the current EKOS command is.

### Narration

> "Let's give EKOS this repository."

Show progress:

```text
Scanning repository...

Analyzing source files...

Building knowledge model...

Detecting dependencies...

Extracting relationships...
```

Then show a concise result:

```text
Files analyzed:       427
Objects discovered:   1,284
Relationships:        3,721
Modules:              18
```

Use real numbers from the actual run.

Don't invent metrics.

---

# 4. Show the Architecture

This should be the **first wow moment**.

Display the generated architecture diagram.

### Narration

> "EKOS can now show us how the repository is structured."

Zoom into the diagram.

Show:

```text
Application
    |
    +--- API
    |
    +--- Services
    |
    +--- Database
    |
    +--- External integrations
```

Then click through 2–3 important components.

### Say:

> "This is not a manually created architecture diagram. It is generated from relationships discovered in the codebase."

That sentence is important.

---

# 5. Generate Documentation

Now introduce your existing `docs-gen`.

### Screen

Run:

```bash
ekos docs generate
```

or your actual command.

Show:

```text
Generated:

README.md
architecture.md
modules.md
dependencies.md
...
```

Open the generated documentation.

### Narration

> "The same knowledge model can be rendered as documentation."

Show the architecture section.

Then show a dependency section.

Then show a Mermaid diagram.

### Important

Don't spend much time reading Markdown.

Instead explain:

> "Documentation is not the primary source of knowledge. It is one of the outputs generated from the same structured model."

This positions `docs-gen` correctly.

---

# 6. The Key Demo: Ask Questions About the Codebase

Now introduce the **AI use case**.

This should be the second major wow moment.

Connect EKOS through MCP to Claude Code.

Show:

```text
Claude Code
     ↓
EKOS MCP
     ↓
Engineering Knowledge
```

Ask:

> **"How does authentication work in this repository?"**

Let Claude answer.

Then ask:

> **"Which components depend on the authentication module?"**

Then:

> **"What could break if I change the authentication interface?"**

This is much more compelling than simply showing:

> "EKOS generated an MCP server."

### Narration

> "The important part is that the AI agent doesn't have to rediscover the entire repository from raw source code every time."

Then:

> "EKOS provides structured engineering context through MCP."

---

# 7. Show Evidence

This is critical.

Don't allow the demo to look like generic AI hallucination.

Ask:

> "Why do you believe these components are dependent on the authentication module?"

Then show the source references.

### Narration

> "Every important relationship can be traced back to evidence in the repository."

Show:

```text
Evidence

src/auth/...
src/api/...
src/services/...
```

This is one of the things that can differentiate EKOS from a generic RAG chatbot.

---

# 8. Impact Analysis

Now introduce the future/server-side direction.

Pick an actual component.

For example:

```text
UserService
```

Ask:

> **"What would be affected if we changed UserService?"**

Show:

```text
Potential impact:

API
Database
Authentication
Analytics
3 downstream services
```

Then explain:

> "This is where the knowledge graph becomes more useful than documentation alone."

Because documentation tells you:

> what exists.

Impact analysis tells you:

> **what depends on it.**

This should become one of your key product messages.

---

# 9. Generate the Technical Walkthrough Video

Now introduce the new feature you were considering.

Say:

> "But developers don't always want to read documentation."

Then:

> "What if EKOS could turn the same engineering knowledge into a technical walkthrough?"

Run:

```text
ekos explain --format video
```

or your planned command.

Show:

```text
Analyzing project...

Generating walkthrough...

Generating architecture scenes...

Generating narration...

Rendering video...
```

---

# 10. Show the Generated Video

Do **not** show the generation process for too long.

Jump to the result.

The video should contain:

### Scene 1

**What is this project?**

### Scene 2

**Architecture**

### Scene 3

**Main components**

### Scene 4

**Data / request flow**

### Scene 5

**Important dependencies**

### Scene 6

**How to start contributing**

---

### Narration inside generated video

Something like:

> "This project is an event-driven application consisting of three major services..."

Then architecture diagram.

> "Requests enter through the API layer and are processed by the Order Service..."

Then actual code.

> "The Order Service publishes events to Kafka, which are consumed by the analytics pipeline..."

This should be based on actual extracted knowledge.

---

# 11. Explain Why Local AI Matters

Now mention your local AI approach.

### Screen

Show:

```text
Local LLM
Local TTS
FFmpeg
```

### Narration

> "The generation pipeline can use local AI models, so the source code doesn't have to be uploaded to a third-party AI service."

This is potentially **very important for enterprise customers**.

Especially for:

* proprietary repositories;
* financial systems;
* healthcare;
* government;
* internal enterprise software.

Don't overpromise privacy until the implementation actually guarantees it.

Say:

> "The architecture is designed to support local models."

if that's the current state rather than implying that everything is already fully local.

---

# 12. Final Workflow

End by showing the entire pipeline.

This should be your final screen:

```text
                 GitHub Repository
                         |
                         ↓
                       EKOS
                         |
              Engineering Knowledge
                         |
       +-----------------+----------------+
       |                 |                |
       ↓                 ↓                ↓
 Documentation          MCP             Video
       |                 |                |
       ↓                 ↓                ↓
   Humans             AI Agents       Onboarding
```

### Narration

> "EKOS doesn't just generate documentation from code."

Pause.

> "It builds a structured knowledge model of the engineering system."

Pause.

> "That knowledge can then be used by developers, documentation, AI agents, and automated workflows."

Final sentence:

> **"The goal is simple: make unfamiliar software understandable to both humans and AI."**

---

# 13. Call to Action

Don't end with:

> Buy EKOS.

You don't have enough product validation for that.

End with:

> **"EKOS is open source. Try it with your own repository and tell us what you discover."**

Then display:

```text
GitHub
github.com/alexeyban/EKOS

Documentation
alexeyban.github.io/EKOS

Product Hunt
...
```

---

# What I would actually build first

Don't implement the entire video system before testing the concept.

Build a **very narrow MVP**:

```text
GitHub repository
        ↓
EKOS
        ↓
architecture.md
        +
architecture.svg
        +
overview.md
        ↓
LLM generates script
        ↓
Local TTS
        ↓
FFmpeg
        ↓
5-minute MP4
```

No AI avatars.

No generative backgrounds.

No fancy animations.

No text-to-video models.

The video should look like a **professional technical architecture presentation generated from the actual repository**.

---

# The most important test

Before investing heavily in this feature, take **three real repositories** and generate videos.

Then show the videos to developers without explaining EKOS.

Ask only:

> **"Would this help you understand a repository you didn't know?"**

Then:

> **"When would you use this?"**

If they answer:

* onboarding;
* understanding legacy code;
* preparing for a project;
* reviewing architecture;
* explaining a project to a new team;
* explaining a PR/release;

you've found real use cases.

If they say:

> "Cool, but I would never use it."

then don't build the feature further.

---

## I would also change the demo's central message

Don't make the extended demo about:

> **"EKOS can generate videos."**

Make it:

> **"EKOS turns a GitHub repository into something you can understand."**

Then documentation, architecture diagrams, MCP, AI questions, impact analysis, and video are all **different ways of consuming the same engineering knowledge**.

That gives you a much stronger product story than adding another isolated AI feature.
