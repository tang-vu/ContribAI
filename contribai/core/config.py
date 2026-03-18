"""Pydantic-based configuration system for ContribAI."""

from __future__ import annotations

from pathlib import Path
from typing import Literal

import yaml
from pydantic import BaseModel, Field, model_validator

from contribai.core.exceptions import ConfigError


class GitHubConfig(BaseModel):
    """GitHub API configuration."""

    token: str = ""
    max_repos_per_run: int = 5
    max_prs_per_day: int = 10
    rate_limit_buffer: int = 100


class LLMConfig(BaseModel):
    """LLM provider configuration."""

    provider: Literal["gemini", "openai", "anthropic", "ollama"] = "gemini"
    model: str = "gemini-2.5-flash"
    api_key: str = ""
    temperature: float = 0.3
    max_tokens: int = 8192
    base_url: str | None = None  # for ollama or custom endpoints

    @model_validator(mode="after")
    def set_defaults_per_provider(self):
        if self.model == "gemini-2.5-flash" and self.provider != "gemini":
            default_models = {
                "openai": "gpt-4o",
                "anthropic": "claude-sonnet-4-20250514",
                "ollama": "codellama:13b",
            }
            self.model = default_models.get(self.provider, self.model)
        return self


class AnalysisConfig(BaseModel):
    """Analysis engine configuration."""

    enabled_analyzers: list[str] = Field(
        default_factory=lambda: ["security", "code_quality", "docs", "ui_ux"]
    )
    severity_threshold: Literal["low", "medium", "high", "critical"] = "medium"
    max_file_size_kb: int = 500
    skip_patterns: list[str] = Field(
        default_factory=lambda: ["*.min.js", "*.min.css", "vendor/*", "node_modules/*", "*.lock"]
    )


class ContributionConfig(BaseModel):
    """Contribution generation configuration."""

    enabled_types: list[str] = Field(
        default_factory=lambda: [
            "security_fix",
            "docs_improve",
            "code_quality",
            "feature_add",
            "ui_ux_fix",
            "performance_opt",
            "refactor",
        ]
    )
    max_files_per_pr: int = 10
    run_tests_before_pr: bool = True
    commit_convention: Literal["conventional", "angular", "none"] = "conventional"
    pr_description_style: Literal["minimal", "detailed"] = "detailed"


class DiscoveryConfig(BaseModel):
    """Repository discovery configuration."""

    languages: list[str] = Field(default_factory=lambda: ["python"])
    stars_range: list[int] = Field(default_factory=lambda: [50, 10000])
    min_last_activity_days: int = 30
    require_contributing_guide: bool = False
    topics: list[str] = Field(default_factory=list)


class StorageConfig(BaseModel):
    """Storage / memory configuration."""

    db_path: str = "~/.contribai/memory.db"
    cache_ttl_hours: int = 24

    @property
    def resolved_db_path(self) -> Path:
        return Path(self.db_path).expanduser()


class SchedulerConfig(BaseModel):
    """Scheduler configuration for cron-based runs."""

    enabled: bool = False
    cron: str = "0 */6 * * *"  # every 6 hours
    timezone: str = "UTC"
    max_concurrent: int = 3


class WebConfig(BaseModel):
    """Web dashboard configuration."""

    host: str = "127.0.0.1"
    port: int = 8787
    enabled: bool = True
    api_keys: list[str] = Field(default_factory=list)
    webhook_secret: str = ""


class PipelineConfig(BaseModel):
    """Pipeline execution configuration."""

    max_concurrent_repos: int = 3
    timeout_per_repo_sec: int = 300


class QuotaConfig(BaseModel):
    """API usage quota configuration."""

    github_daily_limit: int = 5000
    llm_daily_limit: int = 1000
    llm_daily_tokens: int = 1_000_000


class NotificationConfig(BaseModel):
    """Notification channel configuration."""

    slack_webhook: str = ""
    discord_webhook: str = ""
    telegram_token: str = ""
    telegram_chat_id: str = ""
    on_merge: bool = True
    on_close: bool = True
    on_run_complete: bool = True


class ContribAIConfig(BaseModel):
    """Root configuration for ContribAI."""

    github: GitHubConfig = Field(default_factory=GitHubConfig)
    llm: LLMConfig = Field(default_factory=LLMConfig)
    analysis: AnalysisConfig = Field(default_factory=AnalysisConfig)
    contribution: ContributionConfig = Field(default_factory=ContributionConfig)
    discovery: DiscoveryConfig = Field(default_factory=DiscoveryConfig)
    storage: StorageConfig = Field(default_factory=StorageConfig)
    scheduler: SchedulerConfig = Field(default_factory=SchedulerConfig)
    web: WebConfig = Field(default_factory=WebConfig)
    pipeline: PipelineConfig = Field(default_factory=PipelineConfig)
    quota: QuotaConfig = Field(default_factory=QuotaConfig)
    notifications: NotificationConfig = Field(default_factory=NotificationConfig)


def load_config(path: str | Path | None = None) -> ContribAIConfig:
    """Load configuration from YAML file.

    Priority: explicit path > ./config.yaml > ~/.contribai/config.yaml > defaults
    """
    search_paths = [
        Path(path) if path else None,
        Path("config.yaml"),
        Path.home() / ".contribai" / "config.yaml",
    ]

    for p in search_paths:
        if p and p.exists():
            try:
                raw = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
                return ContribAIConfig(**raw)
            except yaml.YAMLError as e:
                raise ConfigError(f"Invalid YAML in {p}: {e}") from e
            except Exception as e:
                raise ConfigError(f"Failed to load config from {p}: {e}") from e

    # No config file found - use defaults
    return ContribAIConfig()
