# Tweet template (reference only — the real prompt lives in `ekos/crates/marketing/src/prompt.rs`)

```
🚀 EKOS now supports <feature, in developer terms, no hype>.

<one sentence: what changed and why it matters to someone building on EKOS>

GitHub:
<github url from ekos.toml [marketing] github>

<up to 3 hashtags from ekos.toml [marketing] hashtags>
```

Rules enforced in code (`tweet::validate_tweet`), not just by the prompt:

- ≤ 280 characters total
- Must mention "EKOS"
- Must include the exact GitHub URL from config
- At most 3 hashtags
