"""Tests for configuration system."""

import pytest
import yaml

from contribai.core.config import ContribAIConfig, LLMConfig, load_config
from contribai.core.exceptions import ConfigError


class TestContribAIConfig:
    def test_default_config(self):
        config = ContribAIConfig()
        assert config.llm.provider == "gemini"
        assert config.llm.model == "gemini-2.5-flash"
        assert config.github.max_prs_per_day == 10
        assert "security" in config.analysis.enabled_analyzers

    def test_custom_config(self):
        config = ContribAIConfig(
            llm=LLMConfig(provider="openai", api_key="test"),
        )
        assert config.llm.provider == "openai"
        assert config.llm.model == "gpt-4o"  # auto-set by validator

    def test_ollama_defaults(self):
        config = LLMConfig(provider="ollama")
        assert config.model == "codellama:13b"


class TestLoadConfig:
    def test_load_from_yaml(self, tmp_path):
        config_data = {
            "github": {"token": "ghp_test123"},
            "llm": {"provider": "gemini", "api_key": "test_key"},
        }
        config_file = tmp_path / "config.yaml"
        config_file.write_text(yaml.dump(config_data))

        config = load_config(config_file)
        assert config.github.token == "ghp_test123"
        assert config.llm.provider == "gemini"

    def test_load_defaults_when_no_file(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        config = load_config("/nonexistent/path/config.yaml")
        assert config.llm.provider == "gemini"

    def test_invalid_yaml(self, tmp_path):
        bad_file = tmp_path / "bad.yaml"
        bad_file.write_text("{{invalid yaml::")
        with pytest.raises(ConfigError):
            load_config(bad_file)
