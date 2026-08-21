# EKOS Marketing Agent v1
## Automatic X (Twitter) Release Announcements

**Status:** Design Document  
**Version:** 0.1  
**Project:** EKOS  
**Author:** Alexey Banaev

---

# Goal

Build the first autonomous marketing agent for EKOS.

The first version has only one responsibility:

> Whenever a new `devlog_XXX.md` release note is created, automatically generate a high-quality X (Twitter) post, ask for approval, and publish it.

The agent should demonstrate how EKOS can be used for real-world autonomous developer marketing.

---

# MVP Scope

Only support:

- X (Twitter)
- Markdown release notes
- Human approval before posting
- Prevent duplicate posts

NOT in v1:

- LinkedIn
- Blog generation
- GitHub Releases
- Images
- Threads
- Analytics
- Learning
- Scheduling

---

# High Level Architecture

```
Developer

↓

Claude Code

↓

Marketing Skill

↓

Read devlog_034.md

↓

Generate Tweet

↓

Ask Approval

↓

X API

↓

Store Metadata
```

---

# Repository Structure

```
marketing/

    README.md

    config.yaml

    prompt.md

    templates/

        tweet.md

    posted/

        tweets.json

    pending/

        generated/

    scripts/

        publish.rs

    logs/
```

---

# Inputs

The agent receives:

```
devlog_034.md
```

Example:

```markdown
# EKOS Devlog 34

## Added

- Incremental compiler

## Changed

- Improved parser performance

## Fixed

- Memory leak during indexing
```

---

# Expected Output

```
🚀 EKOS Update

Incremental compilation is now available.

Only modified knowledge is rebuilt, making large repositories significantly faster.

GitHub:
https://github.com/alexeyban/EKOS

#Rust #AI #MCP
```

---

# Workflow

## Step 1

Claude detects a new file

```
devlog_034.md
```

---

## Step 2

Read file

---

## Step 3

Extract

- New features
- Improvements
- Bug fixes

Ignore

- typo
- documentation
- formatting
- comments

---

## Step 4

Estimate importance

Possible values

```
LOW

MEDIUM

HIGH
```

Rules

LOW

- documentation only
- tests
- refactoring

Result

No tweet.

---

MEDIUM

Feature improvement.

Tweet.

---

HIGH

Major feature.

Tweet.

---

# Tweet Prompt

Claude receives:

```
You are an experienced DevRel engineer.

Your job is to announce software releases.

Rules

Write naturally.

No hype.

No clickbait.

No emojis except optional rocket.

Maximum 280 characters.

Focus on developer value.

Never invent features.

Always mention EKOS.

Always include GitHub.

Include at most 3 hashtags.
```

---

# Example

Input

```
Incremental compilation
```

Output

```
🚀 EKOS now supports incremental compilation.

Only modified knowledge is rebuilt, making large repositories much faster to update.

GitHub:
https://github.com/alexeyban/EKOS

#Rust #AI #MCP
```

---

# Human Approval

Claude displays

```
Tweet Preview

---------------------------------

(tweet)

---------------------------------

Approve?

[Y]

[N]

[E] Edit
```

No automatic publishing.

---

# Publishing Layer

Create small abstraction

```
Publisher

↓

TwitterPublisher
```

Future

```
Publisher

↓

Twitter

↓

LinkedIn

↓

Reddit

↓

GitHub
```

---

# Twitter Publisher

Interface

```
publish(text)

↓

tweet_id
```

Implementation

```
POST /2/tweets
```

---

# Configuration

```
config.yaml
```

```yaml
github: https://github.com/alexeyban/EKOS

twitter:

    enabled: true

    dry_run: false

hashtags:

    - Rust

    - AI

    - MCP
```

---

# Duplicate Detection

Create

```
posted/tweets.json
```

Example

```json
[
    {
        "devlog":"034",
        "tweet_id":"19381283712",
        "date":"2026-08-04",
        "feature":"Incremental Compiler"
    }
]
```

Before posting

```
Already posted?

YES

↓

Stop
```

---

# Logging

Every execution

```
logs/

marketing.log
```

Example

```
Read devlog_034.md

Importance HIGH

Tweet generated

Approved

Published

Tweet ID 123456
```

---

# Error Handling

Possible errors

```
Twitter unavailable

↓

Retry later
```

```
Authentication failed

↓

Stop
```

```
Tweet too long

↓

Regenerate
```

```
Duplicate post

↓

Skip
```

---

# CLI

```
marketing-agent publish devlog_034.md
```

or

```
marketing-agent publish latest
```

---

# Success Criteria

The MVP is complete when the following workflow works end-to-end:

1. New `devlog_XXX.md` appears.
2. Claude reads it.
3. Claude summarizes the release.
4. Claude generates one concise tweet.
5. User approves the tweet.
6. Tweet is published to X.
7. Metadata is stored in `tweets.json`.
8. The same release cannot be published twice.

---

# Future Versions

## v2

- Thread generation
- Image generation
- Architecture diagrams

---

## v3

- LinkedIn posts
- GitHub Release Notes
- CHANGELOG updates

---

## v4

- Blog article generation
- Newsletter
- Reddit
- Hacker News

---

## v5

Marketing Intelligence

- Read X analytics
- Learn which posts perform best
- Improve future posts automatically

---

# Long-term Vision

The Marketing Agent should become the first production-grade autonomous agent built with EKOS.

It will continuously transform engineering knowledge (`devlog_*.md`) into developer-facing communication, enabling a "build once, publish everywhere" workflow while serving as a real-world demonstration of EKOS capabilities.