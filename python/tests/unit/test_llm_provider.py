"""Tests for LLM provider factory and mock interactions."""

from unittest.mock import AsyncMock, patch

import pytest

from contribai.core.config import LLMConfig
from contribai.core.exceptions import LLMError
from contribai.llm.provider import (
    AnthropicProvider,
    GeminiProvider,
    OllamaProvider,
    OpenAIProvider,
    create_llm_provider,
)


class TestCreateProvider:
    def test_create_gemini(self):
        config = LLMConfig(provider="gemini", api_key="test")
        with patch("contribai.llm.provider.GeminiProvider.__init__", return_value=None):
            provider = create_llm_provider(config)
            assert isinstance(provider, GeminiProvider)

    def test_create_openai(self):
        config = LLMConfig(provider="openai", api_key="test")
        with patch("contribai.llm.provider.OpenAIProvider.__init__", return_value=None):
            provider = create_llm_provider(config)
            assert isinstance(provider, OpenAIProvider)

    def test_create_anthropic(self):
        config = LLMConfig(provider="anthropic", api_key="test")
        with patch("contribai.llm.provider.AnthropicProvider.__init__", return_value=None):
            provider = create_llm_provider(config)
            assert isinstance(provider, AnthropicProvider)

    def test_create_ollama(self):
        config = LLMConfig(provider="ollama", api_key="test")
        with patch("contribai.llm.provider.OllamaProvider.__init__", return_value=None):
            provider = create_llm_provider(config)
            assert isinstance(provider, OllamaProvider)

    def test_unknown_provider_raises(self):
        config = LLMConfig(provider="gemini", api_key="test")
        config.provider = "unknown"
        with pytest.raises(LLMError, match="Unknown LLM provider"):
            create_llm_provider(config)


class TestLLMConfigDefaults:
    def test_gemini_default_model(self):
        config = LLMConfig(provider="gemini")
        assert config.model == "gemini-2.5-flash"

    def test_openai_auto_model(self):
        config = LLMConfig(provider="openai")
        assert config.model == "gpt-4o"

    def test_anthropic_auto_model(self):
        config = LLMConfig(provider="anthropic")
        assert config.model == "claude-sonnet-4-20250514"

    def test_ollama_auto_model(self):
        config = LLMConfig(provider="ollama")
        assert config.model == "codellama:13b"

    def test_custom_model_preserved(self):
        config = LLMConfig(provider="openai", model="gpt-4-turbo")
        assert config.model == "gpt-4-turbo"

    def test_temperature_default(self):
        config = LLMConfig(provider="gemini")
        assert config.temperature == 0.3


class TestOllamaProvider:
    @pytest.mark.asyncio
    async def test_complete_calls_chat(self):
        config = LLMConfig(provider="ollama", model="test-model")
        provider = OllamaProvider(config)
        provider.chat = AsyncMock(return_value="test response")

        result = await provider.complete("hello", system="be helpful")
        assert result == "test response"
        # Verify chat was called with proper messages
        call_args = provider.chat.call_args
        messages = call_args[0][0]
        assert any(m["role"] == "system" for m in messages)
        assert any(m["role"] == "user" for m in messages)
        await provider.close()

    @pytest.mark.asyncio
    async def test_provider_close(self):
        config = LLMConfig(provider="ollama", model="test-model")
        provider = OllamaProvider(config)
        await provider.close()  # Should not raise


class TestAnthropicProvider:
    def test_base_url_passed_to_client(self):
        with patch("contribai.llm.provider.anthropic.AsyncAnthropic") as mock_cls:
            config = LLMConfig(
                provider="anthropic",
                api_key="test-key",
                base_url="https://my-proxy.example.com/v1",
            )
            AnthropicProvider(config)
            mock_cls.assert_called_once_with(
                api_key="test-key",
                base_url="https://my-proxy.example.com/v1",
            )

    def test_base_url_omitted_when_not_set(self):
        with patch("contribai.llm.provider.anthropic.AsyncAnthropic") as mock_cls:
            config = LLMConfig(provider="anthropic", api_key="test-key")
            AnthropicProvider(config)
            mock_cls.assert_called_once_with(api_key="test-key")
