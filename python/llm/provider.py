"""LLM Provider abstraction with multi-provider support.

Gemini is the primary/default provider. All providers implement
the same async interface for easy swapping.
"""

from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from typing import TYPE_CHECKING

from contribai.core.exceptions import LLMError, LLMRateLimitError
from contribai.core.retry import rate_limit_retry

if TYPE_CHECKING:
    from contribai.core.config import LLMConfig

logger = logging.getLogger(__name__)


# ── Abstract base ──────────────────────────────────────────────────────────────


class LLMProvider(ABC):
    """Abstract LLM provider interface."""

    def __init__(self, config: LLMConfig):
        self.config = config
        self.model = config.model
        self.temperature = config.temperature
        self.max_tokens = config.max_tokens

    @abstractmethod
    async def complete(
        self,
        prompt: str,
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        """Single-turn completion."""

    @abstractmethod
    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        """Multi-turn chat completion."""

    async def close(self):  # noqa: B027
        """Clean up any resources."""


# ── Gemini (primary) ──────────────────────────────────────────────────────────


class GeminiProvider(LLMProvider):
    """Google Gemini provider - primary/default.

    Supports both API key auth and Vertex AI (Google Cloud).
    Set vertex_project in config to use Vertex AI.
    """

    def __init__(self, config: LLMConfig):
        super().__init__(config)
        try:
            from google import genai

            if config.use_vertex:
                self._client = genai.Client(
                    vertexai=True,
                    project=config.vertex_project,
                    location=config.vertex_location,
                )
                logger.info(
                    "Gemini via Vertex AI (project=%s, location=%s)",
                    config.vertex_project,
                    config.vertex_location,
                )
            else:
                self._client = genai.Client(api_key=config.api_key)
                logger.info("Gemini via API key")
        except ImportError as e:
            raise LLMError("google-genai package not installed") from e

    @rate_limit_retry
    async def complete(
        self,
        prompt: str,
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        model: str | None = None,
    ) -> str:
        from google.genai import types

        temp = temperature if temperature is not None else self.temperature
        max_tok = max_tokens if max_tokens is not None else self.max_tokens
        use_model = model or self.model

        try:
            config = types.GenerateContentConfig(
                system_instruction=system,
                temperature=temp,
                max_output_tokens=max_tok,
            )
            response = self._client.models.generate_content(
                model=use_model,
                contents=prompt,
                config=config,
            )
            return response.text or ""
        except Exception as e:
            error_msg = str(e).lower()
            if "rate" in error_msg or "quota" in error_msg or "429" in error_msg:
                raise LLMRateLimitError(f"Gemini rate limit: {e}") from e
            raise LLMError(f"Gemini error: {e}") from e

    @rate_limit_retry
    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        model: str | None = None,
    ) -> str:
        from google.genai import types

        temp = temperature if temperature is not None else self.temperature
        max_tok = max_tokens if max_tokens is not None else self.max_tokens
        use_model = model or self.model

        try:
            contents = []
            for msg in messages:
                role = "model" if msg["role"] == "assistant" else "user"
                contents.append(types.Content(role=role, parts=[types.Part(text=msg["content"])]))

            config = types.GenerateContentConfig(
                system_instruction=system,
                temperature=temp,
                max_output_tokens=max_tok,
            )
            response = self._client.models.generate_content(
                model=use_model,
                contents=contents,
                config=config,
            )
            return response.text or ""
        except Exception as e:
            error_msg = str(e).lower()
            if "rate" in error_msg or "quota" in error_msg or "429" in error_msg:
                raise LLMRateLimitError(f"Gemini rate limit: {e}") from e
            raise LLMError(f"Gemini chat error: {e}") from e


# ── OpenAI ─────────────────────────────────────────────────────────────────────


class OpenAIProvider(LLMProvider):
    """OpenAI provider (GPT-4o, etc.)."""

    def __init__(self, config: LLMConfig):
        super().__init__(config)
        try:
            from openai import AsyncOpenAI

            kwargs = {"api_key": config.api_key}
            if config.base_url:
                kwargs["base_url"] = config.base_url
            self._client = AsyncOpenAI(**kwargs)
        except ImportError as e:
            raise LLMError("openai package not installed") from e

    async def complete(self, prompt: str, *, system: str | None = None, **kwargs) -> str:
        messages = []
        if system:
            messages.append({"role": "system", "content": system})
        messages.append({"role": "user", "content": prompt})
        return await self.chat(messages, **kwargs)

    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        temp = temperature if temperature is not None else self.temperature
        max_tok = max_tokens if max_tokens is not None else self.max_tokens

        all_messages = list(messages)
        if system and not any(m["role"] == "system" for m in all_messages):
            all_messages.insert(0, {"role": "system", "content": system})

        try:
            response = await self._client.chat.completions.create(
                model=self.model,
                messages=all_messages,
                temperature=temp,
                max_tokens=max_tok,
            )
            return response.choices[0].message.content or ""
        except Exception as e:
            error_msg = str(e).lower()
            if "rate" in error_msg or "429" in error_msg:
                raise LLMRateLimitError(f"OpenAI rate limit: {e}") from e
            raise LLMError(f"OpenAI error: {e}") from e

    async def close(self):
        await self._client.close()


# ── Anthropic ──────────────────────────────────────────────────────────────────


