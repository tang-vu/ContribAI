"""Interactive TUI for ContribAI using Textual.

Provides a rich terminal interface for browsing repos,
reviewing findings, and approving PRs interactively.
"""

from __future__ import annotations

import asyncio
import logging

from rich.console import Console
from rich.panel import Panel
from rich.table import Table

logger = logging.getLogger(__name__)
console = Console()


class InteractiveMode:
    """Interactive terminal UI for ContribAI."""

    def __init__(self, config):
        self._config = config
        self._selected_repos = []
        self._findings = []

    async def run(self):
        """Run the interactive mode."""
        from contribai.orchestrator.pipeline import ContribPipeline

        console.print(
            Panel(
                "[bold cyan]ContribAI Interactive Mode[/bold cyan]\n"
                "Browse, analyze, and contribute interactively",
                border_style="cyan",
            )
        )

        # Step 1: Discover repos
        console.print("\n[bold]Step 1:[/bold] Discovering repositories...\n")
        pipeline = ContribPipeline(self._config)
        repos = await pipeline.discover()

        if not repos:
            console.print("[yellow]No repos found. Check your config.[/yellow]")
            return

        # Step 2: Select repos
        console.print(f"Found [bold]{len(repos)}[/bold] repositories:\n")
        table = Table(title="Available Repositories")
        table.add_column("#", style="dim", width=4)
        table.add_column("Repository", style="cyan")
        table.add_column("Language", style="green")
        table.add_column("Stars")
        table.add_column("Description")

        for i, repo in enumerate(repos, 1):
            table.add_row(
                str(i),
                repo.full_name,
                repo.language or "-",
                f"{repo.stars:,}",
                (repo.description or "")[:50],
            )

        console.print(table)
        console.print()

        # Get user selection
        selection = console.input(
            "[bold]Select repos[/bold] (e.g. 1,3,5 or 'all' or 'q' to quit): "
        )

        if selection.lower() == "q":
            console.print("[dim]Cancelled.[/dim]")
            return

        if selection.lower() == "all":
            self._selected_repos = repos
        else:
            try:
                indices = [int(x.strip()) - 1 for x in selection.split(",")]
                self._selected_repos = [repos[i] for i in indices if 0 <= i < len(repos)]
            except (ValueError, IndexError):
                console.print("[red]Invalid selection[/red]")
                return

        console.print(f"\nSelected [bold]{len(self._selected_repos)}[/bold] repos\n")

        # Step 3: Analyze
        console.print("[bold]Step 2:[/bold] Analyzing selected repos...\n")
        for repo in self._selected_repos:
            console.print(f"  Analyzing [cyan]{repo.full_name}[/cyan]...")

        # Step 4: Ask to proceed
        dry_run = console.input("\n[bold]Create PRs?[/bold] (y/n/dry-run): ")

        if dry_run.lower() == "n":
            console.print("[dim]Skipping PR creation.[/dim]")
            return

        is_dry = dry_run.lower() in ("dry-run", "d")

        console.print(
            f"\n[bold]Step 3:[/bold] Running pipeline {'(dry run)' if is_dry else ''}...\n"
        )

        result = await pipeline.run(
            dry_run=is_dry,
        )

        console.print(
            Panel(
                f"📦 Repos: [bold]{result.repos_analyzed}[/bold]\n"
                f"🔍 Findings: [bold]{result.findings_total}[/bold]\n"
                f"📤 PRs: [bold]{result.prs_created}[/bold]",
                title="Result" + (" (DRY RUN)" if is_dry else ""),
                border_style="green",
            )
        )


def run_interactive(config):
    """Entry point for interactive mode."""
    tui = InteractiveMode(config)
    asyncio.run(tui.run())
