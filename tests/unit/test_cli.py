"""Tests for CLI commands using Click testing."""

import pytest
from click.testing import CliRunner

from contribai.cli.main import cli


@pytest.fixture
def runner():
    return CliRunner()


class TestCLIHelp:
    def test_main_help(self, runner):
        result = runner.invoke(cli, ["--help"])
        assert result.exit_code == 0
        assert "ContribAI" in result.output

    def test_run_help(self, runner):
        result = runner.invoke(cli, ["run", "--help"])
        assert result.exit_code == 0
        assert "--dry-run" in result.output
        assert "--language" in result.output

    def test_target_help(self, runner):
        result = runner.invoke(cli, ["target", "--help"])
        assert result.exit_code == 0
        assert "URL" in result.output

    def test_analyze_help(self, runner):
        result = runner.invoke(cli, ["analyze", "--help"])
        assert result.exit_code == 0

    def test_status_help(self, runner):
        result = runner.invoke(cli, ["status", "--help"])
        assert result.exit_code == 0

    def test_stats_help(self, runner):
        result = runner.invoke(cli, ["stats", "--help"])
        assert result.exit_code == 0

    def test_config_help(self, runner):
        result = runner.invoke(cli, ["config", "--help"])
        assert result.exit_code == 0


class TestCLIConfig:
    def test_show_config_without_file(self, runner, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        result = runner.invoke(cli, ["config"])
        assert result.exit_code == 0
        assert "gemini" in result.output.lower()


class TestCLINoToken:
    def test_run_without_token_fails(self, runner, monkeypatch, tmp_path):
        """Run without any token source should fail gracefully."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.delenv("GITHUB_TOKEN", raising=False)
        monkeypatch.setattr(
            "subprocess.run",
            lambda *a, **kw: type("R", (), {"returncode": 1, "stdout": ""})(),
        )
        result = runner.invoke(cli, ["run"])
        assert result.exit_code != 0 or "token" in result.output.lower()

    def test_analyze_without_token_fails(self, runner, monkeypatch):
        monkeypatch.delenv("GITHUB_TOKEN", raising=False)
        monkeypatch.setattr(
            "subprocess.run",
            lambda *a, **kw: type("R", (), {"returncode": 1, "stdout": ""})(),
        )
        result = runner.invoke(cli, ["analyze", "https://github.com/test/repo"])
        assert result.exit_code != 0 or "token" in result.output.lower()