class AnthropicProvider(LLMProvider):
    """Anthropic provider (Claude)."""

    def __init__(self, config: LLMConfig):
        super().__init__(config)
        try:
            from anthropic import AsyncAnthropic

            kwargs = {"api_key": config.api_key}
            if config.base_url:
                kwargs["base_url"] = config.base_url
            self._client = AsyncAnthropic(**kwargs)
        except ImportError as e:
            raise LLMError("anthropic package not installed") from e

    async def complete(self, prompt: str, *, system: str | None = None, **kwargs) -> str:
        messages = [{"role": "user", "content": prompt}]
        return await self.chat(messages, system=system, **kwargs)

    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        temp = temperature if temperature is not None else self.temperature
        max_tok = max_tokens if max_tokens is not None else self.max_tokens

        try:
            kwargs = {
                "model": self.model,
                "messages": messages,
                "temperature": temp,
                "max_tokens": max_tok,
            }
            if system:
                kwargs["system"] = system

            response = await self._client.messages.create(**kwargs)
            return response.content[0].text
        except Exception as e:
            error_msg = str(e).lower()
            if "rate" in error_msg or "429" in error_msg:
                raise LLMRateLimitError(f"Anthropic rate limit: {e}") from e
            raise LLMError(f"Anthropic error: {e}") from e

    async def close(self):
        await self._client.close()


# ── Ollama (local) ─────────────────────────────────────────────────────────────


class OllamaProvider(LLMProvider):
    """Ollama local model provider."""

    def __init__(self, config: LLMConfig):
        super().__init__(config)
        self._base_url = config.base_url or "http://localhost:11434"
        try:
            import httpx

            self._client = httpx.AsyncClient(base_url=self._base_url, timeout=120.0)
        except ImportError as e:
            raise LLMError("httpx package not installed") from e

    async def complete(self, prompt: str, *, system: str | None = None, **kwargs) -> str:
        messages = []
        if system:
            messages.append({"role": "system", "content": system})
        messages.append({"role": "user", "content": prompt})
        return await self.chat(messages, **kwargs)

    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        **kwargs,
    ) -> str:
        temp = temperature if temperature is not None else self.temperature

        all_messages = list(messages)
        if system and not any(m["role"] == "system" for m in all_messages):
            all_messages.insert(0, {"role": "system", "content": system})

        try:
            payload = {
                "model": self.model,
                "messages": all_messages,
                "stream": False,
                "options": {"temperature": temp},
            }
            response = await self._client.post("/api/chat", json=payload)
            response.raise_for_status()
            data = response.json()
            return data.get("message", {}).get("content", "")
        except Exception as e:
            raise LLMError(f"Ollama error: {e}") from e

    async def close(self):
        await self._client.aclose()


# ── Multi-Model Wrapper ────────────────────────────────────────────────────────


class MultiModelProvider(LLMProvider):
    """Wraps a Gemini provider with task-aware model routing.

    Automatically selects the best model for each task type
    based on the configured routing strategy.
    """

    def __init__(self, config: LLMConfig, strategy: str = "balanced"):
        super().__init__(config)
        from contribai.llm.models import TaskType
        from contribai.llm.router import TaskRouter

        self._inner = GeminiProvider(config)
        self._router = TaskRouter(strategy=strategy)
        self._task_type = TaskType.ANALYSIS  # current task context
        self._call_log: list[dict] = []

    def set_task(self, task_type) -> None:
        """Set the current task context for model routing."""
        self._task_type = task_type

    async def complete(
        self,
        prompt: str,
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        model: str | None = None,
    ) -> str:
        if model is None:
            decision = self._router.route(
                self._task_type,
                complexity=min(len(prompt) // 500, 10),
            )
            model = decision.model.name
            logger.info(
                "🧠 [%s] → %s (%s)",
                self._task_type.value,
                decision.model.display_name,
                decision.reason,
            )
            self._call_log.append(
                {
                    "task": self._task_type.value,
                    "model": model,
                    "reason": decision.reason,
                }
            )
        return await self._inner.complete(
            prompt,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            model=model,
        )

    async def chat(
        self,
        messages: list[dict[str, str]],
        *,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        model: str | None = None,
    ) -> str:
        if model is None:
            decision = self._router.route(
                self._task_type,
                complexity=5,
            )
            model = decision.model.name
            logger.info(
                "🧠 [%s] → %s (%s)",
                self._task_type.value,
                decision.model.display_name,
                decision.reason,
            )
        return await self._inner.chat(
            messages,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            model=model,
        )

    async def close(self):
        await self._inner.close()

    @property
    def routing_log(self) -> list[dict]:
        """Get the log of routing decisions."""
        return self._call_log

    @property
    def routing_stats(self) -> dict:
        return self._router.stats


# ── Factory ────────────────────────────────────────────────────────────────────


_PROVIDERS: dict[str, type[LLMProvider]] = {
    "gemini": GeminiProvider,
    "openai": OpenAIProvider,
    "anthropic": AnthropicProvider,
    "ollama": OllamaProvider,
}


def create_llm_provider(
    config: LLMConfig,
    multi_model: bool = False,
    strategy: str = "balanced",
) -> LLMProvider:
    """Create an LLM provider instance from config.

    Args:
        config: LLM configuration
        multi_model: If True and provider is Gemini, wrap with
                     MultiModelProvider for per-task model routing
        strategy: Routing strategy (performance/balanced/economy)
    """
    provider_cls = _PROVIDERS.get(config.provider)
    if not provider_cls:
        raise LLMError(
            f"Unknown LLM provider: {config.provider}. Available: {', '.join(_PROVIDERS.keys())}"
        )

    if multi_model and config.provider == "gemini":
        logger.info(
            "Using multi-model routing (strategy=%s, default=%s)",
            strategy,
            config.model,
        )
        return MultiModelProvider(config, strategy=strategy)

    logger.info(
        "Using LLM provider: %s (model: %s)",
        config.provider,
        config.model,
    )
    return provider_cls(config)
