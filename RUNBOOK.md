# ContribAI Runbook

> Troubleshooting guide for operators and maintainers.

## Quick Start

```bash
# Install
cargo install --path crates/contribai-rs

# First run
contribai init

# Run pipeline
contribai run --dry-run

# Check status
contribai circuit-breaker
contribai stats
```

## Common Issues

### "LLM calls are timing out"

**Symptoms:**
```
Error: LLM provider error: Gemini HTTP error: timeout
Circuit breaker: OPEN (cooldown remaining: 180s)
```

**Resolution:**
1. Check network connectivity: `curl https://generativelanguage.googleapis.com`
2. Verify API key: `contribai doctor`
3. If circuit breaker is open: wait for cooldown or run `contribai run --dry-run` to reset
4. Increase timeout in config (if applicable)
5. Check for rate limiting: `contribai system-status`

### "SQLite is locked"

**Symptoms:**
```
Error: Database error: database is locked
```

**Resolution:**
1. Check for concurrent ContribAI processes: `ps aux | grep contribai`
2. Ensure only ONE instance is running at a time
3. If lock persists, delete the WAL files (safe):
   ```bash
   rm ~/.contribai/memory.db-wal ~/.contribai/memory.db-shm
   ```
4. WAL mode is enabled — concurrent reads are fine, but writes serialize

### "Rate limited by GitHub"

**Symptoms:**
```
Error: GitHub rate limit exceeded, resets at 2026-04-07T00:00:00Z
```

**Resolution:**
1. Check remaining quota: `curl -H "Authorization: token $GITHUB_TOKEN" https://api.github.com/rate_limit`
2. Authenticated: 5000 req/hour. Unauthenticated: 60 req/hour
3. Wait for reset time or switch to a different token
4. Reduce `pipeline.max_repos_per_run` to consume fewer requests

### "My PRs keep getting rejected"

**Symptoms:** PRs created but closed by maintainers without merge.

**Resolution:**
1. Check dream profiles: `contribai stats` shows repo preferences
2. Increase quality threshold: set `pipeline.min_quality_score: 0.8`
3. Enable sandbox: `sandbox.enabled: true`, `sandbox.mode: "local"`
4. Review rejected PR feedback in patrol mode: `contribai patrol`
5. Check if repo has AI policy — some repos reject AI-generated PRs

### "ContribAI won't start / config is broken"

**Symptoms:**
```
Error: Configuration error: Cannot read config.yaml: ...
```

**Resolution:**
1. Validate config: `contribai doctor`
2. Reset to defaults: delete `config.yaml` and run `contribai init`
3. Check YAML syntax: `python -c "import yaml; yaml.safe_load(open('config.yaml'))"`
4. Ensure required fields: `llm.provider`, `llm.model` (or env vars)

### "No repos found"

**Symptoms:**
```
Found 0 candidate repositories
```

**Resolution:**
1. Broaden search criteria: lower `discovery.stars_min`, add more languages
2. Check GitHub API auth: `contribai doctor` should show GitHub as ✅
3. Try targeted analysis: `contribai target https://github.com/owner/repo`
4. Check if repos are archived: GitHub search excludes archived repos

### "Circuit breaker is stuck OPEN"

**Symptoms:**
```
Circuit breaker: OPEN (failures: 5, cooldown remaining: 240s)
```

**Resolution:**
1. Wait for cooldown (default: 300s = 5 minutes)
2. After cooldown, circuit transitions to HalfOpen automatically
3. Next successful LLM call → Closed. Next failure → Open again
4. Manual reset: run `contribai run --dry-run` (success resets counter)
5. Check root cause: `contribai doctor` for LLM auth issues

### "Web dashboard not accessible"

**Symptoms:**
```
curl http://127.0.0.1:8787 → Connection refused
```

**Resolution:**
1. Start the server: `contribai web-server`
2. Check port: default is 8787. Override with `--port 5000`
3. Check firewall: ensure port is not blocked
4. If TLS enabled: verify cert/key paths are correct
5. Check API keys: if `web.api_keys` is set, include `?api_key=YOUR_KEY`

## Debug Mode

Enable verbose logging for troubleshooting:

```bash
# CLI verbose flag
contribai run --verbose

# Environment variable (RUST_LOG)
RUST_LOG=debug contribai run
RUST_LOG=contribai::analysis=debug contribai run
RUST_LOG=contribai::generator=trace contribai run
```

### Log Locations

| Log | Path |
|-----|------|
| JSONL events | `~/.contribai/events.jsonl` |
| LLM cache | `~/.contribai/llm_cache.db` |
| SQLite memory | `~/.contribai/memory.db` |
| Dream lock | `~/.contribai/.dream.lock` |

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| RAM | 2 GB | 4 GB |
| CPU | 2 cores | 4+ cores |
| Disk | 500 MB | 2 GB |
| Network | GitHub API + LLM API access | Low-latency to APIs |

## Maintenance Tasks

### Clear LLM Cache
```bash
contribai cache-clear    # prompts for confirmation
contribai cache-clear --yes  # skip prompt
```

### Check Cache Stats
```bash
contribai cache-stats
# Output:
# 📊 LLM Response Cache Statistics
# Cache enabled: Yes
# TTL: 7 days
# Total entries: 1234
# Valid entries: 1100
# Expired entries: 134
# Database size: 45.2 KB
```

### View Pipeline History
```bash
contribai stats          # Summary stats
contribai status         # Recent PRs
contribai leaderboard    # Merge rates by repo
```

### Dream System
Dream consolidation runs automatically after pipeline runs when gates are met:
- 24h since last dream
- 5+ sessions since last dream
- No concurrent dream running

Manual trigger:
```bash
contribai dream          # runs if gates met
contribai dream --force  # runs regardless of gates
```

## Emergency Procedures

### Stop All Runs
```bash
# Ctrl+C in terminal
# Or kill the process
kill $(pgrep contribai)
```

### Reset Circuit Breaker
```bash
contribai run --dry-run   # dry run resets circuit on success
```

### Clear All Memory (nuclear option)
```bash
rm ~/.contribai/memory.db
# Note: This loses all PR history, analysis cache, repo preferences
```

### Revoke GitHub Token
1. Go to GitHub Settings → Developer settings → Personal access tokens
2. Delete the token used by ContribAI
3. Generate new token with `public_repo` scope
4. Update config: `contribai config-set github.token YOUR_NEW_TOKEN`
