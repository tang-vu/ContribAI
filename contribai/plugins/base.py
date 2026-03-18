"""Plugin base classes and registry.

Plugins are discovered via Python entry points:
  [project.entry-points."contribai.analyzers"]
  my_analyzer = "my_package:MyAnalyzer"
"""

from __future__ import annotations

import importlib.metadata
import logging
from abc import ABC, abstractmethod

from contribai.core.models import (
    Contribution,
    Finding,
    RepoContext,
)

logger = logging.getLogger(__name__)


# ── Base classes ──────────────────────────────────────


class AnalyzerPlugin(ABC):
    """Base class for analyzer plugins."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Plugin name for identification."""

    @property
    def version(self) -> str:
        return "0.1.0"

    @abstractmethod
    async def analyze(self, context: RepoContext) -> list[Finding]:
        """Analyze a repository and return findings."""


class GeneratorPlugin(ABC):
    """Base class for generator plugins."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Plugin name for identification."""

    @property
    def version(self) -> str:
        return "0.1.0"

    @abstractmethod
    async def generate(
        self,
        finding: Finding,
        context: RepoContext,
    ) -> Contribution | None:
        """Generate a contribution for a finding."""


# ── Registry ─────────────────────────────────────────


class PluginRegistry:
    """Discovers and manages plugins via entry points."""

    ANALYZER_GROUP = "contribai.analyzers"
    GENERATOR_GROUP = "contribai.generators"

    def __init__(self):
        self._analyzers: list[AnalyzerPlugin] = []
        self._generators: list[GeneratorPlugin] = []
        self._loaded = False

    def discover(self) -> None:
        """Discover and load plugins from entry points."""
        if self._loaded:
            return

        # Load analyzers
        for ep in self._get_entry_points(self.ANALYZER_GROUP):
            try:
                cls = ep.load()
                instance = cls()
                if isinstance(instance, AnalyzerPlugin):
                    self._analyzers.append(instance)
                    logger.info(
                        "Loaded analyzer plugin: %s (v%s)",
                        instance.name,
                        instance.version,
                    )
                else:
                    logger.warning(
                        "Entry point %s is not an AnalyzerPlugin",
                        ep.name,
                    )
            except Exception:
                logger.warning(
                    "Failed to load analyzer plugin: %s",
                    ep.name,
                    exc_info=True,
                )

        # Load generators
        for ep in self._get_entry_points(self.GENERATOR_GROUP):
            try:
                cls = ep.load()
                instance = cls()
                if isinstance(instance, GeneratorPlugin):
                    self._generators.append(instance)
                    logger.info(
                        "Loaded generator plugin: %s (v%s)",
                        instance.name,
                        instance.version,
                    )
                else:
                    logger.warning(
                        "Entry point %s is not a GeneratorPlugin",
                        ep.name,
                    )
            except Exception:
                logger.warning(
                    "Failed to load generator plugin: %s",
                    ep.name,
                    exc_info=True,
                )

        self._loaded = True
        logger.info(
            "Plugin discovery complete: %d analyzers, %d generators",
            len(self._analyzers),
            len(self._generators),
        )

    def _get_entry_points(self, group: str):
        """Get entry points, compatible with Python 3.11+."""
        return importlib.metadata.entry_points(group=group)

    @property
    def analyzers(self) -> list[AnalyzerPlugin]:
        if not self._loaded:
            self.discover()
        return self._analyzers

    @property
    def generators(self) -> list[GeneratorPlugin]:
        if not self._loaded:
            self.discover()
        return self._generators

    def register_analyzer(self, plugin: AnalyzerPlugin) -> None:
        """Manually register an analyzer plugin."""
        self._analyzers.append(plugin)

    def register_generator(self, plugin: GeneratorPlugin) -> None:
        """Manually register a generator plugin."""
        self._generators.append(plugin)
